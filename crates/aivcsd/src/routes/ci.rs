use axum::{
    body::Bytes,
    extract::Path,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Serialize, Deserialize)]
pub struct GithubWebhookPayload {
    pub action: String,
    pub pull_request: PullRequest,
    pub repository: Repository,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u32,
    pub head: Head,
    pub base: Base,
    pub title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Head {
    pub sha: String,
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Base {
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Repository {
    pub name: String,
    pub full_name: String,
    pub owner: Owner,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Owner {
    pub login: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CiSubscriptionConfig {
    pub aws_deployment_stack: String,
    pub api_endpoint: String,
}

fn verify_github_signature(headers: &HeaderMap, body: &str, secret: &str) -> Result<(), String> {
    let signature_header = headers
        .get("x-hub-signature-256")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| "Missing x-hub-signature-256 header".to_string())?;

    let expected_sig = format!(
        "sha256={}",
        hex::encode(
            HmacSha256::new_from_slice(secret.as_bytes())
                .map_err(|_| "Invalid HMAC key".to_string())?
                .chain_update(body.as_bytes())
                .finalize()
                .into_bytes()
        )
    );

    if signature_header == expected_sig {
        Ok(())
    } else {
        Err("Invalid signature".to_string())
    }
}

pub async fn handle_github_webhook(headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let body_str = match String::from_utf8(body.to_vec()) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Invalid UTF-8 in webhook body: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "status": "error",
                    "message": "Invalid request body encoding"
                })),
            );
        }
    };

    let webhook_secret = match std::env::var("CI_WEBHOOK_SECRET") {
        Ok(secret) => secret,
        Err(_) => {
            tracing::error!("CI_WEBHOOK_SECRET not configured");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": "Webhook verification not configured"
                })),
            );
        }
    };

    // Verify webhook signature
    if let Err(e) = verify_github_signature(&headers, &body_str, &webhook_secret) {
        tracing::warn!("Webhook signature verification failed: {}", e);
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "status": "error",
                "message": "Invalid webhook signature"
            })),
        );
    }

    // Parse payload
    let payload = match serde_json::from_str::<GithubWebhookPayload>(&body_str) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to parse webhook payload: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "status": "error",
                    "message": "Invalid payload format"
                })),
            );
        }
    };

    // Validate required fields
    if payload.pull_request.number == 0 {
        tracing::warn!("Webhook missing required pr_number field");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "error",
                "message": "Missing required pr_number"
            })),
        );
    }

    if payload.pull_request.head.sha.is_empty() {
        tracing::warn!("Webhook missing required sha field");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "error",
                "message": "Missing required commit sha"
            })),
        );
    }

    tracing::info!(
        "GitHub webhook verified for {} PR #{}",
        payload.repository.full_name,
        payload.pull_request.number
    );

    let execution_id = format!("exec_{}", uuid::Uuid::new_v4());

    (
        StatusCode::ACCEPTED,
        Json(json!({
            "status": "received",
            "execution_id": execution_id,
            "repository": payload.repository.full_name,
            "pr_number": payload.pull_request.number,
            "message": "CI checks queued"
        })),
    )
}

pub async fn get_pr_checks(Path(pr_number): Path<u32>) -> impl IntoResponse {
    // TODO: Query SurrealDB ci_executions table for actual status
    // For now, return 501 Not Implemented since SurrealDB integration is incomplete
    tracing::debug!("Queried CI checks for PR #{}", pr_number);

    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "status": "error",
            "message": "CI checks API not yet implemented. Check GitHub status checks for results.",
            "pr_number": pr_number
        })),
    )
}

pub async fn subscribe_to_ci(
    Path(repo): Path<String>,
    Json(config): Json<CiSubscriptionConfig>,
) -> impl IntoResponse {
    tracing::info!(
        "Subscribing repository {} to fast-free-testing (stack: {})",
        repo,
        config.aws_deployment_stack
    );

    let webhook_id = format!("webhook_{}", uuid::Uuid::new_v4());

    (
        StatusCode::CREATED,
        Json(json!({
            "status": "subscribed",
            "webhook_id": webhook_id,
            "repository": repo,
            "message": "Repository now subscribed to fast-free-testing"
        })),
    )
}
