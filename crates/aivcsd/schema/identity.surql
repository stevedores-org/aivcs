-- AIVCS Identity and State Schema

-- 1. Agents (Synthetic Principals)
DEFINE TABLE agent SCHEMALESS PERMISSIONS FULL;
DEFINE FIELD name ON agent TYPE string;
DEFINE FIELD public_key ON agent TYPE string;
DEFINE FIELD trust_score ON agent TYPE float DEFAULT 0.0;
DEFINE FIELD created_at ON agent TYPE datetime DEFAULT time::now();

-- 2. Hives (Organizations / Swarm Contexts)
DEFINE TABLE hive SCHEMALESS PERMISSIONS FULL;
DEFINE FIELD name ON hive TYPE string;
DEFINE FIELD created_at ON hive TYPE datetime DEFAULT time::now();

-- 3. Relationships
DEFINE TABLE belongs_to SCHEMALESS PERMISSIONS FULL;
DEFINE FIELD in ON belongs_to TYPE record<agent>;
DEFINE FIELD out ON belongs_to TYPE record<hive>;
DEFINE FIELD role ON belongs_to TYPE string DEFAULT 'worker';

-- 4. Human Supervisors (HITL Overrides)
DEFINE TABLE human_supervisor SCHEMALESS PERMISSIONS FULL;
DEFINE FIELD oidc_subject ON human_supervisor TYPE string;
DEFINE FIELD email ON human_supervisor TYPE string;

DEFINE TABLE supervises SCHEMALESS PERMISSIONS FULL;
DEFINE FIELD in ON supervises TYPE record<human_supervisor>;
DEFINE FIELD out ON supervises TYPE record<hive>;

-- 5. Semantic Commits (State Tracking)
DEFINE TABLE commit SCHEMALESS PERMISSIONS FULL;
DEFINE FIELD message ON commit TYPE string;
DEFINE FIELD blob_hash ON commit TYPE string; -- Points to S3 CAS
DEFINE FIELD author ON commit TYPE record<agent>;
DEFINE FIELD hive ON commit TYPE record<hive>;
DEFINE FIELD created_at ON commit TYPE datetime DEFAULT time::now();

-- Graph lineage for commits (Parent -> Child)
DEFINE TABLE parent_of SCHEMALESS PERMISSIONS FULL;
DEFINE FIELD in ON parent_of TYPE record<commit>;
DEFINE FIELD out ON parent_of TYPE record<commit>;
