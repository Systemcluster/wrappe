# wrappe

Packer for creating self-contained single-binary applications from executables and directory trees.

## Features

* Packing of executables and their dependencies into single self-contained binaries
* Compression of packed payloads with Zstandard
* Streaming decompression with minimal memory overhead
* Compression and decompression of files in parallel
* Decompression only when necessary by checking existing files
* Automatic transfer of resources including icons and version information
* Platform support for Windows, macOS, Linux and more

## Usage

### Example

```shell
wrappe --compression 16 dist dist/diogenes.exe packed.exe
```

### Details

Running wrappe requires specifying the path to the input, the executable to run, and the name of the output executable.

`input` specifies the path to a directory or a file. `command` has to specify a file inside the input directory, or in case of an input file, the input file itself. `output` specifies a filename or path to a file. It will be overwritten if it already exists.

```text
wrappe [OPTIONS] <input> <command> <output>

Arguments:
    <input>      Path to the input directory
    <command>    Path to the executable to start after unpacking
    <output>     Path to or filename of the output executable

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
  -s, --version-string <SPECIFIER>
            Version string override [default: randomly generated]
  -i, --show-information <SHOW_INFORMATION>
            Information output details (title, verbose, none) [default: title]
  -n, --console <CONSOLE>
            Show or attach to a console window (auto, always, never) [default: auto]
  -w, --current-dir
            Set the current working directory of the target to the unpack directory
  -l, --list-runners
            Print available runners
  -h, --help
            Print help
  -V, --version
            Print version
```

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

* `auto`: Select the console behavior based on the subsystem of the input executable if available. If not available, it will be disabled for Windows runners, and enabled for all other runners.
* `always` Always attach to or open a console.
* `never`: Never open or attach to a console.

It defaults to `auto`. This option currently only affects Windows runners, other runners will always attach to a console if available.

#### current-dir

By default the working directory of the unpacked executable is set to the working directory of the runner executable. This flag changes the working directory to the unpack directory.

## Download

A snapshot build of the latest version can be found on the [release page](https://github.com/Systemcluster/wrappe/releases).

Snapshot builds contain runners for Windows (`x86_64-pc-windows-gnu`), macOS (`x86_64-apple-darwin` and `aarch64-apple-darwin`) and Linux (`x86_64-unknown-linux-musl`).

## Compilation

Compiling wrappe will also compile a runner for your current platform by default.

```shell
cargo build --release
```

To compile and include additional runners for other platforms, specify the desired [target triplets](https://doc.rust-lang.org/stable/rustc/targets/) in the `WRAPPE_TARGETS` environment variable.

```shell
WRAPPE_TARGETS=x86_64-unknown-linux-gnu;x86_64-pc-windows-msvc cargo build --release
```

Cross compilation of additional runners is performed through [cross](https://github.com/rust-embedded/cross) if available.
To disable compilation through cross, set the `WRAPPE_NO_CROSS` environment variable to `true`.
