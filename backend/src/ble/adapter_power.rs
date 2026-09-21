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
pub use imp::{PowerCycler, power_cycle};

#[cfg(not(target_os = "linux"))]
pub struct PowerCycler;

#[cfg(not(target_os = "linux"))]
pub async fn power_cycle(_cycler: &mut Option<PowerCycler>, _adapter_hint: Option<&str>) {}

#[cfg(target_os = "linux")]
mod imp {
    use std::time::Duration;

    use bluez_async::{BluetoothError, BluetoothSession};
    use tokio::task::JoinHandle;
    use tracing::{info, warn};

    /// A single, long-lived D-Bus session for recovery power-cycles.
    ///
    /// A session's dispatch future must stay alive for as long as the
    /// session is used. More importantly, creating one for every recovery
    /// attempt leaks system-bus connections when the adapter remains sick:
    /// BlueZ eventually rejects the process with its per-UID connection
    /// limit. Keep exactly one for the scanner's lifetime instead.
    pub struct PowerCycler {
        session: BluetoothSession,
        dispatch_task: JoinHandle<()>,
    }

    impl PowerCycler {
        async fn new() -> Result<Self, BluetoothError> {
            let (dispatch, session) = BluetoothSession::new().await?;
            let dispatch_task = tokio::spawn(async move {
                if let Err(err) = dispatch.await {
                    warn!(error = %err, "Bluetooth adapter power-cycle D-Bus task ended");
                }
            });
            Ok(Self {
                session,
                dispatch_task,
            })
        }
    }

    impl Drop for PowerCycler {
        fn drop(&mut self) {
            self.dispatch_task.abort();
        }
    }

    /// Toggle the target adapter's `Powered` off → on. The session is
    /// created only once, then reused for all later recovery attempts.
    /// `None` targets the first adapter. Failures are logged, not
    /// propagated — this is a best-effort recovery step.
    pub async fn power_cycle(cycler: &mut Option<PowerCycler>, adapter_hint: Option<&str>) {
        if cycler.is_none() {
            match PowerCycler::new().await {
                Ok(new_cycler) => *cycler = Some(new_cycler),
                Err(err) => {
                    warn!(error = %err, "Bluetooth adapter power-cycle connection failed");
                    return;
                }
            }
        }

        if let Err(err) = toggle(
            &cycler
                .as_ref()
                .expect("cycler was just initialized")
                .session,
            adapter_hint,
        )
        .await
        {
            warn!(error = %err, "Bluetooth adapter power-cycle failed");
        }
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
            Some(hint) => adapters.iter().find(|adapter| {
                super::super::scanner::adapter_matches(
                    &format!("{} ({})", adapter.id, adapter.modalias),
                    hint,
                )
            }),
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
