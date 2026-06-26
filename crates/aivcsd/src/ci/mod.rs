pub mod graph;
pub mod nodes;
pub mod runner;
/// CI orchestration module
///
/// Provides agentic CI execution via oxidizedgraph:
/// - graph: Builds the CiAgentGraph with SetupNode, ParallelSubgraphs, AggregateNode, DataFabricNode, GitHubStatusNode
/// - nodes: Individual node implementations
/// - runner: CheckRunner trait for pluggable subprocess execution
/// - worker: WorkerLoop for polling data-fabric task queue
/// - state: Context key constants and serde types
///
/// Architecture:
/// 1. MCP tool `run_ci_checks` triggers via aivcs-mcp-gateway
/// 2. aivcs-mcp-gateway calls data_fabric_client.create_task("ci_check_run", params)
/// 3. WorkerLoop polls data_fabric_client.claim_next_task("ci_check_run")
/// 4. On claim, WorkerLoop spawns run_ci_graph() which invokes GraphRunner::invoke(CiAgentGraph)
/// 5. Graph runs 4 checks in parallel, aggregates results, persists to data-fabric, updates GitHub
pub mod state;
pub mod worker;
