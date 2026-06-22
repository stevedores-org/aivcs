# Fast-Free-Testing Integration for stevedores-org/aivcs

**Status**: First customer integration  
**Client**: stevedores-org/aivcs  
**Offering**: Unlimited free CI checks  
**Deployment Target**: Production  

## Overview

Integration of `lornu-ai/fast-free-testing` (deterministic, fast, free AWS CI gate) with stevedores-org/aivcs to provide:

- ✅ Automatic CI checks on every PR
- ✅ Real-time status updates in UI
- ✅ Audit trail in SurrealDB
- ✅ Zero-cost infrastructure (AWS free tier)
- ✅ Agent-identity governed checks (from ci-checks)

## Architecture

```
stevedores-org/aivcs PR
         ↓
    GitHub Webhook
         ↓
fast-free-testing API Gateway
         ↓
    Lambda Orchestrator
         ↓
    ┌────┬────┬────┬───────┐
    ↓    ↓    ↓    ↓       ↓
  Types Tests Secrets Config CodeBuild
    ↓    ↓    ↓    ↓       ↓
    └────┬────┬────┬───────┘
         ↓
   GitHub Status API
         ↓
  aivcsd Backend
         ↓
  SurrealDB (audit trail)
         ↓
  UI Components (dashboard)
```

## Implementation

### Phase 1: Backend Integration (API Endpoints)

Add to `crates/aivcsd/src/main.rs`:

```rust
#[post("/api/v1/ci/webhooks/github")]
async fn handle_github_webhook(
    State(state): State<AppState>,
    Json(payload): Json<GithubWebhookPayload>,
) -> Result<Json<Value>, Error> {
    // 1. Validate webhook signature
    // 2. Parse PR details
    // 3. Create CI execution record in SurrealDB
    // 4. Poll fast-free-testing for results
    // 5. Update GitHub status check
    // 6. Store audit trail
    
    Ok(Json(json!({
        "status": "received",
        "execution_id": execution_id,
        "webhook_id": payload.hook_id
    })))
}

#[get("/api/v1/ci/checks/:pr_number")]
async fn get_pr_checks(
    State(state): State<AppState>,
    Path(pr_number): Path<u32>,
) -> Result<Json<Value>, Error> {
    // Fetch CI check results from SurrealDB
    // Return status, results, audit trail
}

#[post("/api/v1/ci/subscribe/:repo")]
async fn subscribe_to_ci(
    State(state): State<AppState>,
    Path(repo): Path<String>,
    Json(config): Json<CiSubscriptionConfig>,
) -> Result<Json<Value>, Error> {
    // Subscribe repo to fast-free-testing
    // Store webhook URL and config in SurrealDB
    // Create webhook on GitHub
}
```

### Phase 2: Database Schema (SurrealDB)

Create `crates/aivcsd/schemas/002_ci_checks.surql`:

```sql
-- CI Execution Records
DEFINE TABLE ci_executions SCHEMAFULL;
DEFINE FIELD execution_id ON ci_executions TYPE string;
DEFINE FIELD repository ON ci_executions TYPE string;
DEFINE FIELD pr_number ON ci_executions TYPE number;
DEFINE FIELD pr_sha ON ci_executions TYPE string;
DEFINE FIELD pr_title ON ci_executions TYPE string;
DEFINE FIELD status ON ci_executions TYPE string;  -- queued|running|passed|failed
DEFINE FIELD conclusion ON ci_executions TYPE string;  -- success|failure|neutral
DEFINE FIELD started_at ON ci_executions TYPE datetime;
DEFINE FIELD completed_at ON ci_executions TYPE datetime;
DEFINE FIELD checks ON ci_executions TYPE object;  -- { type: {status, duration, error} }
DEFINE FIELD agent_id ON ci_executions TYPE string;  -- from ci-checks identity governance
DEFINE FIELD approval_required ON ci_executions TYPE bool;
DEFINE FIELD approval_granted ON ci_executions TYPE bool;
DEFINE FIELD approved_by ON ci_executions TYPE string;
DEFINE FIELD created_at ON ci_executions TYPE datetime DEFAULT time::now();

-- CI Subscription (per-repo config)
DEFINE TABLE ci_subscriptions SCHEMAFULL;
DEFINE FIELD repository ON ci_subscriptions TYPE string;
DEFINE FIELD owner ON ci_subscriptions TYPE string;
DEFINE FIELD enabled ON ci_subscriptions TYPE bool;
DEFINE FIELD webhook_id ON ci_subscriptions TYPE string;
DEFINE FIELD webhook_secret ON ci_subscriptions TYPE string;
DEFINE FIELD aws_deployment_stack ON ci_subscriptions TYPE string;
DEFINE FIELD created_at ON ci_subscriptions TYPE datetime DEFAULT time::now();

-- CI Audit Trail (from fast-free-testing)
DEFINE TABLE ci_audit_log SCHEMAFULL;
DEFINE FIELD audit_id ON ci_audit_log TYPE string;
DEFINE FIELD execution_id ON ci_audit_log TYPE string;
DEFINE FIELD event_kind ON ci_audit_log TYPE string;  -- permission_check|check_started|check_completed
DEFINE FIELD agent_id ON ci_audit_log TYPE string;
DEFINE FIELD agent_role ON ci_audit_log TYPE string;
DEFINE FIELD result ON ci_audit_log TYPE string;  -- success|failure
DEFINE FIELD reason ON ci_audit_log TYPE string;
DEFINE FIELD created_at ON ci_audit_log TYPE datetime DEFAULT time::now();
```

### Phase 3: CI Integration Crate

Create `crates/aivcs-fast-free-testing/src/lib.rs`:

```rust
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastFreeTestingConfig {
    pub aws_stack_name: String,
    pub api_endpoint: String,
    pub github_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiCheckResult {
    pub check_name: String,
    pub status: String,  // passed|failed
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiExecution {
    pub id: String,
    pub repository: String,
    pub pr_number: u32,
    pub pr_sha: String,
    pub status: String,  // queued|running|passed|failed
    pub checks: Vec<CiCheckResult>,
    pub approval_required: bool,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub async fn subscribe_repo(
    config: FastFreeTestingConfig,
    repo: &str,
    owner: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // Call fast-free-testing to set up webhook
    // Return webhook_id
    Ok("webhook_id".to_string())
}

pub async fn poll_execution(
    config: &FastFreeTestingConfig,
    execution_id: &str,
) -> Result<CiExecution, Box<dyn std::error::Error>> {
    // Poll fast-free-testing for execution status
    // Return current state
    Ok(CiExecution {
        id: execution_id.to_string(),
        repository: "".to_string(),
        pr_number: 0,
        pr_sha: "".to_string(),
        status: "running".to_string(),
        checks: vec![],
        approval_required: false,
        created_at: Utc::now(),
        completed_at: None,
    })
}
```

### Phase 4: UI Components

Create `crates/aivcsd/src/ui/ci_checks.rs`:

```rust
// CiChecksPanel component (for dashboard)
// Shows:
// - PR number and title
// - Overall status (passing/failing)
// - Individual check results with timing
// - Approval status if needed
// - Audit trail link
```

## Deployment Steps

### 1. Deploy fast-free-testing to AWS (Done ✅)

```bash
cd ~/engineering/code/fast-free-testing/infra
sam deploy --stack-name stevedores-aivcs-ci-gate
```

Outputs:
- `GitHubWebhookURL`: Use this in step 2
- `ArtifactBucket`: S3 storage for artifacts
- `DashboardURL`: CloudWatch monitoring

### 2. Add GitHub Webhook

```bash
gh repo edit --add-webhook \
  --url <GitHubWebhookURL> \
  --events pull_request \
  stevedores-org/aivcs
```

### 3. Deploy aivcsd Backend Updates

```bash
cd ~/engineering/code/aivcs
git checkout -b feat/fast-free-testing-integration
# Apply backend changes (Cargo.toml, main.rs, schema)
cargo test
git push -u origin feat/fast-free-testing-integration
# Open PR
```

### 4. Subscribe stevedores-org/aivcs

Call the API:

```bash
curl -X POST http://localhost:8080/api/v1/ci/subscribe/stevedores-org/aivcs \
  -H "Content-Type: application/json" \
  -d '{
    "aws_deployment_stack": "stevedores-aivcs-ci-gate",
    "api_endpoint": "<GitHubWebhookURL>"
  }'
```

## Testing

### Test PR Flow

```bash
cd ~/engineering/code/aivcs
git checkout -b test/ci-checks
# Make a valid change
git commit -am "test: trigger CI gate"
git push -u origin test/ci-checks
# Create PR
```

Expected:
1. ✅ GitHub webhook fires
2. ✅ fast-free-testing Lambda invoked
3. ✅ Checks run in parallel (types, tests, secrets, config)
4. ✅ Results posted to GitHub status API
5. ✅ Dashboard shows CI results
6. ✅ Audit trail logged to SurrealDB

### Monitoring

```bash
# Watch Lambda logs
sam logs --stack-name stevedores-aivcs-ci-gate --tail

# Check SurrealDB records
curl -X POST http://localhost:8000/sql \
  -d "SELECT * FROM ci_executions ORDER BY created_at DESC LIMIT 5"
```

## Agent-Identity Governance Integration

From `ci-checks` module:

- Each PR execution gets an agent identity
- Permissions validated before check execution
- Rate limits enforced (10 checks/minute per agent)
- Untrusted agents require HITL approval
- Full audit trail in SurrealDB

Configuration in aivcsd:

```rust
let policy = IdentityPolicy {
    require_approval: false,  // Trusted agents auto-approve
    rate_limits: RateLimitConfig {
        max_checks_per_minute: 10,
        max_concurrent_checks: 5,
    },
    audit_level: "standard",
    min_trusted_role: "committer",
};
```

## Cost Analysis

### For stevedores-org/aivcs

**Pricing**: $0 (AWS free tier covers unlimited PRs)

| Component | Free Tier | Usage |
|-----------|-----------|-------|
| Lambda | 1M invocations/month | ~50 PRs/month × 5 checks = 250 |
| S3 | 5GB storage | Artifacts cleanup after 30 days |
| SurrealDB | Local (free) | Audit trails |
| **Total** | **$0** | **Unlimited free** |

## Success Criteria

✅ **MVP Complete When:**
1. GitHub webhook successfully triggers on each PR
2. All 4 checks execute in parallel
3. Results appear in GitHub status check
4. Dashboard displays CI status per PR
5. Audit trail stored in SurrealDB
6. Agent-identity governance enforced
7. First customer PR passes all checks

✅ **Production Ready When:**
1. 10+ PRs tested successfully
2. HITL approval flow tested
3. Rate limiting verified
4. Cost monitoring active
5. Runbook documented
6. On-call rotation established

## Roadmap

### Week 1: Core Integration
- [ ] Backend API endpoints (aivcsd)
- [ ] SurrealDB schema
- [ ] GitHub webhook → SurrealDB flow
- [ ] Test with first PR

### Week 2: Dashboard & Monitoring
- [ ] UI components for CI results
- [ ] CloudWatch dashboard
- [ ] Slack notifications
- [ ] Cost monitoring

### Week 3: Agent-IDC & Scale
- [ ] Integrate ci-checks identity module
- [ ] HITL approval flow
- [ ] Scale to other repos
- [ ] Documentation & runbook

## Contacts

- **fast-free-testing**: `lornu-ai/fast-free-testing`
- **ci-checks**: `lornu-ai/ci-checks` (Agent-IDC)
- **Dashboard**: aivcsd UI component
- **Support**: principal@lornu.ai

---

**Status**: Ready for implementation  
**ETA**: 1 week for MVP  
**Cost**: $0 (free tier)  
**First customer**: stevedores-org/aivcs ✅
