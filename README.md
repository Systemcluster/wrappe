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

Snapshot builds contain runners for Windows (`x86_64-pc-windows-gnu`), macOS (`x86_64-apple-darwin` and `aarch64-apple-darwin`) and Linux (`x86_64-unknown-linux-musl`).

Alternatively wrappe can be installed with `cargo`, see the [compilation](#compilation) section for more info on how to compile wrappe with additional runners.

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

#### runner

This option specifies which runner will be used for the output executable. It defaults to the native runner for the current platform. Additional runners have to be included at compile time, see the compilation section for more info.

Partial matches are accepted if unambiguous, for instance `windows` will be accepted if only one runner for Windows is available.

#### compression

This option controls the Zstandard compression level. Accepted values range from `0` to `22`.

#### unpack-target

This option specifies the directory the packed files are unpacked to. Accepted values are:

* `temp`: The files will be unpacked to the systems temporary directory.
* `local`: The files will be unpacked to the local data directory, usually `User/AppData/Local` on Windows and `/home/user/.local/share` on Linux.
* `cwd`: The files will be unpacked to the working directory of the runner executable.

It defaults to `temp`.

#### unpack-directory

This option specifies the unpack directory name inside the `unpack-target`. It defaults to the name of the input file or directory.

#### versioning

This option specifies the versioning strategy. Accepted values are:

* `sidebyside`: An individual directory will be created for every version. The version is determined by a unique identifier created during the packing process, so different runner executables will be unpacked to different directories, unless manually specified. An already unpacked version will not be unpacked again.
* `replace`: Already unpacked files from a different version will be overwritten. Unpacked files from the same version will not be upacked again.
* `none`: Packed files are always unpacked and already unpacked files will be overwritten.

It defaults to `sidebyside`.

#### verification

This option specifies the verification of the unpacked payload before skipping extraction. Accepted values are:

* `existence`: All files in the payload will be checked for existence.
* `checksum`: A checksum for all files will be calculated and compared with the checksum calculated during the packing process.
* `none`: No verification will be performed.

It defaults to `existence`.

#### version-string

This option specifies the version string. It defaults to a randomly generated string of 8 characters.

#### show-information

This option controls the information output of the runner. Accepted values are:

* `title`: The runner will output the `wrappe` version and the unpack directory.
* `verbose`: The runner will output various additional details like unpack status, configuration and payload size.
* `none`: The runner will show no additional output.

It defaults to `title`. Error information is always shown when applicable.

#### show-console

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

#### build-dictionary

This option builds a zstandard compression dictionary from the input files and stores it in the output executable. This can improve the compression ratio when many small and similar files are packed.

At least 8 input files are required to build a dictionary, and at most 128 KB of data from each input file will be sampled.

Building a dictionary can increase the packing time and can in some cases negatively affect the compression ratio. It is recommended to test the results with and without this option to determine whether it is beneficial for the specific use case.

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

Cross compilation of additional runners can be performed through [cross](https://github.com/rust-embedded/cross) when available and the `WRAPPE_USE_CROSS` environment variable is set to `true`.

Some cross compilation targets require certain `AR`, `CC` and `CXX` environment variables to be set. Target-specific `AR`, `CC` and `CXX` can be configured through the `WRAPPE_TARGET_{AR|CC|CXX}_{target triple}` environment variables.
