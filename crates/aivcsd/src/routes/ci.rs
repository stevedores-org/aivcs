use crate::AppState;
use anyhow::Context;
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use surrealdb::engine::any::connect;
use surrealdb::opt::auth::Database;
use tracing::{error, info, warn};

const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Clone)]
pub struct ReconcilerConfig {
    pub repositories: Vec<String>,
    pub dispatch_url: String,
    pub interval: std::time::Duration,
}

impl ReconcilerConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let repositories = env::var("CI_RECONCILER_REPOSITORIES")
            .context("CI_RECONCILER_REPOSITORIES must be set (owner/repo[,owner/repo...])")?
            .split(',')
            .map(str::trim)
            .filter(|repo| !repo.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        anyhow::ensure!(
            !repositories.is_empty(),
            "CI_RECONCILER_REPOSITORIES must contain at least one repository"
        );
        anyhow::ensure!(
            repositories.iter().all(|repo| {
                let mut parts = repo.split('/');
                parts.next().is_some_and(|part| !part.is_empty())
                    && parts.next().is_some_and(|part| !part.is_empty())
                    && parts.next().is_none()
            }),
            "CI_RECONCILER_REPOSITORIES entries must use owner/repo format"
        );
        let dispatch_url = env::var("CI_RECONCILER_DISPATCH_URL")
            .context("CI_RECONCILER_DISPATCH_URL must be set for CI execution dispatch")?;
        anyhow::ensure!(
            dispatch_url.starts_with("https://"),
            "CI_RECONCILER_DISPATCH_URL must use HTTPS"
        );
        let seconds = env::var("CI_RECONCILER_INTERVAL_SECS")
            .unwrap_or_else(|_| "30".to_string())
            .parse::<u64>()
            .context("CI_RECONCILER_INTERVAL_SECS must be an integer")?;
        anyhow::ensure!(
            seconds > 0,
            "CI_RECONCILER_INTERVAL_SECS must be greater than zero"
        );
        Ok(Self {
            repositories,
            dispatch_url,
            interval: std::time::Duration::from_secs(seconds),
        })
    }
}

#[derive(Debug, Deserialize)]
struct PullRequest {
    number: u64,
    head: PullRequestHead,
}

#[derive(Debug, Deserialize)]
struct PullRequestHead {
    sha: String,
}

#[derive(Debug, Deserialize)]
struct CheckRun {
    name: String,
    status: String,
    conclusion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CheckRuns {
    check_runs: Vec<CheckRun>,
}

fn execution_id(repository: &str, pr_number: u64, sha: &str) -> String {
    format!("{}#{}#{}", repository, pr_number, sha)
}

fn execution_status(checks: &[CheckRun]) -> &'static str {
    if checks.is_empty() {
        "pending"
    } else if checks.iter().any(|check| check.status != "completed") {
        "running"
    } else if checks.iter().any(|check| {
        !matches!(
            check.conclusion.as_deref(),
            Some("success" | "neutral" | "skipped")
        )
    }) {
        "failed"
    } else {
        "passed"
    }
}

async fn reconcile_repository(
    client: &reqwest::Client,
    token: &str,
    db: &surrealdb::Surreal<surrealdb::engine::any::Any>,
    repository: &str,
    dispatch_url: &str,
) -> anyhow::Result<()> {
    let pulls = client
        .get(format!("{GITHUB_API}/repos/{repository}/pulls"))
        .bearer_auth(token)
        .header("accept", "application/vnd.github+json")
        .query(&[("state", "open"), ("per_page", "100")])
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<PullRequest>>()
        .await?;

    for pull in pulls {
        if pull.number == 0 || pull.head.sha.trim().is_empty() {
            warn!(repository, "ignoring malformed pull request from GitHub");
            continue;
        }
        let checks = client
            .get(format!(
                "{GITHUB_API}/repos/{repository}/commits/{}/check-runs",
                pull.head.sha
            ))
            .bearer_auth(token)
            .header("accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .json::<CheckRuns>()
            .await?
            .check_runs;
        let id = execution_id(repository, pull.number, &pull.head.sha);
        let mut prior_event = db
            .query("SELECT * FROM ci_reconciler_events WHERE event_id = $event_id LIMIT 1")
            .bind(("event_id", id.clone()))
            .await?;
        let prior_events: Vec<serde_json::Value> = prior_event.take(0)?;
        let first_seen = prior_events.is_empty();
        if first_seen {
            client
                .post(dispatch_url)
                .bearer_auth(token)
                .json(&json!({
                    "repository": repository,
                    "pr_number": pull.number,
                    "sha": pull.head.sha,
                    "source": "aivcsd_outbound_reconciler",
                }))
                .send()
                .await?
                .error_for_status()?;
            let _: Option<serde_json::Value> = db
                .create(("ci_reconciler_events", id.clone()))
                .content(json!({
                    "event_id": id,
                    "repository": repository,
                    "pr_number": pull.number,
                    "sha": pull.head.sha,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                }))
                .await?;
        }
        let status = execution_status(&checks);
        let completed_at =
            (status == "passed" || status == "failed").then(|| chrono::Utc::now().to_rfc3339());
        let status_update = json!({
            "status": status,
            "checks": checks.iter().map(|check| json!({
                "name": check.name,
                "status": check.conclusion.as_deref().unwrap_or(&check.status),
                "duration_ms": 0,
                "output": null,
            })).collect::<Vec<_>>(),
            "duration_ms": 0,
            "completed_at": completed_at,
            "source": "github_outbound_reconciler",
        });
        if first_seen {
            let mut execution_record = status_update.clone();
            let record = execution_record
                .as_object_mut()
                .expect("status update is always a JSON object");
            record.insert("repository".to_string(), json!(repository));
            record.insert("pr_number".to_string(), json!(pull.number));
            record.insert("sha".to_string(), json!(pull.head.sha));
            record.insert(
                "created_at".to_string(),
                json!(chrono::Utc::now().to_rfc3339()),
            );
            record.insert("source".to_string(), json!("github_outbound_reconciler"));
            let _: Option<DbCIExecution> = db
                .upsert(("ci_executions", id))
                .content(execution_record)
                .await?;
        } else {
            let _: Option<DbCIExecution> = db
                .update(("ci_executions", id))
                .merge(status_update)
                .await?;
        }
        if first_seen {
            info!(
                repository,
                pr = pull.number,
                "recorded CI execution from outbound reconciliation"
            );
        }
    }
    Ok(())
}

pub async fn run_reconciler(state: AppState, token: String, config: ReconcilerConfig) {
    let client = reqwest::Client::builder()
        .user_agent("aivcsd-ci-reconciler")
        .build()
        .expect("static GitHub client configuration must be valid");
    loop {
        for repository in &config.repositories {
            if let Err(error) =
                reconcile_repository(&client, &token, &state.db, repository, &config.dispatch_url)
                    .await
            {
                error!(repository, %error, "outbound CI reconciliation failed");
            }
        }
        tokio::time::sleep(config.interval).await;
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

    #[test]
    fn reconciler_config_requires_repository_allowlist() {
        unsafe {
            env::remove_var("CI_RECONCILER_REPOSITORIES");
        }
        let error = ReconcilerConfig::from_env().unwrap_err();
        assert!(error.to_string().contains("CI_RECONCILER_REPOSITORIES"));
    }

    #[test]
    fn execution_id_deduplicates_repository_pr_and_sha() {
        assert_eq!(
            execution_id("stevedores-org/aivcs", 42, "abc"),
            "stevedores-org/aivcs#42#abc"
        );
    }

    #[test]
    fn check_status_is_fail_closed() {
        let checks = vec![CheckRun {
            name: "gate".to_string(),
            status: "completed".to_string(),
            conclusion: None,
        }];
        assert_eq!(execution_status(&checks), "failed");
    }
}
