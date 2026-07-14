//! GCP Secret Manager operator integration for AIVCS secrets plane writes.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use base64::Engine;
use google_cloud_auth::project::Config;
use google_cloud_auth::token::DefaultTokenSourceProvider;
use google_cloud_token::{TokenSource, TokenSourceProvider};
use reqwest::Client;
use serde::Deserialize;

#[derive(Clone)]
pub struct GsmClient {
    http: Client,
    project_id: String,
    token_source: Arc<dyn TokenSource>,
}

#[derive(Debug, Deserialize)]
struct ListSecretsResponse {
    secrets: Option<Vec<SecretEntry>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SecretEntry {
    name: String, // projects/{project_id}/secrets/{secret_id}
}

/// Map Vault KV v2 paths (`secret/data/...`) to GSM secret ids, replacing `/` with `-`
/// for perfect compatibility with Secret Manager.
pub fn vault_kv_path_to_gsm_secret_id(vault_path: &str, prefix: &str) -> Option<String> {
    let path = vault_path.trim_matches('/');
    match path {
        "ci/gcp" => Some(format!("{prefix}ci-gcp")),
        p if p.starts_with("ci/repos/") => {
            let repo = p.strip_prefix("ci/repos/")?;
            if repo.is_empty() {
                None
            } else {
                Some(format!("{prefix}ci-repos-{}", repo.replace('/', "-")))
            }
        }
        p if p.starts_with("agents/") => {
            let id = p.strip_prefix("agents/")?;
            if id.is_empty() {
                None
            } else {
                Some(format!("{prefix}agents-{}", id.replace('/', "-")))
            }
        }
        p if p.starts_with("kubernetes/") => {
            let suffix = p.strip_prefix("kubernetes/")?;
            if suffix.is_empty() {
                None
            } else {
                Some(format!("{prefix}kubernetes-{}", suffix.replace('/', "-")))
            }
        }
        p if p.starts_with("prod/") => {
            let suffix = p.strip_prefix("prod/")?;
            if suffix.is_empty() {
                None
            } else {
                Some(format!("{prefix}prod-{}", suffix.replace('/', "-")))
            }
        }
        _ => None,
    }
}

/// Resolves GCP project ID from override option or environment variables.
pub fn resolve_project_id(override_project: Option<&str>) -> Result<String> {
    if let Some(p) = override_project {
        return Ok(p.to_string());
    }
    if let Ok(p) = std::env::var("GCP_PROJECT_ID") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    if let Ok(p) = std::env::var("GOOGLE_CLOUD_PROJECT") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    bail!("GCP Project ID not specified. Use --project or set GCP_PROJECT_ID / GOOGLE_CLOUD_PROJECT environment variable.")
}

impl GsmClient {
    pub async fn new(project_id: String) -> Result<Self> {
        let provider = DefaultTokenSourceProvider::new(
            Config::default().with_scopes(&["https://www.googleapis.com/auth/cloud-platform"]),
        )
        .await
        .context("init GCP token source")?;
        let token_source = provider.token_source();
        Ok(Self {
            http: Client::new(),
            project_id,
            token_source,
        })
    }

    async fn get_token(&self) -> Result<String> {
        let token = self
            .token_source
            .token()
            .await
            .map_err(|e| anyhow::anyhow!("fetch GCP access token: {e}"))?;
        let bearer = token.strip_prefix("Bearer ").unwrap_or(token.as_str());
        Ok(bearer.to_string())
    }

    /// Creates or updates a GSM secret. Checks that the serialized payload
    /// does not exceed the GCP Secret Manager size limit (64KiB).
    pub async fn store_secret(&self, secret_id: &str, data: HashMap<String, String>) -> Result<()> {
        let payload_bytes = serde_json::to_vec(&data).context("serialize secret payload")?;
        if payload_bytes.len() > 64 * 1024 {
            bail!(
                "Secret payload size ({} bytes) exceeds GCP Secret Manager limit of 64 KiB",
                payload_bytes.len()
            );
        }

        let bearer = self.get_token().await?;

        // 1. Try to create the secret. It will return 409 Conflict if already exists.
        let create_url = format!(
            "https://secretmanager.googleapis.com/v1/projects/{}/secrets?secretId={}",
            self.project_id, secret_id
        );
        let create_body = serde_json::json!({
            "replication": {
                "automatic": {}
            }
        });

        let create_resp = self
            .http
            .post(&create_url)
            .bearer_auth(&bearer)
            .json(&create_body)
            .send()
            .await
            .context("GSM create secret request")?;

        let status = create_resp.status();
        if status.is_success() {
            tracing::info!("Created new GSM secret: {}", secret_id);
        } else if status == reqwest::StatusCode::CONFLICT {
            tracing::debug!("GSM secret {} already exists, skipping creation", secret_id);
        } else {
            let err_text = create_resp.text().await.unwrap_or_default();
            bail!(
                "GSM create secret failed with status {}: {}",
                status,
                err_text
            );
        }

        // 2. Add secret version (upsert logic).
        let add_version_url = format!(
            "https://secretmanager.googleapis.com/v1/projects/{}/secrets/{}/versions:addVersion",
            self.project_id, secret_id
        );
        let b64_data = base64::engine::general_purpose::STANDARD.encode(&payload_bytes);
        let add_body = serde_json::json!({
            "payload": {
                "data": b64_data
            }
        });

        self.http
            .post(&add_version_url)
            .bearer_auth(&bearer)
            .json(&add_body)
            .send()
            .await
            .context("GSM add secret version request")?
            .error_for_status()
            .context("GSM add secret version denied or failed")?;

        tracing::info!(
            "Successfully added a new version to GSM secret {}",
            secret_id
        );
        Ok(())
    }

    /// List all secrets matching canonical prefix, stripping prefix when presenting.
    pub async fn list_secrets(&self, prefix: &str) -> Result<Vec<String>> {
        let mut secrets = Vec::new();
        let mut page_token = None;
        let bearer = self.get_token().await?;

        loop {
            let mut url = format!(
                "https://secretmanager.googleapis.com/v1/projects/{}/secrets?pageSize=100",
                self.project_id
            );
            if let Some(ref token) = page_token {
                url = format!("{}&pageToken={}", url, token);
            }

            let resp = self
                .http
                .get(&url)
                .bearer_auth(&bearer)
                .send()
                .await
                .context("GSM list secrets request failed")?
                .error_for_status()
                .context("GSM list secrets request denied or failed")?;

            let body: ListSecretsResponse = resp.json().await.context("parse GSM list response")?;
            if let Some(entries) = body.secrets {
                for entry in entries {
                    if let Some(secret_id) = entry.name.rsplit('/').next() {
                        if secret_id.starts_with(prefix) {
                            let stripped = secret_id.strip_prefix(prefix).unwrap_or(secret_id);
                            secrets.push(stripped.to_string());
                        }
                    }
                }
            }

            if let Some(token) = body.next_page_token {
                if token.is_empty() {
                    break;
                }
                page_token = Some(token);
            } else {
                break;
            }
        }

        Ok(secrets)
    }

    /// Mark/delete a secret completely in GCP Secret Manager.
    pub async fn delete_secret(&self, secret_id: &str) -> Result<()> {
        let url = format!(
            "https://secretmanager.googleapis.com/v1/projects/{}/secrets/{}",
            self.project_id, secret_id
        );
        let bearer = self.get_token().await?;

        self.http
            .delete(&url)
            .bearer_auth(&bearer)
            .send()
            .await
            .context("GSM delete secret request failed")?
            .error_for_status()
            .context("GSM delete secret denied or failed (secret might not exist)")?;

        tracing::info!("Deleted GSM secret: {}", secret_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_kv_path_to_gsm_secret_id() {
        assert_eq!(
            vault_kv_path_to_gsm_secret_id("ci/gcp", "aivcs-secrets--"),
            Some("aivcs-secrets--ci-gcp".into())
        );
        assert_eq!(
            vault_kv_path_to_gsm_secret_id("ci/repos/lornu-ai/aivcs-lornu-demo", "aivcs-secrets--"),
            Some("aivcs-secrets--ci-repos-lornu-ai-aivcs-lornu-demo".into())
        );
        assert_eq!(
            vault_kv_path_to_gsm_secret_id("agents/code-review-agent", "aivcs-secrets--"),
            Some("aivcs-secrets--agents-code-review-agent".into())
        );
        assert_eq!(
            vault_kv_path_to_gsm_secret_id("kubernetes/lornu-ai-prod/my-secret", "aivcs-secrets--"),
            Some("aivcs-secrets--kubernetes-lornu-ai-prod-my-secret".into())
        );
        assert_eq!(
            vault_kv_path_to_gsm_secret_id("prod/my-secret", "aivcs-secrets--"),
            Some("aivcs-secrets--prod-my-secret".into())
        );
        assert_eq!(
            vault_kv_path_to_gsm_secret_id("invalid/path", "aivcs-secrets--"),
            None
        );
    }
}
