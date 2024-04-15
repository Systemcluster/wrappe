use std::{
    env::{var, vars},
    fs::{create_dir_all, File},
    ops::Deref,
    path::{Path, PathBuf},
    process::Command,
};

use jwalk::WalkDir;
use tar::Archive;
use which::which;


const TARGETS_ENV: &str = "WRAPPE_TARGETS";
const FILES_ENV: &str = "WRAPPE_FILES";
const USE_CROSS_ENV: &str = "WRAPPE_USE_CROSS";
const MACOS_UNIVERSAL_ENV: &str = "WRAPPE_MACOS_UNIVERSAL";
const STARTER_NAME: &str = "startpe";


fn get_runner_targets() -> Vec<String> {
    let rustc = var("RUSTC").unwrap();
    let native_target = var("TARGET").unwrap();
    let mut active_targets = Vec::from([native_target]);
    let requested_targets = var(TARGETS_ENV);
    if let Ok(requested_targets) = requested_targets {
        let mut requested_targets = requested_targets.split(';').collect::<Vec<&str>>();
        requested_targets.sort_unstable();
        requested_targets.dedup();
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
                    .filter(|t| t.contains(target))
                    .collect::<Vec<_>>();
                if matches.len() == 1 {
                    active_targets.push(matches[0].to_string());
                } else {
                    panic!(
                        "couldn't build for target {}, target does not exist",
                        &target
                    );
                }
            } else {
                active_targets.push(target.to_string());
            }
        }
    }
    active_targets
}

fn compile_runner(starter_dir: &Path, target: &str, out_dir: &str) -> bool {
    eprintln!("compiling runner for target {}", &target);
    let profile = var("PROFILE").unwrap();
    let native_target = var("TARGET").unwrap();
    let cargo = PathBuf::from(var("CARGO").unwrap()).canonicalize().unwrap();
    let use_cross = var(USE_CROSS_ENV) == Ok("true".into()) || var(USE_CROSS_ENV) == Ok("1".into());
    let mut command = if target == native_target || !use_cross {
        Command::new(cargo)
    } else {
        Command::new(which("cross").unwrap_or(cargo))
    };
    if let Some(hash) = get_git_hash() {
        command.env("GIT_HASH", hash);
    }
    for (env, _) in vars() {
        if env.starts_with("CARGO") {
            command.env_remove(&env);
        }
        if env.starts_with("RUSTC") {
            command.env_remove(&env);
        }
    }
    command.env_remove("HOST");
    if target != native_target {
        command.env_remove("CC");
        command.env_remove("CXX");
        command.env_remove("AR");
    }
    for set in &["CC", "CXX", "AR"] {
        if let Ok(var) = var(&format!("WRAPPE_TARGET_{}_{}", set, target)) {
            command.env(set, var);
        }
    }
    let mut rustflags = None;
    if target == native_target {
        if let Ok(var) = var("RUSTFLAGS") {
            rustflags = Some(var);
        }
    }
    if let Ok(var) = var(format!("WRAPPE_TARGET_RUSTFLAGS_{}", target)) {
        rustflags = Some(var);
    }
    if let Some(mut rustflags) = rustflags {
        if !rustflags.contains("-Ctarget-feature") && !rustflags.contains("-C target-feature") {
            rustflags = format!("{} -Ctarget-feature=+crt-static", rustflags);
        }
        command.env("RUSTFLAGS", rustflags);
    } else {
        command.env("RUSTFLAGS", "-Ctarget-feature=+crt-static");
    }
    command
        .current_dir(starter_dir)
        .arg("build")
        .arg("--target")
        .arg(target)
        .arg("--target-dir")
        .arg(out_dir);
    if profile == "release" {
        command.arg("--release");
    }
    eprintln!("running {:?}", command);
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("couldn't compile runner for target {}: {}", &target, e));
    if status.success() {
        if let Ok(var) = var(format!("WRAPPE_TARGET_STRIP_{}", target)) {
            strip_runner(target, out_dir, &profile, &var);
        }
    }
    status.success()
}

fn strip_runner(target: &str, out_dir: &str, profile: &str, strip: &str) -> Option<()> {
    eprintln!("stripping runner for target {} with {}", target, strip);
    let (strip, args) = strip.split_once(' ').unwrap_or((strip, ""));
    let strip = which(strip.trim())
        .map_err(|e| {
            eprintln!("couldn't find strip for target {}: {}", target, e);
        })
        .ok()?;
    let args = args
        .split(' ')
        .map(|arg| arg.trim())
        .filter(|arg| !arg.is_empty());
    let mut command = Command::new(strip);
    command.args(args).arg(format!(
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
    ));
    let status = command
        .status()
        .map_err(|e| eprintln!("couldn't strip runner for target {}: {}", target, e))
        .ok()?;
    status.success().then_some(())
}

fn create_universal_macos_binary(
    files: &[(String, String)], combine: &[&str], out_dir: &str,
) -> Option<String> {
    let lipo = which("lipo")
        .map_err(|e| {
            eprintln!("couldn't find lipo for creating universal binary: {}", e);
        })
        .ok()?;
    let universal = format!("{}/universal", out_dir);
    create_dir_all(&universal)
        .map_err(|e| {
            eprintln!("couldn't create universal directory: {}", e);
        })
        .ok()?;
    let universal = format!("{}/{}", universal, STARTER_NAME);
    let mut args = ["-create", "-output", &universal].to_vec();
    args.extend(combine.iter().map(|target| {
        files
            .iter()
            .find(|(t, _)| t == target)
            .map(|(_, file)| file.deref())
            .unwrap()
    }));
    let status = Command::new(lipo)
        .args(args)
        .status()
        .map_err(|e| eprintln!("couldn't create universal binary: {}", e))
        .ok()?;
    if status.success() {
        Some(universal)
    } else {
        None
    }
}

fn get_git_hash() -> Option<String> {
    if !Path::new(".git").is_dir() {
        return None;
    }
    which("git").ok().and_then(|git| {
        Command::new(git)
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .and_then(|output| {
                String::from_utf8(output.stdout)
                    .map(|output| output.trim().into())
                    .ok()
            })
    })
}

fn unpack_starter(target: &str) -> PathBuf {
    if PathBuf::from(STARTER_NAME).is_dir() {
        return PathBuf::from(STARTER_NAME);
    }
    eprintln!("unpacking starter {}.tar", STARTER_NAME);
    let tar_path = PathBuf::from(STARTER_NAME).with_extension("tar");
    if !tar_path.is_file() {
        panic!("couldn't find {}.tar", STARTER_NAME);
    }
    let tar = File::open(tar_path)
        .unwrap_or_else(|err| panic!("couldn't open {}.tar: {}", STARTER_NAME, err));
    let mut archive = Archive::new(tar);
    let target_dir = PathBuf::from(target).join(STARTER_NAME);
    archive
        .unpack(&target_dir)
        .unwrap_or_else(|err| panic!("couldn't unpack {}.tar: {}", STARTER_NAME, err));
    target_dir
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
    let out_dir = var("OUT_DIR").unwrap();
    let starter_dir = unpack_starter(&out_dir);
    for entry in WalkDir::new(&starter_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        println!("cargo:rerun-if-changed={}", entry.path().display());
    }
    if let Ok(macosx_target) = var("MACOSX_DEPLOYMENT_TARGET") {
        println!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET={}", macosx_target);
    }
    let active_targets = get_runner_targets();
    for target in &active_targets {
        let status = compile_runner(&starter_dir, target, &out_dir);
        if !status {
            panic!("couldn't build for target {}, build failed", target);
        }
    }
    let profile = var("PROFILE").unwrap();
    let mut files = active_targets
        .iter()
        .map(|target| {
            (
                target.clone(),
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
                ),
            )
        })
        .collect::<Vec<_>>();
    if let Ok(macos_universal) = var(MACOS_UNIVERSAL_ENV) {
        let combine = macos_universal
            .split(';')
            .map(|c| c.trim())
            .collect::<Vec<_>>();
        if combine
            .iter()
            .all(|target| active_targets.contains(&target.to_string()))
        {
            let file = create_universal_macos_binary(&files, &combine, &out_dir).unwrap();
            files.push(("universal-apple-darwin".to_string(), file));
        } else {
            panic!(
                "couldn't create universal binary, target {} not in active targets",
                combine
                    .iter()
                    .find(|target| !active_targets.contains(&target.to_string()))
                    .unwrap()
            );
        }
    }
    let targets = files
        .iter()
        .map(|(target, _)| target.clone())
        .collect::<Vec<_>>()
        .join(";");
    let files = files
        .iter()
        .map(|(_, file)| file.clone())
        .collect::<Vec<_>>()
        .join(";");
    println!("cargo:rustc-env={}={}", TARGETS_ENV, targets);
    println!("cargo:rustc-env={}={}", FILES_ENV, files);
}
