---
name: mcp-auth
description: >-
  Integrates and reviews Zero-Trust MCP identity for AIVCS agents — JWT bootstrap,
  gateway headers, scope/risk rules, payload digests, human approvals, and revocation.
  Use when working on aivcs-auth, aivcs-mcp-gateway, MCP tool security, HITL approvals,
  or docs/mcp-auth-guide.md.
---

# MCP Zero-Trust Auth Skill

## Docs

- **Canonical:** [docs/mcp-auth-guide.md](../../docs/mcp-auth-guide.md)
- **Hub summary:** Lornu `docs/MCP_ZERO_TRUST_AUTH.md`

## Required Gateway Headers

`Authorization: Bearer <jwt>`, `MCP-Protocol-Version: 2025-06-18`, `Mcp-Session-Id: <id>`

## Local Commands

```bash
cargo run -p aivcs-auth
cargo run -p aivcs-mcp-gateway
cargo test -p aivcs-mcp-gateway
cargo test -p aivcs-auth
```

## Payload Digest

```text
payload_digest = hex(SHA256(tool_name || serde_json::to_string(arguments)))
```

## HITL Loop (destructive tools)

Call tool → `approval_required` → register approval with digest → retry same call.

## Before Merging Auth Changes

Update six governance files and `docs/mcp-auth-guide.md`. Note dev gaps: mock bootstrap, unauthenticated approval/revocation endpoints.
