use anyhow::Result;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    tracing::info!("aivcsd stub started");
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
