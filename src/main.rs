use std::{
    convert::TryInto,
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
};

use clap::Clap;
use console::{style, Emoji};
use indicatif::{ProgressBar, ProgressStyle};
use jwalk::WalkDir;
use minilz4::Decoder;
use rand::{
    distributions::{Alphanumeric, Distribution},
    thread_rng,
};

mod types;
use types::*;

mod compress;
use compress::compress;

mod args;
use args::*;

#[derive(Clap)]
#[clap(about, version)]
pub struct Args {
    /// LZ4 compression level (0-12)
    #[clap(short = 'c', long, default_value = "8")]
    compression:      u32,
    /// Which runner to use
    #[clap(short = 'r', long, default_value = "native")]
    runner:           String,
    /// Unpack directory target (temp, local, cwd)
    #[clap(short = 't', long, default_value = "temp")]
    unpack_target:    String,
    /// Unpack directory name [default: inferred from input directory]
    #[clap(short = 'd', long)]
    unpack_directory: Option<String>,
    /// Versioning strategy (sidebyside, replace, none)
    #[clap(short = 'v', long, default_value = "sidebyside")]
    versioning:       String,
    /// Verify the unpacked payload before skipping extraction (existence, checksum, none)
    #[clap(short = 'e', long, default_value = "existence")]
    verification:     String,
    /// Prints available runners
    #[clap(short = 'l', long)]
    #[allow(dead_code)]
    list_runners:     bool,
    /// Open a console when starting the runner on Windows
    #[clap(short = 's', long)]
    show_console:     bool,
    /// Set the current dir of the target to the unpack directory
    #[clap(short = 'w', long)]
    current_dir:      bool,
    /// Path to the input directory
    #[clap(name = "input", parse(from_os_str))]
    input:            PathBuf,
    /// Path to the executable to start after unpacking
    #[clap(name = "command", parse(from_os_str))]
    command:          PathBuf,
    /// Path to or filename of the output executable
    #[clap(name = "output", parse(from_os_str))]
    output:           PathBuf,
}

fn main() {
    color_backtrace::install();

    if std::env::args().any(|arg| arg == "-l" || arg == "--list-runners") {
        list_runners();
        std::process::exit(0);
    }

    let args = Args::parse();

    let runner = get_runner(&args.runner);
    let unpack_target = get_unpack_target(&args.unpack_target);
    let versioning = get_versioning(&args.versioning);
    let source = get_source(&args.input);
    let output = get_output(&args.output);
    let command = get_command(&args.command, &source);
    let unpack_directory = get_unpack_directory(&args.unpack_directory, &source);
    let verification = get_verification(&args.verification);

    let file = File::create(&output).unwrap_or_else(|_| {
        println!(
            "{}: {}",
            style("couldn't create output file").red(),
            output.display()
        );
        std::process::exit(-1);
    });

    let count = if source.is_dir() {
        println!(
            "{} {}counting contents of {}…",
            style("[1/4]").bold().black(),
            Emoji("🔍 ", ""),
            style(
                &source
                    .strip_prefix(std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap())
                    .unwrap_or(&source)
                    .display()
            )
            .blue()
        );
        WalkDir::new(&source).skip_hidden(false).into_iter().count() as u64 - 1
    } else {
        println!(
            "{} {}checking {}…",
            style("[1/4]").bold().black(),
            Emoji("🔍 ", ""),
            style(
                &source
                    .strip_prefix(std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap())
                    .unwrap_or(&source)
                    .display()
            )
            .blue()
        );
        1
    };

    println!(
        "{} {}writing runner {}…",
        style("[2/4]").bold().black(),
        Emoji("📃 ", ""),
        style(
            &output
                .strip_prefix(std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap())
                .unwrap_or(&output)
                .display()
        )
        .blue()
    );
    let mut writer = BufWriter::new(file);
    let mut decoder = Decoder::new(runner).unwrap();
    std::io::copy(&mut decoder, &mut writer).unwrap();

    println!(
        "{} {}compressing {} files and directories…",
        style("[3/4]").bold().black(),
        Emoji("🚚 ", ""),
        style(count).magenta(),
    );
    let bar_progress =
        ProgressBar::new(0).with_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} {elapsed_precise} [{wide_bar:.green}] {pos}/{len}\n{spinner:.green} {wide_msg}"),
        );
    bar_progress.set_length(count);
    bar_progress.set_position(0);
    bar_progress.enable_steady_tick(12);
    let compressed = compress(
        &source,
        &mut writer,
        args.compression,
        || {
            bar_progress.inc(1);
        },
        |message| {
            bar_progress.inc(1);
            bar_progress.println(&format!(
                "      {}{}",
                Emoji("⚠ ", ""),
                style(message).red()
            ));
        },
        |message| {
            bar_progress.set_message(&format!("{}", style(message).blue()));
        },
    ) as u64;
    bar_progress.finish_and_clear();
    writer.flush().unwrap();

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
            style(format!(""))
        }
    );

    println!(
        "{} {}writing startup configuration…",
        style("[4/4]").bold().black(),
        Emoji("📃 ", "")
    );

    let info = StarterInfo {
        signature: [0x50, 0x45, 0x33, 0x44, 0x41, 0x54, 0x41, 0x00],
        show_console: args.show_console.into(),
        current_dir: args.current_dir.into(),
        verification,
        uid: Alphanumeric
            .sample_iter(thread_rng())
            .take(8)
            .collect::<String>()
            .as_bytes()
            .try_into()
            .unwrap(),
        unpack_target,
        versioning,
        unpack_directory,
        command,
        wrappe_format: WRAPPE_FORMAT,
    };
    writer.write_all(info.as_bytes()).unwrap();

    writer.flush().unwrap();
    println!("      {}{}", Emoji("✨ ", ""), style("done!").green());
}
