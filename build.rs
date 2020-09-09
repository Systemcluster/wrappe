use std::{env::var, path::PathBuf, process::Command, vec::Vec};

use jwalk::WalkDir;
use which::which;


const TARGETS_ENV: &str = "WRAPPE_TARGETS";
const FILES_ENV: &str = "WRAPPE_FILES";
const NO_CROSS_ENV: &str = "WRAPPE_NO_CROSS";
const STARTER_NAME: &str = "startpe";


fn get_runner_targets() -> Vec<String> {
    let rustc = var("RUSTC").unwrap();
    let native_target = var("TARGET").unwrap();
    let mut active_targets = Vec::from([native_target]);
    let requested_targets = var(TARGETS_ENV);
    if let Ok(requested_targets) = requested_targets {
        let requested_targets = requested_targets.split(';').collect::<Vec<&str>>();
        let available_targets = Command::new(rustc)
            .arg("--print")
            .arg("target-list")
            .output()
            .expect("couldn't get available build target triples");
        let available_targets = String::from_utf8(available_targets.stdout)
            .expect("couldn't get available build target triples, output invalid");
        let available_targets = available_targets.lines().collect::<Vec<&str>>();
        for target in requested_targets {
            if active_targets.contains(&target.to_string()) {
                continue;
            }
            if !available_targets.contains(&target) {
                let matches = available_targets
                    .iter()
                    .filter(|t| t.contains(&target))
                    .collect::<Vec<_>>();
                if matches.len() == 1 {
                    active_targets.push(matches[0].to_string());
                } else {
                    eprintln!(
                        "couldn't build for target {}, target does not exist",
                        &target
                    );
                    std::process::exit(1);
                }
            } else {
                active_targets.push(target.to_string());
            }
        }
    }
    active_targets.sort();
    active_targets.dedup();
    active_targets
}

fn compile_runner(target: &str, out_dir: &str) -> bool {
    let profile = var("PROFILE").unwrap();
    let native_target = var("TARGET").unwrap();
    let cargo = PathBuf::from(var("CARGO").unwrap()).canonicalize().unwrap();
    let no_cross = var(NO_CROSS_ENV) == Ok("true".into()) || var(NO_CROSS_ENV) == Ok("1".into());
    let mut command = if target == native_target || no_cross {
        Command::new(cargo)
    } else {
        Command::new(which("cross").unwrap_or(cargo))
    };
    if let Some(hash) = get_git_hash() {
        command.env("GIT_HASH", hash);
    }
    command
        .current_dir(STARTER_NAME)
        .arg("build")
        .arg("--target")
        .arg(&target)
        .arg("--target-dir")
        .arg(&out_dir);
    if profile == "release" {
        command.arg("--release");
    }
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("couldn't compile runner for target {}: {}", &target, e));
    status.success()
}

fn get_git_hash() -> Option<String> {
    which("git").ok().and_then(|git| {
        Command::new(git)
            .args(&["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .and_then(|output| {
                String::from_utf8(output.stdout)
                    .map(|output| output.trim().into())
                    .ok()
            })
    })
}

fn main() {
    println!("cargo:rerun-if-env-changed=OUT_DIR");
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed={}", TARGETS_ENV);
    println!("cargo:rerun-if-changed=build.rs");
    if let Some(hash) = get_git_hash() {
        println!("cargo:rustc-env=GIT_HASH={}", hash);
        println!(
            "cargo:rustc-env=CARGO_PKG_VERSION={} ({})",
            var("CARGO_PKG_VERSION").unwrap(),
            hash
        );
    }
    println!("cargo:rerun-if-changed=.git/HEAD");
    for entry in WalkDir::new(STARTER_NAME)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        println!("cargo:rerun-if-changed={}", entry.path().display());
    }
    let out_dir = var("OUT_DIR").unwrap();
    let active_targets = get_runner_targets();
    for target in &active_targets {
        let status = compile_runner(&target, &out_dir);
        if !status {
            eprintln!("couldn't build for target {}, build failed", &target);
            std::process::exit(1);
        }
    }
    let profile = var("PROFILE").unwrap();
    let files = active_targets
        .iter()
        .map(|target| {
            format!(
                "{}/{}/{}/{}{}",
                out_dir,
                target,
                profile,
                STARTER_NAME,
                if target.contains("windows") {
                    ".exe"
                } else {
                    ""
                }
            )
        })
        .collect::<Vec<_>>()
        .join(";");
    let targets = active_targets.join(";");
    println!("cargo:rustc-env={}={}", TARGETS_ENV, targets);
    println!("cargo:rustc-env={}={}", FILES_ENV, files);
}
