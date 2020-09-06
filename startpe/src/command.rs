use std::{path::Path, process::Command};

pub fn run(path: &Path, current_dir: &Path) -> std::io::Result<u32> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut command = Command::new(path);
    command.args(args);
    command.current_dir(current_dir);
    Ok(command.spawn()?.id())
}
