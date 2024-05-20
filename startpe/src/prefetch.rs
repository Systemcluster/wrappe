use std::{io::Result, thread::JoinHandle};

#[cfg(windows)]
pub fn prefetch_memory(mmap: &[u8], offset: usize) -> Option<JoinHandle<Result<()>>> {
    let virtual_address = mmap.as_ptr() as usize + offset;
    let number_of_bytes = mmap.len() - offset;
    Some(std::thread::spawn(move || {
        use winapi::{
            shared::{
                basetsd::{SIZE_T, ULONG_PTR},
                minwindef::{BOOL, ULONG},
                ntdef::{PCSTR, PVOID, VOID},
            },
            um::{
                libloaderapi::{GetProcAddress, LoadLibraryExA, LOAD_LIBRARY_SEARCH_SYSTEM32},
                processthreadsapi::GetCurrentProcess,
                winnt::HANDLE,
            },
        };
        fn get_function(library: PCSTR, function: PCSTR) -> Result<*const VOID> {
            let module = unsafe {
                LoadLibraryExA(library, std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32)
            };
            if module.is_null() {
                Err(std::io::Error::last_os_error())?;
            }
            let address = unsafe { GetProcAddress(module, function) };
            if address.is_null() {
                Err(std::io::Error::last_os_error())?;
            }
            Ok(address as *const _)
        }
        type PrefetchVirtualMemory = unsafe extern "system" fn(
            hProcess: HANDLE,
            NumberOfEntries: ULONG_PTR,
            VirtualAddresses: *mut WIN32_MEMORY_RANGE_ENTRY,
            Flags: ULONG,
        ) -> BOOL;
        #[repr(C)]
        #[allow(non_camel_case_types, non_snake_case)]
        struct WIN32_MEMORY_RANGE_ENTRY {
            VirtualAddress: PVOID,
            NumberOfBytes:  SIZE_T,
        }
        // Dynamically load PrefetchVirtualMemory since it is only available on Windows 8 and later
        let prefetch_fn = unsafe {
            match get_function(
                b"kernel32.dll\0".as_ptr() as _,
                b"PrefetchVirtualMemory\0".as_ptr() as _,
            ) {
                Err(e) => return Err(e),
                Ok(f) => std::mem::transmute::<*const _, PrefetchVirtualMemory>(f),
            }
        };
        let mut memory = WIN32_MEMORY_RANGE_ENTRY {
            VirtualAddress: virtual_address as *mut _,
            NumberOfBytes:  number_of_bytes as _,
        };
        let result = unsafe { prefetch_fn(GetCurrentProcess(), 1, &mut memory as *mut _, 0) };
        if result == 0 {
            Err(std::io::Error::last_os_error())?;
        }
        Ok(())
    }))
}

#[cfg(not(windows))]
#[inline(always)]
pub fn prefetch_memory(_: &[u8], _: usize) -> Option<JoinHandle<Result<()>>> { None }
