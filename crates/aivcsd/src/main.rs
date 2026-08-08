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

    // CI is reconciled through outbound GitHub API calls. Fail closed when the
    // token or repository allowlist is absent rather than starting without CI.
    let github_token = std::env::var("GITHUB_TOKEN")
        .context("GITHUB_TOKEN env var must be set at server startup")?;
    let reconciler_config = routes::ci::ReconcilerConfig::from_env()?;

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
    let ci_schema = include_str!("../schemas/002_ci_checks.surql");
    db.query(ci_schema)
        .await
        .context("Failed to apply CI checks schema")?;
    let reconciler_schema = include_str!("../schemas/003_ci_reconciler.surql");
    db.query(reconciler_schema)
        .await
        .context("Failed to apply CI reconciler schema")?;
    info!("✅ Schema initialized successfully");

    let cas_dir = std::env::var("AIVCS_CAS_DIR").unwrap_or_else(|_| ".aivcs/cas".to_string());
    let cas = Arc::new(
        aivcs_core::cas::fs::FsCasStore::new(std::path::PathBuf::from(cas_dir))
            .context("Failed to initialize CAS store")?,
    );
    info!("📦 Initialized CAS store");

    let state = AppState { db, cas };
    tokio::spawn(routes::ci::run_reconciler(
        state.clone(),
        github_token,
        reconciler_config,
    ));

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/version", get(version_info))
        .route("/api/v1/push", post(push_state))
        .route("/api/v1/blobs/upload", post(upload_blob))
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
    // Signature over the payload by the agent's key. Not yet verified;
    // recorded as-is so later phases can audit retroactively.
    signature: Option<String>,
}

/// Record a state push: ensure the hive and agent records exist, then create
/// the commit with proper record references per `001_synthetic_principal.surql`
/// (`agent_id: record<agent>`, `hive_id: record<hive>`, `signature` required).
async fn record_push(
    db: &Surreal<surrealdb::engine::any::Any>,
    payload: &PushPayload,
) -> Result<Value> {
    let mut response = db
        .query(
            "UPSERT type::thing('hive', $hive_id) SET name = $hive_id;
             UPSERT type::thing('agent', $agent_id) SET
                 name = $agent_id,
                 public_key = $public_key,
                 hive_id = type::thing('hive', $hive_id),
                 role = 'agent';
             CREATE commit SET
                 agent_id = type::thing('agent', $agent_id),
                 hive_id = type::thing('hive', $hive_id),
                 message = $message,
                 blob_hash = $blob_hash,
                 signature = $signature
             RETURN record::id(id) AS commit_id, message, blob_hash, signature;",
        )
        .bind(("hive_id", payload.hive_id.clone()))
        .bind(("agent_id", payload.agent_id.clone()))
        // Placeholder until agent identity bootstrap supplies a real key;
        // must stay unique per agent (idx_agent_pubkey).
        .bind(("public_key", format!("unverified:{}", payload.agent_id)))
        .bind(("message", payload.message.clone()))
        .bind(("blob_hash", payload.blob_hash.clone()))
        .bind((
            "signature",
            payload
                .signature
                .clone()
                .unwrap_or_else(|| "unsigned".to_string()),
        ))
        .await
        .context("push query failed")?;

    let created: Vec<Value> = response.take(2).context("commit create failed")?;
    created
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("commit create returned no record"))
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

    match record_push(&state.db, &payload).await {
        Ok(commit) => Json(json!({
            "status": "success",
            "message": "State commit recorded successfully in semantic graph",
            "commit_id": commit.get("commit_id").cloned().unwrap_or(Value::Null)
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

    /// Regression test: push_state must write commits that satisfy the real
    /// SCHEMAFULL schema (record<agent>/record<hive> refs + signature), not
    /// ad-hoc field names. Runs against the same schema main() applies.
    #[tokio::test]
    async fn test_record_push_matches_schema() {
        let db = connect("mem://").await.unwrap();
        db.use_ns("ci").use_db("fft").await.unwrap();
        db.query(include_str!("../schemas/001_synthetic_principal.surql"))
            .await
            .unwrap()
            .check()
            .unwrap();

        let payload = PushPayload {
            agent_id: "claude-code-01".to_string(),
            hive_id: "sparky-verify".to_string(),
            message: "first snapshot".to_string(),
            blob_hash: "sha256:deadbeef".to_string(),
            signature: None,
        };

        let commit = record_push(&db, &payload)
            .await
            .expect("push should satisfy schema");
        assert_eq!(commit["message"], "first snapshot");
        assert_eq!(commit["signature"], "unsigned");
        assert!(
            commit["commit_id"].is_string(),
            "commit_id should be returned"
        );

        // Same agent pushing again must reuse the upserted agent/hive records
        // (unique public_key index) — only the duplicate blob_hash is rejected.
        let dup = record_push(&db, &payload).await;
        assert!(
            dup.is_err(),
            "duplicate blob_hash must be rejected by idx_commit_blob"
        );

        let second = PushPayload {
            blob_hash: "sha256:cafebabe".to_string(),
            signature: Some("sig:test".to_string()),
            ..payload
        };
        let commit2 = record_push(&db, &second).await.expect("second push");
        assert_eq!(commit2["signature"], "sig:test");
    }
}
