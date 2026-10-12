pub struct Config {
    pub database_url: String,
    pub bind_address: String,
    /// Minimum seconds between two stored readings for the same device.
    /// `None` (the default, when unset) means no throttling: every parsed
    /// advertisement is stored.
    pub reading_interval_secs: Option<u64>,
    /// Days of readings to keep; older ones are periodically deleted.
    /// `None` (the default, when unset) means readings are kept forever.
    pub retention_days: Option<u64>,
    /// Which Bluetooth adapter to scan on. Matched as a substring against
    /// btleplug's adapter info string (e.g. `"hci1 (usb:v2357p0604d…)"`),
    /// so either an `hciN` name or a USB modalias fragment works. `None`
    /// (the default, when unset) means the first adapter found.
    pub ble_adapter: Option<String>,
    /// How long the scanner may go without seeing any BLE advertisement
    /// before `GET /health` reports 503. Also the startup grace period.
    pub health_max_silence_secs: u64,
}

const DEFAULT_HEALTH_MAX_SILENCE_SECS: u64 = 600;

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://switchbot-home.sqlite".to_string()),
            bind_address: std::env::var("BIND_ADDRESS")
                .unwrap_or_else(|_| "0.0.0.0:3000".to_string()),
            reading_interval_secs: env_positive_int("READING_INTERVAL_SECONDS"),
            retention_days: env_positive_int("RETENTION_DAYS"),
            ble_adapter: std::env::var("BLE_ADAPTER").ok().filter(|s| !s.is_empty()),
            health_max_silence_secs: health_max_silence_secs(),
        }
    }
}

/// `0` would make `/health` fail permanently, so unlike the other numeric
/// variables it is rejected rather than accepted.
fn health_max_silence_secs() -> u64 {
    match env_positive_int("HEALTH_MAX_SILENCE_SECONDS") {
        None => DEFAULT_HEALTH_MAX_SILENCE_SECS,
        Some(0) => panic!("HEALTH_MAX_SILENCE_SECONDS must be greater than 0"),
        Some(secs) => secs,
    }
}

/// Reads an optional env var as a non-negative integer, panicking with a
/// clear message if it's set but not a valid number — silently falling
/// back to "unset" on a typo would be a confusing footgun.
fn env_positive_int(key: &str) -> Option<u64> {
    let value = std::env::var(key).ok()?;
    Some(
        value
            .parse()
            .unwrap_or_else(|_| panic!("{key} must be a non-negative integer, got {value:?}")),
    )
}
