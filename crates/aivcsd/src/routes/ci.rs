use crate::AppState;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use std::env;
use surrealdb::engine::any::connect;
use surrealdb::opt::auth::Database;
use tracing::{error, info, warn};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Deserialize)]
pub struct GithubWebhookPayload {
    pub action: Option<String>,
    pub pull_request: Option<PullRequestPayload>,
    pub repository: Option<RepositoryPayload>,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestPayload {
    pub number: u64,
    pub head: HeadPayload,
}

#[derive(Debug, Deserialize)]
pub struct HeadPayload {
    pub sha: String,
}

#[derive(Debug, Deserialize)]
pub struct RepositoryPayload {
    pub full_name: String,
}

fn verify_github_signature(headers: &HeaderMap, body: &[u8], secret: &str) -> Result<(), String> {
    let signature_header = headers
        .get("x-hub-signature-256")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| "Missing x-hub-signature-256 header".to_string())?;

    let expected_sig = format!(
        "sha256={}",
        hex::encode(
            HmacSha256::new_from_slice(secret.as_bytes())
                .map_err(|_| "Invalid HMAC key".to_string())?
                .chain_update(body)
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

pub async fn handle_github_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let webhook_secret = match std::env::var("CI_WEBHOOK_SECRET") {
        Ok(secret) => secret,
        Err(_) => {
            error!("CI_WEBHOOK_SECRET not configured");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": "Webhook verification secret not configured"
                })),
            )
                .into_response();
        }
    };

    // Verify signature
    if let Err(e) = verify_github_signature(&headers, &body, &webhook_secret) {
        warn!("Webhook signature verification failed: {}", e);
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "status": "error",
                "message": "Invalid webhook signature"
            })),
        )
            .into_response();
    }

    // Parse payload
    let payload = match serde_json::from_slice::<GithubWebhookPayload>(&body) {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to parse webhook payload: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "status": "error",
                    "message": "Invalid payload format"
                })),
            )
                .into_response();
        }
    };

    // Validate payload structure
    let pr = match &payload.pull_request {
        Some(pr) => pr,
        None => {
            warn!("Webhook missing required pull_request object");
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "status": "error",
                    "message": "Missing pull_request"
                })),
            )
                .into_response();
        }
    };

    let repo = match &payload.repository {
        Some(repo) => repo,
        None => {
            warn!("Webhook missing required repository object");
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "status": "error",
                    "message": "Missing repository"
                })),
            )
                .into_response();
        }
    };

    // Validate PR number is non-zero
    if pr.number == 0 {
        warn!("Webhook has zero PR number");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "error",
                "message": "PR number must be non-zero"
            })),
        )
            .into_response();
    }

    // Validate commit SHA is non-empty
    if pr.head.sha.trim().is_empty() {
        warn!("Webhook has empty commit SHA");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "error",
                "message": "Commit SHA must be non-empty"
            })),
        )
            .into_response();
    }

    info!(
        "GitHub webhook verified for {} PR #{} (sha: {})",
        repo.full_name, pr.number, pr.head.sha
    );

    // Create/upsert execution record in SurrealDB under `ci_executions`
    // using the safe delimiter `#` (format: `{repository}#{pr_number}`)
    let execution_id = format!("{}#{}", repo.full_name, pr.number);
    let execution_record = json!({
        "repository": repo.full_name,
        "pr_number": pr.number,
        "sha": pr.head.sha,
        "status": "pending",
        "checks": [],
        "duration_ms": 0,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "completed_at": serde_json::Value::Null,
    });

    let upsert_result: Result<Option<DbCIExecution>, _> = state
        .db
        .upsert(("ci_executions", &execution_id))
        .content(execution_record)
        .await;

    match upsert_result {
        Ok(_) => {
            info!(
                "✅ Recorded FFT execution for {}#{}",
                repo.full_name, pr.number
            );
            (
                StatusCode::ACCEPTED,
                Json(json!({
                    "status": "received",
                    "execution_id": execution_id,
                    "repository": repo.full_name,
                    "pr_number": pr.number,
                    "message": "CI execution record created"
                })),
            )
                .into_response()
        }
        Err(e) => {
            error!("❌ Failed to record execution: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "status": "error",
                    "message": format!("Database error: {}", e)
                })),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ChecksQuery {
    pub repo: Option<String>,
    pub repository: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DbCheck {
    pub name: String,
    pub status: String,
    pub duration_ms: u64,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DbCIExecution {
    pub id: surrealdb::RecordId,
    pub pr_number: u64,
    pub repository: String,
    pub sha: Option<String>,
    pub head_sha: Option<String>,
    pub status: String,
    pub checks: Vec<DbCheck>,
    pub duration_ms: u64,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DbAuditEvent {
    pub id: surrealdb::RecordId,
    pub execution_id: surrealdb::RecordId,
    pub event_kind: String,
    pub check_name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiResponseCheck {
    pub name: String,
    pub status: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiResponseAuditTrailItem {
    pub event: String,
    pub check: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiResponse {
    pub pr_number: u64,
    pub repository: String,
    pub sha: Option<String>,
    pub status: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub duration_ms: u64,
    pub checks: Vec<ApiResponseCheck>,
    pub audit_trail: Vec<ApiResponseAuditTrailItem>,
}

pub async fn get_ci_checks(
    Path(pr_number): Path<u64>,
    Query(query): Query<ChecksQuery>,
) -> impl IntoResponse {
    let repository = match query.repo.or(query.repository) {
        Some(r) => r,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Missing required query parameter: repository or repo (e.g. owner/repo)" })),
            ).into_response();
        }
    };

    let url = env::var("SURREALDB_URL").unwrap_or_else(|_| "http://localhost:8000".to_string());
    let ns = env::var("SURREALDB_NS")
        .or_else(|_| env::var("SURREALDB_NAMESPACE"))
        .unwrap_or_else(|_| "ci".to_string());
    let db_name = env::var("SURREALDB_DB").unwrap_or_else(|_| "fft".to_string());
    let user = env::var("SURREALDB_USER").unwrap_or_else(|_| "web_readonly".to_string());
    let pass = env::var("SURREALDB_PASS").unwrap_or_else(|_| "password".to_string());

    info!(
        "Connecting to SurrealDB at {} (NS: {}, DB: {})",
        url, ns, db_name
    );

    // 1. Connect to SurrealDB
    let db = match connect(&url).await {
        Ok(db) => db,
        Err(e) => {
            error!("SurrealDB connection failed: {}", e);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("Database connection error: {}", e) })),
            )
                .into_response();
        }
    };

    // 2. Select Namespace & Database
    if let Err(e) = db.use_ns(&ns).use_db(&db_name).await {
        error!("Selecting NS/DB failed: {}", e);
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": format!("Database initialization error: {}", e) })),
        )
            .into_response();
    }

    // 3. Sign In
    if let Err(e) = db
        .signin(Database {
            namespace: &ns,
            database: &db_name,
            username: &user,
            password: &pass,
        })
        .await
    {
        error!("SurrealDB signin failed: {}", e);
        // Note: For in-memory (mem://) and embedded (surrealkv://), signin might fail depending on whether auth is enabled.
        // Let's only return 503 if we are not using these embedded backends or if signin is required.
        if !url.starts_with("mem://") && !url.starts_with("surrealkv://") {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("Database authentication error: {}", e) })),
            )
                .into_response();
        }
    }

    // 4. Query ci_executions
    let query_exec = "SELECT * FROM ci_executions WHERE pr_number = $pr AND repository = $repo ORDER BY created_at DESC LIMIT 1";
    let mut response = match db
        .query(query_exec)
        .bind(("pr", pr_number))
        .bind(("repo", repository.clone()))
        .await
    {
        Ok(res) => res,
        Err(e) => {
            error!("Querying ci_executions failed: {}", e);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("Database query error: {}", e) })),
            )
                .into_response();
        }
    };

    let executions: Vec<DbCIExecution> = match response.take(0) {
        Ok(execs) => execs,
        Err(e) => {
            error!("Deserializing execution failed: {}", e);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("Database deserialization error: {}", e) })),
            )
                .into_response();
        }
    };

    let execution = match executions.into_iter().next() {
        Some(exec) => exec,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "No CI execution found",
                    "pr_number": pr_number,
                    "repository": repository
                })),
            )
                .into_response();
        }
    };

    // 5. Query ci_audit_log using the execution's id
    let query_audit = "SELECT * FROM ci_audit_log WHERE execution_id = $id ORDER BY created_at ASC";
    let mut audit_response = match db
        .query(query_audit)
        .bind(("id", execution.id.clone()))
        .await
    {
        Ok(res) => res,
        Err(e) => {
            error!("Querying ci_audit_log failed: {}", e);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": format!("Database query error (audit): {}", e) })),
            )
                .into_response();
        }
    };

    let audit_events: Vec<DbAuditEvent> = match audit_response.take(0) {
        Ok(events) => events,
        Err(e) => {
            error!("Deserializing audit events failed: {}", e);
            Vec::new() // Fallback gracefully if we can't deserialize audit trail but have execution
        }
    };

    // 6. Format response
    let checks = execution
        .checks
        .into_iter()
        .map(|c| ApiResponseCheck {
            name: c.name,
            status: c.status,
            duration_ms: c.duration_ms,
        })
        .collect();

    let audit_trail = audit_events
        .into_iter()
        .map(|a| ApiResponseAuditTrailItem {
            event: a.event_kind,
            check: a.check_name,
            timestamp: a.created_at,
        })
        .collect();

    let payload = ApiResponse {
        pr_number: execution.pr_number,
        repository: execution.repository,
        sha: execution.sha.or(execution.head_sha),
        status: execution.status,
        started_at: execution.created_at,
        completed_at: execution.completed_at,
        duration_ms: execution.duration_ms,
        checks,
        audit_trail,
    };

    (StatusCode::OK, Json(payload)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn setup_db_path() -> (tempfile::TempDir, String) {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().to_str().unwrap().to_string();
        let url = format!("surrealkv://{}", db_path);
        (temp_dir, url)
    }

    #[tokio::test]
    async fn test_get_ci_checks_success() {
        let _guard = TEST_LOCK.lock().await;
        let (_temp_dir, url) = setup_db_path().await;

        env::set_var("SURREALDB_URL", &url);
        env::set_var("SURREALDB_NS", "ci_test");
        env::set_var("SURREALDB_DB", "fft_test");

        let db = connect(&url).await.unwrap();
        db.use_ns("ci_test").use_db("fft_test").await.unwrap();

        // Populate with mock data
        db.query(
            "
            CREATE ci_executions CONTENT {
                id: ci_executions:exec_1,
                pr_number: 296,
                repository: 'stevedores-org/aivcs',
                status: 'passed',
                checks: [
                    {
                        name: 'type-safety',
                        status: 'passed',
                        duration_ms: 1200
                    }
                ],
                duration_ms: 4000,
                created_at: '2026-06-22T22:37:58Z',
                completed_at: '2026-06-22T22:38:02Z'
            };
        ",
        )
        .await
        .unwrap();

        db.query(
            "
            CREATE ci_audit_log CONTENT {
                execution_id: ci_executions:exec_1,
                event_kind: 'check_started',
                check_name: 'type-safety',
                created_at: '2026-06-22T22:37:58Z'
            };
        ",
        )
        .await
        .unwrap();

        // Call the handler
        let response = get_ci_checks(
            Path(296),
            Query(ChecksQuery {
                repo: Some("stevedores-org/aivcs".to_string()),
                repository: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);

        // Get the body
        let body_bytes = axum::body::to_bytes(response.into_body(), 10000)
            .await
            .unwrap();
        let api_response: ApiResponse = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(api_response.pr_number, 296);
        assert_eq!(api_response.repository, "stevedores-org/aivcs");
        assert_eq!(api_response.status, "passed");
        assert_eq!(api_response.duration_ms, 4000);
        assert_eq!(api_response.checks.len(), 1);
        assert_eq!(api_response.checks[0].name, "type-safety");
        assert_eq!(api_response.checks[0].status, "passed");
        assert_eq!(api_response.audit_trail.len(), 1);
        assert_eq!(api_response.audit_trail[0].event, "check_started");
        assert_eq!(
            api_response.audit_trail[0].check,
            Some("type-safety".to_string())
        );
    }

    #[tokio::test]
    async fn test_get_ci_checks_not_found() {
        let _guard = TEST_LOCK.lock().await;
        let (_temp_dir, url) = setup_db_path().await;

        env::set_var("SURREALDB_URL", &url);
        env::set_var("SURREALDB_NS", "ci_test");
        env::set_var("SURREALDB_DB", "fft_test");

        let response = get_ci_checks(
            Path(999),
            Query(ChecksQuery {
                repo: Some("stevedores-org/aivcs".to_string()),
                repository: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_ci_checks_offline() {
        let _guard = TEST_LOCK.lock().await;

        // Set to a port that is guaranteed to be unreachable or fail
        env::set_var("SURREALDB_URL", "http://127.0.0.1:9999");
        env::set_var("SURREALDB_NS", "ci_test");
        env::set_var("SURREALDB_DB", "fft_test");

        let response = get_ci_checks(
            Path(296),
            Query(ChecksQuery {
                repo: Some("stevedores-org/aivcs".to_string()),
                repository: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_handle_github_webhook_missing_secret() {
        let _guard = TEST_LOCK.lock().await;
        env::remove_var("CI_WEBHOOK_SECRET");

        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().to_str().unwrap().to_string();
        let url = format!("surrealkv://{}", db_path);
        let db = connect(&url).await.unwrap();
        let cas_dir = temp_dir.path().join("cas");
        std::fs::create_dir_all(&cas_dir).unwrap();
        let cas = std::sync::Arc::new(aivcs_core::cas::fs::FsCasStore::new(cas_dir).unwrap());
        let state = AppState { db, cas };

        let headers = HeaderMap::new();
        let body = Bytes::from_static(b"{}");

        let response = handle_github_webhook(State(state), headers, body)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_handle_github_webhook_invalid_signature() {
        let _guard = TEST_LOCK.lock().await;
        env::set_var("CI_WEBHOOK_SECRET", "mysecret");

        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().to_str().unwrap().to_string();
        let url = format!("surrealkv://{}", db_path);
        let db = connect(&url).await.unwrap();
        let cas_dir = temp_dir.path().join("cas");
        std::fs::create_dir_all(&cas_dir).unwrap();
        let cas = std::sync::Arc::new(aivcs_core::cas::fs::FsCasStore::new(cas_dir).unwrap());
        let state = AppState { db, cas };

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-hub-signature-256",
            "sha256=invalid-signature-here".parse().unwrap(),
        );
        let body = Bytes::from_static(b"{}");

        let response = handle_github_webhook(State(state), headers, body)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_handle_github_webhook_success() {
        let _guard = TEST_LOCK.lock().await;
        let secret = "mysecret";
        env::set_var("CI_WEBHOOK_SECRET", secret);

        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().to_str().unwrap().to_string();
        let url = format!("surrealkv://{}", db_path);
        let db = connect(&url).await.unwrap();
        db.use_ns("ci_test").use_db("fft_test").await.unwrap();

        let cas_dir = temp_dir.path().join("cas");
        std::fs::create_dir_all(&cas_dir).unwrap();
        let cas = std::sync::Arc::new(aivcs_core::cas::fs::FsCasStore::new(cas_dir).unwrap());
        let state = AppState {
            db: db.clone(),
            cas,
        };

        let payload_json = json!({
            "action": "opened",
            "pull_request": {
                "number": 42,
                "head": {
                    "sha": "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
                }
            },
            "repository": {
                "full_name": "stevedores-org/aivcs"
            }
        });

        let body_bytes = serde_json::to_vec(&payload_json).unwrap();
        let hmac_sig = format!(
            "sha256={}",
            hex::encode(
                HmacSha256::new_from_slice(secret.as_bytes())
                    .unwrap()
                    .chain_update(&body_bytes)
                    .finalize()
                    .into_bytes()
            )
        );

        let mut headers = HeaderMap::new();
        headers.insert("x-hub-signature-256", hmac_sig.parse().unwrap());

        let response = handle_github_webhook(State(state), headers, Bytes::from(body_bytes))
            .await
            .into_response();

        let status = response.status();
        assert_eq!(status, StatusCode::ACCEPTED);

        // Verify the record was persisted in SurrealDB under `ci_executions`
        // using the key `stevedores-org/aivcs#42`
        let query_res = db
            .query("SELECT * FROM ci_executions WHERE pr_number = 42 AND repository = 'stevedores-org/aivcs'")
            .await;
        let mut res = query_res.unwrap();
        let executions: Vec<DbCIExecution> = res.take(0).unwrap();
        assert_eq!(executions.len(), 1);
        let exec = &executions[0];
        assert_eq!(exec.pr_number, 42);
        assert_eq!(exec.repository, "stevedores-org/aivcs");
        assert_eq!(
            exec.sha.as_deref(),
            Some("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")
        );
        assert_eq!(exec.status, "pending");
        assert_eq!(exec.checks.len(), 0);
    }
}
