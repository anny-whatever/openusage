use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::contracts::ErrorCategory;

use super::super::support::{ProviderError, json_object, text, timestamp};
use super::auth::CodexAuthStore;
use super::client::CodexClient;

const MAX_IDEMPOTENCY_KEYS: usize = 64;

#[derive(Debug, Clone, Copy)]
pub struct ResetClaimConfirmation(());

impl ResetClaimConfirmation {
    pub fn confirmed() -> Self {
        Self(())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ResetClaimOutcome {
    Success,
    NothingToReset,
    NoCredit,
}

#[async_trait]
pub trait PostClaimRefresh: Send + Sync {
    async fn refresh_codex(&self);
}

#[derive(Default)]
struct ClaimState {
    matched_credit_ids: HashMap<String, String>,
    order: VecDeque<String>,
}

pub struct CodexResetClaimService {
    auth: CodexAuthStore,
    client: CodexClient,
    refresh: Arc<dyn PostClaimRefresh>,
    state: Mutex<ClaimState>,
}

impl CodexResetClaimService {
    pub fn new(
        auth: CodexAuthStore,
        client: CodexClient,
        refresh: Arc<dyn PostClaimRefresh>,
    ) -> Self {
        Self {
            auth,
            client,
            refresh,
            state: Mutex::new(ClaimState::default()),
        }
    }

    pub async fn claim(
        &self,
        expiry: chrono::DateTime<chrono::Utc>,
        idempotency_key: String,
        _confirmation: ResetClaimConfirmation,
        cancellation: CancellationToken,
    ) -> Result<ResetClaimOutcome, ProviderError> {
        let credentials = self.auth.load_all().await?;
        let replay_id = self
            .state
            .lock()
            .await
            .matched_credit_ids
            .get(&idempotency_key)
            .cloned();
        let credit_id = match replay_id {
            Some(credit_id) => credit_id,
            None => {
                let mut matched = None;
                for candidate in &credentials {
                    let response = self
                        .client
                        .reset_credits(
                            &candidate.access_token,
                            candidate.account_id.as_ref(),
                            cancellation.child_token(),
                        )
                        .await?;
                    if is_auth_rejection(response.status) {
                        continue;
                    }
                    if !response.status.is_success() {
                        return Err(ProviderError::http(response.status));
                    }
                    let body = json_object(&response.body)?;
                    matched = matching_credit_id(&body, expiry);
                    break;
                }
                let Some(credit_id) = matched else {
                    self.refresh.refresh_codex().await;
                    return Ok(ResetClaimOutcome::NoCredit);
                };
                self.remember(idempotency_key.clone(), credit_id.clone())
                    .await;
                credit_id
            }
        };
        let mut outcome = None;
        for candidate in &credentials {
            let response = self
                .client
                .consume_reset(
                    &candidate.access_token,
                    candidate.account_id.as_ref(),
                    &credit_id,
                    &idempotency_key,
                    cancellation.child_token(),
                )
                .await?;
            if is_auth_rejection(response.status) {
                continue;
            }
            outcome = Some(consume_outcome(response.status, &response.body)?);
            break;
        }
        let outcome = outcome.ok_or_else(|| {
            ProviderError::new(
                ErrorCategory::AuthExpired,
                "Reset claim was rejected for every Codex login.",
            )
        })?;
        self.refresh.refresh_codex().await;
        Ok(outcome)
    }

    async fn remember(&self, key: String, credit_id: String) {
        let mut state = self.state.lock().await;
        if state.matched_credit_ids.contains_key(&key) {
            return;
        }
        while state.order.len() >= MAX_IDEMPOTENCY_KEYS {
            if let Some(expired) = state.order.pop_front() {
                state.matched_credit_ids.remove(&expired);
            }
        }
        state.order.push_back(key.clone());
        state.matched_credit_ids.insert(key, credit_id);
    }
}

fn is_auth_rejection(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN
}

fn matching_credit_id(
    body: &serde_json::Value,
    expiry: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    body.get("credits")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .filter(|credit| text(credit.get("status")).is_none_or(|status| status == "available"))
        .find_map(|credit| {
            let expires_at = timestamp(credit.get("expires_at"))?;
            let date = chrono::DateTime::parse_from_rfc3339(&expires_at).ok()?;
            (date.timestamp() - expiry.timestamp())
                .abs()
                .lt(&1)
                .then(|| text(credit.get("id")).map(str::to_owned))
                .flatten()
        })
}

fn consume_outcome(
    status: reqwest::StatusCode,
    body: &[u8],
) -> Result<ResetClaimOutcome, ProviderError> {
    if !status.is_success() {
        return Err(ProviderError::http(status));
    }
    let body = json_object(body)?;
    match text(body.get("code")) {
        Some("reset" | "already_redeemed") => Ok(ResetClaimOutcome::Success),
        Some("nothing_to_reset") => Ok(ResetClaimOutcome::NothingToReset),
        Some("no_credit") => Ok(ResetClaimOutcome::NoCredit),
        _ => Err(ProviderError::new(
            ErrorCategory::Decoding,
            "Reset claim response is invalid.",
        )),
    }
}
