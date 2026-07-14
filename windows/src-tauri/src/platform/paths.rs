use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WindowsPaths {
    pub user_profile: PathBuf,
    pub roaming_app_data: PathBuf,
    pub local_app_data: PathBuf,
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum PathError {
    #[error("required Windows path is missing: {0}")]
    Missing(&'static str),
    #[error("Windows path must be absolute and native: {0}")]
    Invalid(&'static str),
}

impl WindowsPaths {
    pub fn from_environment() -> Result<Self, PathError> {
        Self::new(
            required_path("USERPROFILE")?,
            required_path("APPDATA")?,
            required_path("LOCALAPPDATA")?,
        )
    }

    pub fn new(
        user_profile: PathBuf,
        roaming_app_data: PathBuf,
        local_app_data: PathBuf,
    ) -> Result<Self, PathError> {
        validate_native_absolute(&user_profile, "USERPROFILE")?;
        validate_native_absolute(&roaming_app_data, "APPDATA")?;
        validate_native_absolute(&local_app_data, "LOCALAPPDATA")?;
        Ok(Self {
            user_profile,
            roaming_app_data,
            local_app_data,
        })
    }

    pub fn openusage_data(&self) -> PathBuf {
        self.local_app_data.join("OpenUsage")
    }

    pub fn openusage_logs(&self) -> PathBuf {
        self.openusage_data().join("logs")
    }
}

fn required_path(name: &'static str) -> Result<PathBuf, PathError> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or(PathError::Missing(name))
}

fn validate_native_absolute(path: &Path, name: &'static str) -> Result<(), PathError> {
    let rendered = path.to_string_lossy().to_ascii_lowercase();
    let looks_wsl = rendered.starts_with("\\\\wsl$")
        || rendered.starts_with("\\\\wsl.localhost")
        || rendered.starts_with("/mnt/");
    if !path.is_absolute() || looks_wsl {
        return Err(PathError::Invalid(name));
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn native_windows_environment_resolves_private_app_paths() {
        let paths = WindowsPaths::from_environment().unwrap();

        assert!(paths.openusage_data().is_absolute());
        assert!(paths.openusage_logs().ends_with("OpenUsage\\logs"));
    }

    #[test]
    fn wsl_and_relative_paths_are_rejected() {
        assert_eq!(
            WindowsPaths::new("relative".into(), "C:\\AppData".into(), "C:\\Local".into()),
            Err(PathError::Invalid("USERPROFILE"))
        );
        assert_eq!(
            WindowsPaths::new(
                "\\\\wsl$\\Ubuntu\\home".into(),
                "C:\\AppData".into(),
                "C:\\Local".into()
            ),
            Err(PathError::Invalid("USERPROFILE"))
        );
    }
}
