use aivcs_core::cas::CasStore;
use anyhow::{Context, Result};
use axum::{
    body::Bytes,
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use surrealdb::engine::remote::ws::Ws;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;
use tracing::{info, warn, Level};

mod ci;

#[derive(Clone)]
struct AppState {
    db: Surreal<surrealdb::engine::remote::ws::Client>,
    cas: Arc<aivcs_core::cas::fs::FsCasStore>,
    #[allow(dead_code)]
    db_namespace: String,
    #[allow(dead_code)]
    db_name: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    aivcs_core::init_tracing(false, Level::INFO);
    info!("🚀 aivcsd starting");

    let db_url =
        std::env::var("SURREALDB_URL").unwrap_or_else(|_| "ws://localhost:8000".to_string());
    let db_user = std::env::var("SURREALDB_USER").unwrap_or_else(|_| "root".to_string());
    let db_pass = std::env::var("SURREALDB_PASS").unwrap_or_else(|_| "root".to_string());

    info!("🔌 Connecting to SurrealDB at {}", db_url);

    let db = Surreal::new::<Ws>(&db_url)
        .await
        .context("Failed to connect to SurrealDB")?;

    db.signin(Root {
        username: &db_user,
        password: &db_pass,
    })
    .await
    .context("Failed to authenticate with SurrealDB")?;

    // Make namespace and database configurable
    let db_namespace = std::env::var("SURREALDB_NAMESPACE").unwrap_or_else(|_| "aivcs".to_string());
    let db_name = std::env::var("SURREALDB_DB").unwrap_or_else(|_| "core".to_string());

    db.use_ns(&db_namespace).use_db(&db_name).await?;
    info!(
        "✅ Connected to SurrealDB and selected namespace '{}' database '{}'",
        db_namespace, db_name
    );

    // Initialize Schema
    let schema = include_str!("../schemas/001_synthetic_principal.surql");
    db.query(schema).await.context("Failed to apply schema")?;
    info!("✅ Schema initialized successfully");

    let cas_dir = std::env::var("AIVCS_CAS_DIR").unwrap_or_else(|_| ".aivcs/cas".to_string());
    let cas = Arc::new(
        aivcs_core::cas::fs::FsCasStore::new(std::path::PathBuf::from(cas_dir))
            .context("Failed to initialize CAS store")?,
    );
    info!("📦 Initialized CAS store");

    let state = AppState {
        db,
        cas,
        db_namespace: db_namespace.clone(),
        db_name: db_name.clone(),
    };

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/version", get(version_info))
        .route("/api/v1/push", post(push_state))
        .route("/api/v1/blobs/upload", post(upload_blob))
        .route("/api/v1/ci/webhook", post(ci_webhook_handler))
        .route("/api/v1/ci/checks", get(get_ci_checks))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("📡 listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check(State(state): State<AppState>) -> Json<Value> {
    let db_status = if state.db.version().await.is_ok() {
        "connected"
    } else {
        "disconnected"
    };

    Json(json!({
        "status": "healthy",
        "database": db_status,
        "timestamp": chrono::Utc::now()
    }))
}

#[derive(serde::Deserialize, Debug)]
struct PushPayload {
    agent_id: String,
    hive_id: String,
    message: String,
    blob_hash: String, // Points to S3 CAS
}

async fn push_state(
    State(state): State<AppState>,
    Json(payload): Json<PushPayload>,
) -> Json<Value> {
    // In the real system, we'd verify the cryptographic signature of the agent.
    info!(
        "📥 Received state push from agent {} for hive {}",
        payload.agent_id, payload.hive_id
    );

    // Create the commit in SurrealDB
    let create_result: Result<Option<Value>, _> = state
        .db
        .create("commit")
        .content(json!({
            "message": payload.message,
            "blob_hash": payload.blob_hash,
            "author": format!("agent:{}", payload.agent_id),
            "hive": format!("hive:{}", payload.hive_id),
            "created_at": chrono::Utc::now()
        }))
        .await;

    match create_result {
        Ok(_) => Json(json!({
            "status": "success",
            "message": "State commit recorded successfully in semantic graph"
        })),
        Err(e) => {
            warn!("Failed to record commit: {:?}", e);
            Json(json!({
                "status": "error",
                "message": format!("Database error: {}", e)
            }))
        }
    }
}

async fn upload_blob(State(state): State<AppState>, body: Bytes) -> Json<Value> {
    info!("📥 Received raw blob upload of {} bytes", body.len());

    match state.cas.put(&body) {
        Ok(digest) => Json(json!({
            "status": "success",
            "blob_hash": digest.to_string(),
            "message": "Blob stored successfully"
        })),
        Err(e) => {
            warn!("Failed to store blob: {:?}", e);
            Json(json!({
                "status": "error",
                "message": format!("CAS storage error: {}", e)
            }))
        }
    }
}

async fn version_info() -> Json<Value> {
    Json(json!({
        "name": "aivcsd",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": aivcs_core::domain::Platform::detect().to_string()
    }))
}

/// GitHub webhook payload for FFT execution tracking
#[derive(serde::Deserialize, Debug)]
struct GitHubWebhookPayload {
    #[allow(dead_code)]
    action: Option<String>,
    pull_request: Option<GitHubPR>,
    repository: Option<GitHubRepository>,
}

#[derive(serde::Deserialize, Debug)]
struct GitHubPR {
    number: u64,
    #[allow(dead_code)]
    title: String,
    #[serde(default)]
    head: GitHubRef,
}

#[derive(serde::Deserialize, Debug, Default)]
#[serde(default)]
struct GitHubRef {
    sha: String,
}

#[derive(serde::Deserialize, Debug)]
struct GitHubRepository {
    full_name: String,
    #[allow(dead_code)]
    name: String,
}

/// Handle GitHub webhook for CI execution tracking
async fn ci_webhook_handler(
    State(state): State<AppState>,
    Json(payload): Json<GitHubWebhookPayload>,
) -> (axum::http::StatusCode, Json<Value>) {
    // Validate payload structure
    let pr = match &payload.pull_request {
        Some(pr) => pr,
        None => {
            warn!("⚠️ GitHub webhook missing pull_request");
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error": "Missing pull_request"})),
            );
        }
    };

    let repo = match &payload.repository {
        Some(r) => r,
        None => {
            warn!("⚠️ GitHub webhook missing repository");
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error": "Missing repository"})),
            );
        }
    };

    info!(
        "📥 GitHub webhook: {}/pull/{} ({})",
        repo.full_name, pr.number, pr.head.sha
    );

    // Create execution record in SurrealDB
    let execution_id = format!("{}-{}", repo.full_name, pr.number);
    let create_result: Result<Option<Value>, _> = state
        .db
        .create("ci_executions")
        .content(json!({
            "id": execution_id,
            "repository": repo.full_name,
            "pr_number": pr.number,
            "sha": pr.head.sha,
            "status": "pending",
            "checks": [],
            "duration_ms": 0,
            "created_at": chrono::Utc::now().to_rfc3339(),
            "completed_at": Value::Null,
        }))
        .await;

    match create_result {
        Ok(_) => {
            info!(
                "✅ Recorded FFT execution for {}/pull/{}",
                repo.full_name, pr.number
            );
            (
                axum::http::StatusCode::ACCEPTED,
                Json(json!({
                    "status": "accepted",
                    "execution_id": execution_id,
                    "message": "Execution recorded in SurrealDB"
                })),
            )
        }
        Err(e) => {
            warn!("❌ Failed to record FFT execution: {:?}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to record execution",
                    "details": e.to_string()
                })),
            )
        }
    }
}

/// Query CI check results by PR and repository
async fn get_ci_checks(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> (axum::http::StatusCode, Json<Value>) {
    let pr_number = match params.get("pr_number").and_then(|s| s.parse::<u64>().ok()) {
        Some(n) => n,
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error": "Missing or invalid pr_number query parameter"})),
            );
        }
    };

    let repository = match params.get("repository") {
        Some(r) => r.clone(),
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({"error": "Missing repository query parameter"})),
            );
        }
    };

    info!("🔍 Querying checks for {}/pull/{}", repository, pr_number);

    // Query execution record from SurrealDB
    let query_result = state
        .db
        .query("SELECT * FROM ci_executions WHERE pr_number = $pr AND repository = $repo LIMIT 1")
        .bind(("pr", pr_number))
        .bind(("repo", repository.clone()))
        .await;

    match query_result {
        Ok(mut response) => match response.take::<Vec<Value>>(0) {
            Ok(executions) => {
                if let Some(execution) = executions.first() {
                    return (axum::http::StatusCode::OK, Json(execution.clone()));
                }
                (
                    axum::http::StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "No execution found",
                        "pr_number": pr_number,
                        "repository": repository
                    })),
                )
            }
            Err(e) => {
                warn!("❌ Failed to parse query response: {:?}", e);
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "Failed to parse response",
                        "details": e.to_string()
                    })),
                )
            }
        },
        Err(e) => {
            warn!("❌ Failed to query CI checks: {:?}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to query checks",
                    "details": e.to_string()
                })),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_version_info() {
        let res = version_info().await;
        assert_eq!(res.0["name"], "aivcsd");
    }
}
