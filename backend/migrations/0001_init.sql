CREATE TABLE devices (
    device_id TEXT PRIMARY KEY,
    label TEXT,
    room TEXT,
    blacklisted INTEGER NOT NULL DEFAULT 0,
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL
);

CREATE TABLE readings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id TEXT NOT NULL REFERENCES devices (device_id),
    temperature REAL NOT NULL,
    humidity REAL NOT NULL,
    battery INTEGER,
    recorded_at TEXT NOT NULL
);

CREATE INDEX idx_readings_device_time ON readings (device_id, recorded_at);
