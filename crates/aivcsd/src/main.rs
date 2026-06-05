use anyhow::Result;
use tracing::Level;

fn main() -> Result<()> {
    aivcs_core::init_tracing(false, Level::INFO);

    tracing::info!("aivcsd stub started");
    // Stub daemon: stay alive until systemd sends SIGTERM on stop.
    std::thread::park();
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn aivcsd_smoke_compiles() {
        // Compile-time check: main exists and returns Result
        let _: fn() -> anyhow::Result<()> = super::main;
    }
}
