# EPIC5: Memory, Context, and Retrieval — Implementation Plan

## Goal
Improve agent decision quality via durable memory and retrieval over prior runs and artifacts.

## Features & Deliverables

### 1. Indexed Retrieval over Snapshots, Traces, and Diffs
**Goal:** Enable semantic search over historical execution data

#### 1.1 Memory Index Types
```rust
pub enum IndexedArtifact {
    Snapshot { commit_id: String, state_hash: String },
    Trace { run_id: String, events: Vec<TraceEvent> },
    Diff { from_commit: String, to_commit: String, delta: StateDelta },
    Decision { commit_id: String, action: String, rationale: String },
    Failure { run_id: String, error: String, context: String },
}
```

#### 1.2 Retrieval API
```rust
pub struct MemoryRetriever {
    handle: SurrealHandle,
}

impl MemoryRetriever {
    // Semantic search over indexed content
    pub async fn search_by_embedding(
        &self,
        query_embedding: Vec<f32>,
        limit: usize,
        threshold: f32,
    ) -> Result<Vec<RetrievalResult>>;

    // Retrieve memories by key/namespace
    pub async fn get_by_key(&self, key: &str) -> Result<Vec<MemoryRecord>>;

    // Find similar historical contexts
    pub async fn find_similar_context(
        &self,
        current_state: &serde_json::Value,
        limit: usize,
    ) -> Result<Vec<ContextMatch>>;

    // Get decision history for a task
    pub async fn get_decision_history(
        &self,
        task_key: &str,
    ) -> Result<Vec<Decision>>;
}
```

---

### 2. Decision Memory for Rationale Capture
**Goal:** Record why decisions were made for better planning

#### 2.1 Decision Record Schema
```rust
#[derive(Serialize, Deserialize)]
pub struct Decision {
    pub id: String,                    // UUID
    pub commit_id: String,             // Associated commit
    pub task: String,                  // What decision was about
    pub action: String,                // What was decided
    pub rationale: String,             // Why this decision
    pub alternatives: Vec<String>,     // Other options considered
    pub confidence: f32,               // 0.0-1.0 confidence level
    pub outcome: Option<DecisionOutcome>,  // Result of decision
    pub timestamp: DateTime<Utc>,
}

pub enum DecisionOutcome {
    Success { benefit: f32, duration_ms: u64 },
    Failure { error: String, recovery_time_ms: u64 },
    Pending,
}
```

#### 2.2 Decision Capture in Run Events
- Hook into `run_started`, `run_finished` events
- Auto-extract decisions from tool calls and role handoffs
- Store with agent's explicit rationale (if provided)
- Link to relevant trace events

#### 2.3 Decision Learning
```rust
pub struct DecisionLearner {
    handle: SurrealHandle,
}

impl DecisionLearner {
    // Record a decision with outcome
    pub async fn record_decision(&self, decision: Decision) -> Result<String>;

    // Get lessons learned from similar past decisions
    pub async fn get_lessons(
        &self,
        task_key: &str,
        limit: usize,
    ) -> Result<Vec<Lesson>>;

    // Calculate decision success rate
    pub async fn get_decision_success_rate(
        &self,
        action: &str,
    ) -> Result<f32>;
}
```

---

### 3. Context Packing for Long-Horizon Tasks
**Goal:** Efficiently pack relevant historical context within token limits

#### 3.1 Context Assembly Strategy
```rust
pub struct ContextAssembler {
    handle: SurrealHandle,
    model_context_limit: usize,  // e.g., 128k tokens
    reserve_for_output: usize,   // e.g., 10k tokens
}

impl ContextAssembler {
    // Assemble context for a task, respecting token budget
    pub async fn assemble_context(
        &self,
        current_run: &Run,
        token_budget: usize,
    ) -> Result<AssembledContext>;

    // Prioritize context by relevance
    pub async fn prioritize_context(
        &self,
        artifacts: Vec<Artifact>,
        token_budget: usize,
    ) -> Result<Vec<(Artifact, u32)>>;  // artifact + token count

    // Estimate tokens for artifact
    pub fn estimate_tokens(&self, artifact: &Artifact) -> usize;
}

pub struct AssembledContext {
    pub prior_decisions: Vec<Decision>,      // Most relevant past decisions
    pub similar_traces: Vec<TraceArtifact>,  // Similar execution traces
    pub relevant_diffs: Vec<StateDelta>,     // Related state changes
    pub failure_patterns: Vec<FailurePattern>, // Common failure modes
    pub token_count: u32,
}
```

#### 3.2 Token Budgeting
- Allocate tokens: 40% prior decisions, 30% traces, 20% diffs, 10% failures
- Compress/summarize when over budget
- Prioritize by recency and relevance score

---

### 4. Provenance-Aware Memory Updates
**Goal:** Track lineage of memories and keep them in sync with source

#### 4.1 Provenance Tracking
```rust
#[derive(Serialize, Deserialize)]
pub struct MemoryProvenance {
    pub memory_id: String,
    pub source: ProvenanceSource,
    pub derived_from: Option<String>,    // Parent memory ID
    pub created_at: DateTime<Utc>,
    pub invalidated_at: Option<DateTime<Utc>>,  // When this memory became stale
}

pub enum ProvenanceSource {
    RunTrace { run_id: String, event_idx: usize },
    StateSnapshot { commit_id: String },
    UserAnnotation { user_id: String },
    MemoryDerivation { parent_id: String, derivation: String },
}
```

#### 4.2 Memory Invalidation
- When commit is overwritten, mark derived memories as stale
- When run fails, update decision memory with outcome
- When similar scenario happens again, update success rate

#### 4.3 Memory Compaction
```rust
pub struct MemoryCompactor {
    handle: SurrealHandle,
}

impl MemoryCompactor {
    // Merge similar memories
    pub async fn compact_similar_memories(
        &self,
        key_prefix: &str,
        similarity_threshold: f32,
    ) -> Result<u32>;  // count compacted

    // Archive old memories beyond retention
    pub async fn archive_old_memories(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<u32>;

    // Rebuild embeddings for memories
    pub async fn rebuild_embeddings(
        &self,
        embedding_model: &EmbeddingModel,
    ) -> Result<u32>;
}
```

---

## Acceptance Criteria

- [ ] **Relevant historical context retrievable**
  - [ ] Semantic search returns on-topic memories
  - [ ] Retrieved context improves decision making (measured by success rate)
  - [ ] Query latency < 500ms for 1000-memory database

- [ ] **Prior failures improve planning**
  - [ ] Failure patterns identifiable in memory
  - [ ] Similar failures trigger different strategy
  - [ ] Success rate improves 15%+ after first failure

- [ ] **Decision rationale queryable**
  - [ ] All major decisions have recorded rationale
  - [ ] Can retrieve decisions by task, agent, outcome
  - [ ] Decision audit trail complete and immutable

- [ ] **Context within limits**
  - [ ] Assembled context never exceeds token budget
  - [ ] No token overages in production
  - [ ] Compression doesn't lose critical info (semantic similarity > 0.95)

---

## Implementation Phases

### Phase 1: Foundation (Weeks 1-2)
- [ ] Design Decision and MemoryProvenance records
- [ ] Implement DecisionRecorder (capture from run events)
- [ ] Add decision event hooks to domain
- [ ] Write schema migrations

### Phase 2: Retrieval (Weeks 2-3)
- [ ] Implement MemoryRetriever with search APIs
- [ ] Add semantic search (cosine similarity on embeddings)
- [ ] Implement key-based retrieval
- [ ] Add decision history queries
- [ ] Performance tests (latency < 500ms)

### Phase 3: Context Assembly (Weeks 3-4)
- [ ] Implement ContextAssembler
- [ ] Token budgeting algorithm
- [ ] Relevance scoring
- [ ] Compression for over-budget contexts
- [ ] Integration with agent runtime

### Phase 4: Provenance & Compaction (Week 4-5)
- [ ] Add provenance tracking to memory writes
- [ ] Implement memory invalidation on commit changes
- [ ] Implement MemoryCompactor
- [ ] Archive old memories
- [ ] Re-embedding pipeline

### Phase 5: Testing & Integration (Week 5)
- [ ] End-to-end tests with decision learning
- [ ] Performance benchmarks
- [ ] Failure pattern detection validation
- [ ] Token budget compliance tests

---

## Files to Create

| File | Purpose |
|------|---------|
| `crates/aivcs-core/src/memory/mod.rs` | Module declarations |
| `crates/aivcs-core/src/memory/decision.rs` | Decision and DecisionLearner |
| `crates/aivcs-core/src/memory/retriever.rs` | MemoryRetriever with search |
| `crates/aivcs-core/src/memory/context.rs` | ContextAssembler |
| `crates/aivcs-core/src/memory/provenance.rs` | Provenance tracking |
| `crates/aivcs-core/src/memory/compactor.rs` | Memory compaction |
| `crates/oxidized-state/src/decisions.rs` | Decision table operations |
| `crates/aivcs-core/tests/memory_integration.rs` | Integration tests |

---

## Files to Modify

| File | Change |
|------|--------|
| `crates/oxidized-state/src/schema.rs` | Add Decision, MemoryProvenance records |
| `crates/oxidized-state/src/migrations.rs` | Migration for new tables |
| `crates/aivcs-core/src/lib.rs` | Export memory module |
| `crates/aivcs-core/src/domain.rs` | Add decision hooks to Run events |
| `crates/aivcs-core/src/recording.rs` | Hook decision recording |

---

## Key Metrics

- **Retrieval Latency:** < 500ms for semantic search (p95)
- **Index Size:** < 10GB for 100k decisions
- **Token Efficiency:** Context assembly uses 95%+ of budget without overflow
- **Decision Success Rate Improvement:** 15%+ after first failure on same task
- **Memory Accuracy:** Retrieved context semantic similarity > 0.85

---

## Integration Points

1. **From Run Events:** Auto-capture decisions, outcomes, errors
2. **To Agent Runtime:** Inject assembled context before agent execution
3. **To Dashboard:** Surface decision history and failure patterns
4. **To CLI:** `aivcs memory search`, `aivcs memory lessons`, `aivcs memory compact`

---

## Next Steps

1. Start Phase 1: Design Decision record and add to schema
2. Implement DecisionRecorder to capture events
3. Add database migrations
4. Write unit tests for decision capture
5. Then move to Phase 2 (Retrieval)

