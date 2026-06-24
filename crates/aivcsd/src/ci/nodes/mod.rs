/// CI orchestration nodes for oxidizedgraph

pub mod setup;
pub mod check;
pub mod aggregate;
pub mod data_fabric;
pub mod github_status;

pub use setup::SetupNode;
pub use check::CheckNode;
pub use aggregate::AggregateNode;
pub use data_fabric::DataFabricNode;
pub use github_status::GitHubStatusNode;
