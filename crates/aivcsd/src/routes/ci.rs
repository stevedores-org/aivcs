use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone)]
pub struct CiState {
    pub github_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GithubWebhookPayload {
    pub action: String,
    #[serde(default)]
    pub pull_request: PullRequest,
    pub repository: Repository,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PullRequest {
    pub number: u32,
    pub head: Head,
    pub base: Base,
    pub title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Head {
    pub sha: String,
    pub ref_: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Base {
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

pub fn routes(github_token: String) -> Router {
    let state = CiState { github_token };

    Router::new()
        .route("/webhooks/github", post(handle_github_webhook))
        .route("/checks/:pr_number", get(get_pr_checks))
        .route("/subscribe/:repo", post(subscribe_to_ci))
        .with_state(state)
}

async fn handle_github_webhook(
    State(_state): State<CiState>,
    Json(payload): Json<GithubWebhookPayload>,
) -> impl IntoResponse {
    tracing::info!(
        "GitHub webhook received for {} PR #{}",
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

async fn get_pr_checks(
    State(_state): State<CiState>,
    Path(_pr_number): Path<u32>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "status": "pending",
            "checks": [
                {
                    "name": "type_check",
                    "status": "pending",
                    "message": "Waiting to run"
                },
                {
                    "name": "unit_tests",
                    "status": "pending",
                    "message": "Waiting to run"
                },
                {
                    "name": "secrets_scan",
                    "status": "pending",
                    "message": "Waiting to run"
                },
                {
                    "name": "config_lint",
                    "status": "pending",
                    "message": "Waiting to run"
                }
            ]
        })),
    )
}

async fn subscribe_to_ci(
    State(_state): State<CiState>,
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
