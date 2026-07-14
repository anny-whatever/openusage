use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::platform::atomic_file::FileStoreError;

pub(super) const CURRENT_SCHEMA: u32 = 3;
const MAX_LAYOUT_IDS: usize = 128;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Density {
    Regular,
    Compact,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum MeterStyle {
    Used,
    Remaining,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ResetDisplay {
    Automatic,
    Countdown,
    Time,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Error,
    Info,
    Debug,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationSettings {
    pub under_ten_percent: bool,
    pub healthy_to_close: bool,
    pub close_to_running_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricLayout {
    pub ordered_metric_ids: Vec<String>,
    pub hidden_metric_ids: BTreeSet<String>,
    pub on_demand_metric_ids: BTreeSet<String>,
    pub starred_metric_ids: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub schema: u32,
    pub enabled_provider_ids: BTreeSet<String>,
    pub known_provider_ids: BTreeSet<String>,
    pub provider_order: Vec<String>,
    pub metric_layouts: BTreeMap<String, MetricLayout>,
    pub show_total_spend: bool,
    pub always_show_pacing: bool,
    pub appearance: Appearance,
    pub density: Density,
    pub meter_style: MeterStyle,
    pub reset_display: ResetDisplay,
    pub launch_at_login: bool,
    pub notifications: NotificationSettings,
    pub share_anonymous_usage: bool,
    pub log_level: LogLevel,
    pub automatically_check_updates: bool,
    pub beta_updates: bool,
    pub first_run_hint_dismissed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: CURRENT_SCHEMA,
            enabled_provider_ids: ["claude", "codex", "cursor"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            known_provider_ids: BTreeSet::new(),
            provider_order: provider_order(),
            metric_layouts: BTreeMap::new(),
            show_total_spend: true,
            always_show_pacing: false,
            appearance: Appearance::System,
            density: Density::Regular,
            meter_style: MeterStyle::Remaining,
            reset_display: ResetDisplay::Automatic,
            launch_at_login: false,
            notifications: NotificationSettings::default(),
            share_anonymous_usage: false,
            log_level: LogLevel::Info,
            automatically_check_updates: true,
            beta_updates: false,
            first_run_hint_dismissed: false,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.schema != CURRENT_SCHEMA {
            return Err(SettingsError::Invalid(
                "unsupported settings schema".to_owned(),
            ));
        }
        let registry = provider_registry();
        validate_id_set(&self.enabled_provider_ids, &registry, "enabled providers")?;
        validate_id_set(&self.known_provider_ids, &registry, "known providers")?;
        if self.provider_order.len() != registry.len()
            || self.provider_order.iter().collect::<BTreeSet<_>>().len() != registry.len()
            || self.provider_order.iter().any(|id| !registry.contains(id))
        {
            return Err(SettingsError::Invalid(
                "provider order must contain every provider exactly once".to_owned(),
            ));
        }
        for (provider_id, layout) in &self.metric_layouts {
            if !registry.contains(provider_id) {
                return Err(SettingsError::Invalid(
                    "unknown metric layout provider".to_owned(),
                ));
            }
            validate_layout(layout)?;
        }
        Ok(())
    }
}

fn validate_id_set(
    values: &BTreeSet<String>,
    registry: &BTreeSet<String>,
    label: &str,
) -> Result<(), SettingsError> {
    if values.len() > registry.len() || values.iter().any(|id| !registry.contains(id)) {
        return Err(SettingsError::Invalid(format!(
            "{label} contain an unknown provider"
        )));
    }
    Ok(())
}

fn validate_layout(layout: &MetricLayout) -> Result<(), SettingsError> {
    let ordered = layout.ordered_metric_ids.iter().collect::<BTreeSet<_>>();
    if layout.ordered_metric_ids.len() > MAX_LAYOUT_IDS
        || ordered.len() != layout.ordered_metric_ids.len()
        || layout.hidden_metric_ids.len() > MAX_LAYOUT_IDS
        || layout.on_demand_metric_ids.len() > MAX_LAYOUT_IDS
        || layout.starred_metric_ids.len() > MAX_LAYOUT_IDS
    {
        return Err(SettingsError::Invalid(
            "metric layout exceeds its fixed bound".to_owned(),
        ));
    }
    for id in layout
        .ordered_metric_ids
        .iter()
        .chain(layout.hidden_metric_ids.iter())
        .chain(layout.on_demand_metric_ids.iter())
        .chain(layout.starred_metric_ids.iter())
    {
        if !valid_metric_id(id) {
            return Err(SettingsError::Invalid(
                "metric layout contains an invalid id".to_owned(),
            ));
        }
    }
    if !layout
        .starred_metric_ids
        .is_disjoint(&layout.hidden_metric_ids)
    {
        return Err(SettingsError::Invalid(
            "hidden metrics cannot be starred".to_owned(),
        ));
    }
    Ok(())
}

fn valid_metric_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings I/O failed: {0}")]
    File(#[from] FileStoreError),
    #[error("settings encoding failed: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("settings are invalid: {0}")]
    Invalid(String),
}

pub fn provider_order() -> Vec<String> {
    [
        "claude",
        "codex",
        "cursor",
        "antigravity",
        "copilot",
        "devin",
        "grok",
        "opencode",
        "openrouter",
        "zai",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub fn provider_registry() -> BTreeSet<String> {
    provider_order().into_iter().collect()
}
