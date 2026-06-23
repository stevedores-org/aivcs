# GitHub Actions → Fast-Free-Testing Migration

**Target**: stevedores-org/aivcs (customer: our first "freebie" test case)  
**Status**: PR #295 backend ready; GHA migration planning phase  
**Timeline**: After PR #295 merge + AWS deployment  

## Current State

GitHub Actions CI (`.github/workflows/ci.yml`):
- **rust-checks** — 15-30 min (cargo check, clippy, tests)
- **nix-checks** — 20-45 min (Nix build matrix for checks, packages)
- **aivcs-reproducibility** — 10-20 min (custom reproducibility verification)
- **Total**: 45-90 minutes per PR
- **Cost**: GitHub Actions free tier (mostly free, minor overage)

## Target State

Fast-Free-Testing only:
- **Type checks** — 1-2 sec (cargo check, tsc, rustfmt, prettier)
- **Unit tests** — 3-8 sec (cargo test --lib, bun test)
- **Secrets scan** — 1-2 sec (pattern matching for tokens)
- **Config lint** — 1-2 sec (hardcoded values, TODOs, etc.)
- **Total**: 8-15 seconds per PR
- **Cost**: $0 (AWS free tier)
- **Speed improvement**: 6x faster (90 min → 15 sec)
- **Cost savings**: $0 free tier (was mostly free anyway, but now zero)

## Implementation Plan

### Phase 1: Keep Dual Gates (Safe Approach)

**Until FFT is proven reliable**, run BOTH:
1. Fast-Free-Testing (new, fast gate)
2. GitHub Actions (existing, comprehensive checks)

**Benefit**: Low risk, can verify FFT against GHA  
**Timeline**: 2-3 weeks

**Implementation**:
```yaml
# .github/workflows/ci.yml (keep as-is for now)
# + New endpoint checks FFT status

# OR: Add new workflow that just waits for FFT webhook status check
name: ci-status-check
on:
  pull_request:
  pull_request_target:
    
jobs:
  wait-for-fft:
    runs-on: ubuntu-latest
    steps:
      - name: Check fast-free-testing status
        run: |
          # Poll GitHub status checks for "Deterministic Gate"
          # Fail if not present or status is "failure"
```

### Phase 2: Replace GHA with FFT (Confident Approach)

**After 2-3 weeks of dual gates**, sunset GHA:

**Option A: Disable .github/workflows/ci.yml**
```bash
# Move to archived folder so history is preserved
mv .github/workflows/ci.yml .github/workflows.archived/ci.yml.disabled-2026-06-29

# OR: Empty the workflow
# ci.yml becomes:
# name: ci-deprecated
# on:
#   pull_request:
# jobs:
#   deprecated:
#     runs-on: ubuntu-latest
#     steps:
#       - run: |
#           echo "GitHub Actions CI replaced by fast-free-testing"
#           echo "See: https://github.com/stevedores-org/aivcs/pull/295"
```

**Option B: Simple stub workflow**
```yaml
# .github/workflows/ci.yml (after disabling old checks)
name: ci
on:
  pull_request:
    types: [opened, synchronize, reopened]

jobs:
  fft-status:
    runs-on: ubuntu-latest
    steps:
      - name: Fast-Free-Testing Gate
        run: |
          echo "✅ CI checks handled by fast-free-testing"
          echo "See: /api/v1/ci/checks/{pr_number}"
```

## Action Items

### Immediate (With PR #295 Merge)

- [ ] Merge PR #295 — Backend integration ready
- [ ] Document in PR: "FFT replaces GHA CI for this repo"
- [ ] Deploy AWS stack: `sam deploy --stack-name stevedores-aivcs-ci-gate`
- [ ] Add GitHub webhook to stevedores-org/aivcs
- [ ] Test with first PR to verify FFT fires

### Week 1 (After FFT proven working)

- [ ] Run 5-10 test PRs with both GHA and FFT running
- [ ] Compare results: verify FFT is catching same issues as GHA
- [ ] Document any false positives/negatives
- [ ] Get approval to sunset GHA

### Week 2 (Sunset Decision)

**If FFT proven reliable**:
- [ ] Archive/disable old `ci.yml` workflow
- [ ] Create minimal stub workflow (optional)
- [ ] Update branch protection rules to only require "Deterministic Gate"
- [ ] Announce GHA CI sunset in team docs

**If issues found**:
- [ ] Fix FFT checks
- [ ] Run more comparison tests
- [ ] Delay sunset by 1 week
- [ ] Or run dual gates permanently

## Risk Mitigation

### What if FFT breaks?

1. **Quick rollback**:
   ```bash
   # Re-enable old CI workflow
   git restore .github/workflows/ci.yml
   git push origin main
   ```

2. **Notification**: GitHub status check will fail → obvious to developers

3. **Recovery time**: <5 minutes to restore GitHub Actions

### What if FFT misses a bug that GHA catches?

1. **During dual-gate phase**: Both checks run, GHA catches it
2. **After sunset**: Would need manual re-run of old check
3. **Long-term**: Improve FFT checks based on feedback

## Branch Protection Changes

### Current (With GHA)
```
Required status checks:
- nix-checks-summary
- rust-checks
- aivcs-reproducibility
```

### After FFT Sunset
```
Required status checks:
- Deterministic Gate (from fast-free-testing)
```

**Change command**:
```bash
gh api repos/stevedores-org/aivcs/branches/main/protection \
  -X PUT \
  -f required_status_checks.strict=true \
  -f required_status_checks.contexts='["Deterministic Gate"]'
```

## Monitoring & Metrics

### Week 1: Comparison Metrics
- Count PRs checked by both systems
- Compare results (all pass both, some fail one but not other)
- Document any discrepancies

### Week 2+: FFT Only Metrics
- Lambda invocation count (should be ~5 per PR × 50 PRs = 250/month)
- Average check duration (target: <15 sec)
- Error rate (target: <0.1%)
- False positive rate (issues FFT flags that aren't real)
- False negative rate (bugs FFT misses that would be obvious)

## What FFT Doesn't Check (That GHA Does)

Current GHA checks FFT **doesn't** do:
- ❌ Nix flake builds (nix-checks job)
- ❌ Custom reproducibility verification (aivcs-cli pr verify-reproducibility)
- ❌ Package builds (aivcs, aivcsd nix packages)

**Decisions**:
1. **Nix/Package builds** — Move to separate optional workflow (runs after merge on main)
2. **Reproducibility** — Implement as optional post-merge check, not blocking
3. **Type safety & tests** — ✅ Fully covered by FFT

## Documentation Updates

When FFT sunset is complete:

1. **README.md**: Update CI section
   ```markdown
   ## CI/CD
   
   This project uses [fast-free-testing](https://github.com/lornu-ai/fast-free-testing)
   for deterministic, zero-cost CI checks on every PR.
   
   Checks: type safety, unit tests, secrets scan, config lint
   Time: ~8-15 seconds
   Cost: $0 (AWS free tier)
   ```

2. **CONTRIBUTING.md**: Remove GHA references
3. **PR template**: Reference FFT status check
4. **.github/workflows/**: Archive old workflows

## Long-Term Vision

After stevedores-org/aivcs sunset (proof of concept):

1. **Scale to other repos**: Apply same pattern to other Lornu projects
2. **Optional nix-checks**: Move package/build checks to separate workflow
3. **Reproducibility**: Separate optional post-merge verification
4. **Agent-IDC**: Integrate full identity governance (rate limits, approval flows)
5. **Multi-customer**: Deploy separate stacks for each customer/org

## Timeline

| Date | Phase | Status |
|------|-------|--------|
| 2026-06-22 | Phase 1: Backend integration | ✅ PR #295 ready |
| 2026-06-23 | Phase 2: AWS deployment | ⏳ After merge |
| 2026-06-23 | Phase 3: Dual gates | ⏳ After AWS deploy |
| 2026-06-29 | Decide: Sunset GHA? | ⏳ After 5-6 test PRs |
| 2026-07-06 | Phase 4: Sunset GHA | ⏳ If proven reliable |

## Approval Checklist

Before sunsetting GHA, verify:
- [ ] FFT has processed 10+ PRs successfully
- [ ] FFT caught all issues that GHA would catch (types, tests, secrets)
- [ ] No false positives or negatives observed
- [ ] Team agrees it's safe
- [ ] Rollback tested and documented
- [ ] Branch protection rules updated
- [ ] Documentation updated

---

**Current Status**: Awaiting PR #295 merge  
**Owner**: principal@lornu.ai  
**Customer**: stevedores-org/aivcs (first free test case)  
**Target Completion**: 2 weeks (with testing phase)
