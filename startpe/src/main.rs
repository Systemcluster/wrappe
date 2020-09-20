#![windows_subsystem = "windows"]

use std::{
    env::current_exe,
    fs::{read_link, File},
    mem::size_of,
    process::Command,
};

use memmap::MmapOptions;
use zerocopy::LayoutVerified;

#[cfg(windows)]
use winapi::{
    shared::windef::HWND,
    um::{
        consoleapi::AllocConsole,
        wincon::{AttachConsole, FreeConsole, GetConsoleWindow, ATTACH_PARENT_PROCESS},
    },
};

mod types;
use types::*;

mod decompress;
use decompress::*;

mod versioning;
use versioning::*;

fn main() {
    let mut exe = current_exe().expect("couldn't get handle to current executable");
    while let Ok(link) = read_link(&exe) {
        exe = link;
    }
    let file = File::open(&exe).expect("couldn't open current executable");

    let mmap = unsafe {
        MmapOptions::new()
            .map(&file)
            .expect("couldn't memory map current executable")
    };
    let end = mmap.len();

    let info_start = end - size_of::<StarterInfo>();
    let info = LayoutVerified::<_, StarterInfo>::new(&mmap[info_start..end])
        .expect("couldn't read starter info")
        .into_ref();

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

    println!(
        "{} {}{}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        option_env!("GIT_HASH")
            .map(|hash| format!(" ({})", hash))
            .unwrap_or_default()
    );

    if info.signature != [0x50, 0x45, 0x33, 0x44, 0x41, 0x54, 0x41, 0x00] {
        panic!("file signature is invalid");
    }
    if info.wrappe_format != WRAPPE_FORMAT {
        panic!(
            "runner version ({}) differs from wrapper version ({})",
            WRAPPE_FORMAT, info.wrappe_format
        );
    }

    let unpack_dir_name = std::str::from_utf8(
        &info.unpack_directory[0..(info
            .unpack_directory
            .iter()
            .position(|&c| c == b'\0')
            .unwrap_or(info.unpack_directory.len()))],
    )
    .unwrap();
    println!("{}", unpack_dir_name);
    println!();

    let version = std::str::from_utf8(
        &info.uid[0..(info
            .uid
            .iter()
            .position(|&c| c == b'\0')
            .unwrap_or(info.uid.len()))],
    )
    .unwrap();
    println!("version: {}", version);

    let unpack_root = match info.unpack_target {
        0 => std::env::temp_dir(),
        1 => dirs::data_local_dir().unwrap(),
        2 => std::env::current_dir().unwrap(),
        _ => panic!("invalid unpack target"),
    };
    let mut unpack_dir = unpack_root.join(unpack_dir_name);
    if info.versioning == 0 {
        unpack_dir = unpack_dir.join(version);
    }
    println!("target directory: {}", unpack_dir.display());

    let should_extract = match info.versioning {
        0 => get_version(&unpack_dir) != version,
        1 => get_version(&unpack_dir) != version,
        _ => true,
    };

    let should_verify = info.verify_files == 1 && !should_extract;
    println!("should verify: {}", should_verify);
    println!("should extract: {}", should_extract);

    if should_extract || should_verify {
        decompress(
            &mmap,
            info_start,
            &unpack_dir,
            should_verify,
            should_extract,
            version,
        );
    }

    let current_dir = std::env::current_dir().unwrap();
    let current_dir = if info.current_dir == 1 {
        &unpack_dir
    } else {
        &current_dir
    };
    let run_path = &unpack_dir.join(
        std::str::from_utf8(
            &info.command[0..(info
                .command
                .iter()
                .position(|&c| c == b'\0')
                .unwrap_or(info.command.len()))],
        )
        .unwrap(),
    );
    println!("runpath: {}", run_path.display());
    println!("current dir: {}", current_dir.display());
    println!("running...");
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut command = Command::new(run_path);
    command.args(args);
    command.current_dir(current_dir);
    command
        .spawn()
        .unwrap_or_else(|e| panic!("failed to run: {}", e));


    #[cfg(windows)]
    unsafe {
        if !console.is_null() {
            use std::io::prelude::*;
            let _ = std::io::stdout().flush();
        }
        FreeConsole();
    }
}
