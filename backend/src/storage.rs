use std::str::FromStr;

use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Connection, SqlitePool};

use crate::domain::{Device, NewReading, Reading};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

pub struct SqliteStorage {
    pool: SqlitePool,
}

impl SqliteStorage {
    pub async fn connect(database_url: &str) -> Result<Self, StorageError> {
        let is_memory = database_url.contains(":memory:");
        let mut options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
        if !is_memory {
            // The BLE scanner writes continuously while the API reads
            // concurrently from the same file; WAL lets readers and the
            // writer proceed without blocking each other (the default
            // rollback-journal mode would otherwise risk "database is
            // locked" errors under that access pattern).
            options = options.journal_mode(SqliteJournalMode::Wal);
        }
        // An in-memory database only persists for the lifetime of a single
        // connection, so the pool must be capped at one connection or
        // concurrent queries would each see a separate, empty database.
        let max_connections = if is_memory { 1 } else { 5 };
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Records that a device broadcast an advertisement, creating it on
    /// first sighting. Returns `None` if the device is blacklisted, since
    /// callers should skip storing a reading for it in that case.
    ///
    /// `mac_address` is best-effort and platform-dependent (real on BlueZ,
    /// unavailable on macOS/CoreBluetooth — see `docs/specs/architecture.md`
    /// §3). A `None` here never clears a previously recorded address.
    pub async fn upsert_device_seen(
        &self,
        device_id: &str,
        mac_address: Option<&str>,
        seen_at: DateTime<Utc>,
    ) -> Result<Option<Device>, StorageError> {
        sqlx::query(
            "INSERT INTO devices (device_id, mac_address, first_seen_at, last_seen_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (device_id) DO UPDATE SET
                 last_seen_at = excluded.last_seen_at,
                 mac_address = COALESCE(excluded.mac_address, devices.mac_address)",
        )
        .bind(device_id)
        .bind(mac_address)
        .bind(seen_at)
        .bind(seen_at)
        .execute(&self.pool)
        .await?;

        let device: Device = sqlx::query_as("SELECT * FROM devices WHERE device_id = ?")
            .bind(device_id)
            .fetch_one(&self.pool)
            .await?;

        Ok(if device.blacklisted {
            None
        } else {
            Some(device)
        })
    }

    /// Partially updates a device. `label`/`room` are tri-state: `None`
    /// leaves the field unchanged, `Some(None)` clears it, `Some(Some(v))`
    /// sets it. Runs as a `BEGIN IMMEDIATE` transaction so the read-modify-
    /// write can't lose an update to a concurrent call for the same device.
    pub async fn set_device_label(
        &self,
        device_id: &str,
        label: Option<Option<&str>>,
        room: Option<Option<&str>>,
        blacklisted: Option<bool>,
    ) -> Result<Option<Device>, StorageError> {
        let mut conn = self.pool.acquire().await?;
        let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;

        let existing: Option<Device> = sqlx::query_as("SELECT * FROM devices WHERE device_id = ?")
            .bind(device_id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(existing) = existing else {
            return Ok(None);
        };

        let label = match label {
            None => existing.label,
            Some(None) => None,
            Some(Some(v)) => Some(v.to_string()),
        };
        let room = match room {
            None => existing.room,
            Some(None) => None,
            Some(Some(v)) => Some(v.to_string()),
        };
        let blacklisted = blacklisted.unwrap_or(existing.blacklisted);

        sqlx::query("UPDATE devices SET label = ?, room = ?, blacklisted = ? WHERE device_id = ?")
            .bind(label)
            .bind(room)
            .bind(blacklisted)
            .bind(device_id)
            .execute(&mut *tx)
            .await?;

        let device: Device = sqlx::query_as("SELECT * FROM devices WHERE device_id = ?")
            .bind(device_id)
            .fetch_one(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(Some(device))
    }

    pub async fn list_devices(&self) -> Result<Vec<Device>, StorageError> {
        let devices = sqlx::query_as("SELECT * FROM devices ORDER BY device_id")
            .fetch_all(&self.pool)
            .await?;
        Ok(devices)
    }

    pub async fn insert_reading(&self, reading: &NewReading) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO readings (device_id, temperature, humidity, battery, recorded_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&reading.device_id)
        .bind(reading.temperature)
        .bind(reading.humidity)
        .bind(reading.battery)
        .bind(reading.recorded_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn latest_reading(&self, device_id: &str) -> Result<Option<Reading>, StorageError> {
        let reading = sqlx::query_as(
            "SELECT * FROM readings WHERE device_id = ? ORDER BY recorded_at DESC LIMIT 1",
        )
        .bind(device_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(reading)
    }

    /// Latest reading for every non-blacklisted device that has at least one.
    pub async fn latest_readings_all(&self) -> Result<Vec<Reading>, StorageError> {
        let readings = sqlx::query_as(
            "SELECT r.* FROM readings r
             INNER JOIN (
                 SELECT device_id, MAX(recorded_at) AS max_recorded_at
                 FROM readings
                 GROUP BY device_id
             ) latest
             ON r.device_id = latest.device_id AND r.recorded_at = latest.max_recorded_at
             INNER JOIN devices d ON d.device_id = r.device_id
             WHERE d.blacklisted = 0
             ORDER BY r.device_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(readings)
    }

    pub async fn readings_in_range(
        &self,
        device_id: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<Reading>, StorageError> {
        let readings = sqlx::query_as(
            "SELECT * FROM readings
             WHERE device_id = ? AND recorded_at >= ? AND recorded_at <= ?
             ORDER BY recorded_at ASC",
        )
        .bind(device_id)
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        Ok(readings)
    }

    /// Deletes readings older than `cutoff`. Returns how many rows were
    /// removed, for logging.
    pub async fn delete_readings_before(&self, cutoff: DateTime<Utc>) -> Result<u64, StorageError> {
        let result = sqlx::query("DELETE FROM readings WHERE recorded_at < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    async fn test_storage() -> SqliteStorage {
        SqliteStorage::connect("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn upserting_a_new_device_creates_it_unblacklisted() {
        let storage = test_storage().await;
        let now = Utc::now();

        let device = storage.upsert_device_seen("AA:BB", None, now).await.unwrap();

        let device = device.expect("newly seen device should not be blacklisted");
        assert_eq!(device.device_id, "AA:BB");
        assert_eq!(device.label, None);
        assert!(!device.blacklisted);
    }

    #[tokio::test]
    async fn upserting_a_known_device_updates_last_seen_without_resetting_label() {
        let storage = test_storage().await;
        let first_seen = Utc::now();
        storage
            .upsert_device_seen("AA:BB", None, first_seen)
            .await
            .unwrap();
        storage
            .set_device_label("AA:BB", Some(Some("Cucina")), None, None)
            .await
            .unwrap();

        let later = first_seen + Duration::seconds(30);
        let device = storage
            .upsert_device_seen("AA:BB", None, later)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(device.label.as_deref(), Some("Cucina"));
        assert_eq!(device.last_seen_at, later);
    }

    #[tokio::test]
    async fn upserting_records_the_mac_address_and_keeps_it_on_later_sightings_without_one() {
        let storage = test_storage().await;
        let first_seen = Utc::now();

        let device = storage
            .upsert_device_seen("AA:BB", Some("D2:2E:81:06:5C:61"), first_seen)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(device.mac_address.as_deref(), Some("D2:2E:81:06:5C:61"));

        let later = first_seen + Duration::seconds(30);
        let device = storage
            .upsert_device_seen("AA:BB", None, later)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(device.mac_address.as_deref(), Some("D2:2E:81:06:5C:61"));
    }

    #[tokio::test]
    async fn clearing_a_label_sets_it_to_null_without_touching_room() {
        let storage = test_storage().await;
        storage
            .upsert_device_seen("AA:BB", None, Utc::now())
            .await
            .unwrap();
        storage
            .set_device_label(
                "AA:BB",
                Some(Some("Cucina")),
                Some(Some("Piano terra")),
                None,
            )
            .await
            .unwrap();

        let device = storage
            .set_device_label("AA:BB", Some(None), None, None)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(device.label, None);
        assert_eq!(device.room.as_deref(), Some("Piano terra"));
    }

    #[tokio::test]
    async fn upserting_a_blacklisted_device_returns_none() {
        let storage = test_storage().await;
        let now = Utc::now();
        storage.upsert_device_seen("AA:BB", None, now).await.unwrap();
        storage
            .set_device_label("AA:BB", None, None, Some(true))
            .await
            .unwrap();

        let result = storage.upsert_device_seen("AA:BB", None, now).await.unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn setting_label_on_unknown_device_returns_none() {
        let storage = test_storage().await;

        let result = storage
            .set_device_label("unknown", Some(Some("x")), None, None)
            .await
            .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn readings_round_trip_and_filter_by_range() {
        let storage = test_storage().await;
        let now = Utc::now();
        storage.upsert_device_seen("AA:BB", None, now).await.unwrap();

        let reading = NewReading {
            device_id: "AA:BB".to_string(),
            temperature: 23.5,
            humidity: 67.0,
            battery: Some(90),
            recorded_at: now,
        };
        storage.insert_reading(&reading).await.unwrap();

        let latest = storage.latest_reading("AA:BB").await.unwrap().unwrap();
        assert_eq!(latest.temperature, 23.5);
        assert_eq!(latest.humidity, 67.0);
        assert_eq!(latest.battery, Some(90));

        let in_range = storage
            .readings_in_range(
                "AA:BB",
                now - Duration::minutes(1),
                now + Duration::minutes(1),
            )
            .await
            .unwrap();
        assert_eq!(in_range.len(), 1);

        let out_of_range = storage
            .readings_in_range(
                "AA:BB",
                now + Duration::minutes(1),
                now + Duration::minutes(2),
            )
            .await
            .unwrap();
        assert!(out_of_range.is_empty());
    }

    #[tokio::test]
    async fn latest_readings_all_excludes_blacklisted_devices() {
        let storage = test_storage().await;
        let now = Utc::now();
        storage.upsert_device_seen("AA:BB", None, now).await.unwrap();
        storage.upsert_device_seen("CC:DD", None, now).await.unwrap();
        storage
            .set_device_label("CC:DD", None, None, Some(true))
            .await
            .unwrap();

        for mac in ["AA:BB", "CC:DD"] {
            storage
                .insert_reading(&NewReading {
                    device_id: mac.to_string(),
                    temperature: 20.0,
                    humidity: 50.0,
                    battery: None,
                    recorded_at: now,
                })
                .await
                .unwrap();
        }

        let latest = storage.latest_readings_all().await.unwrap();

        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].device_id, "AA:BB");
    }

    #[tokio::test]
    async fn delete_readings_before_removes_only_older_rows() {
        let storage = test_storage().await;
        let now = Utc::now();
        storage.upsert_device_seen("AA:BB", None, now).await.unwrap();

        for age_days in [10, 5, 1] {
            storage
                .insert_reading(&NewReading {
                    device_id: "AA:BB".to_string(),
                    temperature: 20.0,
                    humidity: 50.0,
                    battery: None,
                    recorded_at: now - Duration::days(age_days),
                })
                .await
                .unwrap();
        }

        let deleted = storage
            .delete_readings_before(now - Duration::days(7))
            .await
            .unwrap();

        assert_eq!(deleted, 1);
        let remaining = storage
            .readings_in_range("AA:BB", now - Duration::days(30), now)
            .await
            .unwrap();
        assert_eq!(remaining.len(), 2);
    }
}
