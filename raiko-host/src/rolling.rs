use std::{fs, path::Path, sync::Mutex};

const L1_CACHE_FILE_SUFFIX: &str = ".l1.json.gz";
const L2_CACHE_FILE_SUFFIX: &str = ".l2.json.gz";

use once_cell::sync::Lazy;

static MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

// delete old cache files
pub fn prune_old_caches<T: AsRef<Path>>(cache_dir: T, max_blocks: usize) {
    let _guard = MUTEX.lock().unwrap();
    let cache_dir = cache_dir.as_ref();
    let files = fs::read_dir(cache_dir).map(|dir| {
        dir.filter_map(|entry| {
            let entry = entry.ok()?;
            let metadata = entry.metadata().ok()?;

            // the appender only creates files, not directories or symlinks,
            // so we should never delete a dir or symlink.
            if !metadata.is_file() {
                return None;
            }

            let filename = entry.file_name();
            // if the filename is not a UTF-8 string, skip it.
            let filename = filename.to_str()?;
            if !filename.ends_with(L1_CACHE_FILE_SUFFIX)
                && !filename.ends_with(L2_CACHE_FILE_SUFFIX)
            {
                return None;
            }

            let created = metadata.created().ok()?;
            Some((entry, created))
        })
        .collect::<Vec<_>>()
    });

    let mut files = match files {
        Ok(files) => files,
        Err(error) => {
            eprintln!("Error reading the log directory/files: {}", error);
            return;
        }
    };
    let max_files = max_blocks * 2;
    if files.len() < max_files {
        return;
    }

    // sort the files by their creation timestamps.
    files.sort_by_key(|(entry, _created_at)| entry.file_name());

    for (file, _) in files.iter().take(files.len() - max_files) {
        if let Err(error) = fs::remove_file(file.path()) {
            eprintln!(
                "Failed to remove old log file {}: {}",
                file.path().display(),
                error
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_prune_old_caches() {
        prune_old_caches("./testdata/rolling_caches/", 2);
    }
}
