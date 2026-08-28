mod api;
mod ble;
mod config;
mod domain;
mod retention;
mod storage;

use std::sync::Arc;

use chrono::Duration;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use config::Config;
use storage::SqliteStorage;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env();

    let storage = SqliteStorage::connect(&config.database_url)
        .await
        .expect("failed to connect to database");
    let storage = Arc::new(storage);

    // BLE scanning runs independently of the HTTP server: it supervises
    // itself (reconnecting on adapter resets) and never returns, but even
    // if that ever changed, a scan problem shouldn't take the API down.
    let reading_interval = config
        .reading_interval_secs
        .map(|secs| Duration::seconds(secs as i64));
    tokio::spawn({
        let storage = Arc::clone(&storage);
        let ble_adapter = config.ble_adapter.clone();
        async move { ble::run(storage, reading_interval, ble_adapter).await }
    });

    if let Some(retention_days) = config.retention_days {
        tokio::spawn({
            let storage = Arc::clone(&storage);
            async move { retention::run(storage, retention_days).await }
        });
    }

    let app = api::router(storage);
    let listener = TcpListener::bind(&config.bind_address)
        .await
        .expect("failed to bind HTTP listener");
    tracing::info!(address = %config.bind_address, "listening");
    axum::serve(listener, app).await.expect("server error");
}
