use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{Duration, Utc};
use tracing::{info, warn};

use crate::storage::SqliteStorage;

/// How often to check for and delete readings past the retention window.
/// Retention is specified in whole days, so hourly is far more than
/// granular enough.
const CLEANUP_INTERVAL: StdDuration = StdDuration::from_secs(60 * 60);

/// Deletes readings older than `retention_days`, once immediately and then
/// every [`CLEANUP_INTERVAL`], for as long as the process runs. Only
/// spawned when `RETENTION_DAYS` is set — otherwise readings are kept
/// forever and there is nothing to prune.
pub async fn run(storage: Arc<SqliteStorage>, retention_days: u64) {
    let mut ticker = tokio::time::interval(CLEANUP_INTERVAL);
    loop {
        ticker.tick().await;
        let cutoff = Utc::now() - Duration::days(retention_days as i64);
        match storage.delete_readings_before(cutoff).await {
            Ok(0) => {}
            Ok(deleted) => info!(deleted, retention_days, "pruned old readings"),
            Err(err) => warn!(error = %err, "failed to prune old readings"),
        }
    }
}
