# Second Customer Deployment: lornu-ai/aivcs-lornu-demo

**Customer**: lornu-ai/aivcs-lornu-demo  
**Status**: Ready for integration  
**Reusing**: Fast-free-testing infrastructure from stevedores-org/aivcs  
**Cost**: $0 (same AWS free tier stack can serve multiple repos)  

## Quick Start

The backend infrastructure is already deployed. Just add a GitHub webhook to this repo:

### Step 1: Get Webhook URL

```bash
# From existing stevedores-aivcs-ci-gate stack
AWS_REGION="us-east-1"
STACK_NAME="stevedores-aivcs-ci-gate"

WEBHOOK_URL=$(aws cloudformation describe-stacks \
  --stack-name $STACK_NAME \
  --region $AWS_REGION \
  --query 'Stacks[0].Outputs[?OutputKey==`GitHubWebhookURL`].OutputValue' \
  --output text)

echo "Webhook URL: $WEBHOOK_URL"
```

### Step 2: Add GitHub Webhook

```bash
gh repo edit \
  --add-webhook \
  --url "$WEBHOOK_URL" \
  --events pull_request \
  --secret "$(openssl rand -hex 32)" \
  lornu-ai/aivcs-lornu-demo
```

### Step 3: Subscribe via API

```bash
curl -X POST http://aivcsd:8080/api/v1/ci/subscribe/lornu-ai/aivcs-lornu-demo \
  -H "Content-Type: application/json" \
  -d '{
    "aws_deployment_stack": "stevedores-aivcs-ci-gate",
    "api_endpoint": "'$WEBHOOK_URL'"
  }'
```

### Step 4: Test

Create a test PR:
```bash
cd ~/engineering/code/aivcs-lornu-demo
git checkout -b test/ci
echo "# CI Test" >> README.md
git commit -am "test: verify FFT integration"
git push -u origin test/ci
# Create PR on GitHub
```

Expected: "Deterministic Gate" status check appears and runs in 8-15 seconds.

## Architecture

Both customers share the same AWS stack:

```
stevedores-org/aivcs PR          lornu-ai/aivcs-lornu-demo PR
         ↓                                    ↓
    GitHub Webhooks
         ↓
fast-free-testing (stevedores-aivcs-ci-gate)
    ├─ API Gateway (single endpoint)
    ├─ Lambda Orchestrator
    ├─ Check Functions (types, tests, secrets, config)
    └─ SurrealDB (shared execution tracking)
```

**Benefits**:
- Zero additional infrastructure cost
- Shared Lambda execution environment
- Unified audit trail in SurrealDB
- Single webhook secret management

## Multi-Tenant Considerations

### SurrealDB Isolation

Records are namespaced by repository:
```sql
-- Execution records include full_repo_name
SELECT * FROM ci_executions 
  WHERE repository = 'lornu-ai/aivcs-lornu-demo'

-- Audit trail for this customer
SELECT * FROM ci_audit_log 
  WHERE execution_id IN (
    SELECT execution_id FROM ci_executions 
    WHERE repository = 'lornu-ai/aivcs-lornu-demo'
  )
```

### Webhook Secret Management

Each repo has its own webhook secret (stored in GitHub):
- stevedores-org/aivcs: secret-123...
- lornu-ai/aivcs-lornu-demo: secret-456...

Both validated against `CI_WEBHOOK_SECRET` env var (currently single shared secret).

**Future**: Support per-repo secrets in a secrets store (AWS Secrets Manager).

## CloudWatch Monitoring

View logs for both customers:
```bash
# All webhooks
aws logs tail /aws/apigateway/agent-ci-webhook --follow

# Filtered by repo
aws logs tail /aws/lambda/stevedores-aivcs-ci-gate-orchestrator --follow | grep 'aivcs-lornu-demo'
```

## Billing & Cost Tracking

Still $0 with AWS free tier. Monitor combined usage:
```bash
# Total Lambda invocations (both customers)
aws cloudwatch get-metric-statistics \
  --namespace AWS/Lambda \
  --metric-name Invocations \
  --start-time $(date -d '30 days ago' +%Y-%m-%dT%H:%M:%SZ) \
  --end-time $(date +%Y-%m-%dT%H:%M:%SZ) \
  --period 2592000 \
  --statistics Sum

# Expected: ~250 for stevedores-org/aivcs + ~250 for lornu-ai/aivcs-lornu-demo = 500 total
# Still well under 1M free tier
```

## Scaling to More Customers

Same pattern for additional repos:
1. Use same webhook URL (reuse stack)
2. Add GitHub webhook to new repo
3. Subscribe via API endpoint
4. Monitor in unified SurrealDB

**No infrastructure changes needed** — the stack supports unlimited repos.

## Next Steps

1. **Add webhook** to aivcs-lornu-demo (Step 1-2 above)
2. **Test** with first PR
3. **Verify** results in GitHub status check
4. **Share** webhook setup with other teams

## Support

For issues:
- Check webhook delivery: repo → Settings → Webhooks → Recent Deliveries
- View logs: `aws logs tail /aws/lambda/stevedores-aivcs-ci-gate-orchestrator --follow`
- Verify SurrealDB: `curl http://localhost:8000/health`

**Contact**: principal@lornu.ai

---

**Cost**: $0/month (shared infrastructure)  
**Setup Time**: 10 minutes  
**Ready**: Yes
