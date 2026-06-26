/// CI orchestration MCP tools
use serde_json::{json, Value};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct RunCiChecksRequest {
    pub repo: String,
    pub pr_number: u64,
    pub sha: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct GetCiStatusRequest {
    pub run_id: String,
}

/// Tool: run_ci_checks({repo, pr_number, sha})
/// Enqueues CI run via data-fabric task queue
/// Returns {run_id}
pub async fn run_ci_checks(req: RunCiChecksRequest) -> Result<Value, String> {
    // TODO: In production:
    // 1. Call data_fabric_client.create_run({repo, pr_number})
    // 2. Call data_fabric_client.create_task("ci_check_run", {repo, pr_number, sha})
    // 3. Return {run_id}

    // For now, return a mock run_id
    let run_id = format!("{}-{}", req.repo, req.pr_number);

    Ok(json!({
        "status": "queued",
        "run_id": run_id,
        "message": format!("CI run queued for {}/{}", req.repo, req.pr_number)
    }))
}

/// Tool: get_ci_status({run_id})
/// Proxies data-fabric for run status
/// Returns {status, checks: [{name, status, duration_ms}]}
pub async fn get_ci_status(req: GetCiStatusRequest) -> Result<Value, String> {
    // TODO: In production:
    // Call data_fabric_client.get_run_summary(run_id)
    // Return aggregated status + check results

    // For now, return a mock response
    Ok(json!({
        "run_id": req.run_id,
        "status": "in_progress",
        "checks": [
            {"name": "type-safety", "status": "passed", "duration_ms": 120},
            {"name": "unit-tests", "status": "passed", "duration_ms": 300},
            {"name": "secrets", "status": "passed", "duration_ms": 50},
            {"name": "config-lint", "status": "in_progress", "duration_ms": null}
        ]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_ci_checks_returns_run_id() {
        let req = RunCiChecksRequest {
            repo: "stevedores-org/aivcs".to_string(),
            pr_number: 42,
            sha: "abc123".to_string(),
        };

        let result = run_ci_checks(req).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response["status"], "queued");
        assert!(response["run_id"].is_string());
    }

    #[tokio::test]
    async fn test_get_ci_status_returns_status() {
        let req = GetCiStatusRequest {
            run_id: "test-run-123".to_string(),
        };

        let result = get_ci_status(req).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert!(response["checks"].is_array());
        assert!(response["status"].is_string());
    }
}
