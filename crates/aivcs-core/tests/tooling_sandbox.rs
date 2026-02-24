use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use aivcs_core::{
    JsonFieldSchema, PolicyAction, PolicyMatrix, ToolAdapter, ToolCapability, ToolExecutionConfig,
    ToolExecutionError, ToolExecutor, ToolInvocation, ToolRegistry, ToolSpec,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

#[derive(Clone)]
enum Step {
    Return(Value),
    Err(&'static str),
    Sleep(u64),
}

#[derive(Clone)]
struct ScriptedAdapter {
    steps: Arc<Mutex<Vec<Step>>>,
    calls: Arc<AtomicUsize>,
}

impl ScriptedAdapter {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            steps: Arc::new(Mutex::new(steps)),
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ToolAdapter for ScriptedAdapter {
    async fn call(&self, _tool_name: &str, _input: &Value) -> std::result::Result<Value, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let step = {
            let mut guard = self.steps.lock().await;
            if guard.is_empty() {
                Step::Err("no scripted step")
            } else {
                guard.remove(0)
            }
        };

        match step {
            Step::Return(v) => Ok(v),
            Step::Err(msg) => Err(msg.to_string()),
            Step::Sleep(ms) => {
                sleep(Duration::from_millis(ms)).await;
                Ok(json!({"ok": true}))
            }
        }
    }
}

fn registry_with_echo_tool() -> ToolRegistry {
    let mut reg = ToolRegistry::default();
    reg.register(ToolSpec {
        name: "echo".to_string(),
        capability: ToolCapability::ShellExec,
        input_schema: JsonFieldSchema::required(["message"]),
        output_schema: JsonFieldSchema::required(["ok"]),
    })
    .expect("register");
    reg
}

#[tokio::test]
async fn disallowed_operation_is_blocked_with_reason() {
    let reg = registry_with_echo_tool();
    let policy =
        PolicyMatrix::default().with_capability(ToolCapability::ShellExec, PolicyAction::Deny);
    let adapter = ScriptedAdapter::new(vec![Step::Return(json!({"ok": true}))]);

    let executor = ToolExecutor::new(
        reg,
        policy,
        adapter,
        ToolExecutionConfig {
            timeout_ms: 100,
            max_retries: 0,
            circuit_breaker_threshold: 3,
        },
    );

    let err = executor
        .execute(ToolInvocation::new("echo", json!({"message": "hi"})), None)
        .await
        .expect_err("should deny");

    match err {
        ToolExecutionError::PolicyDenied { reason, .. } => {
            assert!(
                reason.contains("shell_exec"),
                "reason should mention capability: {reason}"
            );
        }
        other => panic!("expected PolicyDenied, got {other:?}"),
    }
}

#[tokio::test]
async fn input_and_output_schema_are_enforced() {
    let reg = registry_with_echo_tool();
    let policy = PolicyMatrix::default();
    let adapter = ScriptedAdapter::new(vec![Step::Return(json!({"not_ok": true}))]);

    let executor = ToolExecutor::new(reg, policy, adapter, ToolExecutionConfig::default());

    let input_err = executor
        .execute(
            ToolInvocation::new("echo", json!({})),
            Some("run-1".to_string()),
        )
        .await
        .expect_err("input schema should fail");
    assert!(matches!(
        input_err,
        ToolExecutionError::SchemaViolation {
            stage: aivcs_core::SchemaStage::Input,
            ..
        }
    ));

    let output_err = executor
        .execute(
            ToolInvocation::new("echo", json!({"message": "hi"})),
            Some("run-1".to_string()),
        )
        .await
        .expect_err("output schema should fail");
    assert!(matches!(
        output_err,
        ToolExecutionError::SchemaViolation {
            stage: aivcs_core::SchemaStage::Output,
            ..
        }
    ));
}

#[tokio::test]
async fn retries_then_succeeds_and_emits_telemetry() {
    let reg = registry_with_echo_tool();
    let policy = PolicyMatrix::default();
    let adapter = ScriptedAdapter::new(vec![
        Step::Err("transient"),
        Step::Return(json!({"ok": true, "value": "done"})),
    ]);

    let executor = ToolExecutor::new(
        reg,
        policy,
        adapter,
        ToolExecutionConfig {
            timeout_ms: 100,
            max_retries: 1,
            circuit_breaker_threshold: 3,
        },
    );

    let report = executor
        .execute(
            ToolInvocation::new("echo", json!({"message": "hi"})),
            Some("run-retry".to_string()),
        )
        .await
        .expect("second attempt should pass");

    assert_eq!(report.output["ok"], json!(true));
    assert_eq!(report.telemetry.retries, 1);
    assert_eq!(report.telemetry.run_id.as_deref(), Some("run-retry"));
}

#[tokio::test]
async fn timeout_is_reported_no_silent_fallback() {
    let reg = registry_with_echo_tool();
    let policy = PolicyMatrix::default();
    let adapter = ScriptedAdapter::new(vec![Step::Sleep(50)]);

    let executor = ToolExecutor::new(
        reg,
        policy,
        adapter,
        ToolExecutionConfig {
            timeout_ms: 1,
            max_retries: 0,
            circuit_breaker_threshold: 3,
        },
    );

    let err = executor
        .execute(ToolInvocation::new("echo", json!({"message": "hi"})), None)
        .await
        .expect_err("should timeout");

    assert!(matches!(err, ToolExecutionError::Timeout { .. }));
}

#[tokio::test]
async fn circuit_breaker_opens_after_threshold() {
    let reg = registry_with_echo_tool();
    let policy = PolicyMatrix::default();
    let adapter = ScriptedAdapter::new(vec![
        Step::Err("boom1"),
        Step::Err("boom2"),
        Step::Err("boom3"),
    ]);
    let adapter_for_assert = adapter.clone();

    let executor = ToolExecutor::new(
        reg,
        policy,
        adapter,
        ToolExecutionConfig {
            timeout_ms: 100,
            max_retries: 0,
            circuit_breaker_threshold: 2,
        },
    );

    let _ = executor
        .execute(ToolInvocation::new("echo", json!({"message": "1"})), None)
        .await
        .expect_err("first failure");
    let _ = executor
        .execute(ToolInvocation::new("echo", json!({"message": "2"})), None)
        .await
        .expect_err("second failure");

    let err = executor
        .execute(ToolInvocation::new("echo", json!({"message": "3"})), None)
        .await
        .expect_err("circuit should be open");

    assert!(matches!(err, ToolExecutionError::CircuitOpen { .. }));
    assert_eq!(
        adapter_for_assert.call_count(),
        2,
        "adapter should not be called once circuit is open"
    );
}
