//! Status polling — a background thread reads the shared
//! [`gemdata::DataProvider`] snapshot every 2 s and pushes a UI-facing
//! [`Status`] to the compositor.
//!
//! The actual reads live in `gemdata-device` (nmcli / bluetoothctl /
//! wpctl / sysfs) so there is ONE implementation of "how do I read the
//! battery/Wi-Fi/Bluetooth" shared by the status bar, the egui settings
//! panel and gemcli. This module is just the poll loop plus the flat
//! struct the shell chrome (`ui.rs`) draws.
//!
//! Field notes: battery capacity is voltage-derived (no fuel gauge —
//! see docs/desktop-plumbing.md); volume is the PipeWire default sink
//! (0..=100); brightness is the sysfs backlight (0..=brightness_max).

use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use gemdata::{DataProvider, ShellStatus};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// percent, `None` when unknown
    pub battery: Option<i32>,
    pub charging: bool,
    pub wifi: bool,
    pub bluetooth: bool,
    pub volume: i32,
    pub muted: bool,
    pub brightness: i32,
    pub brightness_max: i32,
    pub hour: u32,
    pub minute: u32,
}

impl From<ShellStatus> for Status {
    fn from(s: ShellStatus) -> Self {
        Status {
            battery: s.battery.percent,
            charging: s.battery.charging,
            wifi: s.wifi,
            bluetooth: s.bluetooth,
            volume: s.volume,
            muted: s.muted,
            brightness: s.brightness,
            brightness_max: s.brightness_max,
            hour: s.hour,
            minute: s.minute,
        }
    }
}

/// One snapshot (blocking; called from the poller thread and once at
/// startup).
pub fn read(data: &dyn DataProvider) -> Status {
    data.status().into()
}

/// Spawn the poller thread; the receiver goes to the compositor.
pub fn spawn(data: Arc<dyn DataProvider>, tx: mpsc::Sender<Status>) {
    std::thread::Builder::new()
        .name("gemshell-status".into())
        .spawn(move || {
            let mut last = Instant::now();
            loop {
                let s = read(&*data);
                if tx.send(s).is_err() {
                    return;
                }
                last = Instant::now();
                std::thread::sleep(Duration::from_millis(2000).saturating_sub(last.elapsed()));
            }
        })
        .ok();
}
