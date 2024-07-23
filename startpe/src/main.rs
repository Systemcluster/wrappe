use std::{
    env::{current_exe, var_os},
    fs::{create_dir_all, read_link, File},
    io::Write,
    mem::size_of,
    panic::set_hook,
    process::Command,
    time::SystemTime,
};

#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

#[cfg(any(unix, target_os = "redox"))]
use std::os::unix::process::CommandExt;
#[cfg(not(any(unix, target_os = "redox")))]
use std::process::Stdio;

#[cfg(windows)]
use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};

use fslock_guard::LockFileGuard;
use memchr::memmem;
use memmap2::MmapOptions;
use zerocopy::Ref;

mod types;
use types::*;

mod decompress;
use decompress::*;

mod permissions;
use permissions::*;

mod versioning;
use versioning::*;

#[cfg(feature = "prefetch")]
mod prefetch;

#[cfg(feature = "once")]
mod once;

fn main() {
    set_hook(Box::<_>::new(move |panic| {
        if let Some(message) = panic.payload().downcast_ref::<&str>() {
            eprintln!("error: {}", message);
        } else if let Some(message) = panic.payload().downcast_ref::<String>() {
            eprintln!("error: {}", message);
        } else {
            eprintln!("error: {}", panic);
        }
        #[cfg(windows)]
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static WRITTEN: AtomicBool = AtomicBool::new(false);
            if WRITTEN.swap(true, Ordering::Relaxed) {
                return;
            }
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default();
            if let Ok(mut file) = File::create(format!(
                "error-{}-{}.txt",
                now.as_secs(),
                now.subsec_millis()
            )) {
                let _ = writeln!(file, "An error occurred while starting the application.");
                let _ = writeln!(file, "Please report this error to the developers.");
                let _ = writeln!(file);
                let _ = writeln!(file, "{}", panic);
            }
        }
    }));

    let mut exe = current_exe().expect("couldn't get handle to current executable");
    while let Ok(link) = read_link(&exe) {
        exe = link;
    }
    #[cfg(windows)]
    let file = File::options()
        .read(true)
        .custom_flags(0x10000000) // FILE_FLAG_RANDOM_ACCESS
        .open(&exe)
        .expect("couldn't open current executable");
    #[cfg(not(windows))]
    let file = File::options()
        .read(true)
        .open(&exe)
        .expect("couldn't open current executable");

    let mmap = unsafe {
        MmapOptions::new()
            .map(&file)
            .expect("couldn't memory map current executable")
    };
    let end = mmap.len();
    if end < size_of::<StarterInfo>() {
        panic!("file is too small ({} < {})", end, size_of::<StarterInfo>())
    }

    let mut signature = Vec::with_capacity(8);
    signature.extend_from_slice(&WRAPPE_SIGNATURE_1[..4]);
    signature.extend_from_slice(&WRAPPE_SIGNATURE_2[..4]);

    let mut info_start = end - size_of::<StarterInfo>();
    if mmap[info_start..info_start + 8] != signature[..] {
        if let Some(pos) = memmem::rfind(&mmap[..], &signature) {
            info_start = pos;
        } else {
            panic!("couldn't find starter info")
        }
    }

    if info_start + size_of::<StarterInfo>() > end {
        panic!(
            "starter info is too small ({} < {})",
            end - info_start,
            size_of::<StarterInfo>()
        )
    }
    let info = Ref::<_, StarterInfo>::new(&mmap[info_start..info_start + size_of::<StarterInfo>()])
        .expect("couldn't read starter info")
        .into_ref();
    if info.signature != signature[..] {
        panic!("file signature is invalid")
    }
    if info.wrappe_format != WRAPPE_FORMAT {
        panic!(
            "runner version ({}) differs from wrapper version ({})",
            WRAPPE_FORMAT, info.wrappe_format
        );
    }

    let mut show_information = info.show_information;
    let show_console = info.show_console;

    if show_information < 2 && var_os("STARTPE_FORCE_VERBOSE").is_some() {
        show_information = 2;
    }

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

    let command_name = std::str::from_utf8(
        &info.command[0..(info
            .command
            .iter()
            .position(|&c| c == b'\0')
            .unwrap_or(info.command.len()))],
    )
    .unwrap();
    let run_path = &unpack_dir.join(command_name);
    if show_information >= 2 {
        println!("runpath: {}", run_path.display());
    }

    create_dir_all(&unpack_dir)
        .unwrap_or_else(|e| panic!("couldn't create directory {}: {}", unpack_dir.display(), e));

    let lockfile = if info.once == 1 {
        let lockfile = LockFileGuard::try_lock(unpack_dir.join(LOCK_FILE))
            .unwrap_or_else(|e| panic!("couldn't lock file: {}", e));
        if lockfile.is_none() {
            println!("another instance is already unpacking, exiting...");
            return;
        }
        lockfile.unwrap()
    } else {
        LockFileGuard::lock(unpack_dir.join(LOCK_FILE)).unwrap_or_else(|e| {
            panic!("couldn't lock file: {}", e);
        })
    };

    #[cfg(feature = "once")]
    if info.once == 1 {
        if show_information >= 2 {
            println!("checking for running processes...");
        }
        let running = once::check_instance(run_path).unwrap();
        if running {
            println!("another instance is already running, exiting...");
            return;
        }
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
        let now = SystemTime::now();
        let extracted = decompress(
            &mmap[..info_start],
            &unpack_dir,
            verification,
            should_extract,
            version,
            show_information,
        );
        if extracted {
            if show_information >= 2 {
                println!(
                    "decompressed in {}ms",
                    now.elapsed().unwrap_or_default().as_millis()
                );
            }
            set_executable_permissions(run_path);
        }
    }

    drop(lockfile);

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
    let current_dir = match info.current_dir {
        0 => &launch_dir,
        1 => &unpack_dir,
        2 => exe.parent().unwrap(),
        3 => run_path.parent().unwrap(),
        _ => panic!("invalid current directory"),
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
