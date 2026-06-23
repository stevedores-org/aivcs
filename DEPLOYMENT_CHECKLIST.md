# Fast-Free-Testing Deployment Checklist for stevedores-org/aivcs

## Phase 1: Core Integration (CURRENT - This PR #295)

**Status**: PR #295 ready for review  
**Branch**: `feat/fast-free-testing-integration`  
**Target**: `main`  

### Code Review ✅
- [x] API endpoints implemented (webhooks, checks, subscribe)
- [x] SurrealDB schema created (ci_executions, ci_audit_log, ci_subscriptions, ci_metrics)
- [x] Routes wired into Axum router
- [x] Environment variable configuration (GITHUB_TOKEN)
- [x] Agent-IDC integration points identified
- [x] Documentation complete

### Pre-Merge Tasks
- [ ] Run tests: `cd ~/engineering/code/aivcs && cargo test`
- [ ] Review API endpoints in FAST_FREE_TESTING_INTEGRATION.md
- [ ] Verify SurrealDB schema syntax
- [ ] Check dependencies (uuid, serde_json, etc.)

### Merge & Deploy
- [ ] Merge PR #295 to main
- [ ] Pull latest main in production environment
- [ ] Restart aivcsd service: `systemctl restart aivcsd` (or equivalent)
- [ ] Verify health check: `curl http://localhost:8080/health`
- [ ] Verify CI routes available: `curl http://localhost:8080/api/v1/ci/checks/1`

---

## Phase 2: AWS Deployment (NEXT)

**Timeline**: 1 day after Phase 1 merge  
**Owner**: principal@lornu.ai  

### Pre-Deployment
- [ ] Verify AWS credentials: `aws sts get-caller-identity`
- [ ] Check CloudFormation permissions
- [ ] Ensure fast-free-testing stack already deployed:
  ```bash
  aws cloudformation describe-stacks \
    --stack-name stevedores-aivcs-ci-gate \
    --query 'Stacks[0].StackStatus'
  ```

### Deploy fast-free-testing Stack

If not already deployed:

```bash
cd ~/engineering/code/fast-free-testing/infra

# Validate template
sam validate --template template.yaml

# Build Lambda functions
sam build --use-container

# Deploy
sam deploy --guided \
  --stack-name stevedores-aivcs-ci-gate \
  --parameter-overrides \
    GitHubToken="$(gh auth token)" \
    LambdaMemory=512 \
    LambdaTimeout=180 \
    CodeBuildEnabled=true
```

### Capture Outputs
- [ ] Save `GitHubWebhookURL` → needed for GitHub webhook setup
- [ ] Save `ArtifactBucketName` → for artifact storage
- [ ] Save `DashboardURL` → for monitoring
- [ ] Save `ErrorTopicArn` → for error notifications
- [ ] Save stack name: `stevedores-aivcs-ci-gate`

### Verification
- [ ] Test Lambda directly (invoke check-types)
- [ ] Verify API Gateway endpoint responds
- [ ] Check CloudWatch logs for errors
- [ ] Verify SQS error queue is empty
- [ ] Check IAM role permissions

---

## Phase 3: GitHub Integration (SAME DAY as Phase 2)

**Timeline**: Immediately after AWS deployment  
**Owner**: principal@lornu.ai  

### Add GitHub Webhook

```bash
# Set variables from Phase 2
WEBHOOK_URL="<GitHubWebhookURL from stack outputs>"
WEBHOOK_SECRET="$(openssl rand -hex 32)"

# Add webhook to stevedores-org/aivcs
gh repo edit \
  --add-webhook \
  --url "$WEBHOOK_URL" \
  --events pull_request \
  --secret "$WEBHOOK_SECRET" \
  stevedores-org/aivcs

# Verify webhook was added
gh api repos/stevedores-org/aivcs/hooks --jq '.[] | select(.config.url | contains("execute-api")) | {url: .config.url, active: .active}'
```

### Store Webhook Secret (IMPORTANT!)
- [ ] Store webhook secret in secure location (e.g., AWS Secrets Manager)
  ```bash
  aws secretsmanager create-secret \
    --name stevedores-aivcs-ci-webhook-secret \
    --secret-string "$WEBHOOK_SECRET"
  ```
- [ ] Update Lambda environment with secret if validation needed

### Enable Branch Protection

```bash
# Require the CI gate check on main branch
gh api repos/stevedores-org/aivcs/branches/main/protection \
  -X PUT \
  -f required_status_checks.strict=true \
  -f required_status_checks.contexts='["Deterministic Gate"]' \
  -f required_pull_request_reviews.required_approving_review_count=1

# Verify
gh api repos/stevedores-org/aivcs/branches/main/protection \
  --jq '{status_checks: .required_status_checks.contexts, pull_request_reviews: .required_pull_request_reviews}'
```

---

## Phase 4: Testing (DAY 2 after Phase 2)

**Timeline**: 1 day after GitHub webhook setup  
**Owner**: principal@lornu.ai  

### Create Test PR

```bash
cd ~/engineering/code/aivcs

git checkout -b test/ci-integration-customer-phase4
echo "# CI Integration Test - Phase 4" >> README.md
echo "Testing the fast-free-testing deterministic gate with stevedores-org/aivcs" >> README.md

git commit -am "test: verify CI gate with customer PR"
git push -u origin test/ci-integration-customer-phase4
```

Then open PR on GitHub:
```bash
open "https://github.com/stevedores-org/aivcs/compare/main...test/ci-integration-customer-phase4"
```

### Verify Webhook Fires

- [ ] Check GitHub webhook recent deliveries
  - Go to repo → Settings → Webhooks
  - Look for most recent delivery
  - Response should be `202 Accepted` or `200 OK`

### Monitor Lambda Execution

```bash
# Watch orchestrator logs
sam logs --stack-name stevedores-aivcs-ci-gate --tail

# Or use AWS CLI
aws logs tail /aws/lambda/stevedores-aivcs-ci-gate-orchestrator --follow
```

### Verify PR Status Check

- [ ] PR should show "Deterministic Gate" check
- [ ] Check should transition: pending → running → passed (or failed)
- [ ] Clicking "Details" should show check results or details link

### Verify SurrealDB Recording

```bash
# Connect to SurrealDB and query
curl -X POST http://localhost:8000/sql \
  --header "Accept: application/json" \
  --data "SELECT * FROM ci_executions ORDER BY created_at DESC LIMIT 1"

# Expected output: execution record with pr_number = your test PR number
```

### Verify Audit Trail

```bash
# Get the execution_id from previous query
EXECUTION_ID="<execution_id from ci_executions>"

curl -X POST http://localhost:8000/sql \
  --header "Accept: application/json" \
  --data "SELECT event_kind, agent_id, result FROM ci_audit_log WHERE execution_id = '$EXECUTION_ID' ORDER BY created_at"

# Expected: permission_check → check_started → check_completed
```

---

## Phase 5: Dashboard UI (WEEK 2)

**Timeline**: After Phase 4 verification (optional for MVP)  
**Owner**: TBD  

### Create UI Component

```bash
# New file: crates/aivcsd/src/ui/ci_dashboard.rs
# Features:
# - List recent PRs with CI status
# - Show individual check results
# - Display timing and duration
# - Link to CloudWatch logs
# - Show approval status if HITL required
```

### Display Components

- [ ] CI Status badge (green/red/yellow)
- [ ] Individual check results (type, tests, secrets, config)
- [ ] Timing breakdown
- [ ] Approval flow (if applicable)
- [ ] Audit trail view
- [ ] Links to AWS CloudWatch

### Integration

- [ ] Add to PR detail page
- [ ] Add to dashboard/overview
- [ ] Wire to backend `/api/v1/ci/checks/:pr_number`

---

## Phase 6: Agent-IDC Integration (WEEK 3)

**Timeline**: After dashboard is complete  
**Owner**: TBD  

### Implement Identity Governance

From `lornu-ai/ci-checks` module:

- [ ] Wire AgentIdentity validation
- [ ] Implement rate limiting (10 checks/min per agent)
- [ ] Add permission expiry checks
- [ ] Implement HITL approval flow for untrusted agents

### Configuration

```rust
// In aivcsd config
let policy = IdentityPolicy {
    require_approval: false,  // Initially auto-approve
    rate_limits: RateLimitConfig {
        max_checks_per_minute: 10,
        max_concurrent_checks: 5,
    },
    audit_level: "standard",
    min_trusted_role: "committer",
};
```

### Testing

- [ ] Test with trusted agent (should auto-approve)
- [ ] Test with untrusted agent (should require HITL)
- [ ] Verify rate limiting (queue 11 checks, 10 execute)
- [ ] Verify audit trail logs all decisions

---

## Phase 7: Scale to Other Customers (WEEK 4)

**Timeline**: After stevedores-org/aivcs is stable  

### For Each New Customer

1. Deploy separate AWS stack:
   ```bash
   sam deploy --stack-name customer2-ci-gate
   ```

2. Add webhook to customer repo
3. Subscribe in aivcsd API
4. Test with first PR
5. Document in customer-specific guide

---

## Rollback Plan

If anything goes wrong:

### Quick Rollback to Previous Version

```bash
# On main branch, revert the commit
git revert <commit-hash>
git push origin main

# Restart aivcsd
systemctl restart aivcsd

# Verify health check
curl http://localhost:8080/health
```

### Delete AWS Stack (if needed)

```bash
aws cloudformation delete-stack \
  --stack-name stevedores-aivcs-ci-gate

# Wait for deletion
aws cloudformation wait stack-delete-complete \
  --stack-name stevedores-aivcs-ci-gate
```

### Disable GitHub Webhook

```bash
# Get webhook ID
WEBHOOK_ID=$(gh api repos/stevedores-org/aivcs/hooks \
  --jq '.[] | select(.config.url | contains("execute-api")) | .id')

# Delete webhook
gh api repos/stevedores-org/aivcs/hooks/$WEBHOOK_ID -X DELETE
```

---

## Success Criteria ✅

### MVP (After Phase 4)
- [x] Code integration complete (PR #295)
- [ ] Fast-free-testing deployed and responding
- [ ] GitHub webhook receiving events
- [ ] CI checks executing on test PR
- [ ] Results recorded in SurrealDB
- [ ] GitHub status check showing results
- [ ] No errors in CloudWatch logs

### Production Ready (After Phase 5-6)
- [ ] Dashboard displaying CI results
- [ ] Agent-IDC integration verified
- [ ] Rate limiting enforced
- [ ] HITL approval flow tested
- [ ] 10+ successful test PRs
- [ ] Monitoring alerts configured
- [ ] Runbook documented

---

## Contacts

- **Code**: principal@lornu.ai
- **Deployment**: principal@lornu.ai
- **Support**: principal@lornu.ai
- **Repository**: https://github.com/stevedores-org/aivcs

## Key Links

- **PR #295**: https://github.com/stevedores-org/aivcs/pull/295
- **Integration Guide**: FAST_FREE_TESTING_INTEGRATION.md
- **Deployment Guide**: CUSTOMER_DEPLOYMENT.md
- **fast-free-testing**: https://github.com/lornu-ai/fast-free-testing
- **ci-checks (Agent-IDC)**: https://github.com/lornu-ai/ci-checks

---

**Last Updated**: 2026-06-22  
**Status**: Ready for Phase 1 (PR merged to main)  
**Next Phase**: Phase 2 (AWS deployment) after merge approval
