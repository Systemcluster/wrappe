# wrappe

Packer to create self-contained single-binary applications from executables and directory trees.

## Features

* Packing of executables and their dependencies into single self-contained binaries
* Compression of packed payloads with Zstandard
* Streaming decompression with minimal memory overhead
* Compression and decompression of files in parallel
* Decompression only when necessary by checking existing files

## Usage

### Example

```shell
wrappe --compression 16 dist dist/diogenes.exe packed.exe
```

### Details

Running wrappe requires specifying the path to the input, the executable to run, and the name of the output executable.

`input` specifies the path to a directory or a file. `command` has to specify a file inside the input directory, or in case of an input file, the input file itself. `output` specifies a filename or path to a file. It will be overwritten if it already exists.

```shell
wrappe [FLAGS] [OPTIONS] <input> <command> <output>

ARGS:
    <input>      Path to the input directory
    <command>    Path to the executable to start after unpacking
    <output>     Path to or filename of the output executable

FLAGS:
    -w, --current-dir     Set the current dir of the target to the unpack directory
    -h, --help            Prints help information
    -l, --list-runners    Prints available runners
    -s, --show-console    Open a console when starting the runner on Windows
    -V, --version         Prints version information

OPTIONS:
    -c, --compression <compression>
            Zstd compression level (0-21) [default: 8]
    -r, --runner <runner>
            Which runner to use [default: native]
    -d, --unpack-directory <unpack-directory>
            Unpack directory name [default: inferred from input directory]
    -t, --unpack-target <unpack-target>
            Unpack directory target (temp, local, cwd) [default: temp]
    -e, --verification <verification>
            Verification of existing unpacked data (existence, checksum, none) [default: existence]
    -v, --versioning <versioning>
            Versioning strategy (sidebyside, replace, none) [default: sidebyside]
    -V, --version <version>
            Version specifier override [default: randomly generated]
    -i, --show-information <show-information>
            Information output details (title, verbose, none) [default: title]
```

### Flags and Options

#### current-dir

By default the working directory of the unpacked executable is set to the working directory of the runner executable. This flag changes the working directory to the unpack directory.

#### show-console

On Windows, the runner executable is compiled for the [windows-subsystem](https://rust-lang.github.io/rfcs/1665-windows-subsystem.html) and runs without creating a console window. If this flag is set, a console will be created unconditionally.

Without this flag the runner executable will still attach to a console if started from one.

#### compression

This option controls the Zstandard compression level. Accepted values range from `0` to `21`.

#### runner

This option specifies which runner will be used for the output executable. It defaults to the native runner for the current platform. Additional runners have to be included at compile time, see the compilation section for more info.

Partial matches are accepted if unambiguous, for instance `windows` will be accepted if only one runner for Windows is available.

#### unpack-directory

This option specifies the unpack directory name inside the `unpack-target`. It defaults to the name of the input file or directory.

#### unpack-target

This option specifies the directory the packed files are unpacked to. Accepted values are:

* `temp`: The files will be unpacked to the systems temporary directory.
* `local`: The files will be unpacked to the local data directory, usually `User/AppData/Local` on Windows and `/home/user/.local/share` on Linux.
* `cwd`: The files will be unpacked to the working directory of the runner executable.

It defaults to `temp`.

#### verification

This option specifies the verification of the unpacked payload before skipping extraction. Accepted values are:

* `existence`: All files in the payload will be checked for existence.
* `checksum`: A checksum for all files will be calculated and compared with the checksum calculated during the packing process.
* `none`: No verification will be performed.

It defaults to `existence`.

#### versioning

This option specifies the versioning strategy. Accepted values are:

* `sidebyside`: An individual directory will be created for every version. The version is determined by a unique identifier created during the packing process, so different runner executables will be unpacked to different directories, unless manually specified. An already unpacked version will not be unpacked again.
* `replace`: Already unpacked files from a different version will be overwritten. Unpacked files from the same version will not be upacked again.
* `none`: Packed files are always unpacked and already unpacked files will be overwritten.

It defaults to `sidebyside`.

#### version

This option specifies the version string. It defaults to a randomly generated string of 8 characters.

#### show-information

This option controls the information output of the runner. Accepted values are:

* `title`: The runner will output the `wrappe` version and the unpack directory.
* `verbose`: The runner will output various additional details like unpack status, configuration and payload size.
* `none`: The runner will show no additional output.

It defaults to `title`. Error information is always shown when applicable.

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
