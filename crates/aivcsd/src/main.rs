pub mod routes;

use aivcs_core::cas::CasStore;
use anyhow::{Context, Result};
use axum::{
    body::Bytes,
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use surrealdb::engine::any::connect;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;
use tracing::{info, warn, Level};

#[derive(Clone)]
pub struct AppState {
    pub db: Surreal<surrealdb::engine::any::Any>,
    pub cas: Arc<aivcs_core::cas::fs::FsCasStore>,
}

#[tokio::main]
async fn main() -> Result<()> {
    aivcs_core::init_tracing(false, Level::INFO);
    info!("🚀 aivcsd starting");

    // Verify required env vars for CI integration at startup
    std::env::var("GITHUB_TOKEN").context("GITHUB_TOKEN env var must be set at server startup")?;
    std::env::var("CI_WEBHOOK_SECRET")
        .context("CI_WEBHOOK_SECRET env var must be set at server startup")?;

    let db_url =
        std::env::var("SURREALDB_URL").unwrap_or_else(|_| "ws://localhost:8000".to_string());
    let db_user = std::env::var("SURREALDB_USER").unwrap_or_else(|_| "root".to_string());
    let db_pass = std::env::var("SURREALDB_PASS").unwrap_or_else(|_| "root".to_string());

    info!("🔌 Connecting to SurrealDB at {}", db_url);

    let db = connect(&db_url)
        .await
        .context("Failed to connect to SurrealDB")?;

    db.signin(Root {
        username: &db_user,
        password: &db_pass,
    })
    .await
    .context("Failed to authenticate with SurrealDB")?;

    let db_ns = std::env::var("SURREALDB_NS")
        .or_else(|_| std::env::var("SURREALDB_NAMESPACE"))
        .unwrap_or_else(|_| "ci".to_string());
    let db_name = std::env::var("SURREALDB_DB").unwrap_or_else(|_| "fft".to_string());

    db.use_ns(&db_ns).use_db(&db_name).await?;
    info!(
        "✅ Connected to SurrealDB and selected namespace '{}' database '{}'",
        db_ns, db_name
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

    let state = AppState { db, cas };

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/version", get(version_info))
        .route("/api/v1/push", post(push_state))
        .route("/api/v1/blobs/upload", post(upload_blob))
        .route(
            "/api/v1/ci/webhooks/github",
            post(routes::ci::handle_github_webhook),
        )
        .route(
            "/api/v1/ci/checks/:pr_number",
            get(routes::ci::get_ci_checks),
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_version_info() {
        let res = version_info().await;
        assert_eq!(res.0["name"], "aivcsd");
    }
}
