//! Publish gate rules engine.
//!
//! Evaluates a [`PublishCandidate`] against a [`PublishRuleSet`] to produce a
//! [`PublishVerdict`] — the pass/fail decision that blocks or allows a release
//! promotion. Supports semver validation, version ordering, uniqueness checks,
//! and release-notes/spec-digest requirements.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Semver helpers (manual — no external dep)
// ---------------------------------------------------------------------------

/// Parsed semver: MAJOR.MINOR.PATCH with optional pre-release suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Semver {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Option<String>,
}

impl Semver {
    fn parse(input: &str) -> Option<Self> {
        let (version_part, pre) = match input.split_once('-') {
            Some((v, p)) if !p.is_empty() => (v, Some(p.to_string())),
            _ => (input, None),
        };

        let parts: Vec<&str> = version_part.split('.').collect();
        if parts.len() != 3 {
            return None;
        }

        let major = parts[0].parse::<u64>().ok()?;
        let minor = parts[1].parse::<u64>().ok()?;
        let patch = parts[2].parse::<u64>().ok()?;

        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    /// Compare two semver values. Pre-release < release for equal versions.
    fn cmp_version(&self, other: &Self) -> std::cmp::Ordering {
        let tuple_cmp =
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        if tuple_cmp != std::cmp::Ordering::Equal {
            return tuple_cmp;
        }
        // Same numeric version: pre-release < release
        match (&self.pre, &other.pre) {
            (None, None) => std::cmp::Ordering::Equal,
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(a), Some(b)) => a.cmp(b),
        }
    }
}

// ---------------------------------------------------------------------------
// Publish candidate (input to the gate)
// ---------------------------------------------------------------------------

/// The release candidate being evaluated for publish readiness.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublishCandidate {
    /// Version label to publish (e.g. "1.2.3").
    pub version_label: Option<String>,
    /// Previous published version (for ordering checks).
    pub previous_version: Option<String>,
    /// All previously published version labels (for uniqueness checks).
    pub existing_versions: Vec<String>,
    /// Release notes content.
    pub notes: Option<String>,
    /// Spec digest string.
    pub spec_digest: String,
}

// ---------------------------------------------------------------------------
// Publish rules
// ---------------------------------------------------------------------------

/// A single publish gate rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PublishRule {
    /// `version_label` must parse as valid semver (MAJOR.MINOR.PATCH, optional
    /// pre-release suffix).
    SemverFormat,
    /// Version must be strictly greater than `previous_version` (skipped when
    /// no previous version exists).
    VersionBump,
    /// Version must not already appear in `existing_versions`.
    UniqueVersion,
    /// Release notes must be present and non-empty.
    RequireNotes,
    /// `spec_digest` must be non-empty.
    RequireSpecDigest,
}

/// A set of publish rules with a fail-fast flag.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublishRuleSet {
    pub rules: Vec<PublishRule>,
    pub fail_fast: bool,
}

impl PublishRuleSet {
    /// Standard rule set: `SemverFormat` + `VersionBump` + `RequireSpecDigest`.
    pub fn standard() -> Self {
        Self {
            rules: vec![
                PublishRule::SemverFormat,
                PublishRule::VersionBump,
                PublishRule::RequireSpecDigest,
            ],
            fail_fast: false,
        }
    }

    /// Append a rule.
    pub fn with_rule(mut self, rule: PublishRule) -> Self {
        self.rules.push(rule);
        self
    }
}

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

/// A single rule violation.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishViolation {
    /// Which rule was violated.
    pub rule: PublishRule,
    /// Human-readable explanation.
    pub reason: String,
}

/// The outcome of evaluating a publish rule set against a candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishVerdict {
    /// Whether the gate passed (no violations).
    pub passed: bool,
    /// Violations found (empty when passed).
    pub violations: Vec<PublishViolation>,
}

impl PublishVerdict {
    fn pass() -> Self {
        Self {
            passed: true,
            violations: Vec::new(),
        }
    }

    fn fail(violations: Vec<PublishViolation>) -> Self {
        Self {
            passed: false,
            violations,
        }
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// Evaluate a [`PublishCandidate`] against a [`PublishRuleSet`], returning a
/// [`PublishVerdict`].
///
/// When `fail_fast` is true, evaluation stops at the first violation.
pub fn evaluate_publish_gate(
    rule_set: &PublishRuleSet,
    candidate: &PublishCandidate,
) -> PublishVerdict {
    let mut violations = Vec::new();

    for rule in &rule_set.rules {
        if let Some(v) = check_rule(rule, candidate) {
            violations.push(v);
            if rule_set.fail_fast {
                return PublishVerdict::fail(violations);
            }
        }
    }

    if violations.is_empty() {
        PublishVerdict::pass()
    } else {
        PublishVerdict::fail(violations)
    }
}

fn check_rule(rule: &PublishRule, candidate: &PublishCandidate) -> Option<PublishViolation> {
    match rule {
        PublishRule::SemverFormat => {
            let label = match &candidate.version_label {
                Some(l) if !l.is_empty() => l,
                _ => {
                    return Some(PublishViolation {
                        rule: rule.clone(),
                        reason: "version_label is missing or empty".to_string(),
                    });
                }
            };
            if Semver::parse(label).is_none() {
                Some(PublishViolation {
                    rule: rule.clone(),
                    reason: format!(
                        "'{}' is not valid semver (expected MAJOR.MINOR.PATCH)",
                        label
                    ),
                })
            } else {
                None
            }
        }

        PublishRule::VersionBump => {
            let current_label = match &candidate.version_label {
                Some(l) if !l.is_empty() => l,
                _ => return None, // no label → nothing to compare
            };
            let prev_label = match &candidate.previous_version {
                Some(l) if !l.is_empty() => l,
                _ => return None, // no previous → skip
            };
            let current = Semver::parse(current_label)?;
            let previous = Semver::parse(prev_label)?;
            if current.cmp_version(&previous) != std::cmp::Ordering::Greater {
                Some(PublishViolation {
                    rule: rule.clone(),
                    reason: format!(
                        "version '{}' is not greater than previous '{}'",
                        current_label, prev_label,
                    ),
                })
            } else {
                None
            }
        }

        PublishRule::UniqueVersion => {
            let label = match &candidate.version_label {
                Some(l) if !l.is_empty() => l,
                _ => return None,
            };
            if candidate.existing_versions.contains(label) {
                Some(PublishViolation {
                    rule: rule.clone(),
                    reason: format!("version '{}' already exists in history", label),
                })
            } else {
                None
            }
        }

        PublishRule::RequireNotes => match &candidate.notes {
            Some(n) if !n.trim().is_empty() => None,
            _ => Some(PublishViolation {
                rule: rule.clone(),
                reason: "release notes are missing or empty".to_string(),
            }),
        },

        PublishRule::RequireSpecDigest => {
            if candidate.spec_digest.trim().is_empty() {
                Some(PublishViolation {
                    rule: rule.clone(),
                    reason: "spec_digest is missing or empty".to_string(),
                })
            } else {
                None
            }
        }
    }
}
