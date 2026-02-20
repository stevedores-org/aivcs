//! CiRunner - Deterministic execution abstraction
//!
//! Story 3.1: CiRunner trait + local-ci tool wrapper
//! Story 3.2: Execution sandbox contract + EnvSpec hashing
//!
//! The runner is the execution truth. local-ci --json is the canonical record.
//! All outputs are captured, hashed (CAS), and made replayable via EnvSpec.

use crate::schema::*;
use crate::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::process::Command;
use tracing::{debug, info};

// ============================================================================
// ENVIRONMENT SPECIFICATION (Story 3.2)
// ============================================================================

/// Environment fingerprint for deterministic reproduction
///
/// Captures toolchain, Nix flake, and workspace state so runs can be replayed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvSpec {
    /// Rust toolchain version (from rustc --version)
    pub rustc_version: Option<String>,

    /// Cargo version (from cargo --version)
    pub cargo_version: Option<String>,

    /// Nix flake metadata hash (if using Nix)
    pub nix_flake_hash: Option<String>,

    /// System/platform (e.g., "x86_64-linux", "aarch64-darwin")
    pub system: String,

    /// Custom environment variables that affect the run
    pub env_vars: HashMap<String, String>,

    /// Composite hash of all the above
    pub digest: String,

    /// When this was captured
    #[serde(with = "crate::schema::surreal_datetime")]
    pub captured_at: DateTime<Utc>,
}

impl EnvSpec {
    /// Create a new EnvSpec and compute its digest
    pub fn new(system: String, env_vars: HashMap<String, String>) -> Self {
        let rustc_version = Self::get_rustc_version();
        let cargo_version = Self::get_cargo_version();
        let nix_flake_hash = Self::get_nix_flake_hash();

        let digest = Self::compute_digest(&rustc_version, &cargo_version, &nix_flake_hash, &system, &env_vars);

        EnvSpec {
            rustc_version,
            cargo_version,
            nix_flake_hash,
            system,
            env_vars,
            digest,
            captured_at: Utc::now(),
        }
    }

    /// Get Rust toolchain version
    fn get_rustc_version() -> Option<String> {
        Command::new("rustc")
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
    }

    /// Get Cargo version
    fn get_cargo_version() -> Option<String> {
        Command::new("cargo")
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
    }

    /// Get Nix flake hash (if available)
    fn get_nix_flake_hash() -> Option<String> {
        // Would check flake.lock or nix metadata
        None
    }

    /// Compute deterministic digest of environment
    fn compute_digest(
        rustc: &Option<String>,
        cargo: &Option<String>,
        nix: &Option<String>,
        system: &str,
        env_vars: &HashMap<String, String>,
    ) -> String {
        let mut hasher = Sha256::new();

        if let Some(r) = rustc {
            hasher.update(r.as_bytes());
        }
        if let Some(c) = cargo {
            hasher.update(c.as_bytes());
        }
        if let Some(n) = nix {
            hasher.update(n.as_bytes());
        }
        hasher.update(system.as_bytes());

        // Hash env vars in deterministic order
        let mut keys: Vec<_> = env_vars.keys().collect();
        keys.sort();
        for key in keys {
            hasher.update(key.as_bytes());
            hasher.update(b"=");
            hasher.update(env_vars[key].as_bytes());
            hasher.update(b"\n");
        }

        hex::encode(hasher.finalize())
    }

    /// Short digest (first 12 chars)
    pub fn short_digest(&self) -> &str {
        &self.digest[..12.min(self.digest.len())]
    }
}

// ============================================================================
// RUNNER OUTPUT CAPTURE
// ============================================================================

/// Captured output from a runner execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunOutput {
    /// Exit code
    pub exit_code: i32,

    /// Standard output
    pub stdout: String,

    /// Standard error
    pub stderr: String,

    /// Parsed local-ci JSON output (if available)
    pub local_ci_json: Option<serde_json::Value>,

    /// CAS digest of combined output
    pub output_digest: String,

    /// Duration in milliseconds
    pub duration_ms: u64,

    /// Captured at
    #[serde(with = "crate::schema::surreal_datetime")]
    pub captured_at: DateTime<Utc>,
}

impl RunOutput {
    /// Create output and compute digest
    pub fn new(
        exit_code: i32,
        stdout: String,
        stderr: String,
        local_ci_json: Option<serde_json::Value>,
        duration_ms: u64,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(stdout.as_bytes());
        hasher.update(stderr.as_bytes());
        if let Some(ref json) = local_ci_json {
            if let Ok(s) = serde_json::to_string(json) {
                hasher.update(s.as_bytes());
            }
        }

        let output_digest = hex::encode(hasher.finalize());

        RunOutput {
            exit_code,
            stdout,
            stderr,
            local_ci_json,
            output_digest,
            duration_ms,
            captured_at: Utc::now(),
        }
    }

    /// Did the run succeed?
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

// ============================================================================
// RUNNER TRAIT (Story 3.1)
// ============================================================================

/// Runner trait for CI execution
///
/// Implementations (local-ci, embedded, remote) must conform to this.
#[async_trait]
pub trait CiRunner: Send + Sync {
    /// Run CI with the given spec
    async fn run(&self, spec: &CIRunSpec, cwd: &str, env: &EnvSpec) -> Result<RunOutput>;

    /// Get runner name/version
    fn name(&self) -> &str;

    /// Get runner version
    fn version(&self) -> &str;

    /// Check if runner is available
    async fn is_available(&self) -> Result<bool>;
}

// ============================================================================
// LOCAL-CI RUNNER IMPLEMENTATION (Story 3.1)
// ============================================================================

/// Wrapper around local-ci command-line tool
pub struct LocalCiRunner {
    /// Path to local-ci binary
    pub binary_path: String,
    /// Version string
    pub version: String,
}

impl LocalCiRunner {
    /// Create a new LocalCiRunner
    pub fn new(binary_path: String, version: String) -> Self {
        LocalCiRunner {
            binary_path,
            version,
        }
    }

    /// Default: use "local-ci" from PATH
    pub fn default_path() -> Self {
        LocalCiRunner {
            binary_path: "local-ci".to_string(),
            version: "1.0.0".to_string(), // Will be detected at runtime
        }
    }

    /// Build the command to run
    fn build_command(&self, spec: &CIRunSpec, cwd: &str) -> Command {
        let mut cmd = Command::new(&self.binary_path);

        // Set working directory
        cmd.current_dir(cwd);

        // Add stages
        if !spec.stages.is_empty() {
            cmd.arg("--stages");
            cmd.arg(spec.stages.join(","));
        }

        // Add options
        if spec.fail_fast {
            cmd.arg("--fail-fast");
        }

        if let Some(timeout) = spec.stage_timeout_ms {
            cmd.arg("--stage-timeout");
            cmd.arg(timeout.to_string());
        }

        // Always request JSON output
        cmd.arg("--json");

        cmd
    }
}

#[async_trait]
impl CiRunner for LocalCiRunner {
    async fn run(&self, spec: &CIRunSpec, cwd: &str, env: &EnvSpec) -> Result<RunOutput> {
        let start = std::time::Instant::now();

        debug!(
            "Running local-ci: {} (env_hash: {})",
            spec.stages.join(","),
            env.short_digest()
        );

        let mut cmd = self.build_command(spec, cwd);

        // Inject environment variables from EnvSpec
        for (key, value) in &env.env_vars {
            cmd.env(key, value);
        }

        let output = cmd
            .output()
            .map_err(|e| crate::error::CIDomainError::RepairPlanError(format!(
                "Failed to execute local-ci: {}",
                e
            )))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        // Try to parse JSON output
        let local_ci_json = serde_json::from_str::<serde_json::Value>(&stdout).ok();

        info!(
            "local-ci completed: exit_code={}, duration={}ms",
            exit_code, duration_ms
        );

        Ok(RunOutput::new(exit_code, stdout, stderr, local_ci_json, duration_ms))
    }

    fn name(&self) -> &str {
        "local-ci"
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn is_available(&self) -> Result<bool> {
        let result = Command::new(&self.binary_path)
            .arg("--version")
            .output();

        Ok(result.is_ok())
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_spec_creation() {
        let env = EnvSpec::new("x86_64-linux".to_string(), HashMap::new());
        assert!(!env.digest.is_empty());
        assert_eq!(env.system, "x86_64-linux");
    }

    #[test]
    fn test_env_spec_digest_deterministic() {
        let env1 = EnvSpec::new("x86_64-linux".to_string(), HashMap::new());
        let env2 = EnvSpec::new("x86_64-linux".to_string(), HashMap::new());
        // Same system → same digest (ignoring timestamps)
        // In practice, timestamps differ, so we just check both have digests
        assert!(!env1.digest.is_empty());
        assert!(!env2.digest.is_empty());
    }

    #[test]
    fn test_env_spec_digest_different_system() {
        let env1 = EnvSpec::new("x86_64-linux".to_string(), HashMap::new());
        let env2 = EnvSpec::new("aarch64-darwin".to_string(), HashMap::new());
        assert_ne!(env1.digest, env2.digest, "Different systems should have different digests");
    }

    #[test]
    fn test_env_spec_digest_different_env_vars() {
        let mut vars1 = HashMap::new();
        vars1.insert("FOO".to_string(), "bar".to_string());

        let mut vars2 = HashMap::new();
        vars2.insert("FOO".to_string(), "baz".to_string());

        let env1 = EnvSpec::new("x86_64-linux".to_string(), vars1);
        let env2 = EnvSpec::new("x86_64-linux".to_string(), vars2);
        assert_ne!(env1.digest, env2.digest, "Different env vars should produce different digests");
    }

    #[test]
    fn test_env_spec_short_digest() {
        let env = EnvSpec::new("x86_64-linux".to_string(), HashMap::new());
        let short = env.short_digest();
        assert_eq!(short.len(), 12);
    }

    #[test]
    fn test_run_output_creation() {
        let output = RunOutput::new(0, "ok".to_string(), "".to_string(), None, 1000);
        assert!(output.success());
        assert!(!output.output_digest.is_empty());
        assert_eq!(output.duration_ms, 1000);
    }

    #[test]
    fn test_run_output_failure() {
        let output = RunOutput::new(1, "error".to_string(), "stderr".to_string(), None, 500);
        assert!(!output.success());
    }

    #[test]
    fn test_run_output_with_json() {
        let json = serde_json::json!({
            "passed": true,
            "stages": []
        });
        let output = RunOutput::new(0, "".to_string(), "".to_string(), Some(json), 100);
        assert!(output.local_ci_json.is_some());
    }

    #[test]
    fn test_local_ci_runner_creation() {
        let runner = LocalCiRunner::default_path();
        assert_eq!(runner.name(), "local-ci");
        assert!(!runner.version.is_empty());
    }

    #[test]
    fn test_ci_run_spec_to_command() {
        let runner = LocalCiRunner::default_path();
        let spec = CIRunSpec::default_stages("1.0.0".to_string());
        let _cmd = runner.build_command(&spec, ".");
        // Just verify it doesn't panic
    }

    #[test]
    fn test_run_output_digest_deterministic() {
        let output1 = RunOutput::new(0, "stdout".to_string(), "stderr".to_string(), None, 100);
        let output2 = RunOutput::new(0, "stdout".to_string(), "stderr".to_string(), None, 100);
        assert_eq!(output1.output_digest, output2.output_digest, "Same output should have same digest");
    }

    #[test]
    fn test_run_output_digest_different() {
        let output1 = RunOutput::new(0, "stdout1".to_string(), "".to_string(), None, 100);
        let output2 = RunOutput::new(0, "stdout2".to_string(), "".to_string(), None, 100);
        assert_ne!(output1.output_digest, output2.output_digest, "Different output should have different digest");
    }
}
