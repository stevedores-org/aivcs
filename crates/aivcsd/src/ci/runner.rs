/// CheckRunner trait and implementations for subprocess execution
use anyhow::Result;

/// Trait for pluggable check execution (subprocess or mock)
#[async_trait::async_trait]
pub trait CheckRunner: Send + Sync {
    /// Run a check command and return (exit_code, output, duration_ms)
    async fn run(
        &self,
        cmd: &str,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Result<(i32, String, u64)>;
}

/// Concrete implementation using tokio::process::Command
pub struct ProcessCheckRunner;

#[async_trait::async_trait]
impl CheckRunner for ProcessCheckRunner {
    async fn run(
        &self,
        cmd: &str,
        args: &[&str],
        _env: &[(&str, &str)],
    ) -> Result<(i32, String, u64)> {
        let start = std::time::Instant::now();

        let output = tokio::process::Command::new(cmd)
            .args(args)
            .output()
            .await?;

        let duration_ms = start.elapsed().as_millis() as u64;

        let status_code = output.status.code().unwrap_or(1);
        let output_str = String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr);

        Ok((status_code, output_str, duration_ms))
    }
}
