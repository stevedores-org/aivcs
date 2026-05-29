use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

/// Single eval case result in the persisted eval results artifact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalCaseResultArtifact {
    pub case_id: Uuid,
    pub score: f32,
    pub passed: bool,
}

/// Eval summary section persisted in eval_results.json.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalSummaryArtifact {
    pub total_cases: usize,
    pub passed_cases: usize,
    pub pass_rate: f32,
    pub overall_pass: bool,
}

/// Canonical eval results artifact written for CI and PR reporting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalResultsArtifact {
    pub schema_version: String,
    pub generated_at: DateTime<Utc>,
    pub suite_name: String,
    pub suite_version: String,
    pub suite_digest: String,
    pub summary: EvalSummaryArtifact,
    pub case_results: Vec<EvalCaseResultArtifact>,
}

/// Compact data model used to render diff_summary.md.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffSummaryArtifact {
    pub spec_changed_paths: Vec<String>,
    pub spec_only_in_a: Vec<String>,
    pub spec_only_in_b: Vec<String>,
    pub run_events_a: usize,
    pub run_events_b: usize,
    pub run_added: usize,
    pub run_removed: usize,
    pub run_reordered: usize,
    pub run_param_changed: usize,
}

/// Data model for cross-org integration audit report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrossOrgAuditArtifact {
    pub generated_at: DateTime<Utc>,
    pub coupling: Vec<crate::multi_repo::audit::RepoCoupling>,
    pub critical_spofs: Vec<String>,
    pub health: crate::multi_repo::aggregator::CiHealthReport,
}

/// Write eval_results.json in pretty JSON format.
pub fn write_eval_results_json(path: &Path, artifact: &EvalResultsArtifact) -> Result<()> {
    let content = serde_json::to_string_pretty(artifact).context("serialize eval artifact")?;
    std::fs::write(path, content).with_context(|| format!("write {:?}", path))?;
    Ok(())
}

/// Render markdown summary for PR/comment/check output.
pub fn render_diff_summary_md(artifact: &DiffSummaryArtifact) -> String {
    let mut out = String::new();
    out.push_str("# Diff Summary\n\n");
    out.push_str("## Spec\n");
    out.push_str(&format!(
        "- changed paths: {}\n- only in A: {}\n- only in B: {}\n\n",
        artifact.spec_changed_paths.len(),
        artifact.spec_only_in_a.len(),
        artifact.spec_only_in_b.len()
    ));

    if !artifact.spec_changed_paths.is_empty() {
        out.push_str("### Changed Paths\n");
        for p in &artifact.spec_changed_paths {
            out.push_str(&format!("- `{}`\n", p));
        }
        out.push('\n');
    }

    out.push_str("## Run\n");
    out.push_str(&format!(
        "- events A: {}\n- events B: {}\n- added tool calls: {}\n- removed tool calls: {}\n- reordered tool calls: {}\n- param changed: {}\n",
        artifact.run_events_a,
        artifact.run_events_b,
        artifact.run_added,
        artifact.run_removed,
        artifact.run_reordered,
        artifact.run_param_changed
    ));
    out
}

/// Render cross-org integration audit report in Markdown.
pub fn render_cross_org_audit_md(artifact: &CrossOrgAuditArtifact) -> String {
    let mut out = String::new();
    out.push_str("# Cross-Org Integration Audit Report\n\n");
    out.push_str(&format!("Generated at: {}\n\n", artifact.generated_at.to_rfc3339()));

    out.push_str("## Reliability Coupling Analysis\n\n");
    out.push_str("| Repository | Direct Dependents | Blast Radius | Critical Path |\n");
    out.push_str("|---|---|---|---|\n");
    for c in &artifact.coupling {
        let critical = if c.is_critical_path { "⚠️ YES" } else { "ok" };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            c.repo_id, c.direct_dependents, c.blast_radius, critical
        ));
    }
    out.push('\n');

    if !artifact.critical_spofs.is_empty() {
        out.push_str("### 🚨 Critical Single Points of Failure (SPOFs)\n");
        for spof in &artifact.critical_spofs {
            out.push_str(&format!("- `{}`\n", spof));
        }
        out.push('\n');
    }

    out.push_str("## Integration Health Summary\n\n");
    let status_icon = if artifact.health.all_healthy { "✅" } else { "❌" };
    out.push_str(&format!("Overall Status: {} {}\n\n", status_icon, if artifact.health.all_healthy { "HEALTHY" } else { "DEGRADED" }));

    out.push_str("| Repository | Status | Latest Run | Failing Stages |\n");
    out.push_str("|---|---|---|---|\n");
    for h in &artifact.health.repo_health {
        let (status_text, icon) = match &h.status {
            crate::multi_repo::aggregator::RepoHealthStatus::Healthy => ("Healthy", "✅"),
            crate::multi_repo::aggregator::RepoHealthStatus::Degraded { .. } => ("Degraded", "⚠️"),
            crate::multi_repo::aggregator::RepoHealthStatus::Down => ("Down", "❌"),
            crate::multi_repo::aggregator::RepoHealthStatus::Unknown => ("Unknown", "❓"),
        };
        let failing = match &h.status {
            crate::multi_repo::aggregator::RepoHealthStatus::Degraded { failing_stages } => failing_stages.join(", "),
            _ => "-".to_string(),
        };
        let run_id = h.last_run.as_ref().map(|r| r.run_id.chars().take(8).collect::<String>()).unwrap_or_else(|| "N/A".to_string());
        out.push_str(&format!(
            "| `{}` | {} {} | `{}` | {} |\n",
            h.repo_id, icon, status_text, run_id, failing
        ));
    }
    out.push('\n');

    out.push_str("## Recommendations\n\n");
    for c in &artifact.coupling {
        if c.is_critical_path && c.blast_radius > 5 {
            out.push_str(&format!("- **Decouple `{}`**: This repository has a very high blast radius ({}). Consider splitting it into smaller, more focused services or adding redundant fallback paths.\n", c.repo_id, c.blast_radius));
        }
    }
    
    out.push_str("## Dependency Matrix (Mermaid)\n\n");
    out.push_str("```mermaid\ngraph TD\n");
    // We don't have the original graph here easily, but we can reconstruct it from coupling if we had more info
    // or just pass it in. For now, a placeholder or simple list.
    out.push_str("    %% [Dependency graph visualization]\n");
    out.push_str("```\n");

    out
}

/// Write diff_summary.md.
pub fn write_diff_summary_md(path: &Path, artifact: &DiffSummaryArtifact) -> Result<()> {
    let md = render_diff_summary_md(artifact);
    std::fs::write(path, md).with_context(|| format!("write {:?}", path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn eval_results_schema_has_expected_keys() {
        let artifact = EvalResultsArtifact {
            schema_version: "1.0".to_string(),
            generated_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .expect("parse RFC3339")
                .with_timezone(&Utc),
            suite_name: "smoke".to_string(),
            suite_version: "0.1.0".to_string(),
            suite_digest: "abc".to_string(),
            summary: EvalSummaryArtifact {
                total_cases: 2,
                passed_cases: 1,
                pass_rate: 0.5,
                overall_pass: false,
            },
            case_results: vec![EvalCaseResultArtifact {
                case_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111")
                    .expect("valid UUID"),
                score: 1.0,
                passed: true,
            }],
        };

        let raw = serde_json::to_value(&artifact).expect("serialize artifact");
        let obj = raw.as_object().expect("artifact object");
        assert!(obj.contains_key("schema_version"));
        assert!(obj.contains_key("generated_at"));
        assert!(obj.contains_key("suite_name"));
        assert!(obj.contains_key("suite_version"));
        assert!(obj.contains_key("suite_digest"));
        assert!(obj.contains_key("summary"));
        assert!(obj.contains_key("case_results"));

        assert_eq!(raw["summary"]["total_cases"], json!(2));
        assert_eq!(raw["summary"]["passed_cases"], json!(1));
        assert_eq!(raw["case_results"][0]["score"], json!(1.0));
    }

    #[test]
    fn diff_summary_markdown_render_is_stable() {
        let artifact = DiffSummaryArtifact {
            spec_changed_paths: vec!["/model".to_string(), "/routing/strategy".to_string()],
            spec_only_in_a: vec!["/legacy".to_string()],
            spec_only_in_b: vec![],
            run_events_a: 12,
            run_events_b: 14,
            run_added: 1,
            run_removed: 0,
            run_reordered: 2,
            run_param_changed: 3,
        };

        let actual = render_diff_summary_md(&artifact);
        let expected = "# Diff Summary\n\n## Spec\n- changed paths: 2\n- only in A: 1\n- only in B: 0\n\n### Changed Paths\n- `/model`\n- `/routing/strategy`\n\n## Run\n- events A: 12\n- events B: 14\n- added tool calls: 1\n- removed tool calls: 0\n- reordered tool calls: 2\n- param changed: 3\n";
        assert_eq!(actual, expected);
    }
}
