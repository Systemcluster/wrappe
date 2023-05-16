use std::{
    fs::{read_to_string, write},
    path::Path,
};

const VERSION_FILE: &str = "._wrappe_uid_";

pub fn get_version(target: &Path) -> String {
    read_to_string(target.join(VERSION_FILE)).unwrap_or_else(|_| "0".to_string())
}

pub fn set_version(target: &Path, version: &str) {
    write(target.join(VERSION_FILE), version).unwrap()
}
