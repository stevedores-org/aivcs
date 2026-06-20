-- Define the Hive (Swarm/Team) namespace for agents
DEFINE TABLE hive SCHEMAFULL;
DEFINE FIELD name ON hive TYPE string;
DEFINE FIELD description ON hive TYPE option<string>;
DEFINE FIELD created_at ON hive TYPE datetime DEFAULT time::now() READONLY;
DEFINE FIELD status ON hive TYPE string ASSERT $value INSIDE ['active', 'archived'] DEFAULT 'active';

-- Define the Synthetic Principal (Agent Identity)
DEFINE TABLE agent SCHEMAFULL;
DEFINE FIELD name ON agent TYPE string;
-- Cryptographic identity for signing commits and state mutations
DEFINE FIELD public_key ON agent TYPE string;
-- Link to the Hive this agent belongs to
DEFINE FIELD hive_id ON agent TYPE record<hive>;
-- Agent role/classification (e.g. 'researcher', 'reviewer', 'coder')
DEFINE FIELD role ON agent TYPE string;
-- Array of permitted actions or scopes
DEFINE FIELD capabilities ON agent TYPE array<string> DEFAULT [];
DEFINE FIELD status ON agent TYPE string ASSERT $value INSIDE ['active', 'suspended', 'revoked'] DEFAULT 'active';
DEFINE FIELD created_at ON agent TYPE datetime DEFAULT time::now() READONLY;

-- Define indexes for fast lookup
DEFINE INDEX idx_agent_pubkey ON agent COLUMNS public_key UNIQUE;
DEFINE INDEX idx_agent_hive ON agent COLUMNS hive_id;

-- Define the Commit (State Mutation) made by a Synthetic Principal
DEFINE TABLE commit SCHEMAFULL;
-- The agent that authored this commit
DEFINE FIELD agent_id ON commit TYPE record<agent>;
-- The hive context under which this commit was made
DEFINE FIELD hive_id ON commit TYPE record<hive>;
-- Human-readable message describing the agent's action/state change
DEFINE FIELD message ON commit TYPE string;
-- Cryptographic hash of the state blob
DEFINE FIELD blob_hash ON commit TYPE string;
-- Timestamp of the commit
DEFINE FIELD timestamp ON commit TYPE datetime DEFAULT time::now() READONLY;
-- Cryptographic signature over the payload by the agent's private key
DEFINE FIELD signature ON commit TYPE string;

-- Define index for commit tracking
DEFINE INDEX idx_commit_agent ON commit COLUMNS agent_id;
DEFINE INDEX idx_commit_hive ON commit COLUMNS hive_id;
DEFINE INDEX idx_commit_blob ON commit COLUMNS blob_hash UNIQUE;
