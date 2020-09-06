#![windows_subsystem = "windows"]

use std::{
    cmp::Ordering,
    env::current_exe,
    fs::{create_dir_all, metadata, read_link, File},
    io::{copy, BufReader, ErrorKind, Seek, SeekFrom},
};

use fslock::LockFile;
use memmap::MmapOptions;
use minilz4::Decoder;
use rayon::prelude::*;
use scroll::{ctx::SizeWith, IOread, SizeWith, LE};
use tar::Header;

#[cfg(windows)]
use winapi::{
    shared::windef::HWND,
    um::{
        consoleapi::AllocConsole,
        wincon::{AttachConsole, FreeConsole, GetConsoleWindow, ATTACH_PARENT_PROCESS},
    },
};

mod decompress;
use decompress::*;

mod command;
use command::*;

mod versioning;
use versioning::*;

#[repr(C)]
#[derive(Clone, Copy, SizeWith, IOread)]
struct DistributionInfo {
    unpack_directory: [u8; 128],
    command:          [u8; 128],
}
#[repr(C)]
#[derive(Clone, Copy, SizeWith, IOread)]
pub struct StarterInfo {
    signature:      [u8; 8],
    payload_offset: u64,
    show_console:   u8,
    current_dir:    u8,
    uid:            [u8; 8],
    unpack_target:  u8,
    versioning:     u8,
    wrappe_format:  u8,
}
const WRAPPE_FORMAT: u8 = 100;

fn main() {
    let mut exe = current_exe().expect("couldn't get handle to current executable");
    while let Ok(link) = read_link(&exe) {
        exe = link;
    }
    let file = File::open(&exe).expect("couldn't open current executable");
    let mut reader = BufReader::new(&file);
    reader
        .seek(SeekFrom::End(
            0 - StarterInfo::size_with(&LE) as i64 - DistributionInfo::size_with(&LE) as i64,
        ))
        .unwrap();
    let dist: DistributionInfo = reader.ioread_with(LE).unwrap();
    let info: StarterInfo = reader.ioread_with(LE).unwrap();

    #[cfg(windows)]
    let mut console = std::ptr::null::<HWND>() as HWND;
    #[cfg(windows)]
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            if info.show_console != 0 {
                AllocConsole();
            }
        } else {
            console = GetConsoleWindow();
        }
    }
    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

    if info.signature != [0x50, 0x45, 0x33, 0x44, 0x41, 0x54, 0x41, 0x00] {
        panic!("file signature is invalid");
    }
    if info.wrappe_format != WRAPPE_FORMAT {
        panic!(
            "runner version ({}) differs from wrapper version ({})",
            WRAPPE_FORMAT, info.wrappe_format
        );
    }

    let total_size = reader.seek(SeekFrom::End(0)).unwrap();
    let info_size = StarterInfo::size_with(&LE) as u64 + DistributionInfo::size_with(&LE) as u64;
    let payload_start = reader
        .seek(SeekFrom::End(0 - info.payload_offset as i64))
        .unwrap();
    let payload_size = total_size - payload_start - info_size;

    println!("payload size: {}", payload_size);

    let version = std::str::from_utf8(
        &info.uid[0..(info
            .uid
            .iter()
            .position(|&c| c == b'\0')
            .unwrap_or(info.uid.len()))],
    )
    .unwrap();

    let unpack_root = match info.unpack_target {
        0 => std::env::temp_dir(),
        1 => dirs::data_local_dir().unwrap(),
        2 => std::env::current_dir().unwrap(),
        _ => panic!("invalid unpack target"),
    };
    let mut unpack_dir = unpack_root.join(
        std::str::from_utf8(
            &dist.unpack_directory[0..(dist
                .unpack_directory
                .iter()
                .position(|&c| c == b'\0')
                .unwrap_or(dist.unpack_directory.len()))],
        )
        .unwrap(),
    );
    if info.versioning == 0 {
        unpack_dir = unpack_dir.join(version);
    }
    println!("extracting to: {}", unpack_dir.display());

    let should_extract = match info.versioning {
        0 => get_version(&unpack_dir) != version,
        1 => get_version(&unpack_dir) != version,
        _ => true,
    };

    println!("should extract: {}", should_extract);

    if should_extract {
        create_dir_all(&unpack_dir).unwrap();

        let mut lockfile = LockFile::open(&unpack_dir.join("._wrappe_lock_")).unwrap();
        lockfile.lock().unwrap();

        let mmap = unsafe {
            MmapOptions::new()
                .offset(payload_start)
                .len(payload_size as usize)
                .map(&file)
                .unwrap()
        };
        let mut slices = Vec::new();
        let mut hpos = 0 as usize;

        println!("building slice...");
        while hpos + 512 + 1024 <= payload_size as usize {
            let header = tar::Header::from_byte_slice(&mmap[hpos..(hpos + 512)]);
            let esize = header.entry_size().unwrap() as usize;
            slices.push((header, &mmap[(hpos + 512)..(hpos + 512 + esize)]));
            if esize > 0 {
                let align = (512 - (esize % 512)) % 512;
                hpos = hpos + 512 + esize + align;
            } else {
                hpos += 512;
            }
        }

        println!("sorting...");
        slices.par_sort_by(|(lheader, _), (rheader, _)| {
            match (lheader.entry_type().is_dir(), rheader.entry_type().is_dir()) {
                (true, true) => Ordering::Equal,
                (false, false) => Ordering::Equal,
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
            }
        });

        println!("creating directories...");
        let _ = slices
            .iter()
            .try_for_each(|(header, _): &(&tar::Header, &[u8])| {
                let kind = header.entry_type();
                if kind.is_dir() {
                    let target = unpack_dir.join(&header.path().unwrap());
                    create_dir_all(&target)
                        .or_else(|err| {
                            if err.kind() == ErrorKind::AlreadyExists {
                                let prev = metadata(&target);
                                if prev.map(|m| m.is_dir()).unwrap_or(false) {
                                    return Ok(());
                                }
                            }
                            Err(err)
                        })
                        .unwrap();
                    if let Ok(mode) = header.mode() {
                        set_perms(&target, None, mode, true).unwrap();
                    }
                    Ok(())
                } else {
                    Err(())
                }
            });

        println!("unpacking...");
        slices
            .par_iter()
            .for_each(|(header, data): &(&Header, &[u8])| {
                let kind = header.entry_type();
                if kind.is_file() {
                    let target = unpack_dir.join(header.path().unwrap());
                    let mut file = File::create(&target).unwrap();
                    let mut decoder = Decoder::new(*data).unwrap();
                    copy(&mut decoder, &mut file).unwrap();
                    set_perms(&target, Some(&mut file), header.mode().unwrap(), true).unwrap();
                } else if kind.is_symlink() {
                    let target = unpack_dir.join(header.path().unwrap());
                    symlink(&header.link_name().unwrap().unwrap(), &target).unwrap();
                }
            });

        set_version(&unpack_dir, version);
    }

    let current_dir = std::env::current_dir().unwrap();
    let current_dir = if info.current_dir == 1 {
        &unpack_dir
    } else {
        &current_dir
    };
    let run_path = &unpack_dir.join(
        std::str::from_utf8(
            &dist.command[0..(dist
                .command
                .iter()
                .position(|&c| c == b'\0')
                .unwrap_or(dist.command.len()))],
        )
        .unwrap(),
    );
    println!("runpath: {}", run_path.display());
    println!("current dir: {}", current_dir.display());
    println!("running...");
    run(run_path, current_dir).unwrap();
    #[cfg(windows)]
    unsafe {
        if !console.is_null() {
            use std::io::prelude::*;
            let _ = std::io::stdout().flush();
        }
        FreeConsole();
    }
}
