use std::{
    io::{BufReader, BufWriter, Cursor, Seek, Write},
    path::Path,
};

use jwalk::WalkDir;
use minilz4::{Encode, EncoderBuilder};
use rand::{
    distributions::{Alphanumeric, Distribution},
    thread_rng,
};
use rayon::prelude::*;


pub fn compress_dir_multithread<
    T: AsRef<Path>,
    W: Write + Seek + Sync + Send,
    P: Fn() + Sync + Send,
    E: Fn(&str) + Sync + Send,
    I: Fn(&str) + Sync + Send,
>(
    source: T, target: &mut W, compression: u32, progress_callback: P, error_callback: E,
    info_callback: I,
) -> usize {
    let dir = WalkDir::new(&source)
        .skip_hidden(false)
        .parallelism(jwalk::Parallelism::RayonNewPool(num_cpus::get()))
        .into_iter();

    let source: &Path = source.as_ref();
    let source: &Path = if source.is_dir() {
        source
    } else {
        source.parent().unwrap()
    };

    let archive = tar::Builder::new(target);
    let archive = std::sync::Arc::new(std::sync::Mutex::new(archive));

    let processed = dir
        .par_bridge()
        .filter_map(|entry| {
            let entry = entry.as_ref();
            if let Err(e) = entry {
                error_callback(&format!("couldn't read {:?}: {}", entry, e));
                return None;
            }
            let entry = entry.ok()?;
            let source_path = entry.path();
            let target_path = source_path.strip_prefix(&source);

            if let Err(e) = target_path {
                error_callback(&format!("couldn't read {:?}: {}", entry, e));
                return None;
            }
            let target_path = target_path.ok()?;
            if target_path == Path::new("") {
                return None;
            }

            info_callback(&target_path.display().to_string());

            let mut header = tar::Header::new_ustar();
            if let Err(e) = header.set_path(target_path) {
                error_callback(&format!("failed setting path {:?}: {}", target_path, e));
            };

            if source_path.is_dir() {
                let meta = std::fs::metadata(&source_path);
                if let Err(ref e) = meta {
                    error_callback(&format!(
                        "couldn't read metadata of {:?}: {}",
                        source_path, e
                    ));
                }
                header.set_metadata(&meta.ok()?);
                header.set_cksum();
                let mut lock = archive.lock();
                if let Ok(ref mut archive) = lock {
                    if let Err(e) = archive.append(&header, std::io::empty()) {
                        error_callback(&format!("failed appending to archive: {}", e));
                        return None;
                    };
                }
            } else if entry.path_is_symlink() {
                let meta = std::fs::symlink_metadata(&source_path);
                if let Err(ref e) = meta {
                    error_callback(&format!(
                        "couldn't read metadata of {:?}: {}",
                        source_path, e
                    ));
                    return None;
                }
                header.set_metadata(&meta.ok()?);
                let linkname = std::fs::read_link(&source_path);
                if let Err(ref e) = linkname {
                    error_callback(&format!(
                        "couldn't read linkname of {:?}: {}",
                        source_path, e
                    ));
                    return None;
                }
                let linkname = linkname.ok()?;
                let linkname = linkname.strip_prefix(&source);
                if let Ok(linkname) = linkname {
                    if let Err(e) = header.set_link_name(linkname) {
                        error_callback(&format!("failed setting linkname {:?}: {}", linkname, e));
                        return None;
                    };
                } else {
                    error_callback(&format!(
                        "link points to outside the directory, skipping {}",
                        source_path.display(),
                    ));
                    return None;
                }
                header.set_cksum();
                let mut lock = archive.lock();
                if let Ok(ref mut archive) = lock {
                    if let Err(e) = archive.append(&header, std::io::empty()) {
                        error_callback(&format!(
                            "failed appending {} to archive: {}",
                            target_path.display(),
                            e
                        ));
                        return None;
                    };
                }
            } else {
                let meta = std::fs::metadata(&source_path);
                if let Err(ref e) = meta {
                    error_callback(&format!(
                        "couldn't read metadata of {:?}: {}",
                        source_path, e
                    ));
                    return None;
                }
                let meta = meta.ok()?;
                header.set_metadata(&meta);
                let file = std::fs::File::open(&source_path);
                if let Err(e) = file {
                    error_callback(&format!("couldn't open {:?}: {}", source_path, e));
                    return None;
                }
                let file = file.ok()?;
                if meta.len() > 2000000000 {
                    info_callback(&format!(
                        "{} (compressing large file to disk)",
                        target_path.display(),
                    ));
                    let cache_path = std::env::temp_dir().join(
                        Alphanumeric
                            .sample_iter(thread_rng())
                            .take(16)
                            .collect::<String>(),
                    );
                    if let Err(e) = (|| -> std::io::Result<()> {
                        let mut cache = std::fs::File::create(&cache_path)?;
                        let mut encoder = EncoderBuilder::new()
                            .level(compression)
                            .build(BufWriter::new(&mut cache))?;
                        std::io::copy(&mut BufReader::new(file), &mut encoder)?;
                        encoder.finish()?.flush()?;
                        cache.sync_all()?;
                        Ok(())
                    })() {
                        error_callback(&format!("couldn't compress {:?}: {}", source_path, e));
                        return None;
                    }
                    if let Err(e) = (|| -> std::io::Result<()> {
                        let cache = std::fs::File::open(&cache_path)?;
                        header.set_size(cache.metadata()?.len() as u64);
                        header.set_cksum();
                        let mut lock = archive.lock();
                        if let Ok(ref mut archive) = lock {
                            archive.append(&header, BufReader::new(cache))?;
                        }
                        Ok(())
                    })() {
                        error_callback(&format!(
                            "failed appending {} to archive: {}",
                            target_path.display(),
                            e
                        ));
                        return None;
                    }
                    let _ = std::fs::remove_file(cache_path);
                } else {
                    let data =
                        BufReader::new(file).encode(EncoderBuilder::new().level(compression));
                    if let Err(e) = data {
                        error_callback(&format!("couldn't compress {:?}: {}", source_path, e));
                        return None;
                    }
                    let data = data.ok()?;
                    header.set_size(data.len() as u64);
                    header.set_cksum();
                    let mut lock = archive.lock();
                    if let Ok(ref mut archive) = lock {
                        if let Err(e) = archive.append(&header, Cursor::new(data)) {
                            error_callback(&format!(
                                "failed appending {}, to archive: {}",
                                target_path.display(),
                                e
                            ));
                            return None;
                        };
                    }
                }
            }
            progress_callback();
            Some(())
        })
        .count();
    archive.lock().unwrap().finish().unwrap();
    processed
}
