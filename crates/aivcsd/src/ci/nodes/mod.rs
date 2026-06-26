pub mod aggregate;
pub mod check;
pub mod data_fabric;
pub mod github_status;
/// CI orchestration nodes for oxidizedgraph
pub mod setup;

pub use aggregate::AggregateNode;
pub use check::CheckNode;
pub use setup::SetupNode;
