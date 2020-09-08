use std::{
    fs::{set_permissions, File},
    io::{Error, ErrorKind, Result},
    path::Path,
};

pub fn set_perms(dst: &Path, f: Option<&mut File>, mode: u32, preserve: bool) -> Result<()> {
    _set_perms(dst, f, mode, preserve)
}

#[cfg(any(unix, target_os = "redox"))]
fn _set_perms(dst: &Path, f: Option<&mut File>, mode: u32, preserve: bool) -> Result<()> {
    use ::std::os::unix::prelude::*;

    let mode = if preserve { mode } else { mode & 0o777 };
    let perm = PermissionsExt::from_mode(mode as _);
    match f {
        Some(f) => f.set_permissions(perm),
        None => set_permissions(dst, perm),
    }
}
#[cfg(windows)]
fn _set_perms(dst: &Path, f: Option<&mut File>, mode: u32, _preserve: bool) -> Result<()> {
    use ::std::fs::metadata;
    if mode & 0o200 == 0o200 {
        return Ok(());
    }
    match f {
        Some(f) => {
            let mut perm = f.metadata()?.permissions();
            perm.set_readonly(true);
            f.set_permissions(perm)
        }
        None => {
            let mut perm = metadata(dst)?.permissions();
            perm.set_readonly(true);
            set_permissions(dst, perm)
        }
    }
}
#[cfg(target_arch = "wasm32")]
#[allow(unused_variables)]
fn _set_perms(dst: &Path, f: Option<&mut File>, mode: u32, _preserve: bool) -> Result<()> { Ok(()) }

pub fn symlink(src: &Path, dst: &Path) -> Result<()> {
    _symlink(src, dst)
        .or_else(|err_io| {
            if err_io.kind() == ErrorKind::AlreadyExists {
                std::fs::remove_file(dst).and_then(|()| symlink(&src, dst))
            } else {
                Err(err_io)
            }
        })
        .map_err(|err| {
            Error::new(
                err.kind(),
                format!(
                    "{} when symlinking {} to {}",
                    err,
                    src.display(),
                    dst.display()
                ),
            )
        })
}

#[cfg(windows)]
fn _symlink(src: &Path, dst: &Path) -> Result<()> { ::std::os::windows::fs::symlink_file(src, dst) }

#[cfg(any(unix, target_os = "redox"))]
fn _symlink(src: &Path, dst: &Path) -> Result<()> { ::std::os::unix::fs::symlink(src, dst) }
