-- Make device identity adapter-independent.
--
-- btleplug's BlueZ PeripheralId stringifies as `hciN/dev_AA_BB_...`, so
-- the same physical meter seen through a different Bluetooth adapter used
-- to become a separate `devices` row (and its readings a separate series).
-- The collector now stores the id with the `hciN/` prefix stripped; this
-- rewrites existing rows to match, merging any duplicates.
--
-- Statement order keeps the `readings.device_id -> devices.device_id`
-- foreign key satisfied at every step (it is enforced by default):
--   1. create/merge the stripped `devices` rows (old prefixed rows still
--      exist, so readings still reference a valid parent);
--   2. repoint `readings` to the stripped ids (now that they exist);
--   3. drop the now-unreferenced prefixed `devices` rows.
--
-- Rows whose device_id has no `/` (e.g. a macOS dev database, where the
-- id is a bare CoreBluetooth UUID) are left untouched.

INSERT INTO devices (device_id, mac_address, label, room, blacklisted, first_seen_at, last_seen_at)
SELECT
    substr(device_id, instr(device_id, '/') + 1) AS target,
    MAX(mac_address),
    MAX(label),
    MAX(room),
    MAX(blacklisted),
    MIN(first_seen_at),
    MAX(last_seen_at)
FROM devices
WHERE instr(device_id, '/') > 0
GROUP BY target
ON CONFLICT (device_id) DO UPDATE SET
    mac_address  = COALESCE(devices.mac_address, excluded.mac_address),
    label        = COALESCE(devices.label, excluded.label),
    room         = COALESCE(devices.room, excluded.room),
    blacklisted  = MAX(devices.blacklisted, excluded.blacklisted),
    first_seen_at = MIN(devices.first_seen_at, excluded.first_seen_at),
    last_seen_at  = MAX(devices.last_seen_at, excluded.last_seen_at);

UPDATE readings
SET device_id = substr(device_id, instr(device_id, '/') + 1)
WHERE instr(device_id, '/') > 0;

DELETE FROM devices
WHERE instr(device_id, '/') > 0;
