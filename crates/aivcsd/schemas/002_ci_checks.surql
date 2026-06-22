-- CI Execution Records
-- Stores the result of each PR's CI check run
DEFINE TABLE ci_executions SCHEMAFULL;
DEFINE FIELD id ON ci_executions TYPE string;
DEFINE FIELD execution_id ON ci_executions TYPE string;
DEFINE FIELD repository ON ci_executions TYPE string;
DEFINE FIELD pr_number ON ci_executions TYPE number;
DEFINE FIELD pr_sha ON ci_executions TYPE string;
DEFINE FIELD pr_title ON ci_executions TYPE string;
DEFINE FIELD status ON ci_executions TYPE string ENUM ['queued', 'running', 'passed', 'failed'];
DEFINE FIELD conclusion ON ci_executions TYPE string ENUM ['success', 'failure', 'neutral'];
DEFINE FIELD started_at ON ci_executions TYPE datetime;
DEFINE FIELD completed_at ON ci_executions TYPE datetime;
DEFINE FIELD checks ON ci_executions TYPE object;  -- { type_check: {status, duration_ms, error}, ... }
DEFINE FIELD agent_id ON ci_executions TYPE string;
DEFINE FIELD approval_required ON ci_executions TYPE bool DEFAULT false;
DEFINE FIELD approval_granted ON ci_executions TYPE bool DEFAULT false;
DEFINE FIELD approved_by ON ci_executions TYPE string;
DEFINE FIELD approval_reason ON ci_executions TYPE string;
DEFINE FIELD created_at ON ci_executions TYPE datetime DEFAULT time::now();
DEFINE INDEX idx_execution_id ON ci_executions COLUMNS execution_id UNIQUE;
DEFINE INDEX idx_pr_number ON ci_executions COLUMNS repository, pr_number;

-- CI Subscription (per-repo configuration)
-- Tracks which repos are subscribed to fast-free-testing
DEFINE TABLE ci_subscriptions SCHEMAFULL;
DEFINE FIELD id ON ci_subscriptions TYPE string;
DEFINE FIELD repository ON ci_subscriptions TYPE string;
DEFINE FIELD owner ON ci_subscriptions TYPE string;
DEFINE FIELD enabled ON ci_subscriptions TYPE bool DEFAULT true;
DEFINE FIELD webhook_id ON ci_subscriptions TYPE string;
DEFINE FIELD webhook_secret ON ci_subscriptions TYPE string;
DEFINE FIELD aws_stack_name ON ci_subscriptions TYPE string;
DEFINE FIELD api_endpoint ON ci_subscriptions TYPE string;
DEFINE FIELD created_at ON ci_subscriptions TYPE datetime DEFAULT time::now();
DEFINE INDEX idx_repository ON ci_subscriptions COLUMNS repository UNIQUE;

-- CI Audit Trail
-- Comprehensive audit log of all authorization and execution decisions
DEFINE TABLE ci_audit_log SCHEMAFULL;
DEFINE FIELD id ON ci_audit_log TYPE string;
DEFINE FIELD audit_id ON ci_audit_log TYPE string;
DEFINE FIELD execution_id ON ci_audit_log TYPE string;
DEFINE FIELD event_kind ON ci_audit_log TYPE string ENUM [
  'permission_check',
  'rate_limit_check',
  'check_started',
  'check_completed',
  'approval_required',
  'approval_granted',
  'approval_denied',
  'identity_expired'
];
DEFINE FIELD agent_id ON ci_audit_log TYPE string;
DEFINE FIELD agent_role ON ci_audit_log TYPE string ENUM ['reader', 'committer', 'reviewer', 'deployer', 'admin'];
DEFINE FIELD result ON ci_audit_log TYPE string ENUM ['success', 'failure'];
DEFINE FIELD reason ON ci_audit_log TYPE string;
DEFINE FIELD resource_type ON ci_audit_log TYPE string;
DEFINE FIELD resource_name ON ci_audit_log TYPE string;
DEFINE FIELD created_at ON ci_audit_log TYPE datetime DEFAULT time::now();
DEFINE INDEX idx_execution_id ON ci_audit_log COLUMNS execution_id;
DEFINE INDEX idx_agent_id ON ci_audit_log COLUMNS agent_id;

-- CI Metrics (for monitoring and analytics)
-- Aggregated metrics for dashboard and cost tracking
DEFINE TABLE ci_metrics SCHEMAFULL;
DEFINE FIELD id ON ci_metrics TYPE string;
DEFINE FIELD repository ON ci_metrics TYPE string;
DEFINE FIELD date ON ci_metrics TYPE datetime;
DEFINE FIELD total_prs ON ci_metrics TYPE number DEFAULT 0;
DEFINE FIELD passed_prs ON ci_metrics TYPE number DEFAULT 0;
DEFINE FIELD failed_prs ON ci_metrics TYPE number DEFAULT 0;
DEFINE FIELD avg_check_duration_ms ON ci_metrics TYPE number DEFAULT 0;
DEFINE FIELD total_approvals_required ON ci_metrics TYPE number DEFAULT 0;
DEFINE FIELD total_approvals_granted ON ci_metrics TYPE number DEFAULT 0;
DEFINE FIELD created_at ON ci_metrics TYPE datetime DEFAULT time::now();
DEFINE INDEX idx_repository_date ON ci_metrics COLUMNS repository, date;

-- Seed Client 1 Subscription for Fast-Free-Testing (FFT)
UPSERT ci_subscriptions:client_1 SET
  repository = 'lornu-ai/aivcs-lornu-demo',
  owner = 'lornu-ai',
  enabled = true,
  webhook_id = 'webhook_client_1',
  webhook_secret = 'client_1_secret_key_12345',
  aws_stack_name = 'stevedores-aivcs-ci-gate',
  api_endpoint = 'https://github.com/lornu-ai/aivcs-lornu-demo';

