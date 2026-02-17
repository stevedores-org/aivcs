//! Evaluation suite definitions and scoring configuration.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Enumeration of available scorer types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", content = "value")]
pub enum ScorerType {
    /// Exact match comparison.
    ExactMatch,

    /// Semantic similarity (embeddings-based).
    SemanticSimilarity,

    /// Tool call sequence matching.
    ToolCallSequence,

    /// Custom scorer extension.
    Custom(String),
}

/// Configuration for a single scorer in an evaluation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScorerConfig {
    /// Name of this scorer (for reporting).
    pub name: String,

    /// Type of scorer.
    pub scorer_type: ScorerType,

    /// Scorer-specific parameters.
    pub params: serde_json::Value,
}

/// Thresholds for evaluation pass/fail criteria.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalThresholds {
    /// Minimum pass rate (0.0–1.0) for suite to pass.
    pub min_pass_rate: f32,

    /// Maximum allowed regression rate (0.0–1.0) vs. baseline.
    pub max_regression: f32,

    /// If true, stop on first failure.
    pub fail_fast: bool,
}

impl Default for EvalThresholds {
    fn default() -> Self {
        Self {
            min_pass_rate: 0.95,
            max_regression: 0.05,
            fail_fast: false,
        }
    }
}

/// A single test case within an evaluation suite.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalTestCase {
    /// Unique identifier for this test case.
    pub case_id: Uuid,

    /// Input provided to the agent.
    pub inputs: serde_json::Value,

    /// Expected output (optional for scoring-based evals).
    pub expected: Option<serde_json::Value>,

    /// Tags for categorizing/filtering test cases.
    pub tags: Vec<String>,
}

impl EvalTestCase {
    /// Create a new test case.
    pub fn new(inputs: serde_json::Value, expected: Option<serde_json::Value>) -> Self {
        Self {
            case_id: Uuid::new_v4(),
            inputs,
            expected,
            tags: Vec::new(),
        }
    }

    /// Add a tag to this test case.
    pub fn with_tag(mut self, tag: String) -> Self {
        self.tags.push(tag);
        self
    }
}

/// A complete evaluation suite for testing an agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalSuite {
    /// Unique identifier for this suite.
    pub suite_id: Uuid,

    /// SHA256 hex digest of canonical suite definition.
    pub suite_digest: String,

    /// Name of the evaluation.
    pub name: String,

    /// Version string for the suite.
    pub version: String,

    /// Test cases in this suite.
    pub test_cases: Vec<EvalTestCase>,

    /// Scorers to use for evaluation.
    pub scorers: Vec<ScorerConfig>,

    /// Pass/fail thresholds.
    pub thresholds: EvalThresholds,

    /// When the suite was created.
    pub created_at: DateTime<Utc>,
}

impl EvalSuite {
    /// Create a new evaluation suite.
    pub fn new(name: String, version: String) -> Self {
        Self {
            suite_id: Uuid::new_v4(),
            suite_digest: String::new(), // Will be computed on finalization
            name,
            version,
            test_cases: Vec::new(),
            scorers: Vec::new(),
            thresholds: EvalThresholds::default(),
            created_at: Utc::now(),
        }
    }

    /// Add a test case to the suite.
    pub fn add_test_case(mut self, test_case: EvalTestCase) -> Self {
        self.test_cases.push(test_case);
        self
    }

    /// Add a scorer configuration.
    pub fn add_scorer(mut self, scorer: ScorerConfig) -> Self {
        self.scorers.push(scorer);
        self
    }

    /// Set evaluation thresholds.
    pub fn with_thresholds(mut self, thresholds: EvalThresholds) -> Self {
        self.thresholds = thresholds;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_suite_serde_roundtrip() {
        let suite = EvalSuite::new("test_suite".to_string(), "1.0.0".to_string())
            .add_test_case(EvalTestCase::new(
                serde_json::json!({"input": "test"}),
                Some(serde_json::json!({"output": "expected"})),
            ))
            .add_scorer(ScorerConfig {
                name: "exact_match".to_string(),
                scorer_type: ScorerType::ExactMatch,
                params: serde_json::json!({}),
            });

        let json = serde_json::to_string(&suite).expect("serialize");
        let deserialized: EvalSuite = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(suite, deserialized);
    }

    #[test]
    fn test_eval_thresholds_defaults() {
        let thresholds = EvalThresholds::default();
        assert_eq!(thresholds.min_pass_rate, 0.95);
        assert_eq!(thresholds.max_regression, 0.05);
        assert!(!thresholds.fail_fast);
    }

    #[test]
    fn test_scorer_type_exact_match() {
        let scorer_type = ScorerType::ExactMatch;
        let json = serde_json::to_string(&scorer_type).expect("serialize");
        let deserialized: ScorerType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(scorer_type, deserialized);
    }

    #[test]
    fn test_scorer_type_semantic_similarity() {
        let scorer_type = ScorerType::SemanticSimilarity;
        let json = serde_json::to_string(&scorer_type).expect("serialize");
        let deserialized: ScorerType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(scorer_type, deserialized);
    }

    #[test]
    fn test_scorer_type_custom_roundtrip() {
        let scorer_type = ScorerType::Custom("my_custom_scorer".to_string());
        let json = serde_json::to_string(&scorer_type).expect("serialize");
        let deserialized: ScorerType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(scorer_type, deserialized);
    }

    #[test]
    fn test_scorer_type_tool_call_sequence() {
        let scorer_type = ScorerType::ToolCallSequence;
        let json = serde_json::to_string(&scorer_type).expect("serialize");
        let deserialized: ScorerType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(scorer_type, deserialized);
    }

    #[test]
    fn test_eval_test_case_new() {
        let test_case = EvalTestCase::new(
            serde_json::json!({"input": "test"}),
            Some(serde_json::json!({"output": "expected"})),
        );

        assert_eq!(test_case.inputs, serde_json::json!({"input": "test"}));
        assert_eq!(
            test_case.expected,
            Some(serde_json::json!({"output": "expected"}))
        );
        assert!(test_case.tags.is_empty());
    }

    #[test]
    fn test_eval_test_case_with_tag() {
        let test_case = EvalTestCase::new(
            serde_json::json!({"input": "test"}),
            Some(serde_json::json!({"output": "expected"})),
        )
        .with_tag("critical".to_string());

        assert_eq!(test_case.tags, vec!["critical"]);
    }

    #[test]
    fn test_scorer_config_serde_roundtrip() {
        let config = ScorerConfig {
            name: "test_scorer".to_string(),
            scorer_type: ScorerType::SemanticSimilarity,
            params: serde_json::json!({"threshold": 0.8}),
        };

        let json = serde_json::to_string(&config).expect("serialize");
        let deserialized: ScorerConfig = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_eval_suite_fluent_api() {
        let suite = EvalSuite::new("test".to_string(), "1.0.0".to_string())
            .add_test_case(EvalTestCase::new(
                serde_json::json!({"input": "test"}),
                Some(serde_json::json!({"output": "expected"})),
            ))
            .add_scorer(ScorerConfig {
                name: "scorer1".to_string(),
                scorer_type: ScorerType::ExactMatch,
                params: serde_json::json!({}),
            })
            .with_thresholds(EvalThresholds {
                min_pass_rate: 0.90,
                max_regression: 0.10,
                fail_fast: true,
            });

        assert_eq!(suite.test_cases.len(), 1);
        assert_eq!(suite.scorers.len(), 1);
        assert_eq!(suite.thresholds.min_pass_rate, 0.90);
        assert!(suite.thresholds.fail_fast);
    }
}
