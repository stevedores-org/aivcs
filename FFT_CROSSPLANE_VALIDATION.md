# FFT: Crossplane/Flux/ESO Deterministic Validation (Issue #2)

**Status**: Implementation for issue `lornu-ai/fast-free-testing#2`  
**Approach**: Static K8s manifest validation + Crossplane composition rendering (no AWS resources spun up)  
**Cost**: $0 (all local binary execution)  
**Speed**: Seconds (no Lambda cold starts, no AWS API calls)  

## Gate Checks (Tier 1 Infrastructure Standard)

### 1. Flux Kustomization Build & Kubeconform Validation

```bash
# For each kustomize path in the PR
flux build kustomization ./clusters/hub/agent-app | kubeconform -strict -output json
```

**Validates**:
- All overlays, patches, variable substitutions compile
- K8s resources conform to OpenAPI schema
- No broken references or missing dependencies

**Catches**: Broken ConfigMaps, invalid DeploymentSpec, missing ServiceAccount references

---

### 2. Crossplane Composition Rendering & Validation

```bash
# For each Crossplane Claim/Composition in PR
crossplane render xr.yaml composition.yaml functions.yaml | \
  crossplane beta validate provider-aws-schemas/ -
```

**Validates**:
- Composition rendering succeeds (functions execute)
- Rendered AWS Managed Resources pass AWS Provider schema validation
- No invalid EC2 instance types, S3 bucket name violations, IAM policy syntax errors

**Catches**: 
- Invalid AWS resource configurations
- Composition function errors
- Schema violations (before Flux ever reconciles)

---

### 3. ServiceAccount & OIDC Guardrails

```bash
# For each ServiceAccount in the PR
if ! grep -q "eks.amazonaws.com/role-arn" deployment.yaml; then
  echo "ERROR: ServiceAccount missing OIDC annotation"
  exit 1
fi

# Validate ARN pattern
if ! grep "eks.amazonaws.com/role-arn" deployment.yaml | grep -qE "arn:aws:iam::[0-9]+:role/"; then
  echo "ERROR: Invalid OIDC IAM role ARN"
  exit 1
fi
```

**Validates**:
- ServiceAccount has correct `eks.amazonaws.com/role-arn` annotation
- ARN matches approved pattern

**Catches**: Missing or malformed OIDC bindings

---

### 4. IAM Least-Privilege Enforcement (OPA/Rego)

```rego
# policy.rego: Enforce least-privilege IAM policies
package k8s.iam

deny[msg] {
    resource := input.resource
    resource.kind == "IAMPolicy"
    policy := resource.spec.forProvider.document
    
    # Deny wildcard destructive actions
    contains(policy, "\"Action\": \"*\"")
    msg := sprintf("DENY: Wildcard action in IAM policy %s", [resource.metadata.name])
}

deny[msg] {
    resource := input.resource
    resource.kind == "IAMPolicy"
    policy := resource.spec.forProvider.document
    
    # Deny s3:* on all resources
    contains(policy, "\"Action\": \"s3:*\"")
    contains(policy, "\"Resource\": \"*\"")
    msg := sprintf("DENY: s3:* on all resources in policy %s", [resource.metadata.name])
}

deny[msg] {
    resource := input.resource
    resource.kind == "IAMPolicy"
    policy := resource.spec.forProvider.document
    
    # Deny iam:PassRole with wildcards
    contains(policy, "\"Action\": \"iam:PassRole\"")
    contains(policy, "\"Resource\": \"*\"")
    msg := sprintf("DENY: iam:PassRole with wildcard resources in %s", [resource.metadata.name])
}
```

**Run**:
```bash
# On rendered Crossplane output
crossplane render xr.yaml composition.yaml | \
  conftest test -p policy.rego -
```

**Validates**:
- No wildcard destructive actions
- IAM roles follow least-privilege principle
- No privilege escalation paths

**Catches**: Over-permissioned IAM roles before they're deployed

---

### 5. ExternalSecret & ESO Validation

```bash
# For each ExternalSecret in PR
kubeconform -strict external-secret.yaml

# Verify target.name and data[].secretKey match expected environment vars
if ! grep -q "target.name: agent-secrets" external-secret.yaml; then
  echo "ERROR: ExternalSecret target name mismatch"
  exit 1
fi

# Check SecretStore exists
if ! kubectl get secretstore -o name | grep -q $(grep -A2 "secretStoreRef:" external-secret.yaml | tail -1); then
  echo "ERROR: Referenced SecretStore not found"
  exit 1
fi
```

**Validates**:
- ExternalSecret CRD syntax correct
- Target Secret name matches ConfigMap references
- Referenced SecretStore exists
- data[].secretKey match application environment variables

**Catches**: Broken ESO mappings, missing secrets, env var mismatches

---

### 6. Secret Hardcoding Scan (Gitleaks)

```bash
gitleaks detect --source . --verbose \
  --report-path gitleaks-report.json
```

**Validates**:
- No GitHub tokens, API keys, AWS credentials in YAML
- No hardcoded secrets bypassing ESO

**Catches**: Accidental secret commits before they reach the cluster

---

## Complete CI Workflow

```yaml
name: FFT Deterministic Gate
on: pull_request

jobs:
  fft-validation:
    runs-on: ubuntu-latest
    steps:
      # Install tools
      - uses: actions/checkout@v4
      
      - name: Install Flux CLI
        run: curl -s https://fluxcd.io/install.sh | sudo bash
      
      - name: Install Crossplane CLI
        run: curl -sL https://releases.crossplane.io/crossplane-cli/v1.x/crank-linux-amd64 -o crank && chmod +x crank && sudo mv crank /usr/local/bin/
      
      - name: Install kubeconform
        run: |
          wget https://github.com/yannh/kubeconform/releases/latest/download/kubeconform-linux-amd64.tar.gz
          tar xf kubeconform-linux-amd64.tar.gz && sudo mv kubeconform /usr/local/bin/
      
      - name: Install conftest
        run: curl -L https://github.com/open-policy-agent/conftest/releases/latest/download/conftest_linux_x86_64 -o conftest && chmod +x conftest && sudo mv conftest /usr/local/bin/
      
      - name: Install gitleaks
        run: curl -sL https://github.com/gitleaks/gitleaks/releases/latest/download/gitleaks_linux_x86_64 -o gitleaks && chmod +x gitleaks && sudo mv gitleaks /usr/local/bin/
      
      # Run checks
      - name: 1. Flux Build & Kubeconform
        run: |
          for kustomize_path in $(find . -name kustomization.yaml | xargs dirname); do
            echo "Validating: $kustomize_path"
            flux build kustomization "$kustomize_path" | kubeconform -strict -output json
          done
      
      - name: 2. Crossplane Render & Validate
        run: |
          for xr_path in $(find . -name "*.xr.yaml"); do
            echo "Rendering: $xr_path"
            crossplane render "$xr_path" crossplane/aws/hub/control-plane/base/compositions/composition-*.yaml | \
              crossplane beta validate crossplane/aws/provider-aws-schemas/ -
          done
      
      - name: 3. OIDC ServiceAccount Validation
        run: |
          for sa_path in $(find . -name "*.yaml" -exec grep -l "kind: ServiceAccount" {} \;); do
            echo "Checking OIDC in: $sa_path"
            grep -q "eks.amazonaws.com/role-arn" "$sa_path" || exit 1
          done
      
      - name: 4. IAM Least-Privilege (OPA/Rego)
        run: |
          for xr_path in $(find . -name "*.xr.yaml"); do
            echo "Validating IAM policies: $xr_path"
            crossplane render "$xr_path" crossplane/aws/hub/control-plane/base/compositions/composition-*.yaml | \
              conftest test -p policy.rego -
          done
      
      - name: 5. ExternalSecret Validation
        run: |
          for es_path in $(find . -name "*external-secret*.yaml"); do
            echo "Validating ExternalSecret: $es_path"
            kubeconform -strict "$es_path"
          done
      
      - name: 6. Gitleaks Secret Scan
        run: gitleaks detect --source . --verbose
      
      - name: Report Status
        if: success()
        run: echo "✅ All FFT deterministic checks passed"
      
      - name: Report Failure
        if: failure()
        run: echo "❌ FFT validation failed. Review errors above."
```

## Speed & Cost

| Check | Time | Cost |
|-------|------|------|
| Flux build + kubeconform | ~2s | $0 |
| Crossplane render + validate | ~3s | $0 |
| OIDC validation | ~1s | $0 |
| OPA/conftest IAM check | ~2s | $0 |
| ExternalSecret validation | ~1s | $0 |
| Gitleaks scan | ~2s | $0 |
| **Total** | **~11s** | **$0** |

No Lambda cold starts. No AWS API calls. No infrastructure provisioned.

## vs. Lambda-Based Approach

| Aspect | Lambda-Based FFT | Crossplane Validation |
|--------|------------------|----------------------|
| Speed | 8-15s (cold start) | ~11s (local) |
| Cost | Free tier | Free |
| What it catches | Runtime type errors, test failures | Configuration errors before deployment |
| When it fails | After PR merged | Before PR merged |
| AWS resources needed | Yes (Lambda, API Gateway) | No (local binaries only) |
| Complexity | Medium | Low |
| Best for | Type-safe code validation | Infrastructure validation |

## Recommendation

**Use both**:
1. **Crossplane validation (Issue #2)** — Fast, free, catches config errors early
2. **Lambda-based checks** — Catches type/test errors, complements Crossplane validation

Together they create a **truly deterministic gate**: infrastructure validation + code validation.

## Next: Apply to All PRs

Once implemented, configure GitHub branch protection to require both checks on **all PRs**:

```bash
gh api repos/{owner}/{repo}/branches/main/protection \
  -X PUT \
  -f required_status_checks.strict=true \
  -f required_status_checks.contexts='["Deterministic Gate (Crossplane)", "Deterministic Gate (Lambda)"]'
```

See: `FFT_APPLY_TO_ALL_PRS.md` (next)
