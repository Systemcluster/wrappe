use std::{
    fs::{create_dir_all, read_link, remove_dir, remove_file, File},
    hash::Hasher,
    io::{copy, sink, Cursor, Read, Result},
    mem::size_of,
    path::{Path, PathBuf},
    thread::sleep,
    time::Duration,
};

use filetime::{set_file_times, set_symlink_file_times, FileTime};
use fslock::LockFile;
use rayon::prelude::*;
use twox_hash::XxHash64;
use zerocopy::LayoutVerified;
use zstd::stream::copy_decode;

use crate::types::*;

use crate::versioning::*;

pub const HASH_SEED: u64 = 1246736989840;
pub const LOCK_FILE: &str = "._wrappe_lock_";

pub struct HashReader<R: Read, H: Hasher> {
    reader: R,
    hasher: H,
}
impl<R: Read, H: Hasher> HashReader<R, H> {
    pub fn new(reader: R, hasher: H) -> Self {
        HashReader { reader, hasher }
    }

    pub fn finish(self) -> u64 {
        self.hasher.finish()
    }
}
impl<R: Read, H: Hasher> Read for HashReader<R, H> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let bytes = self.reader.read(buf)?;
        if bytes > 0 {
            self.hasher.write(&buf[0..bytes]);
        }
        Ok(bytes)
    }
}

/// Decompress the payload and section data in `mmap` into `unpack_dir`.
/// The data is expected to be in the following order at the end of `mmap`:
/// - compressed file contents
/// - directory sections
/// - file section headers
/// - symlink sections
/// - payload section header
pub fn decompress(
    mmap: &[u8], unpack_dir: &Path, verification: u8, mut should_extract: bool, version: &str,
    show_information: u8,
) -> bool {
    // read payload header sections
    let payload_header_start = mmap.len() - size_of::<PayloadHeader>();
    let payload_header = LayoutVerified::<_, PayloadHeader>::new(&mmap[payload_header_start..])
        .expect("couldn't read payload header")
        .into_ref();

    let directory_sections = payload_header.directory_sections;
    let file_sections = payload_header.file_sections;
    let symlink_sections = payload_header.symlink_sections;
    let payload_size = payload_header.payload_size;
    if show_information >= 2 {
        println!(
            "payload: {} directories, {} files, {} symlinks ({} total)",
            directory_sections,
            file_sections,
            symlink_sections,
            payload_header.len()
        );
        println!("payload size: {}", payload_size);
    }

    let symlink_sections_start =
        payload_header_start - symlink_sections * size_of::<SymlinkSection>();
    let file_sections_start =
        symlink_sections_start - file_sections * size_of::<FileSectionHeader>();
    let directory_sections_start =
        file_sections_start - directory_sections * size_of::<DirectorySection>();

    let mut section_hasher = XxHash64::with_seed(HASH_SEED);

    if show_information >= 2 {
        println!("reading sections...");
    }
    let directories = mmap[directory_sections_start..file_sections_start]
        .chunks(size_of::<DirectorySection>())
        .enumerate()
        .fold(
            // start with the unpack directory as parent 0
            Vec::<PathBuf>::from([PathBuf::from("")]),
            |mut directories, (i, section)| {
                let section_start = directory_sections_start + i * size_of::<DirectorySection>();
                section_hasher.write(section);
                let section = LayoutVerified::<_, DirectorySection>::new(
                    &mmap[section_start..section_start + size_of::<DirectorySection>()],
                )
                .expect("couldn't read payload header")
                .into_ref();

                directories.push(
                    directories[section.parent as usize].join(
                        std::str::from_utf8(
                            &section.name[0..(section
                                .name
                                .iter()
                                .position(|&c| c == b'\0')
                                .unwrap_or(section.name.len()))],
                        )
                        .unwrap(),
                    ),
                );
                directories
            },
        );
    let files = mmap[file_sections_start..symlink_sections_start]
        .chunks(size_of::<FileSectionHeader>())
        .enumerate()
        .map(|(i, section)| {
            let section_start = file_sections_start + i * size_of::<FileSectionHeader>();
            section_hasher.write(section);
            let section = LayoutVerified::<_, FileSectionHeader>::new(
                &mmap[section_start..section_start + size_of::<FileSectionHeader>()],
            )
            .expect("couldn't read payload header")
            .into_ref();
            (
                section,
                std::str::from_utf8(
                    &section.name[0..(section
                        .name
                        .iter()
                        .position(|&c| c == b'\0')
                        .unwrap_or(section.name.len()))],
                )
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let symlinks = mmap[symlink_sections_start..payload_header_start]
        .chunks(size_of::<SymlinkSection>())
        .enumerate()
        .map(|(i, section)| {
            let section_start = symlink_sections_start + i * size_of::<SymlinkSection>();
            section_hasher.write(section);
            let section = LayoutVerified::<_, SymlinkSection>::new(
                &mmap[section_start..section_start + size_of::<SymlinkSection>()],
            )
            .expect("couldn't read payload header")
            .into_ref();
            (
                section,
                std::str::from_utf8(
                    &section.name[0..(section
                        .name
                        .iter()
                        .position(|&c| c == b'\0')
                        .unwrap_or(section.name.len()))],
                )
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();

    let section_hash = section_hasher.finish();
    if section_hash != payload_header.section_hash {
        let expected = payload_header.section_hash;
        panic!(
            "section hash ({}) differs from expected section hash ({})",
            section_hash, expected
        );
    }

    create_dir_all(unpack_dir)
        .unwrap_or_else(|e| panic!("couldn't create directory {}: {}", unpack_dir.display(), e));

    let mut lockfile = LockFile::open(&unpack_dir.join(LOCK_FILE)).unwrap();
    lockfile.lock().unwrap();

    // verify files
    if verification > 0 && !should_extract && file_sections > 0 {
        if show_information >= 2 {
            println!("verifying files...");
        }
        should_extract = !files.par_iter().all(|(file, file_name)| {
            let path = unpack_dir
                .join(&directories[file.parent as usize])
                .join(&file_name);
            if !path.is_file() {
                eprintln!("verification failed: not a file: {}", path.display());
                return false;
            }
            if verification == 2 {
                // verify checksums
                let target = File::open(&path);
                if target.is_err() {
                    eprintln!(
                        "verification failed: couldn't open file: {}",
                        path.display()
                    );
                    return false;
                }
                let target = target.unwrap();
                let mut hasher = XxHash64::with_seed(HASH_SEED);
                let mut reader = HashReader::new(&target, &mut hasher);
                if copy(&mut reader, &mut sink()).is_err() {
                    eprintln!(
                        "verification failed: couldn't read file: {}",
                        path.display()
                    );
                    return false;
                };
                let file_hash = hasher.finish();
                if file_hash != file.file_hash {
                    let expected = file.file_hash;
                    eprintln!(
                        "verification failed: file hash ({}) differs from expected file hash ({}): {}",
                        file_hash,
                        expected,
                        path.display()
                    );
                    return false;
                }
            }
            true
        });
    }

    // verify symlinks
    if verification > 0 && !should_extract && symlink_sections > 0 {
        if show_information >= 2 {
            println!("verifying symlinks...");
        }
        should_extract = !symlinks.par_iter().all(|(symlink, symlink_name)| {
            let path = unpack_dir
                .join(&directories[symlink.parent as usize])
                .join(&symlink_name);
            let link = read_link(&path);
            if link.is_err() {
                eprintln!(
                    "verification failed: not a valid symlink: {}",
                    path.display()
                );
                return false;
            }
            let link = link.unwrap();
            if !link.starts_with(unpack_dir) {
                eprintln!(
                    "verification failed: symlink points to target outside the target directory: {}",
                    path.display()
                );
                return false;
            }
            // directory symlink
            if symlink.kind == 0 {
                let target = unpack_dir.join(&directories[symlink.target as usize]);
                if link != target
                {
                    eprintln!(
                        "verification failed: symlink points to wrong target: {} (expected: {})",
                        target.display(),
                        link.display(),
                    );
                    return false;
                }
            }
            // file symlink
            if symlink.kind == 1 {
                let (file, file_name) = files[symlink.target as usize];
                let target = unpack_dir
                    .join(&directories[file.parent as usize])
                    .join(&file_name);
                if target != link
                {
                    eprintln!(
                        "verification failed: symlink points to wrong target: {} (expected: {})",
                        target.display(),
                        link.display(),
                    );
                    return false;
                }
            }
            true
        });
    }

    if should_extract {
        // create directories
        if show_information >= 2 {
            println!("creating directories...");
        }
        directories.iter().for_each(|directory| {
            let path = unpack_dir.join(&directory);
            create_dir_all(&path).unwrap_or_else(|e| {
                panic!("couldn't create directory {}: {}", path.display(), e);
            });
        });

        // unpack files
        if show_information >= 2 {
            println!("unpacking...");
        }
        let files_start = directory_sections_start as u64 - payload_size;
        files.par_iter().for_each(|(file, file_name)| {
            let path = unpack_dir
                .join(&directories[file.parent as usize])
                .join(&file_name);
            let content = &mmap[(files_start + file.position) as usize
                ..(files_start + file.position + file.size) as usize];
            let mut reader = HashReader::new(Cursor::new(&content), XxHash64::with_seed(HASH_SEED));
            let mut output = File::create(&path).unwrap();
            copy_decode(&mut reader, &mut output)
                .unwrap_or_else(|e| panic!("failed to unpack file {}: {}", path.display(), e));
            let compressed_hash = reader.finish();
            if file.compressed_hash != compressed_hash {
                let expected = file.compressed_hash;
                panic!(
                    "file hash ({}) differs from expected file hash ({}) for {}",
                    compressed_hash,
                    expected,
                    path.display()
                );
            }
            #[cfg(windows)]
            {
                use ::std::fs::{metadata, set_permissions};
                let meta = metadata(&path);
                if let Ok(ref meta) = meta {
                    let read = file.readonly != 0;
                    let mut perm = meta.permissions();
                    perm.set_readonly(read);
                    set_permissions(&path, perm).unwrap_or_else(|e| {
                        eprintln!("failed to set permissions for {}: {}", path.display(), e)
                    });
                }
            }
            #[cfg(any(unix, target_os = "redox"))]
            {
                use ::std::{
                    fs::{set_permissions, Permissions},
                    os::unix::prelude::*,
                };
                let mode = file.mode;
                let mut perm: Permissions = PermissionsExt::from_mode(mode as u32);
                let read = file.readonly != 0;
                perm.set_readonly(read);
                set_permissions(&path, perm).unwrap_or_else(|e| {
                    eprintln!("failed to set permissions for {}: {}", path.display(), e)
                });
            }
            set_file_times(
                &path,
                FileTime::from_unix_time(
                    file.time_accessed_seconds as i64,
                    file.time_accessed_nanos as u32,
                ),
                FileTime::from_unix_time(
                    file.time_modified_seconds as i64,
                    file.time_modified_nanos as u32,
                ),
            )
            .unwrap_or_else(|e| println!("failed to set file times for {}: {}", path.display(), e));
        });

        // create symlinks
        #[cfg(not(any(windows, unix, target_os = "redox")))]
        {
            eprintln!("skipping symlink creation on unsupported platform");
        }
        #[cfg(any(windows, unix, target_os = "redox"))]
        {
            if show_information >= 2 {
                println!("creating symlinks...");
            }
            symlinks.par_iter().for_each(|(symlink, symlink_name)| {
                let path = unpack_dir
                    .join(&directories[symlink.parent as usize])
                    .join(&symlink_name);
                // directory symlink
                if symlink.kind == 0 {
                    if path.exists() {
                        remove_dir(&path).unwrap_or_else(|e| {
                            panic!(
                                "failed to remove existing symlink {}: {}",
                                path.display(),
                                e
                            )
                        });
                    }
                    while path.exists() {
                        sleep(Duration::from_millis(20));
                    }
                    let target = unpack_dir.join(&directories[symlink.target as usize]);
                    #[cfg(windows)]
                    {
                        use ::std::os::windows::fs::symlink_dir;
                        symlink_dir(&target, &path).unwrap_or_else(|e| {
                            panic!("failed to create symlink {}: {}", path.display(), e)
                        });
                    }
                    #[cfg(any(unix, target_os = "redox"))]
                    {
                        use ::std::os::unix::fs::symlink;
                        symlink(&target, &path).unwrap_or_else(|e| {
                            panic!("failed to create symlink {}: {}", path.display(), e)
                        });
                    }
                }
                // file symlink
                if symlink.kind == 1 {
                    if path.exists() {
                        remove_file(&path).unwrap_or_else(|e| {
                            panic!(
                                "failed to remove existing symlink {}: {}",
                                path.display(),
                                e
                            )
                        });
                    }
                    while path.exists() {
                        sleep(Duration::from_millis(20));
                    }
                    let (file, file_name) = files[symlink.target as usize];
                    let target = unpack_dir
                        .join(&directories[file.parent as usize])
                        .join(&file_name);
                    #[cfg(windows)]
                    {
                        use ::std::os::windows::fs::symlink_file;
                        symlink_file(&target, &path).unwrap_or_else(|e| {
                            panic!("failed to create symlink {}: {}", path.display(), e)
                        });
                    }
                    #[cfg(any(unix, target_os = "redox"))]
                    {
                        use ::std::os::unix::fs::symlink;
                        symlink(&target, &path).unwrap_or_else(|e| {
                            panic!("failed to create symlink {}: {}", path.display(), e)
                        });
                    }
                    set_symlink_file_times(
                        &path,
                        FileTime::from_unix_time(
                            symlink.time_accessed_seconds as i64,
                            symlink.time_accessed_nanos as u32,
                        ),
                        FileTime::from_unix_time(
                            symlink.time_modified_seconds as i64,
                            symlink.time_modified_nanos as u32,
                        ),
                    )
                    .unwrap_or_else(|e| {
                        eprintln!("failed to set file times for {}: {}", path.display(), e)
                    });
                }
            });
        }

        set_version(unpack_dir, version);
    }

    should_extract
}
