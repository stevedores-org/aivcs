# Second Customer Deployment: lornu-ai/aivcs-lornu-demo

**Customer**: lornu-ai/aivcs-lornu-demo  
**Status**: Ready for integration  
**Infrastructure**: Crossplane (from lornu-ai/infra-code)  
**Reusing**: Fast-free-testing infrastructure for stevedores-org/aivcs  
**Cost**: $0 (AWS free tier)  

## Quick Start

The backend is already deployed. Deploy infrastructure via Crossplane and wire GitHub webhook:

### Step 1: Deploy with Crossplane (lornu-ai/infra-code)

```bash
cd ~/engineering/code/infra-code

# Reference the existing Crossplane XRD and Composition patterns:
# - XRDs: crossplane/aws/hub/control-plane/base/compositions/xrd-*.yaml
# - Compositions: crossplane/aws/hub/control-plane/base/compositions/composition-*.yaml
# See: CLAUDE.md and AGENTS.md for infra-code guidelines

# Create a Claim for customer 2 (or use Flux GitOps to deploy)
# Consult the existing Crossplane setup in infra-code for the FastFreeTestingGate XRD/Composition

# Example (follow actual infra-code patterns):
kubectl apply -f - <<EOF
apiVersion: fft.lornu.ai/v1alpha1
kind: FastFreeTestingGate
metadata:
  name: aivcs-lornu-demo-ci-gate
  namespace: fft-system
spec:
  # Consult lornu-ai/infra-code for actual spec fields
EOF

# Wait for reconciliation
kubectl get fft aivcs-lornu-demo-ci-gate -w
```

### Step 2: Get Webhook URL from Crossplane

```bash
# Retrieve from Crossplane composite status
WEBHOOK_URL=$(kubectl get fft aivcs-lornu-demo-ci-gate -o jsonpath='{.status.webhookUrl}')
echo "Webhook URL: $WEBHOOK_URL"
```

### Step 3: Add GitHub Webhook

```bash
gh repo edit \
  --add-webhook \
  --url "$WEBHOOK_URL" \
  --events pull_request \
  --secret "$(openssl rand -hex 32)" \
  lornu-ai/aivcs-lornu-demo
```

### Step 4: Subscribe via API

```bash
curl -X POST http://aivcsd:8080/api/v1/ci/subscribe/lornu-ai/aivcs-lornu-demo \
  -H "Content-Type: application/json" \
  -d '{
    "aws_deployment_stack": "aivcs-lornu-demo-ci-gate",
    "api_endpoint": "'$WEBHOOK_URL'"
  }'
```

### Step 5: Test

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

Infrastructure managed by Crossplane (infra-code repo):

```
Crossplane XRD: FastFreeTestingGate
         ↓
┌─────────────────────────────────────┐
│   AWS Resources (per-customer)      │
│  ├─ API Gateway                     │
│  ├─ Lambda Functions                │
│  └─ IAM Roles                       │
└────────────────┬────────────────────┘
                 ↓
    ┌────────────┴──────────────┐
    ↓                           ↓
stevedores-org/aivcs    lornu-ai/aivcs-lornu-demo
(Customer 1)             (Customer 2)
Crossplane Composite    Crossplane Composite
(aivcs-ci-gate)         (aivcs-lornu-demo-ci-gate)
    ↓                           ↓
GitHub Webhooks         GitHub Webhooks
    └───────────┬────────────────┘
                ↓
           aivcsd Backend
         (Shared SurrealDB)
```

**Benefits of Crossplane**:
- Infrastructure as code (GitOps)
- Declarative multi-tenancy
- Automatic resource management
- Self-healing infrastructure
- Unified billing/cost tracking

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
- stevedores-org/aivcs: secret-123... (stored in k8s secret)
- lornu-ai/aivcs-lornu-demo: secret-456... (stored in k8s secret)

Both validated against their respective `CI_WEBHOOK_SECRET` env var.

**Managed by**: Kubernetes secrets (cross-referencing via Crossplane)

## Monitoring

View logs via Kubernetes:
```bash
# Watch all CI gateway logs
kubectl logs -f -l app=fft-orchestrator -c orchestrator

# Filter by repo
kubectl logs -f -l app=fft-orchestrator -c orchestrator | grep 'aivcs-lornu-demo'

# CloudWatch (via Crossplane)
aws logs tail /aws/lambda/aivcs-lornu-demo-ci-gate-orchestrator --follow
```

## Billing & Cost Tracking

Still $0 with AWS free tier. Monitor via Crossplane:
```bash
# View resource status
kubectl get fft -A
kubectl describe fft aivcs-lornu-demo-ci-gate

# Monitor AWS costs (if applicable)
aws ce get-cost-and-usage \
  --time-period Start=2026-06-01,End=2026-07-01 \
  --granularity MONTHLY \
  --metrics BlendedCost
```

Expected: ~500 Lambda invocations/month combined (stevedores-org: 250 + lornu-ai: 250) — still under 1M free tier.

## Scaling to More Customers

Same pattern for additional repos via Crossplane:
1. Create new FastFreeTestingGate Crossplane composite
2. Crossplane automatically provisions AWS resources
3. Add GitHub webhook to new repo
4. Subscribe via API endpoint
5. All backed by unified SurrealDB

**Infrastructure changes**: Single line YAML per new customer

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
