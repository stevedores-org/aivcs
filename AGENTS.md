# aivcs — Agent Capabilities

## LedgerAgent

The **LibrarianAgent** (often referred to as LedgerAgent in context) is the core service. It maintains the content-addressed run ledger and handles state capture requests.

### Capabilities

1. **Deterministic State Capture**
   - Creates immutable snapshots of agent workflow state.
   - Content-addressed storage ensures data integrity.
   - Supports branching and merging of state for parallel agent sessions.

2. **Run Ledger Management**
   - Appends events to the execution log (A2A protocol).
   - Provides replay capabilities for debugging and evaluation.
   - Supports semantic merging of divergent agent decisions.

3. **Audit & Compliance**
   - Provides a verifiable audit trail for every agent action.
   - Integrates with the Lornu platform for org-wide visibility.

## A2A Protocol

| Direction | Event | Purpose |
|-----------|-------|---------|
| Inbound | `CHECKPOINT_SAVED` | Record a new state snapshot |
| Inbound | `CODE_COMMITTED` | Notify of a code change in a task branch |
| Outbound | `RUN_SUMMARY` | Provide a summary of a completed execution |
