use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Device {
    pub device_id: String,
    pub mac_address: Option<String>,
    pub label: Option<String>,
    pub room: Option<String>,
    pub blacklisted: bool,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}
