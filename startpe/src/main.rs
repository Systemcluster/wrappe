use std::{
    env::current_exe,
    fs::{read_link, File},
    io::Write,
    mem::size_of,
    panic::set_hook,
    process::Command,
};

#[cfg(any(unix, target_os = "redox"))]
use std::os::unix::process::CommandExt;
#[cfg(not(any(unix, target_os = "redox")))]
use std::process::Stdio;
#[cfg(windows)]
use std::time::SystemTime;

#[cfg(windows)]
use winapi::um::wincon::{AttachConsole, ATTACH_PARENT_PROCESS};

use memmap2::MmapOptions;
use zerocopy::LayoutVerified;

mod types;
use types::*;

mod decompress;
use decompress::*;

mod permissions;
use permissions::*;

mod versioning;
use versioning::*;

fn main() {
    set_hook(Box::<_>::new(move |panic| {
        if let Some(message) = panic.payload().downcast_ref::<&str>() {
            eprintln!("{}", message);
        } else if let Some(message) = panic.payload().downcast_ref::<String>() {
            eprintln!("{}", message);
        } else {
            eprintln!("{}", panic);
        }
        #[cfg(windows)]
        if let Ok(mut file) = File::create(format!(
            "error-{}.txt",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        )) {
            let _ = writeln!(file, "{}", panic);
        }
    }));

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

    let show_information = info.show_information;
    let show_console = info.show_console;

    #[cfg(not(windows))]
    let console_attached = false;
    #[cfg(windows)]
    let mut console_attached = false;
    #[cfg(windows)]
    if show_console == 2 || (show_console == 0 && show_information == 2) {
        console_attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 };
    }

    if show_information >= 1 {
        println!(
            "{} {}{}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            option_env!("GIT_HASH")
                .map(|hash| format!(" ({})", hash))
                .unwrap_or_default()
        );
    }

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
    if show_information >= 1 {
        println!("{}", unpack_dir_name);
    }

    let version = std::str::from_utf8(
        &info.uid[0..(info
            .uid
            .iter()
            .position(|&c| c == b'\0')
            .unwrap_or(info.uid.len()))],
    )
    .unwrap();
    if show_information >= 2 {
        println!();
        println!("version: {}", version);
        println!(
            "show console: {} (attached: {})",
            show_console, console_attached
        );
    }

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
    if show_information >= 2 {
        println!("target directory: {}", unpack_dir.display());
    }

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
    if show_information >= 2 {
        println!("runpath: {}", run_path.display());
    }

    let should_extract = match info.versioning {
        0 => get_version(&unpack_dir) != version,
        1 => get_version(&unpack_dir) != version,
        _ => true,
    };

    let verification = if !should_extract {
        info.verification
    } else {
        0
    };
    if show_information >= 2 {
        println!("should verify: {}", verification);
        println!("should extract: {}", should_extract);
    }

    if should_extract || verification > 0 {
        let extracted = decompress(
            &mmap[..info_start],
            &unpack_dir,
            verification,
            should_extract,
            version,
            show_information,
        );
        if extracted {
            set_executable_permissions(run_path);
        }
    }

    let baked_arguments = std::str::from_utf8(
        &info.arguments[0..(info
            .arguments
            .iter()
            .position(|&c| c == b'\0')
            .unwrap_or(info.arguments.len()))],
    )
    .expect("couldn't parse baked arguments");
    let baked_arguments = baked_arguments
        .split('\u{1f}')
        .map(|arg| arg.trim().to_string())
        .filter(|arg| !arg.is_empty())
        .collect::<Vec<_>>();
    if show_information >= 2 && !baked_arguments.is_empty() {
        println!("baked arguments: {:?}", baked_arguments);
    }

    let forwarded_arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if show_information >= 2 && !forwarded_arguments.is_empty() {
        println!("forwarded arguments: {:?}", forwarded_arguments);
    }

    let launch_dir = std::env::current_dir().unwrap();
    let current_dir = if info.current_dir == 1 {
        &unpack_dir
    } else {
        &launch_dir
    };
    if show_information >= 2 {
        println!("current dir: {}", current_dir.display());
    }

    drop(mmap);
    drop(file);

    if show_information >= 2 {
        println!("running...");
    }

    if console_attached && show_console == 0 {
        let _ = std::io::stdout().flush();
    }

    let mut command = Command::new(run_path);
    command.args(baked_arguments);
    command.args(forwarded_arguments);
    command.env("WRAPPE_UNPACK_DIR", unpack_dir.as_os_str());
    command.env("WRAPPE_LAUNCH_DIR", launch_dir.as_os_str());
    command.current_dir(current_dir);
    #[cfg(any(unix, target_os = "redox"))]
    {
        let e = command.exec();
        panic!("failed to run {}: {}", run_path.display(), e);
    }
    #[cfg(not(any(unix, target_os = "redox")))]
    {
        if show_console == 0 || (show_console == 2 && !console_attached) {
            command.stdout(Stdio::null());
            command.stderr(Stdio::null());
            command.stdin(Stdio::null());
        }
        let mut child = command
            .spawn()
            .unwrap_or_else(|e| panic!("failed to run {}: {}", run_path.display(), e));
        if show_console == 1 || (show_console == 2 && console_attached) {
            let result = child
                .wait()
                .unwrap_or_else(|e| panic!("failed to run {}: {}", run_path.display(), e));
            std::process::exit(result.code().unwrap_or(0))
        }
    }
}
