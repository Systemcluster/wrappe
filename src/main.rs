use std::{
    error::Error,
    fs::File,
    io::{BufWriter, Cursor, Write},
    path::PathBuf,
    time::{Duration, SystemTime},
};

use clap::Parser;
use console::{Emoji, style};
use editpe::Image;
use indicatif::{ProgressBar, ProgressStyle};
use jwalk::WalkDir;
use zstd::stream::copy_decode;

mod types;
use types::*;

mod compress;
use compress::compress;

mod args;
use args::*;

#[derive(Parser)]
#[clap(about)]
pub struct Args {
    /// Platform to pack for (see --list-runners for available options)
    #[arg(short = 'r', long, default_value = "native")]
    runner:           String,
    /// Zstd compression level (0-22)
    #[arg(short = 'c', long, default_value = "8")]
    compression:      u32,
    /// Unpack directory target (temp, local, cwd)
    #[arg(short = 't', long, default_value = "temp")]
    unpack_target:    String,
    /// Unpack directory name [default: inferred from input directory]
    #[arg(short = 'd', long)]
    unpack_directory: Option<String>,
    /// Versioning strategy (sidebyside, replace, none)
    #[arg(short = 'v', long, default_value = "sidebyside")]
    versioning:       String,
    /// Verification of existing unpacked data (existence, checksum, none)
    #[arg(short = 'e', long, default_value = "existence")]
    verification:     String,
    /// Version string override [default: randomly generated]
    #[arg(short = 's', long)]
    version_string:   Option<String>,
    /// Information output details (title, verbose, none)
    #[arg(short = 'i', long, default_value = "title")]
    show_information: String,
    /// Show or attach to a console window (auto, always, never, attach)
    #[arg(short = 'n', long, default_value = "auto")]
    console:          String,
    /// Working directory of the command (inherit, unpack, runner, command)
    #[arg(short = 'w', long, default_value = "inherit")]
    current_dir:      String,
    /// Cleanup the unpack directory after exit
    #[arg(short = 'u', long, default_value = "false")]
    cleanup:          bool,
    /// Only allow one instance of the application to run
    #[arg(short = 'o', long, default_value = "false")]
    once:             bool,
    /// Build compression dictionary
    #[arg(short = 'z', long, default_value = "false")]
    build_dictionary: bool,
    /// Print available runners
    #[arg(short = 'l', long)]
    #[allow(dead_code)]
    list_runners:     bool,
    /// Path to the input directory
    #[arg(name = "input")]
    input:            PathBuf,
    /// Path to the executable to start after unpacking
    #[arg(name = "command")]
    command:          PathBuf,
    /// Path to or filename of the output executable
    #[arg(name = "output")]
    output:           Option<PathBuf>,
    /// Command line arguments to pass to the executable
    #[arg(last = true)]
    arguments:        Vec<String>,
    /// Print version
    #[arg(short = 'V', long)]
    #[allow(dead_code)]
    version:          bool,
}

fn main() {
    color_backtrace::install();

    if std::env::args().any(|arg| arg == "-l" || arg == "--list-runners") {
        list_runners();
        std::process::exit(0);
    }

    println!(
        "{}",
        style(format!(
            "{} {}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        ))
        .bold()
        .bright(),
    );

    if std::env::args().any(|arg| arg == "-V" || arg == "--version") {
        std::process::exit(0);
    }

    let args = Args::parse();

    let runner = get_runner(&args.runner);
    let runner_name = get_runner_name(&args.runner);
    let unpack_target = get_unpack_target(&args.unpack_target);
    let versioning = get_versioning(&args.versioning);
    let version = get_version(args.version_string.as_deref());
    let source = get_source(&args.input);
    let command_path = get_command_path(&args.command, &source);
    let command = get_command(&command_path);
    let output = get_output(args.output.as_deref(), &command_path);
    let unpack_directory = get_unpack_directory(args.unpack_directory.as_deref(), &source);
    let verification = get_verification(&args.verification);
    let show_information = get_show_information(&args.show_information);
    let arguments = get_arguments(&args.arguments);
    let current_dir = get_current_dir(&args.current_dir);

    let mut show_console = get_show_console(&args.console, runner_name);
    let once = if args.once { 1 } else { 0 };
    let cleanup = if args.cleanup { 1 } else { 0 };

    if (versioning == 1 || versioning == 2) && once == 0 {
        println!(
            "{} {} {} {} {}",
            style("note: chosen versioning").yellow().dim(),
            style(&args.versioning).yellow().bold(),
            style("without option").yellow().dim(),
            style("once").yellow().bold(),
            style("can cause unpacking to fail while the application is already running").dim(),
        );
    }
    if versioning == 2 && verification != 0 {
        println!(
            "{} {} {}",
            style("note: verification will be ignored with")
                .yellow()
                .dim(),
            style(&args.versioning).yellow().bold(),
            style("versioning").yellow().dim(),
        );
    }
    if once == 1 && !(runner_name.contains("windows") || runner_name.contains("linux")) {
        println!(
            "{} {} {} {}",
            style("note: option").yellow().dim(),
            style("once").yellow().bold(),
            style("is only supported for Windows and Linux runners")
                .yellow()
                .dim(),
            style(format!("(target: {})", runner_name)).yellow().dim(),
        );
    }
    if show_console != 2 && !runner_name.contains("windows") {
        println!(
            "{}",
            style("note: setting console mode is only supported for Windows runners")
                .yellow()
                .dim(),
        );
    }

    if output == source {
        println!(
            "{}: {}",
            style("output file can't be the input file").red(),
            output.display()
        );
        std::process::exit(-1);
    }
    let file = File::create(&output).unwrap_or_else(|_| {
        println!(
            "{}: {}",
            style("couldn't create output file").red(),
            output.display()
        );
        std::process::exit(-1);
    });

    let canonical_current_dir = std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
    let relative_source = source
        .strip_prefix(&canonical_current_dir)
        .unwrap_or(&source);
    let relative_source = if relative_source.components().count() == 0 {
        &canonical_current_dir
    } else {
        relative_source
    };
    let count = if source.is_dir() {
        println!(
            "{} {}counting contents of {}…",
            style("[1/4]").bold().dim(),
            Emoji("🔍 ", ""),
            style(relative_source.display()).blue().bright()
        );
        WalkDir::new(&source).skip_hidden(false).into_iter().count() as u64 - 1
    } else {
        println!(
            "{} {}checking {}…",
            style("[1/4]").bold().dim(),
            Emoji("🔍 ", ""),
            style(relative_source.display()).blue().bright()
        );
        1
    };

    println!(
        "{} {}writing runner {} for target {}…",
        style("[2/4]").bold().dim(),
        Emoji("📃 ", ""),
        style(
            &output
                .strip_prefix(&canonical_current_dir)
                .unwrap_or(&output)
                .display()
        )
        .blue()
        .bright(),
        style(&runner_name).magenta(),
    );
    let mut writer = BufWriter::new(file);
    if runner_name.contains("windows") {
        let mut decompressed = Vec::new();
        copy_decode(Cursor::new(runner), &mut decompressed).unwrap();

        let decompressed = (|| -> Result<Vec<u8>, Box<dyn Error>> {
            let mut runner_image = Image::parse(&decompressed)?;
            runner_image.set_subsystem(if show_console == 1 { 3 } else { 2 });
            Ok(runner_image.data().to_owned())
        })()
        .unwrap_or_else(|error| {
            println!(
                "      {}{} {}",
                Emoji("❗ ", ""),
                style("failed to set subsystem for runner:").yellow(),
                style(error).yellow()
            );
            decompressed
        });
        let decompressed = (|| -> Result<Vec<u8>, Box<dyn Error>> {
            let mut runner_image = Image::parse(&decompressed)?;
            let command_path = if source.is_file() {
                source.clone()
            } else {
                source.join(get_command_path(&args.command, &source))
            };
            let command_data = std::fs::read(command_path)?;
            let command_image = Image::parse(command_data)?;
            let command_resources = command_image
                .resource_directory()
                .cloned()
                .unwrap_or_default();
            if args.console == "auto" {
                show_console = if command_image.subsystem() == 3 { 1 } else { 0 };
                runner_image.set_subsystem(command_image.subsystem());
            }
            runner_image.set_resource_directory(command_resources)?;
            Ok(runner_image.data().to_owned())
        })()
        .unwrap_or_else(|error| {
            println!(
                "      {}{} {}",
                Emoji("❗ ", ""),
                style("failed to copy resources to runner:").yellow(),
                style(error).yellow()
            );
            decompressed
        });

        writer.write_all(&decompressed).unwrap();
    } else {
        copy_decode(Cursor::new(&runner), &mut writer).unwrap();
    }

    println!(
        "{} {}compressing {} files and directories…",
        style("[3/4]").bold().dim(),
        Emoji("🚚 ", ""),
        style(count).magenta(),
    );
    let bar_progress =
        ProgressBar::new(0).with_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} {elapsed_precise} [{wide_bar:.green}] {pos}/{len}\n{spinner:.green} {wide_msg}").unwrap(),
        );
    bar_progress.set_length(count);
    bar_progress.set_position(0);
    bar_progress.enable_steady_tick(Duration::from_millis(12));
    let now = SystemTime::now();
    let (compressed, read, written) = compress(
        &source,
        &mut writer,
        &output,
        args.compression,
        args.build_dictionary,
        || {
            bar_progress.inc(1);
        },
        |message| {
            bar_progress.inc(1);
            bar_progress.println(format!(
                "      {}{}",
                Emoji("❗ ", ""),
                style(message).red()
            ));
        },
        |message| {
            bar_progress.set_message(format!("{}", style(message).blue().bright()));
        },
        |message| {
            bar_progress.println(format!(
                "      {}{}",
                Emoji("💡 ", ""),
                style(message).dim()
            ));
        },
    );
    bar_progress.finish_and_clear();
    writer.flush().unwrap();

    println!(
        "      {}{}",
        Emoji("💾 ", ""),
        style(format!(
            "{:.2}MB read, {:.2}MB written, {:.2}% of original size",
            read as f64 / 1024.0 / 1024.0,
            written as f64 / 1024.0 / 1024.0,
            (written as f64 / read as f64) * 100.0
        ))
        .dim(),
    );
    println!(
        "      {}{}",
        Emoji("📍 ", ""),
        style(format!(
            "took {:.2}s",
            now.elapsed().unwrap_or_default().as_secs_f64()
        ))
        .dim(),
    );
    println!(
        "      {}{} {} {}{}",
        Emoji("✨ ", ""),
        style("successfully compressed").green(),
        style(compressed).magenta(),
        style("files and directories").green(),
        if compressed < count {
            style(format!(" (skipped {})", count - compressed))
                .bold()
                .red()
        } else {
            style(String::new())
        }
    );

    println!(
        "{} {}writing startup configuration…",
        style("[4/4]").bold().dim(),
        Emoji("📃 ", "")
    );

    let info = StarterInfo {
        signature: WRAPPE_SIGNATURE,
        show_console,
        current_dir,
        verification,
        show_information,
        cleanup,
        uid: version.as_bytes().try_into().unwrap(),
        unpack_target,
        versioning,
        unpack_directory,
        once,
        command,
        arguments,
        wrappe_format: WRAPPE_FORMAT,
    };
    writer.write_all(info.as_bytes()).unwrap();

    writer.flush().unwrap();
    drop(writer);

    #[cfg(any(unix, target_os = "redox"))]
    {
        use ::std::{
            fs::{metadata, set_permissions},
            os::unix::prelude::*,
        };
        let mode = metadata(&output)
            .map(|metadata| metadata.permissions().mode())
            .unwrap_or(0o755);
        set_permissions(&output, PermissionsExt::from_mode(mode | 0o111)).unwrap_or_else(|e| {
            eprintln!(
                "      {} failed to set permissions for {}: {}",
                Emoji("❗ ", ""),
                output.display(),
                e
            )
        });
    }

    println!("      {}{}", Emoji("✨ ", ""), style("done!").green());
}
