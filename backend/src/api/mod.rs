mod devices;
mod health;
mod readings;

use std::sync::Arc;

use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::health::ScannerHealth;
use crate::storage::{SqliteStorage, StorageError};

pub type AppState = Arc<SqliteStorage>;

pub fn router(storage: AppState, scanner_health: Arc<ScannerHealth>) -> Router {
    Router::new()
        .merge(devices::routes())
        .merge(readings::routes())
        .with_state(storage)
        .merge(health::routes().with_state(scanner_health))
}

/// Wraps a [`StorageError`] surfaced from a handler into a generic 500
/// response; the underlying error is logged, not exposed to the client.
pub struct ApiError(StorageError);

impl From<StorageError> for ApiError {
    fn from(err: StorageError) -> Self {
        Self(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::error!(error = %self.0, "request failed");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use chrono::{Duration, Utc};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;
    use crate::domain::NewReading;

    async fn test_app() -> (Router, AppState) {
        let (app, storage, _health) = test_app_with_health(Duration::seconds(5)).await;
        (app, storage)
    }

    /// `process_age` is how long ago the process started, which decides
    /// whether `/health` is still inside its startup grace period.
    async fn test_app_with_health(process_age: Duration) -> (Router, AppState, Arc<ScannerHealth>) {
        let storage = Arc::new(SqliteStorage::connect("sqlite::memory:").await.unwrap());
        let health = Arc::new(ScannerHealth::new(
            Utc::now() - process_age,
            Duration::seconds(600),
        ));
        (
            router(Arc::clone(&storage), Arc::clone(&health)),
            storage,
            health,
        )
    }

    async fn get_health(app: Router) -> Response {
        app.oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn body_json(response: Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn listing_devices_on_an_empty_database_returns_an_empty_array() {
        let (app, _storage) = test_app().await;

        let response = app
            .oneshot(Request::get("/devices").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await, json!([]));
    }

    #[tokio::test]
    async fn updating_an_unknown_device_returns_404() {
        let (app, _storage) = test_app().await;

        let response = app
            .oneshot(
                Request::put("/devices/unknown")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "label": "Cucina" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn labeling_a_known_device_updates_it_and_it_shows_up_in_the_listing() {
        let (app, storage) = test_app().await;
        storage
            .upsert_device_seen("AA:BB", None, Utc::now())
            .await
            .unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::put("/devices/AA:BB")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "label": "Cucina", "room": "Cucina" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await["label"], "Cucina");

        let response = app
            .oneshot(Request::get("/devices").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let devices = body_json(response).await;
        assert_eq!(devices[0]["label"], "Cucina");
    }

    #[tokio::test]
    async fn sending_an_empty_label_clears_it() {
        let (app, storage) = test_app().await;
        storage
            .upsert_device_seen("AA:BB", None, Utc::now())
            .await
            .unwrap();
        storage
            .set_device_label("AA:BB", Some(Some("Cucina")), None, None)
            .await
            .unwrap();

        let response = app
            .oneshot(
                Request::put("/devices/AA:BB")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({ "label": "" }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response).await["label"], Value::Null);
    }

    #[tokio::test]
    async fn latest_reading_for_a_device_with_no_readings_is_404() {
        let (app, storage) = test_app().await;
        storage
            .upsert_device_seen("AA:BB", None, Utc::now())
            .await
            .unwrap();

        let response = app
            .oneshot(
                Request::get("/devices/AA:BB/latest")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn latest_readings_endpoint_returns_stored_readings() {
        let (app, storage) = test_app().await;
        let now = Utc::now();
        storage.upsert_device_seen("AA:BB", None, now).await.unwrap();
        storage
            .insert_reading(&NewReading {
                device_id: "AA:BB".to_string(),
                temperature: 23.5,
                humidity: 67.0,
                battery: Some(90),
                recorded_at: now,
            })
            .await
            .unwrap();

        let response = app
            .oneshot(
                Request::get("/readings/latest")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let readings = body_json(response).await;
        assert_eq!(readings[0]["temperature"], 23.5);
    }

    #[tokio::test]
    async fn health_is_ok_during_the_startup_grace_period_with_no_event_yet() {
        let (app, _storage, _health) = test_app_with_health(Duration::seconds(5)).await;

        let response = get_health(app).await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["healthy"], true);
        assert_eq!(body["last_event_at"], Value::Null);
    }

    #[tokio::test]
    async fn health_is_503_when_the_scanner_has_been_silent_past_the_grace_period() {
        let (app, _storage, health) = test_app_with_health(Duration::hours(1)).await;
        health.record_session_ended(Some("adapter not found".to_string()));

        let response = get_health(app).await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = body_json(response).await;
        assert_eq!(body["healthy"], false);
        assert_eq!(body["session_state"], "retrying");
        assert_eq!(body["last_error"], "adapter not found");
        assert_eq!(body["consecutive_failures"], 1);
    }

    #[tokio::test]
    async fn health_is_ok_for_an_old_process_that_is_still_receiving_events() {
        let (app, _storage, health) = test_app_with_health(Duration::hours(1)).await;
        health.record_scan_started();
        health.record_event();

        let response = get_health(app).await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["session_state"], "scanning");
        assert_ne!(body["last_event_at"], Value::Null);
    }
}
