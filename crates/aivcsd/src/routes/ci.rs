use axum::{
    body::Bytes,
    extract::{Path, State, Query},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ChecksQueryParams {
    pub repository: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DbCiExecution {
    execution_id: String,
    repository: String,
    pr_number: u32,
    pr_sha: String,
    pr_title: Option<String>,
    status: String, // queued|running|passed|failed
    conclusion: Option<String>, // success|failure|neutral
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    checks: serde_json::Value,
    agent_id: Option<String>,
    approval_required: Option<bool>,
    approval_granted: Option<bool>,
    approved_by: Option<String>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DbCiAuditLog {
    audit_id: String,
    execution_id: String,
    event_kind: String,
    agent_id: Option<String>,
    agent_role: Option<String>,
    result: String,
    reason: Option<String>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize)]
struct ReturnedCheck {
    check_name: String,
    status: String,
    duration_ms: u64,
    error: Option<String>,
}

fn format_checks(checks_val: &serde_json::Value) -> Vec<ReturnedCheck> {
    let mut results = Vec::new();
    if let Some(obj) = checks_val.as_object() {
        for (check_name, details) in obj {
            let status = details
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending")
                .to_string();

            let duration_ms = details
                .get("duration_ms")
                .and_then(|v| v.as_u64())
                .or_else(|| details.get("duration").and_then(|v| v.as_u64()))
                .unwrap_or(0);

            let error = details
                .get("error")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            results.push(ReturnedCheck {
                check_name: check_name.clone(),
                status,
                duration_ms,
                error,
            });
        }
    }
    results
}

pub async fn get_pr_checks(
    State(state): State<AppState>,
    Path(pr_number): Path<u32>,
    Query(params): Query<ChecksQueryParams>,
) -> impl IntoResponse {
    tracing::debug!(
        "Queried CI checks for repo {} PR #{}",
        params.repository,
        pr_number
    );

    // Query SurrealDB for the CI execution record
    let mut db_res = match state
        .db
        .query("SELECT * FROM ci_executions WHERE pr_number = $pr AND repository = $repo ORDER BY created_at DESC LIMIT 1")
        .bind(("pr", pr_number))
        .bind(("repo", params.repository.clone()))
        .await
    {
        Ok(res) => res,
        Err(e) => {
            tracing::error!("Database query failed for ci_executions: {:?}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": format!("Database error: {}", e)
                })),
            ).into_response();
        }
    };

    let executions: Vec<DbCiExecution> = match db_res.take(0) {
        Ok(vec) => vec,
        Err(e) => {
            tracing::error!("Failed to parse ci_executions from response: {:?}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": format!("Failed to parse execution: {}", e)
                })),
            ).into_response();
        }
    };

    let execution = match executions.into_iter().next() {
        Some(exec) => exec,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "status": "error",
                    "message": format!("No CI execution found for PR #{} in repository {}", pr_number, params.repository)
                })),
            ).into_response();
        }
    };

    // Query SurrealDB for the CI audit logs corresponding to this execution
    let mut audit_res = match state
        .db
        .query("SELECT * FROM ci_audit_log WHERE execution_id = $exec_id ORDER BY created_at ASC")
        .bind(("exec_id", execution.execution_id.clone()))
        .await
    {
        Ok(res) => res,
        Err(e) => {
            tracing::error!("Database query failed for ci_audit_log: {:?}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": format!("Database error fetching audit logs: {}", e)
                })),
            ).into_response();
        }
    };

    let audit_trail: Vec<DbCiAuditLog> = match audit_res.take(0) {
        Ok(vec) => vec,
        Err(e) => {
            tracing::error!("Failed to parse ci_audit_log from response: {:?}", e);
            Vec::new() // Fallback to empty log rather than failing the whole request
        }
    };

    // Map database status to expected output status (passed|failed|pending|error)
    let status = match execution.status.as_str() {
        "passed" => "passed",
        "failed" => "failed",
        "queued" | "running" => "pending",
        _ => "pending",
    };

    let formatted_checks = format_checks(&execution.checks);

    (
        StatusCode::OK,
        Json(json!({
            "status": status,
            "checks": formatted_checks,
            "audit_trail": audit_trail,
        })),
    ).into_response()
}

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
