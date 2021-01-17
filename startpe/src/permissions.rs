use std::path::Path;

#[cfg(any(unix, target_os = "redox"))]
pub fn set_executable_permissions(path: &Path) {
    use ::std::{
        fs::{metadata, set_permissions, Permissions},
        os::unix::prelude::*,
    };
    let meta = metadata(&path);
    if let Ok(ref meta) = meta {
        let mut perm: Permissions = meta.permissions();
        perm.set_mode(perm.mode() | 0o110);
        set_permissions(&path, perm).unwrap_or_else(|e| {
            eprintln!(
                "failed to set executable permissions for {}: {}",
                path.display(),
                e
            )
        });
    }
}

#[cfg(not(any(unix, target_os = "redox")))]
pub fn set_executable_permissions(_: &Path) {}
