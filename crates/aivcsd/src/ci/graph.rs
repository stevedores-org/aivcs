use crate::ci::nodes::data_fabric::DataFabricNode;
use crate::ci::nodes::github_status::GitHubStatusNode;
/// CiAgentGraph builder
use crate::ci::nodes::{AggregateNode, CheckNode, SetupNode};
use crate::ci::runner::{CheckRunner, ProcessCheckRunner};
use crate::df::MockDataFabricGateway;
use crate::github::MockGitHubClient;
use oxidizedgraph::graph::{CompiledGraph, GraphBuilder};
use std::sync::Arc;

pub struct CiDeps {
    pub data_fabric_gateway: Arc<dyn crate::df::DataFabricGateway>,
    pub github_client: Arc<dyn crate::github::GitHubClient>,
    pub check_runner: Arc<dyn CheckRunner>,
}

impl Default for CiDeps {
    fn default() -> Self {
        Self {
            data_fabric_gateway: Arc::new(MockDataFabricGateway::new()),
            github_client: Arc::new(MockGitHubClient::new()),
            check_runner: Arc::new(ProcessCheckRunner),
        }
    }
}

/// Build the complete CI orchestration graph
pub fn build_ci_graph(deps: CiDeps) -> Result<CompiledGraph, String> {
    let mut builder = GraphBuilder::new();

    // 1. SetupNode — reads task params from context
    let setup_node = SetupNode;
    builder = builder.add_node(setup_node);

    // 2. Four parallel check nodes
    let type_safety_node = CheckNode {
        name: "type-safety".to_string(),
        cmd: "cargo".to_string(),
        args: vec![
            "clippy".to_string(),
            "--".to_string(),
            "-D".to_string(),
            "warnings".to_string(),
        ],
        runner: deps.check_runner.clone(),
    };
    builder = builder.add_node(type_safety_node);

    let unit_tests_node = CheckNode {
        name: "unit-tests".to_string(),
        cmd: "cargo".to_string(),
        args: vec!["test".to_string(), "--lib".to_string()],
        runner: deps.check_runner.clone(),
    };
    builder = builder.add_node(unit_tests_node);

    let secrets_node = CheckNode {
        name: "secrets".to_string(),
        cmd: "bash".to_string(),
        args: vec!["-c".to_string(), "echo 'Secrets check stub'".to_string()],
        runner: deps.check_runner.clone(),
    };
    builder = builder.add_node(secrets_node);

    let config_lint_node = CheckNode {
        name: "config-lint".to_string(),
        cmd: "bash".to_string(),
        args: vec!["-c".to_string(), "echo 'Config lint stub'".to_string()],
        runner: deps.check_runner.clone(),
    };
    builder = builder.add_node(config_lint_node);

    // 3. AggregateNode — collects results
    let aggregate_node = AggregateNode;
    builder = builder.add_node(aggregate_node);

    // 4. DataFabricNode — persists results
    let data_fabric_node = DataFabricNode {
        gateway: deps.data_fabric_gateway,
    };
    builder = builder.add_node(data_fabric_node);

    // 5. GitHubStatusNode — posts commit status
    let github_status_node = GitHubStatusNode {
        client: deps.github_client,
    };
    builder = builder.add_node(github_status_node);

    // Wire the graph:
    // setup -> [type_safety, unit_tests, secrets, config_lint] (parallel)
    // -> aggregate -> data_fabric -> github_status

    builder = builder.set_entry_point("setup");

    // Direct edges from setup to each check (parallel execution via oxidizedgraph)
    builder = builder.add_edge("setup", "type-safety");
    builder = builder.add_edge("setup", "unit-tests");
    builder = builder.add_edge("setup", "secrets");
    builder = builder.add_edge("setup", "config-lint");

    // Each check routes to aggregate
    builder = builder.add_edge("type-safety", "aggregate");
    builder = builder.add_edge("unit-tests", "aggregate");
    builder = builder.add_edge("secrets", "aggregate");
    builder = builder.add_edge("config-lint", "aggregate");

    // Sequential path: aggregate -> data_fabric -> github_status
    builder = builder.add_edge("aggregate", "data_fabric");
    builder = builder.add_edge("data_fabric", "github_status");
    builder = builder.add_edge_to_end("github_status");

    builder
        .compile()
        .map_err(|e| format!("Graph compilation failed: {:?}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_compiles() {
        let deps = CiDeps::default();
        let result = build_ci_graph(deps);
        assert!(result.is_ok(), "graph should compile successfully");
    }
}
