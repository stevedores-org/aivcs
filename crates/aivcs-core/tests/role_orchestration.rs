use std::collections::BTreeMap;

use aivcs_core::{
    default_role_templates, deterministic_role_order, merge_role_outputs, validate_handoff,
    validate_parallel_roles, AgentRole, HandoffValidationError, MergeConflictStrategy,
    ParallelPlanError, RoleHandoff, RoleOutput,
};

#[test]
fn planner_to_coder_handoff_validates_with_required_payload() {
    let templates = default_role_templates();
    let handoff = RoleHandoff {
        task_id: "task-1".to_string(),
        from: AgentRole::Planner,
        to: AgentRole::Coder,
        payload: BTreeMap::from([("task_plan".to_string(), "implement x".to_string())]),
    };

    let result = validate_handoff(&templates, &handoff);
    assert!(result.is_ok());
}

#[test]
fn handoff_rejects_missing_required_keys() {
    let templates = default_role_templates();
    let handoff = RoleHandoff {
        task_id: "task-2".to_string(),
        from: AgentRole::Planner,
        to: AgentRole::Coder,
        payload: BTreeMap::new(),
    };

    let result = validate_handoff(&templates, &handoff);
    assert_eq!(
        result,
        Err(HandoffValidationError::MissingRequiredKeys {
            role: AgentRole::Coder,
            keys: vec!["task_plan".to_string()],
        })
    );
}

#[test]
fn handoff_rejects_forbidden_route() {
    let templates = default_role_templates();
    let handoff = RoleHandoff {
        task_id: "task-3".to_string(),
        from: AgentRole::Tester,
        to: AgentRole::Planner,
        payload: BTreeMap::new(),
    };

    let result = validate_handoff(&templates, &handoff);
    assert_eq!(
        result,
        Err(HandoffValidationError::ForbiddenRoute {
            from: AgentRole::Tester,
            to: AgentRole::Planner,
        })
    );
}

#[test]
fn merge_fail_on_conflict_surfaces_conflict_and_keeps_existing() {
    let a = RoleOutput {
        role: AgentRole::Coder,
        step: 1,
        values: BTreeMap::from([("code_patch".to_string(), "v1".to_string())]),
    };
    let b = RoleOutput {
        role: AgentRole::Reviewer,
        step: 2,
        values: BTreeMap::from([("code_patch".to_string(), "v2".to_string())]),
    };

    let merged = merge_role_outputs(&[a, b], MergeConflictStrategy::FailOnConflict);
    assert_eq!(merged.values.get("code_patch"), Some(&"v1".to_string()));
    assert_eq!(merged.conflicts.len(), 1);
    assert_eq!(merged.conflicts[0].key, "code_patch");
}

#[test]
fn merge_prefer_role_priority_uses_fixer_value() {
    let coder = RoleOutput {
        role: AgentRole::Coder,
        step: 1,
        values: BTreeMap::from([("code_patch".to_string(), "initial".to_string())]),
    };
    let fixer = RoleOutput {
        role: AgentRole::Fixer,
        step: 2,
        values: BTreeMap::from([("code_patch".to_string(), "fixed".to_string())]),
    };

    let merged = merge_role_outputs(&[coder, fixer], MergeConflictStrategy::PreferRolePriority);
    assert_eq!(merged.values.get("code_patch"), Some(&"fixed".to_string()));
    assert_eq!(merged.conflicts.len(), 1);
}

#[test]
fn parallel_role_plan_rejects_duplicate_role() {
    let result = validate_parallel_roles(&[AgentRole::Coder, AgentRole::Coder]);
    assert_eq!(result, Err(ParallelPlanError::DuplicateRole(AgentRole::Coder)));
}

#[test]
fn deterministic_role_order_is_reproducible() {
    let first = deterministic_role_order(&[
        AgentRole::Reviewer,
        AgentRole::Planner,
        AgentRole::Fixer,
        AgentRole::Coder,
    ]);
    let second = deterministic_role_order(&[
        AgentRole::Coder,
        AgentRole::Fixer,
        AgentRole::Planner,
        AgentRole::Reviewer,
    ]);

    assert_eq!(first, second);
    assert_eq!(
        first,
        vec![
            AgentRole::Fixer,
            AgentRole::Reviewer,
            AgentRole::Coder,
            AgentRole::Planner
        ]
    );
}
