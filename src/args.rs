use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use console::style;
use rand::{
    distributions::{Alphanumeric, Distribution},
    thread_rng,
};
use staticfilemap::StaticFileMap;

use crate::types::{ARGS_SIZE, NAME_SIZE};

#[derive(StaticFileMap)]
#[parse("env")]
#[names("WRAPPE_TARGETS")]
#[files("WRAPPE_FILES")]
#[compression(16)]
#[algorithm("zstd")]
struct StarterMap;

pub fn list_runners() {
    println!("{}:", style("available runners").blue().bright());
    println!(
        "  {} {}",
        StarterMap::keys()[0],
        style("(default)").bold().dim()
    );
    for runner in &StarterMap::keys()[1..] {
        println!("  {}", runner);
    }
}

pub fn get_runner_name(name: &str) -> &'static str {
    if name == "native" || name == "default" {
        return StarterMap::keys()[0];
    }
    StarterMap::get_match_index(name)
        .map(|id| StarterMap::keys()[id])
        .unwrap_or_else(|| {
            println!(
                "{}: {}",
                style("not a valid runner").red(),
                style(name).red()
            );
            list_runners();
            std::process::exit(-1);
        })
}

pub fn get_runner(name: &str) -> &'static [u8] {
    let runner_name = if name == "native" || name == "default" {
        StarterMap::keys()[0]
    } else {
        name
    };
    StarterMap::get_match(runner_name).unwrap_or_else(|| {
        println!(
            "{}: {}",
            style("not a valid runner").red(),
            style(runner_name).red()
        );
        list_runners();
        std::process::exit(-1);
    })
}

pub fn get_unpack_target(directory: &str) -> u8 {
    match directory.to_lowercase().as_str() {
        "temp" => 0,
        "default" => 0,
        "local" => 1,
        "cwd" => 2,
        _ => {
            println!(
                "{}: {}",
                style("not a valid target directory").red(),
                style(directory).red(),
            );
            println!(
                "{}: temp {}, local, cwd",
                style("available target directories").blue().bright(),
                style("(default)").bold().black()
            );
            std::process::exit(-1);
        }
    }
}

pub fn get_versioning(versioning: &str) -> u8 {
    match versioning.to_lowercase().as_str() {
        "sidebyside" => 0,
        "default" => 0,
        "replace" => 1,
        "none" => 2,
        _ => {
            println!(
                "{}: {}",
                style("not a valid versioning strategy").red(),
                style(versioning).red(),
            );
            println!(
                "{}: sidebyside {}, replace",
                style("available versioning strategies").blue().bright(),
                style("(default)").bold().black()
            );
            std::process::exit(-1);
        }
    }
}

pub fn get_version(version: Option<&str>) -> String {
    let mut version = if let Some(version) = version {
        if version.len() > 16 {
            println!(
                "{}",
                style("version specifier is longer than 16 characters").red(),
            );
            std::process::exit(-1);
        }
        version.chars().collect::<Vec<_>>()
    } else {
        Alphanumeric
            .sample_iter(thread_rng())
            .map(char::from)
            .take(8)
            .collect::<Vec<_>>()
    };
    version.resize(16, 0 as char);
    version.iter().collect()
}

pub fn get_verification(verification: &str) -> u8 {
    match verification.to_lowercase().as_str() {
        "none" => 0,
        "default" => 1,
        "existence" => 1,
        "checksum" => 2,
        _ => {
            println!(
                "{}: {}",
                style("not a valid verification option").red(),
                style(verification).red(),
            );
            println!(
                "{}: none, existence {}, checksum",
                style("available verification options").blue().bright(),
                style("(default)").bold().black()
            );
            std::process::exit(-1);
        }
    }
}

pub fn get_show_information(show_information: &str) -> u8 {
    match show_information.to_lowercase().as_str() {
        "none" => 0,
        "default" => 1,
        "title" => 1,
        "verbose" => 2,
        _ => {
            println!(
                "{}: {}",
                style("not a valid information details option").red(),
                style(show_information).red(),
            );
            println!(
                "{}: none, title {}, verbose",
                style("available information details options")
                    .blue()
                    .bright(),
                style("(default)").bold().black()
            );
            std::process::exit(-1);
        }
    }
}

pub fn get_show_console(show_console: &str, runner_name: &str) -> u8 {
    match show_console.to_lowercase().as_str() {
        "auto" => {
            if runner_name.contains("windows") {
                0
            } else {
                1
            }
        }
        "never" => 0,
        "always" => 1,
        "attach" => 2,
        _ => {
            println!(
                "{}: {}",
                style("not a valid console option").red(),
                style(show_console).red(),
            );
            println!(
                "{}: auto {}, always, never",
                style("available console options").blue().bright(),
                style("(default)").bold().black()
            );
            std::process::exit(-1);
        }
    }
}

pub fn get_source(source: &Path) -> PathBuf {
    let source = Path::new(&std::env::current_dir().unwrap()).join(source);
    let source = std::fs::canonicalize(&source).unwrap_or_else(|_| {
        println!(
            "{}: {}",
            style("input path does not exist").red(),
            source.display()
        );
        std::process::exit(-1);
    });
    if !source.is_dir() && !source.is_file() {
        println!(
            "{}: {}",
            style("input path is not a file or directory").red(),
            source.display()
        );
        std::process::exit(-1);
    }
    source
}

pub fn get_output(output: Option<&Path>, command_path: &Path) -> PathBuf {
    let output = output
        .map(|path| path.as_os_str().to_owned())
        .unwrap_or_else(|| {
            let name = command_path
                .file_name()
                .unwrap_or_else(|| {
                    println!(
                        "{}",
                        style("couldn't infer output path from the command path").red()
                    );
                    std::process::exit(-1);
                })
                .to_owned();
            let mut prefix = OsString::from("packed-");
            prefix.push(name);
            prefix
        });
    let output = Path::new(&std::env::current_dir().unwrap()).join(output);
    if !output.parent().map(|path| path.is_dir()).unwrap_or(false) {
        println!(
            "{}: {}",
            style("output path has no parent directory").red(),
            output.parent().unwrap().display()
        );
        std::process::exit(-1);
    }
    if output.is_dir() {
        println!(
            "{}: {}",
            style("output path is a directory").red(),
            output.display()
        );
        std::process::exit(-1);
    }
    std::fs::canonicalize(output.parent().unwrap())
        .unwrap_or_else(|_| {
            println!(
                "{}: {}",
                style("output path is invalid").red(),
                output.display()
            );
            std::process::exit(-1);
        })
        .join(output.file_name().unwrap())
}

pub fn get_unpack_directory(directory: Option<&str>, source: &Path) -> [u8; NAME_SIZE] {
    let directory = if let Some(directory) = directory {
        directory.as_bytes()
    } else {
        source
            .file_name()
            .unwrap_or_else(|| {
                println!(
                    "{}",
                    style("couldn't infer unpack directory name from the input directory").red()
                );
                std::process::exit(-1);
            })
            .to_str()
            .unwrap_or_else(|| {
                println!(
                    "{}",
                    style("couldn't infer unpack directory name from the input directory, not valid utf8").red()
                );
                std::process::exit(-1);
            })
            .as_bytes()
    };
    if directory.len() >= NAME_SIZE {
        println!(
            "{}",
            style("unpack directory name is longer than 127 characters").red()
        );
        std::process::exit(-1);
    }
    let mut _directory = [0; NAME_SIZE];
    _directory[0..directory.len()].copy_from_slice(directory);
    _directory
}

pub fn get_command_path(command: &Path, source: &Path) -> PathBuf {
    let source = if source.is_file() {
        source.parent().unwrap_or_else(|| {
            println!("{}", style("source path has no parent").red());
            std::process::exit(-1);
        })
    } else {
        source
    };
    let command = match std::fs::canonicalize(source.join(command)) {
        Err(_) => std::fs::canonicalize(Path::new(&std::env::current_dir().unwrap()).join(command)),
        command => command,
    }
    .unwrap_or_else(|e| {
        println!("{}: {}", style("command path is invalid").red(), e);
        std::process::exit(-1);
    });
    if !command.is_file() {
        println!("{}", style("command path is not a file").red());
        std::process::exit(-1);
    }
    let command = if source.is_dir() {
        command.strip_prefix(source).unwrap_or_else(|_| {
            println!(
                "{}",
                style("command path is not contained in the source directory").red()
            );
            std::process::exit(-1);
        })
    } else {
        command.strip_prefix(source).unwrap_or_else(|_| {
            println!(
                "{}",
                style("command path is not contained in the source directory").red()
            );
            std::process::exit(-1);
        })
    };
    command.to_owned()
}

pub fn get_command(command_path: &Path) -> [u8; NAME_SIZE] {
    let command = command_path
        .to_str()
        .unwrap_or_else(|| {
            println!("{}", style("command path is not valid utf8").red());
            std::process::exit(-1);
        })
        .as_bytes();
    if command.len() >= NAME_SIZE {
        println!(
            "{}",
            style("command path is longer than 127 characters").red()
        );
        std::process::exit(-1);
    }
    let mut _command = [0; NAME_SIZE];
    _command[0..command.len()].copy_from_slice(command);
    _command
}

pub fn get_arguments(arguments: &[String]) -> [u8; ARGS_SIZE] {
    let arguments = arguments.join("\u{1f}");
    let arguments = arguments.as_bytes();
    if arguments.len() >= ARGS_SIZE {
        println!(
            "{}",
            style("arguments list is longer than 127 characters").red(),
        );
        std::process::exit(-1);
    }
    let mut _arguments = [0; ARGS_SIZE];
    _arguments[0..arguments.len()].copy_from_slice(arguments);
    _arguments
}
