use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Reading {
    pub id: i64,
    pub device_id: String,
    pub temperature: f64,
    pub humidity: f64,
    pub battery: Option<i64>,
    pub recorded_at: DateTime<Utc>,
}

/// A reading parsed from a BLE advertisement, not yet persisted.
#[derive(Debug, Clone)]
pub struct NewReading {
    pub device_id: String,
    pub temperature: f64,
    pub humidity: f64,
    pub battery: Option<i64>,
    pub recorded_at: DateTime<Utc>,
}
