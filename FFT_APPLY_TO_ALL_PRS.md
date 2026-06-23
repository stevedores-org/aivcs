# FFT: Apply to All PRs (including fast-free-testing's own)

**Goal**: Run Crossplane validation + Lambda checks on **all PRs** across all repos  
**Repos included**:
- `lornu-ai/fast-free-testing` (its own PRs)
- `stevedores-org/aivcs` (Customer 1)
- `lornu-ai/aivcs-lornu-demo` (Customer 2)
- Any future repo that needs FFT checks

## Architecture

```
GitHub PR (any repo)
    ↓
Webhook to fast-free-testing API Gateway
    ↓
┌─────────────────────────────────────────┐
│ Orchestrator Lambda                     │
├─────────────────────┬───────────────────┤
│ Check 1: Crossplane │ Check 2: Lambda   │
│ Validation (Issue#2)│ Type/Tests        │
│                     │                   │
│ flux build +        │ cargo check       │
│ kubeconform +       │ cargo test        │
│ crossplane render + │ tsc --noEmit      │
│ conftest (OPA) +    │ prettier --check  │
│ gitleaks            │                   │
└─────────────────────┴───────────────────┘
         ↓
GitHub Status Check: "Deterministic Gate"
```

## Configuration

### 1. For each repo, add GitHub webhook

```bash
# For fast-free-testing's own PRs
gh repo edit --add-webhook \
  --url "https://xxxxx.execute-api.us-east-1.amazonaws.com/prod/github" \
  --events pull_request \
  --secret "$(openssl rand -hex 32)" \
  lornu-ai/fast-free-testing

# For customer repos
gh repo edit --add-webhook \
  --url "https://xxxxx.execute-api.us-east-1.amazonaws.com/prod/github" \
  --events pull_request \
  --secret "$(openssl rand -hex 32)" \
  stevedores-org/aivcs

gh repo edit --add-webhook \
  --url "https://xxxxx.execute-api.us-east-1.amazonaws.com/prod/github" \
  --events pull_request \
  --secret "$(openssl rand -hex 32)" \
  lornu-ai/aivcs-lornu-demo
```

### 2. Enable branch protection on all repos

```bash
# For each repo
for repo in "lornu-ai/fast-free-testing" "stevedores-org/aivcs" "lornu-ai/aivcs-lornu-demo"; do
  echo "Protecting $repo/main..."
  
  gh api "repos/$repo/branches/main/protection" \
    -X PUT \
    -f required_status_checks.strict=true \
    -f required_status_checks.contexts='["Deterministic Gate"]' \
    -f required_pull_request_reviews.required_approving_review_count=1 \
    -f allow_force_pushes=false \
    -f allow_deletions=false
done
```

### 3. Subscribe repos via API

```bash
# For each repo
for repo in "lornu-ai/fast-free-testing" "stevedores-org/aivcs" "lornu-ai/aivcs-lornu-demo"; do
  curl -X POST http://aivcsd:8080/api/v1/ci/subscribe/$repo \
    -H "Content-Type: application/json" \
    -d '{
      "aws_deployment_stack": "fft-global-gate",
      "api_endpoint": "https://xxxxx.execute-api.us-east-1.amazonaws.com/prod/github"
    }'
done
```

## Orchestrator Lambda Enhancement

The Orchestrator (currently in `fast-free-testing/lambda/orchestrator/index.ts`) needs to:

1. **Route by repository**:
   ```typescript
   const { repository } = payload;
   
   // Route to appropriate checks based on repo
   if (repository.includes('fast-free-testing')) {
     // FFT's own validation (issue #2 checks + lambda checks)
     // Crossplane validation is critical for FFT itself
   } else {
     // Customer repos (lambda checks + optional crossplane)
   }
   ```

2. **For fast-free-testing PRs** (issue #2 checks):
   ```typescript
   // Run Crossplane/Flux validation
   await invokeCheckFunction('check-crossplane-validation', {
     repository: payload.repository.full_name,
     pr_number: payload.pull_request.number,
     sha: payload.pull_request.head.sha,
   });
   ```

3. **For all repos** (standard checks):
   ```typescript
   // Parallel execution
   await Promise.all([
     invokeCheckFunction('check-types', payload),
     invokeCheckFunction('check-unit-tests', payload),
     invokeCheckFunction('check-secrets', payload),
     invokeCheckFunction('check-config', payload),
   ]);
   ```

## Lambda: New Check Function (Crossplane Validation)

Create: `lambda/check-crossplane-validation/index.ts`

```typescript
import { execSync } from 'child_process';

interface CheckResult {
  status: 'passed' | 'failed';
  checks: Record<string, { status: string; duration_ms: number; error?: string }>;
}

exports.handler = async (event: any): Promise<CheckResult> => {
  const startTime = Date.now();
  const checks: Record<string, any> = {};

  try {
    // 1. Flux build + kubeconform
    checks.flux_kubeconform = await runFluxValidation();

    // 2. Crossplane render + validate
    checks.crossplane_validate = await runCrossplaneValidation();

    // 3. OIDC ServiceAccount check
    checks.oidc_validation = await runOIDCValidation();

    // 4. IAM least-privilege (OPA/Rego)
    checks.iam_opa = await runOPAValidation();

    // 5. ExternalSecret validation
    checks.external_secret = await runExternalSecretValidation();

    // 6. Gitleaks secret scan
    checks.gitleaks_scan = await runGitleaksValidation();

    const failed = Object.entries(checks).some(([_, check]) => check.status === 'failed');

    return {
      status: failed ? 'failed' : 'passed',
      checks,
    };
  } catch (error) {
    return {
      status: 'failed',
      checks: {
        error: {
          status: 'failed',
          duration_ms: Date.now() - startTime,
          error: String(error),
        },
      },
    };
  }
};

async function runFluxValidation(): Promise<any> {
  const start = Date.now();
  try {
    execSync('flux build kustomization . | kubeconform -strict');
    return { status: 'passed', duration_ms: Date.now() - start };
  } catch (error) {
    return { status: 'failed', duration_ms: Date.now() - start, error: String(error) };
  }
}

async function runCrossplaneValidation(): Promise<any> {
  const start = Date.now();
  try {
    execSync('crossplane render xr.yaml composition.yaml | crossplane beta validate provider-aws-schemas/');
    return { status: 'passed', duration_ms: Date.now() - start };
  } catch (error) {
    return { status: 'failed', duration_ms: Date.now() - start, error: String(error) };
  }
}

// ... similar for other checks
```

## Testing the Gate

### Test on fast-free-testing itself

```bash
cd ~/engineering/code/fast-free-testing

# Create test PR
git checkout -b test/fft-gate
echo "# Test FFT Gate" >> README.md
git commit -am "test: verify FFT runs on its own PRs"
git push -u origin test/fft-gate

# Open PR and check:
# 1. "Deterministic Gate" status check appears
# 2. Crossplane validation runs (issue #2 checks)
# 3. Lambda type/test checks run
# 4. Both pass/fail independently in status
```

### Test on customer repo

```bash
cd ~/engineering/code/aivcs

git checkout -b test/fft-gate-customer
echo "# Test FFT Customer Gate" >> README.md
git commit -am "test: verify FFT runs on customer PRs"
git push -u origin test/fft-gate-customer

# Expected: Only Lambda checks run (no Crossplane checks)
# because this is not a Crossplane-heavy repo
```

## Rollout Plan

1. **Week 1**: Implement Crossplane validation (issue #2)
   - [ ] Create check-crossplane-validation Lambda
   - [ ] Wire into Orchestrator
   - [ ] Test on fast-free-testing PRs

2. **Week 2**: Enable on customer repos
   - [ ] Add webhooks to stevedores-org/aivcs
   - [ ] Add webhooks to lornu-ai/aivcs-lornu-demo
   - [ ] Enable branch protection

3. **Week 3**: Scale to all repos
   - [ ] Define list of repos that need FFT
   - [ ] Bulk-add webhooks via automation
   - [ ] Monitor success rate

## Monitoring

Track check results via SurrealDB:

```sql
-- All checks (all repos)
SELECT repository, status, COUNT() as count 
FROM ci_executions 
GROUP BY repository, status

-- Crossplane validation failures
SELECT repository, pr_number, checks.crossplane_validate.error 
FROM ci_executions 
WHERE checks.crossplane_validate.status = 'failed'
ORDER BY created_at DESC

-- Success rate trend
SELECT DATE(created_at) as date, status, COUNT() as count 
FROM ci_executions 
GROUP BY DATE(created_at), status 
ORDER BY date DESC
```

## Success Criteria

- ✅ FFT checks run on all PRs in all repos
- ✅ Crossplane validation catches config errors early
- ✅ Lambda checks catch type/test errors
- ✅ <15 second total gate time
- ✅ $0 cost (AWS free tier)
- ✅ 100% deterministic (no randomness, filesystem isolated)
