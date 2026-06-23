use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, Level};

#[derive(serde::Deserialize)]
struct CheckParams {
    repo: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct CIExecutionRecord {
    id: surrealdb::sql::Thing,
    pr_number: u64,
    repository: String,
    status: String,
    started_at: Option<String>,
    completed_at: Option<String>,
    duration_ms: Option<u64>,
    checks: Option<serde_json::Value>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct CIAuditRecord {
    id: Option<surrealdb::sql::Thing>,
    execution_id: Option<surrealdb::sql::Thing>,
    event: String,
    #[serde(rename = "check")]
    check_name: Option<String>,
    created_at: Option<String>,
    timestamp: Option<String>,
    actor: Option<String>,
    result: Option<String>,
    duration_ms: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    aivcs_core::init_tracing(false, Level::INFO);

    info!("🚀 aivcsd starting");

    let db_handle = match oxidized_state::SurrealHandle::setup_from_env().await {
        Ok(handle) => Arc::new(handle),
        Err(e) => {
            tracing::error!("Failed to connect to SurrealDB: {}", e);
            return Err(anyhow::anyhow!("Failed to initialize database: {}", e));
        }
    };

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/version", get(version_info))
        .route("/api/v1/ci/checks/:pr_number", get(ci_checks))
        .with_state(db_handle);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("📡 listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now()
    }))
}

async fn version_info() -> Json<Value> {
    Json(json!({
        "name": "aivcsd",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": aivcs_core::domain::Platform::detect().to_string()
    }))
}

async fn ci_checks(
    State(handle): State<Arc<oxidized_state::SurrealHandle>>,
    Path(pr_number): Path<u64>,
    Query(params): Query<CheckParams>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let db = handle.db();

    let mut response = db
        .query("SELECT * FROM ci_executions WHERE repository = $repo AND pr_number = $pr LIMIT 1")
        .bind(("repo", params.repo.clone()))
        .bind(("pr", pr_number))
        .await
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "Database unavailable" }))))?;

    let execution: Option<CIExecutionRecord> = response
        .take(0)
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "Database unavailable" }))))?;

    let execution = match execution {
        Some(exec) => exec,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("PR #{} not found in {}", pr_number, params.repo) })),
            ));
        }
    };

    let mut audit_response = db
        .query("SELECT * FROM ci_audit_log WHERE execution_id = $id ORDER BY created_at ASC")
        .bind(("id", execution.id.clone()))
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": format!("audit query error: {}", e) }))))?;

    let audit_trail: Vec<CIAuditRecord> = audit_response
        .take(0)
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": format!("audit take error: {}", e) }))))?;

    Ok(Json(json!({
        "pr_number": execution.pr_number,
        "repository": execution.repository,
        "status": execution.status,
        "started_at": execution.started_at,
        "completed_at": execution.completed_at,
        "duration_ms": execution.duration_ms,
        "checks": execution.checks.unwrap_or(json!([])),
        "audit_trail": audit_trail,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check() {
        let res = health_check().await;
        assert_eq!(res.0["status"], "healthy");
    }

    #[tokio::test]
    async fn test_ci_checks_not_found() {
        let handle = Arc::new(oxidized_state::SurrealHandle::setup_db().await.unwrap());
        let params = CheckParams {
            repo: "stevedores-org/aivcs".to_string(),
        };
        let res = ci_checks(State(handle), Path(999), Query(params)).await;
        assert!(res.is_err());
        let (status, body) = res.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body.0["error"], "PR #999 not found in stevedores-org/aivcs");
    }

    #[tokio::test]
    async fn test_ci_checks_success() {
        let handle = Arc::new(oxidized_state::SurrealHandle::setup_db().await.unwrap());
        let db = handle.db();

        // Insert mock execution
        let _ = db
            .query("CREATE ci_executions:mock_exec CONTENT { pr_number: 100, repository: 'owner/repo', status: 'passed', started_at: '2026-06-22T22:37:58Z', completed_at: '2026-06-22T22:38:02Z', duration_ms: 4000, checks: [{ name: 'unit-tests', status: 'passed', duration_ms: 1200 }] }")
            .await
            .unwrap();

        // Insert mock audit logs
        let _ = db
            .query("CREATE ci_audit_log CONTENT { execution_id: ci_executions:mock_exec, event: 'check_started', check: 'unit-tests', created_at: '2026-06-22T22:37:58Z' }")
            .await
            .unwrap();

        let params = CheckParams {
            repo: "owner/repo".to_string(),
        };
        let res = ci_checks(State(handle), Path(100), Query(params)).await;
        if let Err(ref e) = res {
            println!("TEST ERROR: {:?}", e);
        }
        assert!(res.is_ok());
        let body = res.unwrap();
        assert_eq!(body.0["pr_number"], 100);
        assert_eq!(body.0["repository"], "owner/repo");
        assert_eq!(body.0["status"], "passed");
        assert_eq!(body.0["checks"][0]["name"], "unit-tests");
        assert_eq!(body.0["audit_trail"][0]["event"], "check_started");
    }
}
