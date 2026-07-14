use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

use async_trait::async_trait;

const DEFAULT_MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FileStoreError {
    #[error("file exceeds the configured size limit")]
    TooLarge,
    #[error("file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("blocking file task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

#[async_trait]
pub trait PrivateFileStore: Send + Sync {
    async fn read(&self, path: &Path) -> Result<Vec<u8>, FileStoreError>;
    async fn write(&self, path: &Path, contents: &[u8]) -> Result<(), FileStoreError>;
}

#[derive(Debug, Clone)]
pub struct AtomicFileStore {
    max_file_bytes: u64,
}

impl Default for AtomicFileStore {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
        }
    }
}

impl AtomicFileStore {
    pub fn with_max_file_bytes(max_file_bytes: u64) -> Self {
        Self { max_file_bytes }
    }
}

#[async_trait]
impl PrivateFileStore for AtomicFileStore {
    async fn read(&self, path: &Path) -> Result<Vec<u8>, FileStoreError> {
        let metadata = tokio::fs::metadata(path).await?;
        if metadata.len() > self.max_file_bytes {
            return Err(FileStoreError::TooLarge);
        }
        Ok(tokio::fs::read(path).await?)
    }

    async fn write(&self, path: &Path, contents: &[u8]) -> Result<(), FileStoreError> {
        if contents.len() as u64 > self.max_file_bytes {
            return Err(FileStoreError::TooLarge);
        }
        let path = path.to_owned();
        let contents = contents.to_vec();
        tokio::task::spawn_blocking(move || write_atomic(&path, &contents)).await??;
        Ok(())
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("destination has no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".openusage-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    temporary.write_all(contents)?;
    temporary.as_file_mut().sync_all()?;
    let temporary_path = temporary.into_temp_path();
    replace_file(&temporary_path, path)?;
    sync_directory(parent)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = wide_path(source);
    let destination = wide_path(destination);
    let succeeded = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    std::fs::rename(source, destination)
}

fn sync_directory(parent: &Path) -> Result<(), std::io::Error> {
    #[cfg(not(windows))]
    std::fs::File::open(parent)?.sync_all()?;
    let _ = parent;
    Ok(())
}

pub fn sibling_temporary_files(path: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let Some(parent) = path.parent() else {
        return Ok(Vec::new());
    };
    let files = std::fs::read_dir(parent)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|candidate| {
            candidate
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".openusage-"))
        })
        .collect();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replaces_contents_and_leaves_no_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let store = AtomicFileStore::default();

        store.write(&path, b"first").await.unwrap();
        store.write(&path, b"second").await.unwrap();

        assert_eq!(store.read(&path).await.unwrap(), b"second");
        assert!(sibling_temporary_files(&path).unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_oversized_io_before_allocation_or_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.json");
        let store = AtomicFileStore::with_max_file_bytes(3);

        assert!(matches!(
            store.write(&path, b"four").await,
            Err(FileStoreError::TooLarge)
        ));
        assert!(!path.exists());
    }
}
