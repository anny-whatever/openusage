use std::fmt;
use std::path::{Path, PathBuf};

use zeroize::Zeroize;

use super::atomic_file::{AtomicFileStore, FileStoreError, PrivateFileStore};

const SECRET_FILE_LIMIT: u64 = 64 * 1024;

pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub fn new(contents: Vec<u8>) -> Self {
        Self(contents)
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretBytes([REDACTED])")
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretStatus {
    Missing,
    Stored,
}

#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error("secret name is invalid")]
    InvalidName,
    #[error("secret storage failed: {0}")]
    Storage(#[from] FileStoreError),
    #[error("secret protection failed: {0}")]
    Protection(String),
    #[error("blocking secret task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

#[derive(Debug, Clone)]
pub struct DpapiSecretStore {
    root: PathBuf,
    files: AtomicFileStore,
}

impl DpapiSecretStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            files: AtomicFileStore::with_max_file_bytes(SECRET_FILE_LIMIT),
        }
    }

    pub async fn status(&self, name: &str) -> Result<SecretStatus, SecretStoreError> {
        let path = self.path(name)?;
        Ok(if tokio::fs::try_exists(path).await? {
            SecretStatus::Stored
        } else {
            SecretStatus::Missing
        })
    }

    pub async fn save(&self, name: &str, secret: SecretBytes) -> Result<(), SecretStoreError> {
        let path = self.path(name)?;
        let encrypted = tokio::task::spawn_blocking(move || protect(secret.expose())).await??;
        self.files.write(&path, &encrypted).await?;
        Ok(())
    }

    pub async fn load(&self, name: &str) -> Result<Option<SecretBytes>, SecretStoreError> {
        let path = self.path(name)?;
        if !tokio::fs::try_exists(&path).await? {
            return Ok(None);
        }
        let encrypted = self.files.read(&path).await?;
        let secret = tokio::task::spawn_blocking(move || unprotect(&encrypted)).await??;
        Ok(Some(SecretBytes::new(secret)))
    }

    pub async fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        let path = self.path(name)?;
        match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(FileStoreError::Io(error).into()),
        }
    }

    fn path(&self, name: &str) -> Result<PathBuf, SecretStoreError> {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(SecretStoreError::InvalidName);
        }
        Ok(self.root.join(format!("{name}.dpapi")))
    }
}

impl From<std::io::Error> for SecretStoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(FileStoreError::Io(error))
    }
}

#[cfg(windows)]
fn protect(secret: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
    use std::ptr;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: secret.len() as u32,
        pbData: secret.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let succeeded = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if succeeded == 0 {
        return Err(SecretStoreError::Protection(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let protected = unsafe {
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        bytes
    };
    Ok(protected)
}

#[cfg(windows)]
fn unprotect(protected: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
    use std::ptr;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: protected.len() as u32,
        pbData: protected.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let succeeded = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if succeeded == 0 {
        return Err(SecretStoreError::Protection(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    let secret = unsafe {
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        bytes
    };
    Ok(secret)
}

#[cfg(not(windows))]
fn protect(_secret: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
    Err(SecretStoreError::Protection(
        "DPAPI is available only on Windows".to_owned(),
    ))
}

#[cfg(not(windows))]
fn unprotect(_protected: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
    Err(SecretStoreError::Protection(
        "DPAPI is available only on Windows".to_owned(),
    ))
}

pub fn is_secret_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "dpapi")
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dpapi_round_trip_never_exposes_plaintext_on_disk() {
        let directory = tempfile::tempdir().unwrap();
        let store = DpapiSecretStore::new(directory.path().to_owned());

        store
            .save("openrouter", SecretBytes::new(b"secret-value".to_vec()))
            .await
            .unwrap();
        let disk = tokio::fs::read(directory.path().join("openrouter.dpapi"))
            .await
            .unwrap();
        let loaded = store.load("openrouter").await.unwrap().unwrap();

        assert!(!disk.windows(12).any(|bytes| bytes == b"secret-value"));
        assert_eq!(loaded.expose(), b"secret-value");
        assert_eq!(
            store.status("openrouter").await.unwrap(),
            SecretStatus::Stored
        );
        store.delete("openrouter").await.unwrap();
        assert_eq!(
            store.status("openrouter").await.unwrap(),
            SecretStatus::Missing
        );
    }
}
