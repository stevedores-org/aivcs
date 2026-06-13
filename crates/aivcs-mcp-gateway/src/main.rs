use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn, Level};
use uuid::Uuid;

const PUBLIC_KEY_PEM: &str = include_str!("../../aivcs-auth/keys/public.pem");

/// Maximum age of a `HumanApproval` grant before it stops counting as a valid
/// authorisation. Per the AIVCS Zero-Trust MCP Identity Model
/// (stevedores-org/aivcs#228, Feature 3.1): *"The grant must be single-use and
/// expire (e.g., within 2 hours)."* Expressed in hours rather than as a fixed
/// `Duration` so the policy value is human-greppable in audit logs.
const APPROVAL_TTL_HOURS: i64 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpClaims {
    sub: String,
    aud: String,
    tenant_id: String,
    workspace_id: String,
    agent_id: String,
    run_id: String,
    task_id: String,
    scopes: Vec<String>,
    max_risk: String,
    delegated_by: String,
    jti: String,
    exp: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthorityRecord {
    authority_id: String,
    actor: String,
    agent_instance: String,
    on_behalf_of: String,
    tenant_id: String,
    workspace_id: String,
    repo: String,
    run_id: String,
    task_id: String,
    tool_id: String,
    tool_manifest_hash: String,
    tool_schema_version: String,
    action: String,
    payload_digest: String,
    policy_decision_id: String,
    policy_version: String,
    approval_id: Option<String>,
    risk_level: String,
    expiry: i64,
    outcome: String, // "pending" | "executed" | "denied" | "expired" | "superseded"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HumanApproval {
    approval_id: String,
    run_id: String,
    task_id: String,
    action: String,
    payload_digest: String,
    approved_by: String,
    created_at: DateTime<Utc>,
    used: bool,
}

#[derive(Debug, Deserialize)]
struct ToolCallRequest {
    tool: String,
    arguments: Value,
    repo: String,
    /// Reserved for the client-side approval-id passing pattern (#228 Feature
    /// 3.1). The gateway currently looks up approvals by the
    /// (run_id, task_id, action, payload_digest) tuple on `McpClaims`, so this
    /// field is accepted on the wire but not yet consulted. Kept on the
    /// request shape so callers can start sending it before the lookup path
    /// switches over.
    #[allow(dead_code)]
    approval_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ToolCallResponse {
    status: String, // "success" | "approval_required" | "denied"
    authority_id: Option<String>,
    result: Option<Value>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateApprovalRequest {
    run_id: String,
    task_id: String,
    action: String,
    payload_digest: String,
    approved_by: String,
}

// In-memory hot list for JTI revocation (Phase 5)
struct RevocationList {
    revoked_jtis: HashSet<String>,
    revoked_sessions: HashSet<String>,
}

struct GatewayState {
    db: aivcs_core::SurrealHandle,
    authority_records: Mutex<HashMap<String, AuthorityRecord>>,
    approvals: Mutex<HashMap<String, HumanApproval>>,
    revocations: Mutex<RevocationList>,
}

#[tokio::main]
async fn main() -> std::result::Result<(), anyhow::Error> {
    aivcs_core::init_tracing(false, Level::INFO);
    info!("🚀 aivcs-mcp-gateway starting");

    // Connect to in-memory SurrealDB for development and tests
    let db = aivcs_core::SurrealHandle::setup_db().await?;

    let state = Arc::new(GatewayState {
        db,
        authority_records: Mutex::new(HashMap::new()),
        approvals: Mutex::new(HashMap::new()),
        revocations: Mutex::new(RevocationList {
            revoked_jtis: HashSet::new(),
            revoked_sessions: HashSet::new(),
        }),
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/mcp/tools/list", get(list_tools))
        .route("/v1/mcp/tools/call", post(call_tool))
        .route("/v1/mcp/approvals", post(create_approval))
        .route("/v1/mcp/revocation", post(revoke_token))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8082));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("📡 listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Returns true when a tool's risk level is above the token's `max_risk` ceiling.
/// Mirrors the visibility rules in `list_tools`.
fn exceeds_max_risk(risk_level: &str, max_risk: &str) -> bool {
    match risk_level {
        "read" => false,
        "write" => max_risk == "read",
        "destructive" => max_risk != "write",
        _ => true,
    }
}

async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "aivcs-mcp-gateway",
        "timestamp": Utc::now()
    }))
}

// Helper to validate headers and token
fn validate_auth(
    headers: &HeaderMap,
    revocations: &RevocationList,
) -> std::result::Result<McpClaims, (StatusCode, Json<Value>)> {
    // Check MCP headers
    let version = headers
        .get("MCP-Protocol-Version")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if version.is_empty() || session_id.is_empty() {
        warn!("Missing required MCP headers");
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing_required_headers" })),
        ));
    }

    if revocations.revoked_sessions.contains(session_id) {
        warn!("Session {} is revoked", session_id);
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "session_revoked" })),
        ));
    }

    // Check authorization token
    let auth_header = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if !auth_header.starts_with("Bearer ") {
        warn!("Missing or malformed Authorization header");
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "missing_bearer_token" })),
        ));
    }

    let token = auth_header.trim_start_matches("Bearer ").trim();

    // Verify JWT
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&["https://mcp.aivcs.lornu.ai"]);

    let dec_key = DecodingKey::from_rsa_pem(PUBLIC_KEY_PEM.as_bytes()).map_err(|e| {
        error!("Decoding key load failed: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "internal_error" })),
        )
    })?;

    let token_data = decode::<McpClaims>(token, &dec_key, &validation).map_err(|e| {
        warn!("JWT validation failed: {}", e);
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid_token", "detail": e.to_string() })),
        )
    })?;

    let claims = token_data.claims;

    // Check if token JTI is revoked
    if revocations.revoked_jtis.contains(&claims.jti) {
        warn!("Token JTI {} is revoked", claims.jti);
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "token_revoked" })),
        ));
    }

    Ok(claims)
}

async fn list_tools(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
) -> axum::response::Response {
    let revocations = state.revocations.lock().unwrap();
    let claims = match validate_auth(&headers, &revocations) {
        Ok(c) => c,
        Err(err) => return err.into_response(),
    };

    info!("Listing tools for agent: {}", claims.agent_id);

    // Dynamic filtering based on max risk and scopes in the token
    let mut tools = vec![json!({
        "name": "repo.diff.read",
        "description": "Read file diffs in the repository",
        "risk_level": "read",
        "required_scopes": ["repo.diff.read"]
    })];

    if claims.scopes.contains(&"repo.diff.write".to_string()) && claims.max_risk != "read" {
        tools.push(json!({
            "name": "repo.diff.write",
            "description": "Write code diff changes",
            "risk_level": "write",
            "required_scopes": ["repo.diff.write"]
        }));
    }

    if claims.scopes.contains(&"repo.merge.execute".to_string()) && claims.max_risk == "write" {
        tools.push(json!({
            "name": "repo.merge.execute",
            "description": "Merge feature branches with human guardrails",
            "risk_level": "destructive",
            "required_scopes": ["repo.merge.execute"]
        }));
    }

    Json(json!({ "tools": tools })).into_response()
}

#[axum::debug_handler]
async fn call_tool(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    Json(req): Json<Value>,
) -> axum::response::Response {
    let claims = {
        let revocations = state.revocations.lock().unwrap();
        match validate_auth(&headers, &revocations) {
            Ok(c) => c,
            Err(err) => return err.into_response(),
        }
    };

    let req: ToolCallRequest = match serde_json::from_value(req) {
        Ok(r) => r,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": err.to_string()})),
            )
                .into_response()
        }
    };

    info!(
        "Agent {} requesting tool {} on repo {} (run={} task={})",
        claims.agent_id, req.tool, req.repo, claims.run_id, claims.task_id
    );

    // Step 1: Resolve risk level and scope requirements for tool
    let (risk_level, required_scope) = match req.tool.as_str() {
        "repo.diff.read" => ("read", "repo.diff.read"),
        "repo.diff.write" => ("write", "repo.diff.write"),
        "repo.merge.execute" => ("destructive", "repo.merge.execute"),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "unknown_tool" })),
            )
                .into_response()
        }
    };

    // Step 2: Validate token scope and max risk
    if !claims.scopes.contains(&required_scope.to_string()) {
        return Json(ToolCallResponse {
            status: "denied".to_string(),
            authority_id: None,
            result: None,
            reason: Some(format!("Missing required scope: {}", required_scope)),
        })
        .into_response();
    }

    if exceeds_max_risk(risk_level, &claims.max_risk) {
        return Json(ToolCallResponse {
            status: "denied".to_string(),
            authority_id: None,
            result: None,
            reason: Some("Tool risk level exceeds maximum allowed risk level".to_string()),
        })
        .into_response();
    }

    // Step 3: Compute canonical payload digest to lock down tool arguments (rpelevin requirement)
    let payload_string = serde_json::to_string(&req.arguments).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(req.tool.as_bytes());
    hasher.update(payload_string.as_bytes());
    let payload_digest = hex::encode(hasher.finalize());

    // Step 4: Policy & Human Approval Check
    //
    // For destructive tools we need a `HumanApproval` matching the
    // (run_id, task_id, action, payload_digest) tuple that is:
    //   - not already consumed (`used == false`),
    //   - not older than `APPROVAL_TTL_HOURS` (#228 Feature 3.1).
    //
    // We deliberately classify "stale or consumed" separately from
    // "no approval ever recorded" so operators can tell why their grant
    // didn't take effect from the response `reason` alone, without
    // grepping the audit log.
    let mut active_approval_id = None;
    let mut stale_status: Option<&'static str> = None;
    if risk_level == "destructive" {
        let now = Utc::now();
        let ttl = chrono::Duration::hours(APPROVAL_TTL_HOURS);
        let mut approvals = state.approvals.lock().unwrap();

        // First pass: pick a fresh, unused matching approval and consume it.
        let fresh_match_id = approvals
            .values()
            .find(|a| {
                a.run_id == claims.run_id
                    && a.task_id == claims.task_id
                    && a.action == req.tool
                    && a.payload_digest == payload_digest
                    && !a.used
                    && (now - a.created_at) <= ttl
            })
            .map(|a| a.approval_id.clone());

        if let Some(id) = fresh_match_id {
            let appr = approvals.get_mut(&id).expect("just found this id");
            info!("Valid human approval found: {}", id);
            appr.used = true; // Mark as single-use consumed
            active_approval_id = Some(id);
        } else {
            // Second pass: classify *any* matching approval so the reason
            // tells the operator what actually went wrong.
            stale_status = approvals
                .values()
                .filter(|a| {
                    a.run_id == claims.run_id
                        && a.task_id == claims.task_id
                        && a.action == req.tool
                        && a.payload_digest == payload_digest
                })
                .map(|a| {
                    if a.used {
                        "consumed"
                    } else if (now - a.created_at) > ttl {
                        "expired"
                    } else {
                        // Should be unreachable given the first pass, but
                        // tag it as "stale" rather than panic.
                        "stale"
                    }
                })
                .next();
        }
    }

    if risk_level == "destructive" && active_approval_id.is_none() {
        let outcome_tag = match stale_status {
            Some("expired") => "expired",
            Some("consumed") => "consumed",
            _ => "escalated",
        };
        warn!(outcome = outcome_tag, "Tool requires fresh human approval");

        // Record the policy decision to SurrealDB with the precise outcome
        // (escalated / expired / consumed) so the audit trail distinguishes
        // a never-approved request from one whose grant aged out or was
        // already used.
        let mut decision = aivcs_core::DecisionRecord::new(
            Uuid::new_v4().to_string(),
            "".to_string(),
            format!("run:{}/task:{}", claims.run_id, claims.task_id),
            req.tool.clone(),
            match outcome_tag {
                "expired" => format!(
                    "Existing approval expired (TTL = {}h); a fresh approval is required",
                    APPROVAL_TTL_HOURS
                ),
                "consumed" => {
                    "Existing approval already consumed; a fresh approval is required".to_string()
                }
                _ => "Requires human approval before execution".to_string(),
            },
            1.0,
        );
        decision.alternatives = vec!["Deny".to_string(), "Escalate".to_string()];
        decision.outcome = Some(outcome_tag.to_string());
        let _ = state.db.save_decision(&decision).await;

        let reason = match outcome_tag {
            "expired" => format!(
                "Existing human approval expired (TTL = {}h). Request a fresh approval. Payload digest: {}",
                APPROVAL_TTL_HOURS, payload_digest
            ),
            "consumed" => format!(
                "Existing human approval already consumed. Request a fresh approval. Payload digest: {}",
                payload_digest
            ),
            _ => format!(
                "Human approval required for action. Payload digest: {}",
                payload_digest
            ),
        };

        return Json(ToolCallResponse {
            status: "approval_required".to_string(),
            authority_id: None,
            result: None,
            reason: Some(reason),
        })
        .into_response();
    }

    // Step 5: Mint One-Use Authority Record (rpelevin invariant)
    let authority_id = format!("auth-{}", Uuid::new_v4());
    let policy_decision_id = Uuid::new_v4().to_string();

    let authority = AuthorityRecord {
        authority_id: authority_id.clone(),
        actor: claims.sub.clone(),
        agent_instance: claims.agent_id.clone(),
        on_behalf_of: claims.delegated_by.clone(),
        tenant_id: claims.tenant_id.clone(),
        workspace_id: claims.workspace_id.clone(),
        repo: req.repo.clone(),
        run_id: claims.run_id.clone(),
        task_id: claims.task_id.clone(),
        tool_id: req.tool.clone(),
        tool_manifest_hash: "sha256-manifest-placeholder".to_string(),
        tool_schema_version: "2025-06-18".to_string(),
        action: req.tool.clone(),
        payload_digest: payload_digest.clone(),
        policy_decision_id: policy_decision_id.clone(),
        policy_version: "1.0.0".to_string(),
        approval_id: active_approval_id,
        risk_level: risk_level.to_string(),
        expiry: (Utc::now() + chrono::Duration::minutes(5)).timestamp(),
        outcome: "executed".to_string(), // single-use execution consumes it immediately
    };

    // Save authority record
    state
        .authority_records
        .lock()
        .unwrap()
        .insert(authority_id.clone(), authority);

    // Save policy decision to SurrealDB
    let mut decision = aivcs_core::DecisionRecord::new(
        policy_decision_id,
        "".to_string(),
        format!("run:{}/task:{}", claims.run_id, claims.task_id),
        req.tool.clone(),
        format!(
            "Authorized tool execution with authority_id={}",
            authority_id
        ),
        1.0,
    );
    decision.alternatives = vec!["Allow".to_string()];
    decision.outcome = Some("allowed".to_string());
    let _ = state.db.save_decision(&decision).await;

    // Simulate tool execution
    let result = match req.tool.as_str() {
        "repo.diff.read" => json!({ "diff": "--- a/src/main.rs\n+++ b/src/main.rs\n" }),
        "repo.diff.write" => json!({ "status": "changes_written" }),
        "repo.merge.execute" => {
            json!({ "status": "merged", "commit_id": "sha256-merge-placeholder" })
        }
        _ => json!({}),
    };

    info!("Tool {} executed successfully", req.tool);

    Json(ToolCallResponse {
        status: "success".to_string(),
        authority_id: Some(authority_id),
        result: Some(result),
        reason: None,
    })
    .into_response()
}

async fn create_approval(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<CreateApprovalRequest>,
) -> Json<Value> {
    let approval_id = format!("appr-{}", Uuid::new_v4());
    let approval = HumanApproval {
        approval_id: approval_id.clone(),
        run_id: req.run_id,
        task_id: req.task_id,
        action: req.action,
        payload_digest: req.payload_digest,
        approved_by: req.approved_by,
        created_at: Utc::now(),
        used: false,
    };

    state
        .approvals
        .lock()
        .unwrap()
        .insert(approval_id.clone(), approval);

    info!("Human approval registered: {}", approval_id);

    Json(json!({
        "status": "approval_created",
        "approval_id": approval_id
    }))
}

async fn revoke_token(
    State(state): State<Arc<GatewayState>>,
    Json(req): Json<Value>,
) -> Json<Value> {
    let mut revocations = state.revocations.lock().unwrap();

    if let Some(jti) = req.get("jti").and_then(|v| v.as_str()) {
        revocations.revoked_jtis.insert(jti.to_string());
        info!("Revoked JTI: {}", jti);
    }

    if let Some(session_id) = req.get("session_id").and_then(|v| v.as_str()) {
        revocations.revoked_sessions.insert(session_id.to_string());
        info!("Revoked session ID: {}", session_id);
    }

    Json(json!({ "status": "revocation_updated" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::EncodingKey;
    use tower::ServiceExt;

    const PRIVATE_KEY_PEM: &str = include_str!("../../aivcs-auth/keys/private.pem");

    fn mint_test_token(max_risk: &str, scopes: Vec<&str>) -> String {
        let private_key = EncodingKey::from_rsa_pem(PRIVATE_KEY_PEM.as_bytes()).unwrap();
        let exp = (Utc::now() + chrono::Duration::minutes(5)).timestamp() as usize;

        let claims = McpClaims {
            sub: "agent_instance:test-agent-123".to_string(),
            aud: "https://mcp.aivcs.lornu.ai".to_string(),
            tenant_id: "tenant-default".to_string(),
            workspace_id: "ws-default".to_string(),
            agent_id: "agent-opt".to_string(),
            run_id: "run-test-run".to_string(),
            task_id: "task-test-task".to_string(),
            scopes: scopes.into_iter().map(|s| s.to_string()).collect(),
            max_risk: max_risk.to_string(),
            delegated_by: "policy:builder-feature-branch-write".to_string(),
            jti: "test-jti-1".to_string(),
            exp,
        };

        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
        header.kid = Some("key-id-aivcs".to_string());

        jsonwebtoken::encode(&header, &claims, &private_key).unwrap()
    }

    /// Build a fresh gateway router + return the shared state so tests can
    /// inspect or mutate the in-memory approvals/revocations table directly
    /// (needed e.g. to back-date a `HumanApproval` past the TTL).
    async fn setup_router_with_state() -> (Router, Arc<GatewayState>) {
        let db = aivcs_core::SurrealHandle::setup_db().await.unwrap();
        let state = Arc::new(GatewayState {
            db,
            authority_records: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
            revocations: Mutex::new(RevocationList {
                revoked_jtis: HashSet::new(),
                revoked_sessions: HashSet::new(),
            }),
        });

        let router = Router::new()
            .route("/v1/mcp/tools/list", get(list_tools))
            .route("/v1/mcp/tools/call", post(call_tool))
            .route("/v1/mcp/approvals", post(create_approval))
            .route("/v1/mcp/revocation", post(revoke_token))
            .with_state(state.clone());

        (router, state)
    }

    /// Convenience wrapper for the existing tests that don't need to poke at
    /// state directly.
    async fn setup_router() -> Router {
        setup_router_with_state().await.0
    }

    #[tokio::test]
    async fn test_gateway_zero_trust_mcp_validation() {
        let app = setup_router().await;

        // 1. Check list tools with valid credentials
        let token = mint_test_token("write", vec!["repo.diff.read", "repo.diff.write"]);

        let req = axum::http::Request::builder()
            .uri("/v1/mcp/tools/list")
            .header("Authorization", format!("Bearer {}", token))
            .header("MCP-Protocol-Version", "2025-06-18")
            .header("Mcp-Session-Id", "session-1")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(json["tools"].as_array().unwrap().len() >= 2);

        // 2. Call read tool - should succeed
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/mcp/tools/call")
            .header("Authorization", format!("Bearer {}", token))
            .header("MCP-Protocol-Version", "2025-06-18")
            .header("Mcp-Session-Id", "session-1")
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "tool": "repo.diff.read",
                    "arguments": {},
                    "repo": "stevedores-org/aivcs"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let res_json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(res_json["status"], "success");
        assert!(res_json["authority_id"].as_str().is_some());

        // 3. Call destructive tool without human approval - should escalate
        let token_merge = mint_test_token("write", vec!["repo.merge.execute"]);
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/mcp/tools/call")
            .header("Authorization", format!("Bearer {}", token_merge))
            .header("MCP-Protocol-Version", "2025-06-18")
            .header("Mcp-Session-Id", "session-1")
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "tool": "repo.merge.execute",
                    "arguments": {
                        "branch": "develop"
                    },
                    "repo": "stevedores-org/aivcs"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let res_json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(res_json["status"], "approval_required");

        // 4. Register human approval and call again - should succeed
        let payload_string = serde_json::to_string(&json!({ "branch": "develop" })).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(b"repo.merge.execute");
        hasher.update(payload_string.as_bytes());
        let payload_digest = hex::encode(hasher.finalize());

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/mcp/approvals")
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "run_id": "run-test-run",
                    "task_id": "task-test-task",
                    "action": "repo.merge.execute",
                    "payload_digest": payload_digest,
                    "approved_by": "human:supervisor@lornu.ai"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Call again with approval
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/mcp/tools/call")
            .header("Authorization", format!("Bearer {}", token_merge))
            .header("MCP-Protocol-Version", "2025-06-18")
            .header("Mcp-Session-Id", "session-1")
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "tool": "repo.merge.execute",
                    "arguments": {
                        "branch": "develop"
                    },
                    "repo": "stevedores-org/aivcs"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let res_json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(res_json["status"], "success");
        assert!(res_json["authority_id"].as_str().is_some());

        // 5. Replaying the exact same request should fail because human approval is single-use
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/mcp/tools/call")
            .header("Authorization", format!("Bearer {}", token_merge))
            .header("MCP-Protocol-Version", "2025-06-18")
            .header("Mcp-Session-Id", "session-1")
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "tool": "repo.merge.execute",
                    "arguments": {
                        "branch": "develop"
                    },
                    "repo": "stevedores-org/aivcs"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let res_json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(res_json["status"], "approval_required");
    }

    #[tokio::test]
    async fn test_gateway_max_risk_escalation() {
        let app = setup_router().await;

        // Token with max_risk="read" but having merge scope
        let token = mint_test_token("read", vec!["repo.merge.execute"]);

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/mcp/tools/call")
            .header("Authorization", format!("Bearer {}", token))
            .header("MCP-Protocol-Version", "2025-06-18")
            .header("Mcp-Session-Id", "session-1")
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "tool": "repo.merge.execute",
                    "arguments": {
                        "branch": "develop"
                    },
                    "repo": "stevedores-org/aivcs"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let res_json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(res_json["status"], "denied");
        assert!(res_json["reason"]
            .as_str()
            .unwrap()
            .contains("exceeds maximum allowed risk"));
    }

    #[tokio::test]
    async fn test_gateway_max_risk_blocks_write_tool() {
        let app = setup_router().await;

        // Token with max_risk="read" but write scope — must not bypass list_tools filtering.
        let token = mint_test_token("read", vec!["repo.diff.write"]);

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/mcp/tools/call")
            .header("Authorization", format!("Bearer {}", token))
            .header("MCP-Protocol-Version", "2025-06-18")
            .header("Mcp-Session-Id", "session-1")
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "tool": "repo.diff.write",
                    "arguments": {},
                    "repo": "stevedores-org/aivcs"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let res_json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(res_json["status"], "denied");
        assert!(res_json["reason"]
            .as_str()
            .unwrap()
            .contains("exceeds maximum allowed risk"));
    }

    #[tokio::test]
    async fn test_gateway_revocation() {
        let app = setup_router().await;
        let token = mint_test_token("write", vec!["repo.diff.read"]);

        // Revoke the JTI
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/mcp/revocation")
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "jti": "test-jti-1"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Try to call tool - should fail with 401 token_revoked
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/mcp/tools/call")
            .header("Authorization", format!("Bearer {}", token))
            .header("MCP-Protocol-Version", "2025-06-18")
            .header("Mcp-Session-Id", "session-1")
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "tool": "repo.diff.read",
                    "arguments": {},
                    "repo": "stevedores-org/aivcs"
                }))
                .unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let res_json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(res_json["error"], "token_revoked");
    }

    /// Issue [#228](https://github.com/stevedores-org/aivcs/issues/228)
    /// Feature 3.1 requires human approvals to expire — the AC says
    /// *"The grant must be single-use and expire (e.g., within 2 hours)."*
    ///
    /// This test:
    /// 1. Posts a human approval via the public `/v1/mcp/approvals` endpoint.
    /// 2. Reaches into the in-memory store and back-dates the approval's
    ///    `created_at` past `APPROVAL_TTL_HOURS`.
    /// 3. Calls the destructive tool with the same `(run_id, task_id, action,
    ///    payload_digest)` tuple and asserts the gateway responds with
    ///    `approval_required` whose `reason` names the *expired* path
    ///    (not the never-approved one) so operators can tell the two cases
    ///    apart without grepping the audit log.
    #[tokio::test]
    async fn test_gateway_human_approval_ttl_expiry() {
        let (app, state) = setup_router_with_state().await;
        let token_merge = mint_test_token("write", vec!["repo.merge.execute"]);

        // 1. Register a fresh human approval for the merge action.
        let payload_string = serde_json::to_string(&json!({ "branch": "develop" })).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(b"repo.merge.execute");
        hasher.update(payload_string.as_bytes());
        let payload_digest = hex::encode(hasher.finalize());

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/mcp/approvals")
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "run_id": "run-test-run",
                    "task_id": "task-test-task",
                    "action": "repo.merge.execute",
                    "payload_digest": payload_digest,
                    "approved_by": "human:supervisor@lornu.ai"
                }))
                .unwrap(),
            ))
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // 2. Back-date the approval's `created_at` past the TTL. Touching
        // the in-memory state directly is the simplest test seam — the
        // alternative (sleeping > 2h or wiring a time source) is impractical.
        {
            let mut approvals = state.approvals.lock().unwrap();
            assert_eq!(approvals.len(), 1, "exactly one approval should be staged");
            for appr in approvals.values_mut() {
                appr.created_at = Utc::now() - chrono::Duration::hours(APPROVAL_TTL_HOURS + 1);
            }
        }

        // 3. Call the destructive tool — must be rejected with the
        //    `expired`-specific reason.
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/mcp/tools/call")
            .header("Authorization", format!("Bearer {}", token_merge))
            .header("MCP-Protocol-Version", "2025-06-18")
            .header("Mcp-Session-Id", "session-1")
            .header("Content-Type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&json!({
                    "tool": "repo.merge.execute",
                    "arguments": { "branch": "develop" },
                    "repo": "stevedores-org/aivcs"
                }))
                .unwrap(),
            ))
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let res_json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(res_json["status"], "approval_required");

        // The reason must explicitly call out "expired" (not the
        // never-approved phrasing) so the caller can act differently.
        let reason = res_json["reason"].as_str().unwrap_or("");
        assert!(
            reason.to_lowercase().contains("expired"),
            "expired-approval reason should contain 'expired'; got: {reason}"
        );
        assert!(
            reason.contains(&format!("{}h", APPROVAL_TTL_HOURS)),
            "expired-approval reason should surface the TTL; got: {reason}"
        );

        // And the in-memory approval must NOT have been marked `used` —
        // we never consumed it, we rejected it.
        let approvals = state.approvals.lock().unwrap();
        for appr in approvals.values() {
            assert!(
                !appr.used,
                "expired approval must not be marked as consumed"
            );
        }
    }
}
