use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;

use super::{ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/devices", get(list_devices))
        .route("/devices/{device_id}", put(update_device))
}

async fn list_devices(State(storage): State<AppState>) -> Result<Response, ApiError> {
    let devices = storage.list_devices().await?;
    Ok(Json(devices).into_response())
}

/// `label`/`room`: omit the field to leave it unchanged, send an empty
/// string to clear it, or a non-empty string to set it.
#[derive(Debug, Deserialize)]
pub struct UpdateDeviceRequest {
    pub label: Option<String>,
    pub room: Option<String>,
    pub blacklisted: Option<bool>,
}

async fn update_device(
    State(storage): State<AppState>,
    Path(device_id): Path<String>,
    Json(body): Json<UpdateDeviceRequest>,
) -> Result<Response, ApiError> {
    let label = body.label.as_deref().map(|s| (!s.is_empty()).then_some(s));
    let room = body.room.as_deref().map(|s| (!s.is_empty()).then_some(s));

    let device = storage
        .set_device_label(&device_id, label, room, body.blacklisted)
        .await?;

    Ok(match device {
        Some(device) => Json(device).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    })
}
