use std::path::Path;

#[cfg(windows)]
pub fn check_instance(run_path: &Path) -> Result<bool, std::io::Error> {
    use core::ffi::c_void;
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
        UI::WindowsAndMessaging::EnumWindows,
    };

    unsafe extern "system" fn enum_windows_proc(hwnd: *mut c_void, lparam: isize) -> i32 {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowThreadProcessId, SW_SHOW, SetForegroundWindow, ShowWindow,
        };
        let mut process_id = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut process_id);
        }
        if process_id == lparam as u32 {
            unsafe { ShowWindow(hwnd, SW_SHOW) };
            let result = unsafe { SetForegroundWindow(hwnd) };
            if result == 0 {
                return 1;
            }
            0
        } else {
            1
        }
    }

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    if unsafe { Process32FirstW(snapshot, &mut entry) } != 0 {
        let command_name = run_path.file_name().unwrap().to_os_string();
        let mut path = [0u16; 1024];
        loop {
            let process_name: &[u16] = unsafe {
                std::slice::from_raw_parts(
                    entry.szExeFile.as_ptr().cast::<u16>(),
                    entry
                        .szExeFile
                        .iter()
                        .take(entry.szExeFile.len())
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len()),
                )
            };
            let process_name = OsString::from_wide(process_name);
            if process_name == command_name {
                let process = unsafe {
                    OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID)
                };
                let mut len = path.len() as u32;
                let result =
                    unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len) };
                if result == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let path = OsString::from_wide(&path[..len as usize]);
                if path == run_path.as_os_str() {
                    let result =
                        unsafe { EnumWindows(Some(enum_windows_proc), entry.th32ProcessID as _) };
                    if result == 0 {
                        let err = std::io::Error::last_os_error();
                        if err.raw_os_error() != Some(0) {
                            return Err(err);
                        }
                    }
                    return Ok(true);
                }
            }
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }
    }

    Ok(false)
}

#[cfg(target_os = "linux")]
pub fn check_instance(run_path: &Path) -> Result<bool, std::io::Error> {
    let processes = procfs::process::all_processes();
    if let Err(_e) = processes {
        #[cfg(debug_assertions)]
        eprintln!("error: {}", _e);
        return Ok(false);
    }
    for proc in processes.unwrap() {
        match proc {
            Ok(p) => match p.exe() {
                Ok(exe) => {
                    if exe == run_path {
                        return Ok(true);
                    }
                }
                Err(_e) => {
                    #[cfg(debug_assertions)]
                    eprintln!("error: {}", _e);
                    continue;
                }
            },
            Err(_e) => {
                #[cfg(debug_assertions)]
                eprintln!("error: {}", _e);
                continue;
            }
        }
    }
    Ok(false)
}

#[cfg(not(any(windows, target_os = "linux")))]
#[inline(always)]
pub fn check_instance(_: &Path) -> Result<bool, std::io::Error> { Ok(false) }
