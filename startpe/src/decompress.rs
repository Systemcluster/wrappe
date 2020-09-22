use std::{
    fs::{create_dir_all, read_link, File},
    hash::Hasher,
    io::{copy, sink, Cursor, Read, Result},
    mem::size_of,
    path::{Path, PathBuf},
};

use filetime::{set_file_times, set_symlink_file_times, FileTime};
use fslock::LockFile;
use minilz4::Decoder;
use rayon::prelude::*;
use twox_hash::XxHash64;
use zerocopy::LayoutVerified;

use crate::types::*;

use crate::versioning::*;

pub const HASH_SEED: u64 = 1246736989840;
pub const LOCK_FILE: &str = "._wrappe_lock_";

pub struct HashReader<R: Read, H: Hasher> {
    reader: R,
    hasher: H,
}
impl<R: Read, H: Hasher> HashReader<R, H> {
    pub fn new(reader: R, hasher: H) -> Self { HashReader { reader, hasher } }

    pub fn finish(self) -> u64 { self.hasher.finish() }
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

pub fn decompress(
    mmap: &[u8], end: usize, unpack_dir: &Path, verification: u8, mut should_extract: bool,
    version: &str,
) {
    // parse payload information
    let payload_header_start = end - size_of::<PayloadHeader>();
    let payload_header = LayoutVerified::<_, PayloadHeader>::new(&mmap[payload_header_start..end])
        .expect("couldn't read payload header")
        .into_ref();

    let directory_sections = payload_header.directory_sections;
    let file_sections = payload_header.file_sections;
    let symlink_sections = payload_header.symlink_sections;
    let payload_size = payload_header.payload_size;
    println!(
        "payload: {} directories, {} files, {} symlinks ({} total)",
        directory_sections,
        file_sections,
        symlink_sections,
        payload_header.len()
    );
    println!("payload size: {}", payload_size);

    let symlink_sections_start =
        payload_header_start - symlink_sections * size_of::<SymlinkSection>();
    let file_sections_start =
        symlink_sections_start - file_sections * size_of::<FileSectionHeader>();
    let directory_sections_start =
        file_sections_start - directory_sections * size_of::<DirectorySection>();

    let mut section_hasher = XxHash64::with_seed(HASH_SEED);

    let mut directories = Vec::<PathBuf>::from([PathBuf::from("")]);
    let mut file_names = Vec::<&str>::new();
    let mut files = Vec::<&FileSectionHeader>::new();
    let mut symlink_names = Vec::<&str>::new();
    let mut symlinks = Vec::<&SymlinkSection>::new();

    println!("reading sections...");
    // read directory sections
    for (i, section) in mmap[directory_sections_start..file_sections_start]
        .chunks(size_of::<DirectorySection>())
        .enumerate()
    {
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
    }
    // read file sections
    for (i, section) in mmap[file_sections_start..symlink_sections_start]
        .chunks(size_of::<FileSectionHeader>())
        .enumerate()
    {
        let section_start = file_sections_start + i * size_of::<FileSectionHeader>();
        section_hasher.write(section);
        let section = LayoutVerified::<_, FileSectionHeader>::new(
            &mmap[section_start..section_start + size_of::<FileSectionHeader>()],
        )
        .expect("couldn't read payload header")
        .into_ref();
        file_names.push(
            std::str::from_utf8(
                &section.name[0..(section
                    .name
                    .iter()
                    .position(|&c| c == b'\0')
                    .unwrap_or(section.name.len()))],
            )
            .unwrap(),
        );
        files.push(section);
    }
    // read symlink sections
    for (i, section) in mmap[symlink_sections_start..payload_header_start]
        .chunks(size_of::<SymlinkSection>())
        .enumerate()
    {
        let section_start = symlink_sections_start + i * size_of::<SymlinkSection>();
        section_hasher.write(section);
        let section = LayoutVerified::<_, SymlinkSection>::new(
            &mmap[section_start..section_start + size_of::<SymlinkSection>()],
        )
        .expect("couldn't read payload header")
        .into_ref();
        symlink_names.push(
            std::str::from_utf8(
                &section.name[0..(section
                    .name
                    .iter()
                    .position(|&c| c == b'\0')
                    .unwrap_or(section.name.len()))],
            )
            .unwrap(),
        );
        symlinks.push(section);
    }

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
    if verification > 0 && !should_extract {
        println!("verifying files...");
        should_extract = !files.par_iter().enumerate().all(|(i, file)| {
            let path = unpack_dir
                .join(&directories[file.parent as usize])
                .join(&file_names[i]);
            if !path.is_file() {
                println!("verification failed: not a file: {}", path.display());
                return false;
            }
            if verification == 2 {
                // verify checksums
                let target = File::open(&path);
                if target.is_err() {
                    println!(
                        "verification failed: couldn't open file: {}",
                        path.display()
                    );
                    return false;
                }
                let target = target.unwrap();
                let mut hasher = XxHash64::with_seed(HASH_SEED);
                let mut reader = HashReader::new(&target, &mut hasher);
                if copy(&mut reader, &mut sink()).is_err() {
                    println!(
                        "verification failed: couldn't read file: {}",
                        path.display()
                    );
                    return false;
                };
                let file_hash = hasher.finish();
                if file_hash != file.file_hash {
                    let expected = file.file_hash;
                    println!(
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
    if verification > 0 && !should_extract {
        println!("verifying symlinks...");
        should_extract = !symlinks.par_iter().enumerate().all(|(i, symlink)| {
            let path = unpack_dir
                .join(&directories[symlink.parent as usize])
                .join(&symlink_names[i]);
            let link = read_link(&path);
            if link.is_err() {
                println!(
                    "verification failed: not a valid symlink: {}",
                    path.display()
                );
                return false;
            }
            true
        });
    }

    if should_extract {
        // create directories
        println!("creating directories...");
        directories.iter().for_each(|directory| {
            let path = unpack_dir.join(&directory);
            create_dir_all(&path).unwrap_or_else(|e| {
                panic!("couldn't create directory {}: {}", path.display(), e);
            });
        });

        // unpack files
        println!("unpacking...");
        let files_start = directory_sections_start as u64 - payload_size;
        files.par_iter().enumerate().for_each(|(i, file)| {
            let path = unpack_dir
                .join(&directories[file.parent as usize])
                .join(&file_names[i]);
            let content = &mmap[(files_start + file.position) as usize
                ..(files_start + file.position + file.size) as usize];
            let mut reader = HashReader::new(Cursor::new(&content), XxHash64::with_seed(HASH_SEED));
            let mut decoder = Decoder::new(&mut reader).unwrap();
            let mut output = File::create(&path).unwrap();
            copy(&mut decoder, &mut output)
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
                        println!("failed to set permissions for {}: {}", path.display(), e)
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
                    println!("failed to set permissions for {}: {}", path.display(), e)
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
            println!("skipping symlink creation on unsupported platform");
        }
        #[cfg(any(windows, unix, target_os = "redox"))]
        {
            println!("creating symlinks...");
            symlinks.par_iter().enumerate().for_each(|(i, symlink)| {
                let name = &symlink_names[i];
                let path = &directories[symlink.parent as usize].join(&name);
                // directory symlink
                if symlink.kind == 0 {
                    let target = &directories[symlink.target as usize];
                    #[cfg(windows)]
                    {
                        use ::std::os::windows::fs::symlink_dir;
                        symlink_dir(&path, &target).unwrap_or_else(|e| {
                            panic!("failed to create symlink {}: {}", path.display(), e)
                        });
                    }
                    #[cfg(any(unix, target_os = "redox"))]
                    {
                        use ::std::os::unix::fs::symlink;
                        symlink(&path, &target).unwrap_or_else(|e| {
                            panic!("failed to create symlink {}: {}", path.display(), e)
                        });
                    }
                }
                // file symlink
                if symlink.kind == 1 {
                    let target = &files[symlink.target as usize];
                    let target = &directories[target.parent as usize].join(
                        std::str::from_utf8(
                            &target.name[0..(target
                                .name
                                .iter()
                                .position(|&c| c == b'\0')
                                .unwrap_or(target.name.len()))],
                        )
                        .unwrap(),
                    );
                    #[cfg(windows)]
                    {
                        use ::std::os::windows::fs::symlink_file;
                        symlink_file(&path, &target).unwrap_or_else(|e| {
                            panic!("failed to create symlink {}: {}", path.display(), e)
                        });
                    }
                    #[cfg(any(unix, target_os = "redox"))]
                    {
                        use ::std::os::unix::fs::symlink;
                        symlink(&path, &target).unwrap_or_else(|e| {
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
                        println!("failed to set file times for {}: {}", path.display(), e)
                    });
                }
            });
        }

        set_version(&unpack_dir, version);
    }
}
