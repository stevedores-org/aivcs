use anyhow::Result;
use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tracing::{info, Level};

#[tokio::main]
async fn main() -> Result<()> {
    aivcs_core::init_tracing(false, Level::INFO);

    info!("🚀 aivcsd starting");

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/version", get(version_info));

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("📡 listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now()
    }))
}

async fn version_info() -> Json<Value> {
    Json(json!({
        "name": "aivcsd",
        "version": env!("CARGO_PKG_VERSION"),
        "platform": aivcs_core::domain::Platform::detect().to_string()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check() {
        let res = health_check().await;
        assert_eq!(res.0["status"], "healthy");
    }
}
