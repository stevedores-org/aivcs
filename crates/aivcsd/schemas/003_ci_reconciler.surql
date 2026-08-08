-- Durable idempotency records for outbound GitHub CI reconciliation.
-- The record key is repository#pull_request#head_sha.
DEFINE TABLE ci_reconciler_events SCHEMAFULL;
DEFINE FIELD event_id ON ci_reconciler_events TYPE string;
DEFINE FIELD repository ON ci_reconciler_events TYPE string;
DEFINE FIELD pr_number ON ci_reconciler_events TYPE number;
DEFINE FIELD sha ON ci_reconciler_events TYPE string;
DEFINE FIELD created_at ON ci_reconciler_events TYPE datetime;
DEFINE INDEX idx_ci_reconciler_event_id ON ci_reconciler_events COLUMNS event_id UNIQUE;
