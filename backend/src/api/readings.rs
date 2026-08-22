use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use super::{ApiError, AppState};

const DEFAULT_RANGE: Duration = Duration::hours(24);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/devices/{device_id}/latest", get(latest_for_device))
        .route("/devices/{device_id}/readings", get(readings_in_range))
        .route("/readings/latest", get(latest_all))
}

async fn latest_for_device(
    State(storage): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Response, ApiError> {
    let reading = storage.latest_reading(&device_id).await?;
    Ok(match reading {
        Some(reading) => Json(reading).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    })
}

#[derive(Debug, Deserialize)]
pub struct RangeQuery {
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
}

/// Defaults to the last 24 hours when `from`/`to` are omitted, so a bare
/// `GET /devices/{id}/readings` can't return an unbounded result set.
async fn readings_in_range(
    State(storage): State<AppState>,
    Path(device_id): Path<String>,
    Query(range): Query<RangeQuery>,
) -> Result<Response, ApiError> {
    let to = range.to.unwrap_or_else(Utc::now);
    let from = range.from.unwrap_or(to - DEFAULT_RANGE);
    let readings = storage.readings_in_range(&device_id, from, to).await?;
    Ok(Json(readings).into_response())
}

async fn latest_all(State(storage): State<AppState>) -> Result<Response, ApiError> {
    let readings = storage.latest_readings_all().await?;
    Ok(Json(readings).into_response())
}
