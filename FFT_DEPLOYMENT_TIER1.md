# Fast-Free-Testing Deployment — Tier 1 Infrastructure

**Reference**: `lornu-ai/six-files` — [docs/DEPLOYMENT_TIERS.md](https://github.com/lornu-ai/six-files/blob/main/docs/DEPLOYMENT_TIERS.md) (ADR-002)

FFT is **Tier 1 infrastructure** (infra/services). Deploy via Crossplane on GKE Autopilot:

## Infrastructure

```
GKE Autopilot (Tier 1)
  ├─ Crossplane Provider (AWS)
  ├─ Flux GitRepository + Kustomization
  ├─ External Secrets Operator (GCP Secret Manager)
  └─ Workload Identity Federation (OIDC)
        ↓
    AWS Resources (via Crossplane)
      ├─ API Gateway
      ├─ Lambda Orchestrator + 4 Check Functions
      └─ SurrealDB Integration
```

## Deployment (via lornu-ai/infra-code patterns)

1. **Reference the Crossplane XRD/Composition** patterns in `lornu-ai/infra-code`:
   - Location: `crossplane/aws/hub/control-plane/base/compositions/`
   - Follow existing `xrd-*.yaml` and `composition-*.yaml` patterns

2. **Create Crossplane Claim** for each customer:
   ```yaml
   # See infra-code/crossplane/aws patterns for actual spec
   apiVersion: fft.lornu.ai/v1alpha1
   kind: FastFreeTestingGate
   metadata:
     name: stevedores-aivcs-ci-gate
     namespace: fft-system
   spec:
     # Exact spec fields from actual XRD in infra-code
   ```

3. **Deploy via Flux GitOps**:
   ```bash
   cd ~/engineering/code/infra-code
   # Apply Claim → Crossplane reconciles AWS resources
   kubectl apply -f claims/stevedores-aivcs-ci-gate.yaml
   ```

4. **Wire GitHub webhooks**:
   ```bash
   # Get API Gateway endpoint from Crossplane status
   WEBHOOK_URL=$(kubectl get fft stevedores-aivcs-ci-gate -o jsonpath='{.status.webhookUrl}')
   
   # Add to GitHub
   gh repo edit --add-webhook \
     --url "$WEBHOOK_URL" \
     --events pull_request \
     stevedores-org/aivcs
   ```

5. **Subscribe via aivcsd API**:
   ```bash
   curl -X POST http://aivcsd:8080/api/v1/ci/subscribe/stevedores-org/aivcs \
     -H "Content-Type: application/json" \
     -d '{"aws_deployment_stack":"stevedores-aivcs-ci-gate","api_endpoint":"'$WEBHOOK_URL'"}'
   ```

## Multiple Customers

Each customer gets a separate Crossplane Claim (same stack suffix varies):
- `stevedores-aivcs-ci-gate` → Customer 1
- `aivcs-lornu-demo-ci-gate` → Customer 2
- etc.

All backed by shared SurrealDB (namespaced by repo).

## Registry & Container Image

- **Build registry**: GAR (`us-central1-docker.pkg.dev/gcp-lornu-ai/lornu-ai/fast-free-testing`)
- **Deploy method**: Crossplane XRD/Composition (no container registry needed for infrastructure)
- **Auth**: GCP Workload Identity Federation (see `six-files` Tier 1 canonical wiring)

## Cluster

- **Type**: GKE Autopilot (Tier 1 standard)
- **Auth**: OIDC + Workload Identity
- **Where**: Hub cluster (`hub` namespace)
- **GitOps**: Flux reconciliation

## See Also

- **Deployment principles**: `lornu-ai/six-files/docs/DEPLOYMENT_TIERS.md` (ADR-002)
- **Implementation patterns**: `lornu-ai/infra-code/crossplane/aws/hub/control-plane/base/compositions/`
- **XRD examples**: `xrd-*.yaml` and `composition-*.yaml` in infra-code
- **CLAUDE.md & AGENTS.md**: infra-code repo guidelines
