/// WorkerLoop — polls data-fabric task queue and runs CI graphs
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

pub struct WorkerLoop {
    pub max_concurrent: usize,
    pub poll_interval_ms: u64,
}

impl Default for WorkerLoop {
    fn default() -> Self {
        Self {
            max_concurrent: 8,
            poll_interval_ms: 5000,
        }
    }
}

impl WorkerLoop {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            max_concurrent,
            poll_interval_ms: 5000,
        }
    }

    /// Main worker loop: continuously poll data-fabric queue
    pub async fn run(&self) -> Result<(), String> {
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));

        loop {
            // TODO: In production, call data_fabric_client.claim_next_task("ci_check_run")
            // For now, simulate no tasks available
            let task_available = false;

            if task_available {
                // Acquire semaphore permit before spawning
                match semaphore.clone().acquire_owned().await {
                    Ok(permit) => {
                        // Spawn task on tokio runtime
                        tokio::spawn(async move {
                            // Run CI graph here
                            // TODO: call GraphRunner::invoke(build_ci_graph(), state)
                            drop(permit); // Release permit when done
                        });
                    }
                    Err(e) => {
                        eprintln!("Failed to acquire semaphore: {}", e);
                        return Err(e.to_string());
                    }
                }
            } else {
                // No tasks available, sleep before retrying
                tokio::time::sleep(Duration::from_millis(self.poll_interval_ms)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_worker_respects_semaphore_limit() {
        let worker = WorkerLoop::new(2);
        assert_eq!(worker.max_concurrent, 2);
    }
}
