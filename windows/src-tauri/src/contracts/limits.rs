use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::{
    ContractError, LIMITS_SCHEMA, LIMITS_TTL_SECONDS, Validate, validate_identifier,
    validate_number, validate_text, validate_timestamp,
};

const MAX_LIMIT_RESOURCES: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LimitEnvelope {
    pub schema: String,
    pub provider_id: String,
    pub fetched_at: String,
    pub expires_at: String,
    pub resources: Vec<LimitResource>,
}

impl Validate for LimitEnvelope {
    fn validate(&self) -> Result<(), ContractError> {
        if self.schema != LIMITS_SCHEMA {
            return Err(ContractError::UnsupportedSchema(self.schema.clone()));
        }
        validate_identifier(&self.provider_id, "providerId")?;
        validate_timestamp(&self.fetched_at, "fetchedAt")?;
        validate_timestamp(&self.expires_at, "expiresAt")?;
        let fetched_at = chrono::DateTime::parse_from_rfc3339(&self.fetched_at)
            .map_err(|_| ContractError::InvalidTimestamp { field: "fetchedAt" })?;
        let expires_at = chrono::DateTime::parse_from_rfc3339(&self.expires_at)
            .map_err(|_| ContractError::InvalidTimestamp { field: "expiresAt" })?;
        if (expires_at - fetched_at).num_seconds() != LIMITS_TTL_SECONDS {
            return Err(ContractError::Invalid(
                "limits expiry must be exactly five minutes after fetchedAt".to_owned(),
            ));
        }
        if self.resources.len() > MAX_LIMIT_RESOURCES {
            return Err(ContractError::Invalid(
                "too many limit resources".to_owned(),
            ));
        }
        let mut ids = HashSet::new();
        for resource in &self.resources {
            resource.validate()?;
            if !ids.insert(resource.id.as_str()) {
                return Err(ContractError::Duplicate {
                    field: "limit resource",
                    value: resource.id.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LimitResource {
    pub id: String,
    pub label: String,
    pub kind: LimitResourceKind,
    pub source: LimitSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
}

impl Validate for LimitResource {
    fn validate(&self) -> Result<(), ContractError> {
        validate_identifier(&self.id, "resource id")?;
        validate_text(&self.label, "resource label")?;
        if let Some(used) = self.used {
            validate_number(used, "resource used")?;
        }
        if let Some(limit) = self.limit {
            validate_number(limit, "resource limit")?;
        }
        if let Some(resets_at) = &self.resets_at {
            validate_timestamp(resets_at, "resource resetsAt")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum LimitResourceKind {
    RateLimit,
    Credits,
    Balance,
    Spend,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum LimitSource {
    ProviderApi,
    LocalHistory,
    Estimated,
}
