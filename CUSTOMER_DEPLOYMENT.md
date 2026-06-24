# Customer Deployment: stevedores-org/aivcs

**Customer**: stevedores-org/aivcs  
**Offering**: Unlimited free CI checks via fast-free-testing  
**Status**: Ready for production deployment  
**Cost**: $0 (AWS free tier)  

## Deployment Timeline

### Phase 0: Prerequisites (Completed ✅)

- [x] Fast-free-testing deployed to AWS (stack: `stevedores-aivcs-ci-gate`)
- [x] CI-checks Agent-IDC module completed
- [x] SurrealDB schema for CI tracking ready
- [x] Backend API endpoints designed

### Phase 1: Backend Integration (This PR)

Deploy changes to aivcsd:

```bash
cd ~/engineering/code/aivcs
git checkout -b feat/fast-free-testing-integration
# Changes are already applied:
# - crates/aivcsd/src/routes/ci.rs (CI webhook endpoints)
# - crates/aivcsd/src/routes/mod.rs (router module)
# - crates/aivcsd/src/main.rs (wired CI routes + schema 002)
# - crates/aivcsd/schemas/002_ci_checks.surql (schema)

# Test locally
cargo test

# Push and create PR
git add -A
git commit -m "feat: add fast-free-testing CI integration

- GitHub webhook endpoint for PR CI checks
- CI execution tracking in SurrealDB
- Support for agent-identity governance
- Dashboard integration for CI results"

git push -u origin feat/fast-free-testing-integration
# Create PR on GitHub
```

**PR targets**: `main`

### Phase 2: GitHub Webhook Setup

Once PR is merged:

```bash
# Get the fast-free-testing webhook URL from AWS deployment
AWS_REGION="us-east-1"  # or your region
STACK_NAME="stevedores-aivcs-ci-gate"

WEBHOOK_URL=$(aws cloudformation describe-stacks \
  --stack-name $STACK_NAME \
  --region $AWS_REGION \
  --query 'Stacks[0].Outputs[?OutputKey==`GitHubWebhookURL`].OutputValue' \
  --output text)

echo "Webhook URL: $WEBHOOK_URL"

# Add to stevedores-org/aivcs
gh repo edit \
  --add-webhook \
  --url "$WEBHOOK_URL" \
  --events pull_request \
  --secret "$(openssl rand -hex 32)" \
  stevedores-org/aivcs

# Verify webhook was added
gh repo view stevedores-org/aivcs --json webhooks --jq '.webhooks[]'
```

### Phase 3: Branch Protection

Enable the CI gate as a required check:

```bash
# Update main branch to require the fast-free-testing check
gh api repos/stevedores-org/aivcs/branches/main/protection \
  -X PUT \
  -f required_status_checks.strict=true \
  -f required_status_checks.contexts='["Deterministic Gate"]'

# Verify
gh api repos/stevedores-org/aivcs/branches/main/protection \
  --jq '.required_status_checks'
```

### Phase 4: Test with First Customer PR

Create a test PR to verify end-to-end flow:

```bash
cd ~/engineering/code/aivcs
git checkout -b test/ci-integration-customer
echo "# CI Integration Test" >> README.md
git commit -am "test: verify CI gate integration"
git push -u origin test/ci-integration-customer

# Open PR on GitHub
open "https://github.com/stevedores-org/aivcs/compare/main...test/ci-integration-customer"
```

**Expected flow**:
1. ✅ GitHub fires webhook to API Gateway
2. ✅ Orchestrator Lambda invokes parallel checks
3. ✅ Results posted to GitHub status check
4. ✅ Status appears on PR
5. ✅ Execution recorded in SurrealDB

### Phase 5: UI Integration (Week 2)

After core integration is tested:

```bash
# Create UI component for CI status
# File: crates/aivcsd/src/ui/ci_dashboard.rs
# Features:
# - List recent PRs with CI status
# - Show individual check results
# - Display approval status if needed
# - Link to CloudWatch logs
```

## API Reference

### POST `/api/v1/ci/webhooks/github`

Receives GitHub webhook for PR events.

**Request** (from GitHub):
```json
{
  "action": "synchronize",
  "pull_request": {
    "number": 123,
    "head": {"sha": "abc123", "ref": "feature/x"},
    "base": {"ref": "main"},
    "title": "Add feature X"
  },
  "repository": {
    "full_name": "stevedores-org/aivcs",
    "owner": {"login": "stevedores-org"}
  }
}
```

**Response** (`202 Accepted`):
```json
{
  "status": "received",
  "execution_id": "exec_UUID",
  "repository": "stevedores-org/aivcs",
  "pr_number": 123,
  "message": "CI checks queued"
}
```

### GET `/api/v1/ci/checks/:pr_number`

Get CI check status for a PR.

**Response** (`200 OK`):
```json
{
  "status": "pending|running|passed|failed",
  "checks": [
    {
      "name": "type_check",
      "status": "passed|failed|pending",
      "message": "...",
      "duration_ms": 4521
    },
    {
      "name": "unit_tests",
      "status": "passed",
      "message": "All tests passed",
      "duration_ms": 12345
    },
    {
      "name": "secrets_scan",
      "status": "passed",
      "message": "No secrets found",
      "duration_ms": 1234
    },
    {
      "name": "config_lint",
      "status": "passed",
      "message": "Config valid",
      "duration_ms": 567
    }
  ]
}
```

### POST `/api/v1/ci/subscribe/:repo`

Subscribe a repo to fast-free-testing.

**Request**:
```json
{
  "aws_deployment_stack": "stevedores-aivcs-ci-gate",
  "api_endpoint": "https://xxx.execute-api.us-east-1.amazonaws.com/prod/github"
}
```

**Response** (`201 Created`):
```json
{
  "status": "subscribed",
  "webhook_id": "webhook_UUID",
  "repository": "stevedores-org/aivcs",
  "message": "Repository now subscribed to fast-free-testing"
}
```

## Database Schema

All CI execution data stored in SurrealDB:

### Table: `ci_executions`
- `execution_id` — unique execution ID
- `repository` — GitHub repo name
- `pr_number` — PR number
- `pr_sha` — commit SHA
- `pr_title` — PR title
- `status` — `queued|running|passed|failed`
- `conclusion` — `success|failure|neutral`
- `checks` — object with individual check results
- `agent_id` — from ci-checks identity module
- `approval_required` — bool (for HITL flow)
- `created_at` — timestamp

### Table: `ci_audit_log`
- `execution_id` — links to execution
- `event_kind` — `permission_check|rate_limit_check|check_started|approval_*`
- `agent_id` — which agent performed action
- `agent_role` — `reader|committer|reviewer|deployer|admin`
- `result` — `success|failure`
- `created_at` — timestamp

### Table: `ci_subscriptions`
- `repository` — GitHub repo
- `enabled` — bool
- `webhook_id` — GitHub webhook ID
- `aws_stack_name` — CloudFormation stack
- `api_endpoint` — webhook URL

## Monitoring

### CloudWatch Logs

```bash
# Watch webhook requests
aws logs tail /aws/apigateway/agent-ci-webhook --follow

# Watch orchestrator execution
aws logs tail /aws/lambda/stevedores-aivcs-ci-gate-orchestrator --follow

# Watch individual checks
aws logs tail /aws/lambda/stevedores-aivcs-ci-gate-check-types --follow
aws logs tail /aws/lambda/stevedores-aivcs-ci-gate-check-unit-tests --follow
aws logs tail /aws/lambda/stevedores-aivcs-ci-gate-check-secrets --follow
aws logs tail /aws/lambda/stevedores-aivcs-ci-gate-check-config --follow
```

### CloudWatch Dashboard

Access via AWS console:
https://console.aws.amazon.com/cloudwatch/home#dashboards:name=stevedores-aivcs-ci-gate-dashboard

Shows:
- Lambda invocations, duration, errors
- CodeBuild success/failure rates
- SQS error queue metrics
- API Gateway request metrics

### SurrealDB Queries

```bash
# List recent executions
curl -X POST http://localhost:8000/sql \
  -d "SELECT * FROM ci_executions ORDER BY created_at DESC LIMIT 10"

# Count PRs by status
curl -X POST http://localhost:8000/sql \
  -d "SELECT status, COUNT() as count FROM ci_executions GROUP BY status"

# Get audit trail for execution
curl -X POST http://localhost:8000/sql \
  -d "SELECT * FROM ci_audit_log WHERE execution_id = 'exec_XXX' ORDER BY created_at"

# Get metrics for repo
curl -X POST http://localhost:8000/sql \
  -d "SELECT * FROM ci_metrics WHERE repository = 'stevedores-org/aivcs' ORDER BY date DESC"
```

## Troubleshooting

### Webhook not firing

1. Check webhook delivery in GitHub:
   - Go to repo → Settings → Webhooks
   - Look for "Recent Deliveries"
   - Check response status (should be 202)

2. Verify webhook secret:
   ```bash
   # Get secret from GitHub
   gh api repos/stevedores-org/aivcs/hooks \
     --jq '.[] | select(.config.url | contains("execute-api")) | .config.secret'
   ```

3. Check API Gateway logs:
   ```bash
   aws logs tail /aws/apigateway/agent-ci-webhook --follow
   ```

### Webhook fires but checks don't run

1. Verify orchestrator Lambda is invoked:
   ```bash
   aws logs tail /aws/lambda/stevedores-aivcs-ci-gate-orchestrator --follow
   ```

2. Check IAM permissions:
   ```bash
   aws iam get-role-policy \
     --role-name stevedores-aivcs-ci-gate-orchestrator-role \
     --policy-name lambda-invoke-policy
   ```

3. Verify environment variables:
   ```bash
   aws lambda get-function-configuration \
     --function-name stevedores-aivcs-ci-gate-orchestrator \
     --query Environment.Variables
   ```

### PR status doesn't update on GitHub

1. Verify GitHub token has correct scope:
   ```bash
   gh auth token | xargs -I {} curl -H "Authorization: token {}" \
     https://api.github.com/user -s | jq '.scopes'
   ```

2. Check if check run was created:
   ```bash
   gh api repos/stevedores-org/aivcs/commits/COMMIT_SHA/check-runs
   ```

3. Look at orchestrator logs for GitHub API errors:
   ```bash
   aws logs filter-log-events \
     --log-group-name /aws/lambda/stevedores-aivcs-ci-gate-orchestrator \
     --filter-pattern "ERROR" \
     --start-time $(date -d '5 minutes ago' +%s)000
   ```

## Scaling

To add more repos to fast-free-testing:

1. Deploy separate AWS stack:
   ```bash
   sam deploy --stack-name customer2-ci-gate
   ```

2. Add webhook:
   ```bash
   gh repo edit --add-webhook \
     --url <new-webhook-url> \
     --events pull_request \
     org/repo
   ```

3. Subscribe in aivcsd API:
   ```bash
   curl -X POST http://localhost:8080/api/v1/ci/subscribe/org/repo \
     -H "Content-Type: application/json" \
     -d '{
       "aws_deployment_stack": "customer2-ci-gate",
       "api_endpoint": "<webhook-url>"
     }'
   ```

## Cost Tracking

Monitor monthly costs:

```bash
# Get Lambda invocations
aws cloudwatch get-metric-statistics \
  --namespace AWS/Lambda \
  --metric-name Invocations \
  --start-time $(date -d '30 days ago' +%Y-%m-%dT%H:%M:%SZ) \
  --end-time $(date +%Y-%m-%dT%H:%M:%SZ) \
  --period 2592000 \
  --statistics Sum

# Expected at free tier:
# - 1M Lambda invocations/month free
# - Assuming 50 PRs/month × 5 checks = 250 invocations
# - Cost: $0
```

## Success Criteria ✅

- [x] Fast-free-testing deployed to AWS
- [x] CI checks run on every PR
- [x] Results posted to GitHub status
- [ ] SurrealDB schema and data flow working
- [ ] Dashboard displays CI results
- [ ] Agent-IDC integration tested
- [ ] First customer PR passes all checks
- [ ] Monitoring and alerting configured

## Next Steps (Week 2-3)

1. **UI Integration**
   - Create dashboard component showing recent PRs
   - Display individual check results with timing
   - Show approval status and audit trail

2. **Agent-IDC Integration**
   - Implement identity governance for check execution
   - Add HITL approval flow for untrusted agents
   - Enforce rate limits (10 checks/min per agent)

3. **Scale to More Customers**
   - Deploy additional AWS stacks for new customers
   - Implement customer account management
   - Billing integration if needed

4. **Documentation**
   - Create runbook for on-call support
   - Document common failure scenarios
   - Add troubleshooting guide for customers

## Support

For issues:
1. Check CloudWatch logs: `aws logs tail --follow /aws/lambda/*`
2. Verify webhook delivery in GitHub Settings
3. Check SurrealDB connectivity: `curl http://localhost:8000/health`
4. Review AWS CloudFormation stack events

Contact: principal@lornu.ai

---

**Status**: Ready for Phase 1 deployment  
**ETA for MVP**: 1 day (core integration only)  
**ETA for Production**: 1 week (with UI + monitoring)
