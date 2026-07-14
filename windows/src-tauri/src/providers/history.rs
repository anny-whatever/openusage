use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const MAX_HISTORY_FILES: usize = 2048;
const MAX_HISTORY_ENTRIES: usize = 100_000;
const MAX_HISTORY_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_DISCOVERY_DEPTH: usize = 8;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct Fingerprint {
    size: u64,
    modified: Option<SystemTime>,
}

#[derive(Clone)]
struct CachedFile<Entry> {
    fingerprint: Fingerprint,
    entries: Vec<Entry>,
}

#[derive(Default)]
struct CacheState<Entry> {
    files: HashMap<PathBuf, CachedFile<Entry>>,
}

#[derive(Clone)]
pub struct IncrementalJsonlCache<Entry> {
    state: Arc<Mutex<CacheState<Entry>>>,
}

impl<Entry> Default for IncrementalJsonlCache<Entry> {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(CacheState {
                files: HashMap::new(),
            })),
        }
    }
}

#[derive(Debug, Clone, thiserror::Error, Eq, PartialEq)]
pub enum HistoryError {
    #[error("local history could not be read")]
    Unreadable,
    #[error("local history exceeds the supported bound")]
    TooLarge,
    #[error("local history task failed")]
    TaskFailed,
}

impl<Entry: Clone + Send + 'static> IncrementalJsonlCache<Entry> {
    pub async fn scan(
        &self,
        files: Vec<PathBuf>,
        parser: fn(&[u8]) -> Vec<Entry>,
    ) -> Result<Vec<Entry>, HistoryError> {
        if files.len() > MAX_HISTORY_FILES {
            return Err(HistoryError::TooLarge);
        }
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || scan_sync(state, files, parser))
            .await
            .map_err(|_| HistoryError::TaskFailed)?
    }
}

fn scan_sync<Entry: Clone>(
    state: Arc<Mutex<CacheState<Entry>>>,
    mut files: Vec<PathBuf>,
    parser: fn(&[u8]) -> Vec<Entry>,
) -> Result<Vec<Entry>, HistoryError> {
    files.sort();
    files.dedup();
    let current = files.iter().cloned().collect::<HashSet<_>>();
    let mut cache = state.lock().map_err(|_| HistoryError::TaskFailed)?;
    cache.files.retain(|path, _| current.contains(path));

    for path in &files {
        let metadata = std::fs::metadata(path).map_err(|_| HistoryError::Unreadable)?;
        if metadata.len() > MAX_HISTORY_FILE_BYTES {
            return Err(HistoryError::TooLarge);
        }
        let fingerprint = Fingerprint {
            size: metadata.len(),
            modified: metadata.modified().ok(),
        };
        let unchanged = cache
            .files
            .get(path)
            .is_some_and(|cached| cached.fingerprint == fingerprint);
        if unchanged {
            continue;
        }
        let contents = std::fs::read(path).map_err(|_| HistoryError::Unreadable)?;
        cache.files.insert(
            path.clone(),
            CachedFile {
                fingerprint,
                entries: parser(&contents),
            },
        );
    }

    let entry_count: usize = cache.files.values().map(|file| file.entries.len()).sum();
    if entry_count > MAX_HISTORY_ENTRIES {
        return Err(HistoryError::TooLarge);
    }
    Ok(files
        .iter()
        .filter_map(|path| cache.files.get(path))
        .flat_map(|file| file.entries.iter().cloned())
        .collect())
}

pub fn discover_jsonl(roots: &[PathBuf]) -> Result<Vec<PathBuf>, HistoryError> {
    let mut files = Vec::new();
    let mut pending = roots
        .iter()
        .cloned()
        .map(|root| (root, 0_usize))
        .collect::<Vec<_>>();
    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_DISCOVERY_DEPTH || !directory.exists() {
            continue;
        }
        let entries = std::fs::read_dir(directory).map_err(|_| HistoryError::Unreadable)?;
        for entry in entries {
            let entry = entry.map_err(|_| HistoryError::Unreadable)?;
            let file_type = entry.file_type().map_err(|_| HistoryError::Unreadable)?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                pending.push((path, depth + 1));
            } else if is_jsonl(&path) {
                files.push(path);
                if files.len() > MAX_HISTORY_FILES {
                    return Err(HistoryError::TooLarge);
                }
            }
        }
    }
    files.sort();
    Ok(files)
}

fn is_jsonl(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_lines(contents: &[u8]) -> Vec<String> {
        contents
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect()
    }

    #[tokio::test]
    async fn unchanged_files_reuse_bounded_cache_and_changed_files_reparse() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("session.jsonl");
        std::fs::write(&file, b"one\n").unwrap();
        let cache = IncrementalJsonlCache::default();

        assert_eq!(
            cache.scan(vec![file.clone()], parse_lines).await.unwrap(),
            ["one"]
        );
        std::fs::write(&file, b"one\ntwo\n").unwrap();
        assert_eq!(
            cache.scan(vec![file], parse_lines).await.unwrap(),
            ["one", "two"]
        );
    }

    #[test]
    fn discovery_skips_symlink_trees() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("one.jsonl"), b"{}").unwrap();

        let files = discover_jsonl(&[directory.path().to_owned()]).unwrap();

        assert_eq!(files.len(), 1);
    }
}
