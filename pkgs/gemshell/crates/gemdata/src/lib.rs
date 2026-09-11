//! `gemdata` — the system-data abstraction for gemshell.
//!
//! The UI (shell status bar, settings panel, and anything else) never
//! talks to `nmcli`/`bluetoothctl`/`wpctl`/sysfs directly. It holds a
//! `&dyn DataProvider` and reads snapshots / issues mutations through
//! this one trait. Two crates implement it:
//!
//! * `gemdata-device` — the real thing (nmcli, bluetoothctl, wpctl,
//!   sysfs); each responsibility is a small pure parser plus a thin
//!   command wrapper, so the parsing is unit-testable on its own.
//! * `gemdata-dummy` — an in-memory fake used by the nested x86_64 dev
//!   loop (`GEMSHELL_NESTED=1`), so the UI can be built and exercised
//!   without touching the workstation's network/audio.
//!
//! Keeping the trait here (and free of UI/GL/wayland types) means both
//! implementations are testable with plain `cargo test`.

use std::fmt;

/// An error returned by a mutating operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataError(pub String);

impl DataError {
    pub fn new(msg: impl Into<String>) -> Self {
        DataError(msg.into())
    }
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DataError {}

pub type Result<T> = std::result::Result<T, DataError>;

// ---------------------------------------------------------------------------
// Wi-Fi

/// One visible access point.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WifiNetwork {
    pub ssid: String,
    /// 0..=100
    pub signal: i32,
    /// `"open"` for an open network, otherwise the security description
    /// (e.g. `"WPA2"`, `"WPA1 WPA2"`).
    pub security: String,
    /// This is the network the device is currently associated with.
    pub connected: bool,
}

impl WifiNetwork {
    pub fn is_open(&self) -> bool {
        self.security.eq_ignore_ascii_case("open") || self.security.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WifiState {
    pub enabled: bool,
    /// SSID of the active connection, if any.
    pub active: Option<String>,
    pub networks: Vec<WifiNetwork>,
}

// ---------------------------------------------------------------------------
// Bluetooth

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BtDevice {
    pub mac: String,
    pub name: String,
    pub connected: bool,
    pub paired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BtState {
    pub enabled: bool,
    pub discovering: bool,
    pub devices: Vec<BtDevice>,
}

// ---------------------------------------------------------------------------
// Audio

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AudioSink {
    /// PipeWire node id (as a decimal string, the way `wpctl` wants it).
    pub id: String,
    pub name: String,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioState {
    /// 0.0..=1.0 (or more, PipeWire allows >1.0; clamped by the UI).
    pub volume: f32,
    pub muted: bool,
    pub sinks: Vec<AudioSink>,
}

impl Default for AudioState {
    fn default() -> Self {
        AudioState { volume: 0.0, muted: false, sinks: Vec::new() }
    }
}

// ---------------------------------------------------------------------------
// Battery / status bar

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BatteryState {
    /// percent 0..=100, `None` when unknown (no fuel gauge — the value
    /// is voltage-derived on this device).
    pub percent: Option<i32>,
    pub charging: bool,
}

/// Everything the status bar needs, in one snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShellStatus {
    pub battery: BatteryState,
    pub wifi: bool,
    pub bluetooth: bool,
    pub volume: i32,
    pub muted: bool,
    pub brightness: i32,
    pub brightness_max: i32,
    pub hour: u32,
    pub minute: u32,
}

// ---------------------------------------------------------------------------
// The trait

/// Read snapshots and issue mutations for the device subsystems gemshell
/// surfaces. Implementations must be cheap enough to call on the UI
/// thread for reads (the device implementation shells out; the status
/// poller caches results) and must not panic on a missing tool — they
/// degrade to `enabled: false` / empty lists.
pub trait DataProvider: Send + Sync {
    /// A single status-bar snapshot (battery, radio state, volume,
    /// brightness, clock).
    fn status(&self) -> ShellStatus;

    fn wifi(&self) -> WifiState;
    fn set_wifi_enabled(&self, enabled: bool) -> Result<()>;
    /// Force a rescan; the next `wifi()` reflects it (may block briefly).
    fn scan_wifi(&self) -> Result<()>;
    fn connect_wifi(&self, ssid: &str, password: Option<&str>) -> Result<()>;
    fn disconnect_wifi(&self) -> Result<()>;

    fn bluetooth(&self) -> BtState;
    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<()>;
    fn connect_bluetooth(&self, mac: &str) -> Result<()>;
    fn scan_bluetooth(&self) -> Result<()>;

    fn audio(&self) -> AudioState;
    fn set_default_sink(&self, id: &str) -> Result<()>;
    fn set_volume(&self, volume: f32) -> Result<()>;
    fn set_muted(&self, muted: bool) -> Result<()>;

    fn set_brightness(&self, percent: i32) -> Result<()>;
}
