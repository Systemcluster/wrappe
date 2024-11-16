# wrappe

[![Release](https://img.shields.io/github/release/Systemcluster/wrappe)](https://github.com/Systemcluster/wrappe/releases)
[![Crates.io](https://img.shields.io/crates/v/wrappe)](https://crates.io/crates/wrappe)
[![Tests & Checks](https://img.shields.io/github/actions/workflow/status/Systemcluster/wrappe/tests.yml?label=tests%20%26%20checks)](https://github.com/Systemcluster/wrappe/actions/workflows/tests.yml)

**Packer for creating self-contained single-binary applications from executables and directories.**

## Features

* Packing of executables and their dependencies into single self-contained binaries
* Compression of packed payloads with Zstandard
* Streaming decompression with minimal memory overhead
* Compression and decompression of files in parallel
* Decompression only when necessary by checking existing files
* Automatic transfer of resources including icons and version information
* Platform support for Windows, macOS, Linux and more

With wrappe you can distribute your application and its files as a single executable without the need for an installer, while resulting in a smaller file size and faster startup than many alternatives.

## Usage

### Download

A snapshot build of the latest version can be found on the [release page](https://github.com/Systemcluster/wrappe/releases).

Snapshot builds contain runners for Windows (`x86_64-pc-windows-gnu`), macOS (`x86_64-apple-darwin` and `aarch64-apple-darwin`) and Linux (`x86_64-unknown-linux-musl`), allowing packing for these platforms without additional setup.

Alternatively wrappe can be installed with `cargo`, see the [compilation](#compilation) section for more info on how to compile wrappe with additional runners for other platforms.

### Example

```shell
wrappe --compression 16 app app/diogenes.exe packed.exe
```

### Details

Run `wrappe` with an `input` directory, the `command` to launch and  the `output` filename to create a single-binary executable.
The input directory and all contained files and links will be packed. The command must be an executable file within the input directory that should be launched after unpacking.

```text
wrappe [OPTIONS] <input> <command> [output] [-- <ARGUMENTS>...]

Arguments:
  <input>         Path to the input directory
  <command>       Path to the executable to start after unpacking
  [output]        Path to or filename of the output executable
  [ARGUMENTS]...  Command line arguments to pass to the executable

Options:
  -r, --runner <RUNNER>
        Platform to pack for (see --list-runners for available options) [default: native]
  -c, --compression <COMPRESSION>
        Zstd compression level (0-22) [default: 8]
  -t, --unpack-target <UNPACK_TARGET>
        Unpack directory target (temp, local, cwd) [default: temp]
  -d, --unpack-directory <UNPACK_DIRECTORY>
        Unpack directory name [default: inferred from input directory]
  -v, --versioning <VERSIONING>
        Versioning strategy (sidebyside, replace, none) [default: sidebyside]
  -e, --verification <VERIFICATION>
        Verification of existing unpacked data (existence, checksum, none) [default: existence]
  -s, --version-string <VERSION_STRING>
        Version string override [default: randomly generated]
  -i, --show-information <SHOW_INFORMATION>
        Information output details (title, verbose, none) [default: title]
  -n, --console <CONSOLE>
        Show or attach to a console window (auto, always, never, attach) [default: auto]
  -w, --current-dir <CURRENT_DIR>
        Working directory of the command (inherit, unpack, runner, command) [default: inherit]
  -u, --cleanup
        Cleanup the unpack directory after exit
  -o, --once
        Allow only one running instance
  -z, --build-dictionary
        Build compression dictionary
  -l, --list-runners
        Print available runners
  -h, --help
        Print help
  -V, --version
        Print version
```

Additional arguments for the packed executable can be specified after `--` and will automatically be passed to the command when launched.

If the packed executable needs to access packed files by relative path and expects a certain working directory, use the [`--current-dir`](#current-dir) option to set it to its parent directory or the unpack directory. The `WRAPPE_UNPACK_DIR` and `WRAPPE_LAUNCH_DIR` environment variables will always be set for the command with the paths to the unpack directory and the inherited working directory.

Packed Windows executables will have their subsystem, icons and other resources automatically transferred to the output executable through [editpe](https://github.com/Systemcluster/editpe).

### Options

The packing and unpacking behavior is highly customizable. The default options are suitable for most use cases, but can be adjusted to fit specific requirements.

#### runner

This option specifies which runner will be used for the output executable. The runner is the pre-built executable that unpacks the payload and starts the packed command.
Partial matches are accepted if unambiguous, for instance `windows` will be accepted if only one runner for Windows is available.

It defaults to the native runner for the current platform. Additional runners have to be included at compile time, see the compilation section for more info.

#### compression

This option controls the Zstandard compression level. Accepted values range from `0` to `22`. Higher compression levels will result in smaller output files, but will also increase the packing time.

It defaults to `8`.

#### unpack-target

This option specifies the directory the packed files are unpacked to. Accepted values are:

* `temp`: The files will be unpacked to the systems temporary directory.
* `local`: The files will be unpacked to the local data directory, usually `User/AppData/Local` on Windows and `/home/user/.local/share` on Linux.
* `cwd`: The files will be unpacked to the working directory of the runner executable.

It defaults to `temp`.

#### unpack-directory

This option specifies the unpack directory name inside the [`unpack-target`](#unpack-target). It defaults to the name of the input file or directory.

#### versioning

This option specifies the versioning strategy. Accepted values are:

* `sidebyside`: An individual directory will be created for every version. An already unpacked version will not be unpacked again.
* `replace`: Already unpacked files from a different version will be overwritten. Unpacked files from the same version will not be upacked again.
* `none`: Packed files are always unpacked and already unpacked files will be overwritten.

It defaults to `sidebyside`. The version is determined by a unique identifier generated during the packing process or specified with the [`version-string`](#version-string) option.

Using `replace` or `none` might cause unpacking to fail if another instance of the packed executable is already running unless the [`once`](#once) option is set.

#### verification

This option specifies the verification of the unpacked payload before skipping extraction. Accepted values are:

* `existence`: All files in the payload will be checked for existence.
* `checksum`: A checksum for all files will be calculated and compared with the checksum calculated during the packing process.
* `none`: No verification will be performed. Unpacking will be skipped if the unpack directory exists and was created with the same version string.

It defaults to `existence`. This option has no effect when [`versioning`](#versioning) is set to `none`.

#### version-string

This option specifies the version string. It defaults to a randomly generated string of 8 characters.

#### show-information

This option controls the information output of the runner. Accepted values are:

* `title`: The runner will output the `wrappe` version and the unpack directory.
* `verbose`: The runner will output various additional details like unpack status, configuration and payload size.
* `none`: The runner will show no additional output.

It defaults to `title`. Error information is always shown when applicable. Windows runners using the GUI subsystem will only show information output when launched from a console and this option is set to `verbose`, or a console is attached or opened through the [`console`](#console) option.

#### console

This option controls if the runner should attach to a console or if a console window should be opened when launching a Windows application from the Windows explorer. Accepted values are:

* `auto`: Select the console behavior based on the subsystem of the input executable if available. If not available, it will fall back to `never` for Windows runners, and `always` for all other runners.
* `always` Always attach to or open a console. The runner will block the console until the packed executable exits.
* `never`: Never open or attach to a console. The runner will immediately exit after launching the packed executable.
* `attach`: Never open a new console window, but attach to an existing console if available. The runner will unblock the console immediately, but output will still be shown.

It defaults to `auto`. This option currently only affects Windows runners, other runners will always attach to a console if available. This option will also not prevent packed Windows command line applications from opening a console on their own when launched from the Windows explorer.

#### current-dir

This option changes the working directory of the packed executable. Accepted values are:

* `inherit`: The working directory will be inherited from the runner. This is usually the directory containing the runner or the directory from which the runner was launched.
* `unpack`: The working directory will be set to the unpack directory. This is the top-level directory of the unpacked payload.
* `runner`: The working directory will be set to the directory containing the runner, with all symbolic links resolved.
* `command`: The working directory will be set to the directory containing the unpacked executable. This will either be the unpack directory or a subdirectory within the unpacked payload.

It defaults to `inherit`.

#### cleanup

This option controls if the unpacking directory should be deleted after exiting the packed executable.

It can also be set at runtime by setting the `STARTPE_CLEANUP` environment variable to `1`.

#### once

This option prevents multiple instances of the packed executable from running at the same time. When set, the runner will check for running processes on the system and will exit immediately if a running instance of the executable is found during startup.

This option currently only affects Windows and Linux runners. On Windows, if the packed executable is a GUI application, the runner will bring its window into the foreground and activate it.

#### build-dictionary

This option builds a zstandard compression dictionary from the input files and stores it in the output executable. This can improve the compression ratio when many small and similar files are packed.

At least 8 input files are required to build a dictionary, and at most 128 KB of data from each input file will be sampled.

Building a dictionary can increase the packing time and can in some cases negatively affect the compression ratio. It is recommended to test the results with and without this option to determine whether it is beneficial for the specific use case.

## Performance

Wrappe is optimized for compression ratio and decompression speed, generally matching or outperforming other packers in terms of both. It uses a custom metadata format designed for parallel iteration and decompression and compact storage of file information. Packed files are concurrently decompressed from the memory-mapped executable directly to disk, while extraction is skipped when the files are already unpacked to enable fast startup of packed executables with minimal overhead.

> As an example, a 400 MB PyInstaller one-directory output with 1500 files packed with wrappe at maximum compression level results in a 100 MB executable that unpacks and starts in around 500 milliseconds on a modern Windows system on the first run and instantly on subsequent runs. This is around 50% faster and only 5% larger than the same project packed by PyInstaller in one-file mode with UPX compression, which unpacks and loads into memory on every run.

Generally, on a reasonably modern system, the decompression speed of wrappe is limited by the read and write speed of the system and storage medium.

## Compilation

Compiling wrappe will also compile a runner for your current platform by default.

```shell
cargo install wrappe
```

To compile and include additional runners for other platforms, specify the desired [target triples](https://doc.rust-lang.org/stable/rustc/platform-support.html) in the `WRAPPE_TARGETS` environment variable.

```shell
WRAPPE_TARGETS=x86_64-unknown-linux-gnu;x86_64-pc-windows-msvc cargo install wrappe
```

Target-specific [rustflags](https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags) for runners can be configured through the `WRAPPE_TARGET_RUSTFLAGS_{target triple}` environment variable.

### Cross Compilation

Additional targets need to be available to `cargo` for cross compilation. Targets can be installed with `rustup`, for example `rustup target add x86_64-unknown-linux-musl`.

Some cross compilation targets require certain `AR`, `CC` and `CXX` environment variables to be set. Target-specific `AR`, `CC` and `CXX` can be configured through the `WRAPPE_TARGET_{AR|CC|CXX}_{target triple}` environment variables.

Cross compilation of additional runners can alternatively be performed through [cross](https://github.com/rust-embedded/cross) when available and the `WRAPPE_USE_CROSS` environment variable is set to `true`.

When including runners for multiple macOS targets, the `WRAPPE_MACOS_UNIVERSAL` environment variable can be set to a list of targets to build a universal runner with `lipo` containing the specified architectures, for example `x86_64-apple-darwin;aarch64-apple-darwin`. This runner will be included as `universal-apple-darwin`.
