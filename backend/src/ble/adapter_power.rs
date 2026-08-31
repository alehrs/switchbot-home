//! Best-effort Bluetooth adapter power-cycle, used by the scanner's
//! stall-recovery path (`ble::scanner::run`).
//!
//! When a Realtek RTL8761 USB dongle wedges — stops delivering LE
//! advertisements while still reporting `UP` / `Discovering` — a
//! `StopDiscovery`/`StartDiscovery` cycle does not revive it, but
//! toggling the adapter's `Powered` property off then on (an HCI reset)
//! does. This talks to BlueZ over the system D-Bus, which the container
//! already has mounted.
//!
//! No-op on non-Linux: CoreBluetooth exposes no adapter power control.

#[cfg(target_os = "linux")]
pub use imp::power_cycle;

#[cfg(not(target_os = "linux"))]
pub async fn power_cycle(_adapter_hint: Option<&str>) {}

#[cfg(target_os = "linux")]
mod imp {
    use std::time::Duration;

    use bluez_async::{BluetoothError, BluetoothSession};
    use tracing::{info, warn};

    /// Toggle the target adapter's `Powered` off → on. `adapter_hint` is
    /// the `BLE_ADAPTER` value (an `hciN` name or a modalias fragment),
    /// matched the same way `scanner::adapter_matches` selects the adapter;
    /// `None` targets the first adapter. Failures are logged, not
    /// propagated — this is a best-effort recovery step.
    pub async fn power_cycle(adapter_hint: Option<&str>) {
        if let Err(err) = run(adapter_hint).await {
            warn!(error = %err, "Bluetooth adapter power-cycle failed");
        }
    }

    async fn run(adapter_hint: Option<&str>) -> Result<(), BluetoothError> {
        // `BluetoothSession::new` hands back a background dispatch task to
        // run for the session's lifetime; abort it once we're done.
        let (task, session) = BluetoothSession::new().await?;
        let task = tokio::spawn(task);
        let result = toggle(&session, adapter_hint).await;
        task.abort();
        result
    }

    async fn toggle(
        session: &BluetoothSession,
        adapter_hint: Option<&str>,
    ) -> Result<(), BluetoothError> {
        let adapters = session.get_adapters().await?;
        let target = match adapter_hint {
            // Never fall back to "some other adapter" when a specific one
            // was asked for and isn't present — power-cycling the wrong
            // dongle would be worse than doing nothing.
            Some(hint) => adapters
                .iter()
                .find(|a| format!("{} ({})", a.id, a.modalias).contains(hint)),
            None => adapters.first(),
        };
        let Some(target) = target else {
            warn!(hint = ?adapter_hint, "adapter power-cycle: no matching adapter");
            return Ok(());
        };

        let id = target.id.clone();
        info!(adapter = %id, "power-cycling the Bluetooth adapter");
        session.set_powered(&id, false).await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
        session.set_powered(&id, true).await?;
        // BlueZ needs a moment after power-on before StartDiscovery takes.
        tokio::time::sleep(Duration::from_secs(2)).await;
        Ok(())
    }
}
