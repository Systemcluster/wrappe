use std::{
    env::temp_dir,
    fs::{read_link, remove_file, symlink_metadata, File},
    hash::Hasher,
    io::{copy, BufReader, Cursor, Read, Result, Seek, Write},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::SystemTime,
};

use jwalk::WalkDir;
use path_slash::PathExt;
use rand::{
    distributions::{Alphanumeric, Distribution},
    thread_rng,
};
use rayon::prelude::*;
use sysinfo::System;
use twox_hash::XxHash64;
use zstd::{dict::EncoderDictionary, Encoder};

use crate::types::*;

pub const HASH_SEED: u64 = 1246736989840;

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

pub fn copy_encode<R: Read, W: Write>(
    mut source: R, destination: W, level: i32, threads: u32, dict: Option<&EncoderDictionary>,
) -> Result<()> {
    let mut encoder = if let Some(dict) = dict {
        Encoder::with_prepared_dictionary(destination, dict)?
    } else {
        Encoder::new(destination, level)?
    };
    encoder.multithread(threads)?;
    copy(&mut source, &mut encoder)?;
    encoder.finish()?;
    Ok(())
}

/// Compress the payload in `source` and write it into `target`.
/// The data is written subsequently in the following order:
/// - compressed file contents
/// - compression dictionary
/// - directory sections
/// - file section headers
/// - symlink sections
/// - payload section header
#[allow(clippy::too_many_arguments)]
pub fn compress<
    T: AsRef<Path>,
    W: Write + Seek + Sync + Send,
    X: AsRef<Path>,
    P: Fn() + Sync + Send,
    E: Fn(&str) + Sync + Send,
    S: Fn(&str) + Sync + Send,
    I: Fn(&str) + Sync + Send,
>(
    source: T, target: &mut W, exclude: X, compression: u32, build_dict: bool,
    progress_callback: P, error_callback: E, step_callback: S, info_callback: I,
) -> (u64, u64, u64) {
    let source: &Path = source.as_ref();
    let exclude: &Path = exclude.as_ref();

    let num_cpus = num_cpus::get() as u64;
    let system = System::new_with_specifics(
        sysinfo::RefreshKind::new().with_memory(sysinfo::MemoryRefreshKind::new().with_ram()),
    );
    let memory = system.total_memory();
    let in_memory_limit = memory / num_cpus * 1000;

    let entries = WalkDir::new(source)
        .skip_hidden(false)
        .sort(true)
        .into_iter()
        .filter(|entry| {
            if let Err(e) = entry {
                error_callback(&format!("couldn't read entry: {}", e));
                return false;
            }
            true
        })
        .collect::<Vec<_>>();

    let source: &Path = if source.is_dir() {
        source
    } else {
        source.parent().unwrap()
    };

    // create compression dictionary
    let dictionary_data = if build_dict {
        step_callback("creating compression dictionary");
        let mut sizes = Vec::new();
        let mut sample = Vec::new();
        let _ = entries
            .iter()
            .filter_map(|entry| {
                // zstd dictionary data is limited to 4GB
                if sample.len() >= 4 * 1024 * 1024 * 1024 - 128 * 1024 {
                    return None;
                }
                let entry = entry.as_ref().ok()?;
                if !entry.file_type().is_file() {
                    return None;
                }
                let entry = entry.path();
                if entry == exclude {
                    return None;
                }
                if entry.file_name()?.len() > NAME_SIZE {
                    return None;
                }
                let file = File::open(&entry).ok()?;
                let size = BufReader::new(file.take(128 * 1024))
                    .read_to_end(&mut sample)
                    .ok()?;
                sizes.push(size);
                Some(())
            })
            .count();
        if sizes.len() < 8 {
            error_callback("couldn't build dictionary: not enough samples");
            None
        } else {
            let dict = zstd::dict::from_continuous(&sample, &sizes, 128 * 1024).unwrap();
            info_callback(&format!(
                "built {:.2}MB dictionary from {:.2}MB of data",
                dict.len() as f64 / 1024.0 / 1024.0,
                sample.len() as f64 / 1024.0 / 1024.0
            ));
            Some(dict)
        }
    } else {
        None
    };

    let dictionary = dictionary_data
        .as_ref()
        .map(|dict| EncoderDictionary::copy(dict, compression as i32));

    let mut directories = Vec::<DirectorySection>::new();
    // start with the source directory as parent 0
    let mut parents = Vec::<String>::from(["".to_string()]);

    // enumerate directories
    let _ = entries
        .iter()
        .filter_map(|entry| {
            let entry = entry.as_ref().ok()?;
            if !entry.file_type().is_dir() {
                return None;
            }
            let entry = entry.path();
            if entry == exclude {
                error_callback(&format!("skipping excluded file: {}", entry.display()));
                return None;
            }
            let entry = entry.strip_prefix(source).ok()?;

            if entry.file_name()?.len() > NAME_SIZE {
                error_callback(&format!(
                    "skipping directory with name longer than {}: {}",
                    NAME_SIZE,
                    entry.display()
                ));
                return None;
            }

            step_callback(&entry.display().to_string());

            let name = entry.file_name()?.to_str()?;

            parents.push(entry.to_slash()?.into_owned());

            let parent = entry.parent().unwrap().to_slash().unwrap();
            let parent = match parents.iter().position(|element| element == &parent) {
                Some(index) => index,
                None => {
                    error_callback(&format!(
                        "skipping directory with no included parent: {}",
                        entry.display()
                    ));
                    return None;
                }
            };

            let mut name_array = [0; NAME_SIZE];
            name_array[0..name.len()].copy_from_slice(name.as_bytes());
            directories.push(DirectorySection {
                name:   name_array,
                parent: parent as u32,
            });

            progress_callback();
            Some(())
        })
        .count();

    let zero = target.stream_position().unwrap();
    let archive = Arc::new(Mutex::new(target));

    let files = Arc::new(Mutex::new(Vec::<FileSectionHeader>::new()));
    let links = Arc::new(Mutex::new(Vec::<String>::new()));

    let read = AtomicU64::new(0);

    // compress and append files
    let _ = entries
        .par_iter()
        .filter_map(|entry| {
            let entry = entry.as_ref().ok()?;
            if !entry.file_type().is_file() {
                return None;
            }
            let entry = entry.path();
            if entry == exclude {
                error_callback(&format!("skipping excluded file: {}", entry.display()));
                return None;
            }

            if entry.file_name()?.len() > NAME_SIZE {
                error_callback(&format!(
                    "skipping file with name longer than: {}: {}",
                    NAME_SIZE,
                    entry.display()
                ));
                return None;
            }

            step_callback(&entry.strip_prefix(source).ok()?.display().to_string());

            let parent = entry.strip_prefix(source).ok()?.parent()?.to_slash()?;
            let parent = match parents.iter().position(|element| element == &parent) {
                Some(index) => index,
                None => {
                    error_callback(&format!(
                        "skipping file with no included parent: {}",
                        entry.display()
                    ));
                    return None;
                }
            };

            let name = entry.file_name()?.to_str()?;

            let file = File::open(&entry);
            if let Err(e) = file {
                error_callback(&format!("couldn't open {}: {}", entry.display(), e));
                return None;
            }
            let file = file.ok()?;

            let mut in_memory = true;
            let mut meta_len = 0;
            let meta = file.metadata();
            if let Ok(ref meta) = meta {
                meta_len = meta.len();
                if meta_len > in_memory_limit {
                    in_memory = false;
                }
            }

            let mut start = 0;
            let mut end = 0;
            let mut compressed_hash = 0;
            let mut reader = HashReader::new(file, XxHash64::with_seed(HASH_SEED));

            if in_memory {
                let mut data = Vec::new();
                let mut reader = BufReader::new(&mut reader);
                if let Err(e) = copy_encode(
                    &mut reader,
                    &mut data,
                    compression as i32,
                    0,
                    dictionary.as_ref(),
                ) {
                    error_callback(&format!("couldn't compress {}: {}", entry.display(), e));
                    return None;
                }

                let mut archive = archive.lock();
                if let Ok(ref mut archive) = archive {
                    start = archive.stream_position().unwrap();
                    let mut hasher =
                        HashReader::new(Cursor::new(&data), XxHash64::with_seed(HASH_SEED));
                    if let Err(e) = copy(&mut hasher, archive.by_ref()) {
                        error_callback(&format!(
                            "couldn't write {} to archive: {}",
                            entry.display(),
                            e
                        ));
                        return None;
                    }
                    compressed_hash = hasher.finish();
                    end = archive.stream_position().unwrap();
                }
            } else {
                step_callback(&format!(
                    "{} (compressing large file to disk)",
                    entry.display(),
                ));
                let cache_path = temp_dir().join(
                    Alphanumeric
                        .sample_iter(thread_rng())
                        .map(char::from)
                        .take(16)
                        .collect::<String>(),
                );

                if let Err(e) = (|| -> Result<()> {
                    let mut reader = BufReader::new(&mut reader);
                    let mut cache = File::create(&cache_path)?;
                    copy_encode(
                        &mut reader,
                        &cache,
                        compression as i32,
                        u64::min(num_cpus / 2, meta_len / in_memory_limit + 1) as u32,
                        dictionary.as_ref(),
                    )?;
                    cache.flush()?;
                    cache.sync_all()?;
                    Ok(())
                })() {
                    error_callback(&format!("couldn't compress {}: {}", entry.display(), e));
                    return None;
                }

                if let Err(e) = (|| -> Result<()> {
                    let cache = File::open(&cache_path)?;
                    let mut reader = BufReader::new(&cache);
                    let mut archive = archive.lock();
                    let mut hasher = HashReader::new(&mut reader, XxHash64::with_seed(HASH_SEED));
                    if let Ok(ref mut archive) = archive {
                        start = archive.stream_position().unwrap();
                        copy(&mut hasher, archive.by_ref())?;
                        end = archive.stream_position().unwrap();
                    }
                    compressed_hash = hasher.finish();
                    Ok(())
                })() {
                    error_callback(&format!(
                        "couldn't write {} to archive: {}",
                        entry.display(),
                        e
                    ));
                    return None;
                }

                let _ = remove_file(cache_path);
            }
            let file_hash = reader.finish();

            read.fetch_add(meta_len, Ordering::AcqRel);

            let mut name_array = [0; NAME_SIZE];
            name_array[0..name.len()].copy_from_slice(name.as_bytes());
            let mut header = FileSectionHeader {
                name: name_array,
                parent: parent as u32,
                position: start - zero,
                size: end - start,
                file_hash,
                compressed_hash,
                time_accessed_nanos: 0,
                time_accessed_seconds: 0,
                time_modified_nanos: 0,
                time_modified_seconds: 0,
                mode: 0,
                readonly: 0,
            };

            if let Ok(ref meta) = meta {
                if let Ok(accessed) = meta.accessed() {
                    if let Ok(accessed) = accessed.duration_since(SystemTime::UNIX_EPOCH) {
                        header.time_accessed_seconds = accessed.as_secs();
                        header.time_accessed_nanos = accessed.subsec_nanos();
                    }
                }
                if let Ok(modified) = meta.modified() {
                    if let Ok(modified) = modified.duration_since(SystemTime::UNIX_EPOCH) {
                        header.time_modified_seconds = modified.as_secs();
                        header.time_modified_nanos = modified.subsec_nanos();
                    }
                }
                header.readonly = meta.permissions().readonly() as u8;
                #[cfg(any(unix, target_os = "redox"))]
                {
                    use std::os::unix::fs::PermissionsExt;
                    header.mode = meta.permissions().mode();
                }
            }

            let mut files = files.lock();
            if let Ok(ref mut files) = files {
                files.push(header);
                let mut links = links.lock();
                if let Ok(ref mut links) = links {
                    links.push(entry.strip_prefix(source).ok()?.to_slash()?.into_owned());
                }
            }

            progress_callback();
            Some(())
        })
        .count();

    let symlinks = Arc::new(Mutex::new(Vec::<SymlinkSection>::new()));

    // enumerate symlinks
    let _ = entries
        .par_iter()
        .filter_map(|entry| {
            let entry = entry.as_ref().ok()?;
            if !entry.file_type().is_symlink() {
                return None;
            }
            let entry = entry.path();
            if entry == exclude {
                error_callback(&format!("skipping excluded file: {}", entry.display()));
                return None;
            }

            if entry.file_name()?.len() > NAME_SIZE {
                error_callback(&format!(
                    "skipping file with name longer than: {}: {}",
                    NAME_SIZE,
                    entry.display()
                ));
                return None;
            }

            step_callback(&entry.strip_prefix(source).ok()?.display().to_string());

            let parent = entry.strip_prefix(source).ok()?.parent()?.to_slash()?;
            let parent = match parents.iter().position(|element| element == &parent) {
                Some(index) => index,
                None => {
                    error_callback(&format!(
                        "skipping file with no included parent: {}",
                        entry.display()
                    ));
                    return None;
                }
            };

            let meta = symlink_metadata(&entry);
            let name = entry.file_name()?.to_str()?;

            let link = read_link(&entry);
            if let Err(ref e) = link {
                error_callback(&format!("couldn't read link {}: {}", entry.display(), e));
                return None;
            }
            let link = link.ok()?;
            let link = link.strip_prefix(".").unwrap_or(&link);
            let link = entry.parent().unwrap().join(link);
            let link = link.canonicalize();
            if let Err(e) = link {
                error_callback(&format!(
                    "link could not be canonicalized, skipping {}: {}",
                    entry.display(),
                    e
                ));
                return None;
            }
            let link = link.ok()?;
            let is_file = link.is_file();
            let link = link.strip_prefix(source);
            if let Err(e) = link {
                error_callback(&format!(
                    "link points to outside the directory, skipping {}: {}",
                    entry.display(),
                    e
                ));
                return None;
            }
            let link = link.ok()?;

            let target = if is_file {
                let link = link.to_slash()?;
                match links
                    .lock()
                    .unwrap()
                    .iter()
                    .position(|element| element == &link)
                {
                    Some(index) => index,
                    None => {
                        error_callback(&format!(
                            "skipping link with no included target: {}",
                            entry.display()
                        ));
                        return None;
                    }
                }
            } else {
                let link = link.to_slash()?;
                match parents.iter().position(|element| element == &link) {
                    Some(index) => index,
                    None => {
                        error_callback(&format!(
                            "skipping link with no included target: {}",
                            entry.display()
                        ));
                        return None;
                    }
                }
            };

            let mut name_array = [0; NAME_SIZE];
            name_array[0..name.len()].copy_from_slice(name.as_bytes());
            let mut header = SymlinkSection {
                name:                  name_array,
                parent:                parent as u32,
                kind:                  is_file as u8,
                target:                target as u32,
                time_accessed_nanos:   0,
                time_accessed_seconds: 0,
                time_modified_nanos:   0,
                time_modified_seconds: 0,
                mode:                  0,
                readonly:              0,
            };

            if let Ok(ref meta) = meta {
                if let Ok(accessed) = meta.accessed() {
                    if let Ok(accessed) = accessed.duration_since(SystemTime::UNIX_EPOCH) {
                        header.time_accessed_seconds = accessed.as_secs();
                        header.time_accessed_nanos = accessed.subsec_nanos();
                    }
                }
                if let Ok(modified) = meta.modified() {
                    if let Ok(modified) = modified.duration_since(SystemTime::UNIX_EPOCH) {
                        header.time_modified_seconds = modified.as_secs();
                        header.time_modified_nanos = modified.subsec_nanos();
                    }
                }
                header.readonly = meta.permissions().readonly() as u8;
                #[cfg(any(unix, target_os = "redox"))]
                {
                    use std::os::unix::fs::PermissionsExt;
                    header.mode = meta.permissions().mode();
                }
            }

            let mut symlinks = symlinks.lock();
            if let Ok(ref mut symlinks) = symlinks {
                symlinks.push(header);
            }

            progress_callback();
            Some(())
        })
        .count();

    let mut target = archive.lock().unwrap();
    let end = target.stream_position().unwrap();

    // write sections
    let mut hasher = XxHash64::with_seed(HASH_SEED);
    if let Some(dict) = &dictionary_data {
        target.write_all(dict).unwrap();
    }
    for section in directories.iter() {
        hasher.write(section.as_bytes());
        target.write_all(section.as_bytes()).unwrap();
    }
    for section in files.lock().unwrap().iter() {
        hasher.write(section.as_bytes());
        target.write_all(section.as_bytes()).unwrap();
    }
    for section in symlinks.lock().unwrap().iter() {
        hasher.write(section.as_bytes());
        target.write_all(section.as_bytes()).unwrap();
    }
    let payload_header = PayloadHeader {
        kind:               0,
        directory_sections: directories.len() as u64,
        file_sections:      files.lock().unwrap().len() as u64,
        symlink_sections:   symlinks.lock().unwrap().len() as u64,
        dictionary_size:    dictionary_data.map_or(0, |dict| dict.len() as u64),
        section_hash:       hasher.finish(),
        payload_size:       end - zero,
    };
    target.write_all(payload_header.as_bytes()).unwrap();
    target.flush().unwrap();
    let written = target.stream_position().unwrap();

    (
        payload_header.len() as u64,
        read.load(Ordering::Acquire),
        written - zero,
    )
}
