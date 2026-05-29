# Sovereign Intelligence Standards — aivcs

## 1. Architectural Integrity
- State snapshots must remain immutable and content-addressed.
- The A2A protocol is the canonical interface for remote state management.
- All core crates must maintain strict trait boundaries.

## 2. Distributed Safety
- Timeouts must be enforced for all remote network operations.
- State merging logic must be deterministic and verifiable.
- Repository names and run IDs must be sanitized to prevent injection.

## 3. Operational Excellence
- `cargo test --workspace` is the definitive source of truth for build health.
- Documentation must adhere to the 7-File Rule.
- Every release must include updated Swagger/OpenAPI specifications.
