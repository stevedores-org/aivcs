# Propel Phase 1 Specification & oci-dockworker-build Integration

**Status:** Phase 1.5 Complete — Ready for Phase 1 Implementation

**Date:** 2026-06-23

---

## Overview

This document tracks the Propel Phase 1 specification and its integration with `oci-dockworker-build`, the centralized OCI image builder for lornu-ai.

**Goal:** Replace GitHub Actions with Propel (NixOS-based CI/CD) for OCI image builds and general CI checks.

---

## Phase 1.5 Deliverables (COMPLETE)

### Configuration & Build Matrix

**File:** `lornu-ai/oci-dockworker-build/.propel/config.toml`

Defines all 14 OCI images to be built via Propel:

**Local images (oci-dockworker-build):**
- zen, sre-agent-rs, yaml-optimizer, zero-copy-connector, lornu-gateway, lornu-mcp-hub-rs

**Satellite images (external repos):**
- brains, ciso-agent, skills-registry, data-fabric, due-op-frontend, due-op-backend, dashboard, fft-dashboard, dockworker-ai-frontend, dockworker-ai-backend

Each entry includes:
- Name, context path, Nix flake output attribute
- Repository reference (for satellites)
- Build system (Nix only for Phase 1)

**Registry strategy:**
- Primary (Phase 1): GAR (Google Artifact Registry) — auth via Workload Identity
- Secondary (Phase 2+): GHCR (GitHub Container Registry) — deferred for secrets integration

**Build parameters:**
- System: x86_64-linux
- Parallelism: 8 concurrent builds
- Timeout: 45 minutes
- Cache: Attic (https://nix-cache.lornu.ai)

### Implementation Specification

**File:** `lornu-ai/oci-dockworker-build/docs/PHASE1_IMPLEMENTATION.md`

Complete technical specification for Phase 1:

#### 1.1: GitHub Webhook Receiver
- Validates webhook signature (HMAC-SHA256)
- Parses push/PR events
- Creates Run records (in-memory for Phase 1)
- Org/repo allowlist filtering

#### 1.2: K8s Nix Build Job
- Job template: `nix build .#<nix_attr>`
- Pod spec: nixos/nix image, Workload Identity auth
- Resource requests: 4-8 CPU, 8-16Gi memory
- Nix cache mount (tmpfs for Phase 1, PVC for Phase 2+)

#### 1.3: Registry Strategy
- Phase 1: Build outputs tested locally, no push
- Phase 1.2: Push to GAR via skopeo (Workload Identity auth)
- Phase 2: Multi-registry push (GAR + GHCR) with aivcs-secrets

#### 1.4: Logs Capture & Streaming
- Stream K8s Job logs via `kubectl logs`
- API endpoint: `GET /v1/runs/{run_id}/logs`
- Storage: in-memory (Phase 1) → S3 (Phase 2+)

#### 1.5: GitHub Checks API Publishing
- Check-run lifecycle: queued → in_progress → completed
- GitHub App authentication (App ID 2665041)
- Status updates every 10 seconds
- Includes image digest on success, error logs on failure

### Validation & Deployment

**Validation script:** `lornu-ai/oci-dockworker-build/scripts/validate-nix-outputs.sh`

Pre-flight checks for all 14 images:
```bash
./scripts/validate-nix-outputs.sh          # Check all
./scripts/validate-nix-outputs.sh --local  # Local only
./scripts/validate-nix-outputs.sh --satellite  # Satellites only
```

**Kustomize manifests:** `lornu-ai/oci-dockworker-build/deploy/base/`

- `namespace.yaml` — propel namespace
- `rbac.yaml` — service accounts + roles (propel-api, propel-runner)
- `webhook-secret.yaml` — GitHub webhook secret (injected via overlay)
- `configmap.yaml` — propel-config.toml + allowed orgs/repos
- `kustomization.yaml` — base manifest aggregation

Ready for overlays: `deploy/overlays/{dev,staging,prod}/`

### CI/CD Pipeline

**File:** `lornu-ai/oci-dockworker-build/.github/workflows/ci.yml`

Validation checks for every PR:
- **nix-check:** `nix flake check` (syntax validation)
- **validate-config:** TOML schema + build entry fields
- **validate-kustomize:** Manifest dry-run via kubectl
- **shellcheck:** Shell script linting
- **format:** Prettier checks (YAML, markdown, shell)

### Removed GitHub Actions

**Archived (will be replaced by Propel):**
- `.github/workflows/build-oci-images.yml` — OCI image build (startup_failure → webhook)
- `.github/workflows/oci-build.yaml` — Reusable OCI build workflow

These workflows will be re-enabled if Propel Phase 1 is delayed beyond 2 weeks.

---

## Phase 1 Implementation Plan (NEXT)

**Timeline:** 1–2 weeks

**Deliverables:**
1. propel-api webhook receiver (propel crate)
2. K8s Job scheduler for Nix builds
3. Logs streaming integration
4. GitHub Checks API publisher
5. Kustomize deployment to GKE dev cluster
6. FFT integration test

**Success criteria:**
- First webhook received and logged
- K8s Job created and completed (< 10 min)
- Job logs retrieved via API
- Check-run published to GitHub PR
- All 14 images validated and built once

---

## Phase 2 Implementation Plan (BLOCKED ON PHASE 1)

**Timeline:** 1 week (after Phase 1)

**Changes:**
- Change detection logic (only rebuild affected images)
- aivcs-secrets integration (scoped secret reads)
- ExternalSecrets Operator (static pod-level secrets)
- Multi-registry push (GAR + GHCR)
- PostgreSQL for durable state (replaces in-memory)

---

## Phase 3: Full Migration (BLOCKED ON PHASE 2)

**Timeline:** 1 week (after Phase 2)

**Changes:**
- Staging deployment validation
- GHA workflow archival
- Production cutover
- FFT + oci-dockworker-build fully on Propel

---

## Integration Points

### With propel repo (lornu-ai/propel)

- `propel-api` crate implements webhook receiver + K8s scheduler per Phase 1 spec
- `propel-scheduler` crate dispatches Job to K8s
- `propel-github` crate publishes check-runs
- Kustomize base in `propel/deploy/base/` (reusable across repos)

### With oci-dockworker-build repo

- `.propel/config.toml` defines build matrix
- Validation script ensures Nix outputs exist
- Kustomize manifests in `deploy/base/` override secrets/config
- GHA workflows archived; no more OCI image builds on GHA
- CI checks run on every PR (validation only, not builds)

### With fast-free-testing repo (Phase 1.5+)

- FFT will be first real test of Propel webhook + K8s Job execution
- propel.toml will define format, typecheck, test, build checks
- Flake checks outputs will map to Propel check definitions
- GHA deploy-lambdas.yml will be archived (replaced by Propel)

### With propel-api crate (Phase 1)

Implementation checklist:
- [ ] GitHub webhook receiver: POST /v1/webhooks/github
- [ ] GitHub signature validation (HMAC-SHA256)
- [ ] Org/repo allowlist filtering
- [ ] Run creation (in-memory store)
- [ ] K8s Job creation (via scheduler)
- [ ] Logs streaming: GET /v1/runs/{run_id}/logs
- [ ] GitHub Checks API integration (post check-runs)
- [ ] Webhook secret management (from ConfigMap)

---

## Validation Checklist (Before Phase 1 Cutover)

### Pre-Flight Tasks

- [ ] Run `./scripts/validate-nix-outputs.sh` — all 14 images pass
- [ ] Verify Attic cache connectivity: `curl https://nix-cache.lornu.ai/v2/_catalog`
- [ ] Verify K8s namespace + RBAC: `kubectl get ns propel && kubectl auth can-i create jobs -n propel`
- [ ] Verify GitHub App installation: `gh api /app/installations | jq '.[] | select(.app_id == 2665041)'`

### Phase 1 Success Metrics

- ✅ First webhook received and logged
- ✅ K8s Job created for oci-dockworker-build push event
- ✅ Nix build completes (success or failure) in < 10 minutes
- ✅ Job logs appear in Propel API
- ✅ Check-run published to GitHub PR
- ✅ All 14 images validated and built at least once

---

## Known Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Webhook signature validation fails | All webhooks rejected | Unit tests + staging webhook test |
| K8s Job quota exceeded | Builds queued indefinitely | Monitor max_parallel = 8, request quota increase |
| Nix flake output paths wrong | Builds fail at eval time | Run validation script before Phase 1 |
| Attic cache unreachable | Nix builds very slow | Fallback to nixpkgs cache (slower, works) |
| GitHub App auth fails | Checks don't post | Verify token exchange, test with gh CLI |
| GHA workflows removed too early | Production breaks | Keep archived copies; re-enable if Phase 1 delayed > 2 weeks |

---

## References

- **oci-dockworker-build PR #16:** Build matrix configuration (MERGED)
- **oci-dockworker-build PR #17:** Phase 1.5 specification + K8s manifests (OPEN)
- **Propel README:** https://github.com/lornu-ai/propel/blob/main/README.md
- **Propel PHASES.md:** https://github.com/lornu-ai/propel/blob/main/PHASES.md
- **GitHub App (lornu-ai):** App ID 2665041, already installed on org

---

## Timeline & Ownership

| Phase | Timeline | Owner | Status |
|-------|----------|-------|--------|
| **1.5** (Config + Spec) | Done | Claude Code | ✅ Complete |
| **1** (Webhook + K8s) | 1-2 weeks | propel team | ⏳ Ready to start |
| **2** (Change detection + Secrets) | 1 week | propel team | Blocked on Phase 1 |
| **3** (Staging + Cutover) | 1 week | propel team | Blocked on Phase 2 |

---

**Next action:** propel-api implementation begins immediately per Phase 1 spec.
