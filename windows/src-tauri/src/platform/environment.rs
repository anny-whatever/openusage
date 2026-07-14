use std::collections::HashMap;
use std::ffi::OsString;

use crate::platform::secret::SecretBytes;

pub trait EnvironmentReader: Send + Sync {
    fn value(&self, name: &str) -> Option<SecretBytes>;

    fn is_present(&self, name: &str) -> bool {
        self.value(name).is_some()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SystemEnvironment;

impl EnvironmentReader for SystemEnvironment {
    fn value(&self, name: &str) -> Option<SecretBytes> {
        if !valid_environment_name(name) {
            return None;
        }
        std::env::var_os(name).map(os_string_bytes)
    }

    fn is_present(&self, name: &str) -> bool {
        valid_environment_name(name) && std::env::var_os(name).is_some()
    }
}

#[derive(Debug, Default)]
pub struct FakeEnvironment {
    values: HashMap<String, Vec<u8>>,
}

impl FakeEnvironment {
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<Vec<u8>>) {
        self.values.insert(name.into(), value.into());
    }
}

impl EnvironmentReader for FakeEnvironment {
    fn value(&self, name: &str) -> Option<SecretBytes> {
        self.values.get(name).cloned().map(SecretBytes::new)
    }
}

fn valid_environment_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(windows)]
fn os_string_bytes(value: OsString) -> SecretBytes {
    use std::os::windows::ffi::OsStrExt;
    use zeroize::Zeroize;

    let mut wide = value.encode_wide().collect::<Vec<_>>();
    let text = String::from_utf16_lossy(&wide);
    wide.zeroize();
    SecretBytes::new(text.into_bytes())
}

#[cfg(not(windows))]
fn os_string_bytes(value: OsString) -> SecretBytes {
    use std::os::unix::ffi::OsStringExt;

    SecretBytes::new(value.into_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_exposes_value_only_inside_secret_wrapper() {
        let mut environment = FakeEnvironment::default();
        environment.insert("OPENROUTER_API_KEY", b"secret".to_vec());

        assert!(environment.is_present("OPENROUTER_API_KEY"));
        assert_eq!(
            format!("{:?}", environment.value("OPENROUTER_API_KEY").unwrap()),
            "SecretBytes([REDACTED])"
        );
    }

    #[test]
    fn invalid_environment_names_are_never_queried() {
        assert!(!SystemEnvironment.is_present("bad=name"));
    }
}
