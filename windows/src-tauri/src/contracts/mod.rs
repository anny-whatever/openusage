mod limits;
mod provider;

pub use limits::{LimitEnvelope, LimitResource, LimitResourceKind, LimitSource};
pub use provider::{
    ErrorCategory, MetricChartPoint, MetricFormat, MetricKind, MetricLine, MetricValue,
    ProviderSnapshot, ProviderSnapshotEnvelope, UsageDay, UsageHistory,
};

pub const PROVIDER_SNAPSHOT_SCHEMA: &str = "openusage.provider-snapshot.v1";
pub const LIMITS_SCHEMA: &str = "openusage.limits.v1";
pub const LIMITS_TTL_SECONDS: i64 = 5 * 60;
pub const MAX_HISTORY_DAYS: usize = 31;
const MAX_CONTRACT_TEXT_BYTES: usize = 4096;

pub trait Validate {
    fn validate(&self) -> Result<(), ContractError>;
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum ContractError {
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
    #[error("{field} contains an invalid identifier")]
    InvalidIdentifier { field: &'static str },
    #[error("{field} must be finite and non-negative")]
    InvalidNumber { field: &'static str },
    #[error("{field} must be an RFC 3339 timestamp")]
    InvalidTimestamp { field: &'static str },
    #[error("{field} must use YYYY-MM-DD")]
    InvalidDay { field: &'static str },
    #[error("duplicate {field}: {value}")]
    Duplicate { field: &'static str, value: String },
    #[error("unsupported schema: {0}")]
    UnsupportedSchema(String),
    #[error("{0}")]
    Invalid(String),
}

pub(crate) fn validate_identifier(value: &str, field: &'static str) -> Result<(), ContractError> {
    if value.is_empty() {
        return Err(ContractError::Empty { field });
    }
    if value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(ContractError::InvalidIdentifier { field });
    }
    Ok(())
}

pub(crate) fn validate_text(value: &str, field: &'static str) -> Result<(), ContractError> {
    if value.trim().is_empty() || value.len() > MAX_CONTRACT_TEXT_BYTES {
        return Err(ContractError::Empty { field });
    }
    Ok(())
}

pub(crate) fn validate_color(value: Option<&str>) -> Result<(), ContractError> {
    let Some(value) = value else { return Ok(()) };
    let valid = value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid {
        return Err(ContractError::Invalid(
            "colorHex must use #RRGGBB".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_number(value: f64, field: &'static str) -> Result<(), ContractError> {
    if !value.is_finite() || value < 0.0 {
        return Err(ContractError::InvalidNumber { field });
    }
    Ok(())
}

pub(crate) fn validate_timestamp(value: &str, field: &'static str) -> Result<(), ContractError> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|_| ())
        .map_err(|_| ContractError::InvalidTimestamp { field })
}

pub(crate) fn validate_day(value: &str, field: &'static str) -> Result<(), ContractError> {
    let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| ContractError::InvalidDay { field })?;
    if date.format("%Y-%m-%d").to_string() != value {
        return Err(ContractError::InvalidDay { field });
    }
    Ok(())
}
