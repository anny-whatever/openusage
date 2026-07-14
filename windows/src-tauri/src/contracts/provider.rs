use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::{
    ContractError, MAX_HISTORY_DAYS, PROVIDER_SNAPSHOT_SCHEMA, Validate, validate_color,
    validate_day, validate_identifier, validate_number, validate_text, validate_timestamp,
};

const MAX_METRIC_LINES: usize = 128;
const MAX_VALUES_PER_LINE: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderSnapshotEnvelope {
    pub schema: String,
    pub snapshot: ProviderSnapshot,
}

impl Validate for ProviderSnapshotEnvelope {
    fn validate(&self) -> Result<(), ContractError> {
        if self.schema != PROVIDER_SNAPSHOT_SCHEMA {
            return Err(ContractError::UnsupportedSchema(self.schema.clone()));
        }
        self.snapshot.validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderSnapshot {
    pub provider_id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub lines: Vec<MetricLine>,
    pub refreshed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_history: Option<UsageHistory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_category: Option<ErrorCategory>,
}

impl Validate for ProviderSnapshot {
    fn validate(&self) -> Result<(), ContractError> {
        validate_identifier(&self.provider_id, "providerId")?;
        validate_text(&self.display_name, "displayName")?;
        validate_timestamp(&self.refreshed_at, "refreshedAt")?;
        if let Some(plan) = &self.plan {
            validate_text(plan, "plan")?;
        }
        if let Some(warning) = &self.warning {
            validate_text(warning, "warning")?;
        }
        if self.lines.len() > MAX_METRIC_LINES {
            return Err(ContractError::Invalid("too many metric lines".to_owned()));
        }
        let mut labels = HashSet::new();
        for line in &self.lines {
            line.validate()?;
            if !labels.insert(line.label()) {
                return Err(ContractError::Duplicate {
                    field: "metric label",
                    value: line.label().to_owned(),
                });
            }
        }
        if let Some(history) = &self.usage_history {
            history.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MetricLine {
    Text {
        label: String,
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        color_hex: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
    },
    Values {
        label: String,
        values: Vec<MetricValue>,
        #[serde(skip_serializing_if = "Option::is_none")]
        color_hex: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        expiries_at: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        unknown_models: Vec<String>,
    },
    Progress {
        label: String,
        used: f64,
        limit: f64,
        format: MetricFormat,
        #[serde(skip_serializing_if = "Option::is_none")]
        resets_at: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        period_duration_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        color_hex: Option<String>,
    },
    Badge {
        label: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        color_hex: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        subtitle: Option<String>,
    },
    Chart {
        label: String,
        points: Vec<MetricChartPoint>,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}

impl MetricLine {
    pub fn label(&self) -> &str {
        match self {
            Self::Text { label, .. }
            | Self::Values { label, .. }
            | Self::Progress { label, .. }
            | Self::Badge { label, .. }
            | Self::Chart { label, .. } => label,
        }
    }
}

impl Validate for MetricLine {
    fn validate(&self) -> Result<(), ContractError> {
        validate_text(self.label(), "metric label")?;
        validate_color(self.color_hex())?;
        match self {
            Self::Text {
                value, subtitle, ..
            } => {
                validate_text(value, "text value")?;
                if let Some(subtitle) = subtitle {
                    validate_text(subtitle, "subtitle")?;
                }
                Ok(())
            }
            Self::Badge { text, subtitle, .. } => {
                validate_text(text, "badge text")?;
                if let Some(subtitle) = subtitle {
                    validate_text(subtitle, "subtitle")?;
                }
                Ok(())
            }
            Self::Values {
                values,
                expiries_at,
                unknown_models,
                ..
            } => {
                if values.is_empty() || values.len() > MAX_VALUES_PER_LINE {
                    return Err(ContractError::Empty { field: "values" });
                }
                if expiries_at.len() > 64 || unknown_models.len() > 64 {
                    return Err(ContractError::Invalid(
                        "metric metadata exceeds its fixed bound".to_owned(),
                    ));
                }
                for value in values {
                    value.validate()?;
                }
                for expiry in expiries_at {
                    validate_timestamp(expiry, "expiriesAt")?;
                }
                for model in unknown_models {
                    validate_text(model, "unknownModels")?;
                }
                Ok(())
            }
            Self::Progress {
                used,
                limit,
                resets_at,
                period_duration_ms,
                format,
                ..
            } => {
                validate_number(*used, "used")?;
                validate_number(*limit, "limit")?;
                if *limit == 0.0 || used > limit {
                    return Err(ContractError::Invalid(
                        "progress requires 0 <= used <= limit and a positive limit".to_owned(),
                    ));
                }
                if let Some(timestamp) = resets_at {
                    validate_timestamp(timestamp, "resetsAt")?;
                }
                if period_duration_ms == &Some(0) {
                    return Err(ContractError::Invalid(
                        "periodDurationMs must be positive".to_owned(),
                    ));
                }
                format.validate()?;
                Ok(())
            }
            Self::Chart { points, .. } => {
                if points.len() > MAX_HISTORY_DAYS {
                    return Err(ContractError::Invalid(
                        "chart exceeds history window".to_owned(),
                    ));
                }
                for point in points {
                    point.validate()?;
                }
                Ok(())
            }
        }
    }
}

impl MetricLine {
    fn color_hex(&self) -> Option<&str> {
        match self {
            Self::Text { color_hex, .. }
            | Self::Values { color_hex, .. }
            | Self::Progress { color_hex, .. }
            | Self::Badge { color_hex, .. } => color_hex.as_deref(),
            Self::Chart { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricValue {
    pub number: f64,
    pub kind: MetricKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub estimated: bool,
}

impl Validate for MetricValue {
    fn validate(&self) -> Result<(), ContractError> {
        validate_number(self.number, "number")?;
        if let Some(label) = &self.label {
            validate_text(label, "value label")?;
        }
        if self.kind == MetricKind::Percent && self.number > 100.0 {
            return Err(ContractError::Invalid(
                "percent values must be between zero and 100".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MetricKind {
    Percent,
    Dollars,
    Count,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MetricFormat {
    Percent,
    Dollars,
    Count { suffix: String },
}

impl Validate for MetricFormat {
    fn validate(&self) -> Result<(), ContractError> {
        if let Self::Count { suffix } = self {
            validate_text(suffix, "count suffix")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricChartPoint {
    pub value: f64,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_label: Option<String>,
}

impl Validate for MetricChartPoint {
    fn validate(&self) -> Result<(), ContractError> {
        validate_number(self.value, "chart point value")?;
        validate_text(&self.label, "chart point label")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageHistory {
    pub days: Vec<UsageDay>,
}

impl Validate for UsageHistory {
    fn validate(&self) -> Result<(), ContractError> {
        if self.days.len() > MAX_HISTORY_DAYS {
            return Err(ContractError::Invalid("history exceeds 31 days".to_owned()));
        }
        let mut days = HashSet::new();
        for day in &self.days {
            day.validate()?;
            if !days.insert(day.date.as_str()) {
                return Err(ContractError::Duplicate {
                    field: "history day",
                    value: day.date.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageDay {
    pub date: String,
    pub value: f64,
}

impl Validate for UsageDay {
    fn validate(&self) -> Result<(), ContractError> {
        validate_day(&self.date, "history date")?;
        validate_number(self.value, "history value")
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    NotLoggedIn,
    AuthExpired,
    AuthInvalid,
    CredentialAccess,
    Network,
    Decoding,
    #[serde(rename = "http_4xx")]
    Http4xx,
    #[serde(rename = "http_5xx")]
    Http5xx,
    RateLimited,
    NotAvailable,
    Other,
}
