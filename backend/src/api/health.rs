use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::Utc;

use crate::health::ScannerHealth;

pub fn routes() -> Router<Arc<ScannerHealth>> {
    Router::new().route("/health", get(health))
}

/// 200 while the scanner is receiving BLE events (or the process is still in
/// its startup grace period), 503 otherwise. The body is the same either
/// way so an operator can see *why* from a single `curl`.
async fn health(State(health): State<Arc<ScannerHealth>>) -> Response {
    let snapshot = health.snapshot(Utc::now());
    let status = if snapshot.healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(snapshot)).into_response()
}
