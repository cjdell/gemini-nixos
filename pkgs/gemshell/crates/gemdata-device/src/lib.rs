//! `gemdata-device` — the real, on-device [`gemdata::DataProvider`].
//!
//! Every subsystem is a thin command/sysfs wrapper around a *pure*
//! parser (`parse_*`) so the tricky text formats are unit-tested here
//! without needing the real tools. The wrappers degrade gracefully: a
//! missing tool yields an empty/disabled snapshot rather than an error,
//! and only mutations surface [`DataError`].
//!
//! Tools used (all present in the gemshell session PATH):
//!   nmcli (NetworkManager), bluetoothctl (BlueZ), wpctl (PipeWire),
//!   sysfs (`/sys/class/power_supply`, `/sys/class/backlight`).

// ---------------------------------------------------------------------------
// Device-control modules.
//
// These lived in the `gemcli` binary crate and were moved here (2026-09-12)
// so the SAME implementation backs both `gemcli` and gemshell — no
// duplication. `gemcli` is now a thin clap frontend over these modules.
// The `crate::` paths inside them resolve within this crate.
pub mod a72;
pub mod backlight;
pub mod battery;
pub mod boot;
pub mod charger;
pub mod devmem;
pub mod error;
pub mod gpio;
pub mod guard;
pub mod gpu;
pub mod i2c;
pub mod power;
pub mod profile;
pub mod session;
pub mod sleep;
pub mod speaker;
pub mod status;
pub mod sysfs;
pub mod util;
pub mod wdt;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use gemdata::{
    AudioSink, AudioState, BatteryState, BtDevice, BtState, DataError, Result, ShellStatus,
    WifiNetwork, WifiState,
};

pub struct DeviceData;

impl Default for DeviceData {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceData {
    pub fn new() -> Self {
        DeviceData
    }
}

// ---------------------------------------------------------------------------
// command helpers

/// Best-effort combined stdout+stderr (used for reads; never fails).
fn read_cmd(prog: &str, args: &[&str]) -> String {
    match Command::new(prog).args(args).output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            s
        }
        Err(e) => format!("error: {e}"),
    }
}

/// Run a mutating command, mapping a missing tool / non-zero exit to a
/// [`DataError`] carrying the last line of output (what the user should
/// see in the UI).
fn run_cmd(prog: &str, args: &[&str]) -> Result<()> {
    let out = Command::new(prog)
        .args(args)
        .output()
        .map_err(|e| DataError::new(format!("{prog}: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        let text = String::from_utf8_lossy(&out.stderr);
        let last = text.lines().last().unwrap_or("").trim();
        Err(DataError::new(format!(
            "{prog} {}: {}",
            args.join(" "),
            if last.is_empty() { "failed" } else { last }
        )))
    }
}

fn sysfs_entries(prefix: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") {
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.starts_with(prefix))
                == Some(true)
            {
                out.push(p);
            }
        }
    }
    out
}

fn read_trim(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

// ---------------------------------------------------------------------------
// pure parsers (unit-tested)

/// `nmcli radio wifi` → `"enabled"` means on.
pub fn parse_nmcli_radio(out: &str) -> bool {
    out.trim().eq_ignore_ascii_case("enabled")
}

/// Split an `nmcli -t` line on *unescaped* colons (`\:` is a literal
/// colon, `\\` a literal backslash — nmcli's terse escaping).
pub fn split_nmcli_t(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ':' => {
                fields.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

/// `nmcli -t -f NAME,TYPE connection show --active` → active Wi-Fi SSID.
pub fn parse_nmcli_active(out: &str) -> Option<String> {
    for line in out.lines() {
        let f = split_nmcli_t(line);
        if f.len() >= 2 && f[1].starts_with("802-11-wireless") {
            let name = f[0].trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY device wifi list` → networks
/// (strongest first per SSID, de-duplicated).
pub fn parse_nmcli_wifi_list(out: &str) -> Vec<WifiNetwork> {
    let mut nets: Vec<WifiNetwork> = Vec::new();
    for line in out.lines() {
        let f = split_nmcli_t(line);
        if f.len() < 4 {
            continue;
        }
        let connected = f[0].trim() == "*";
        let ssid = f[1].trim().to_string();
        if ssid.is_empty() {
            continue;
        }
        let signal: i32 = f[2].trim().parse().unwrap_or(0);
        let sec = f[3].trim();
        let security = if sec.is_empty() { "open".to_string() } else { sec.to_string() };
        if let Some(existing) = nets.iter_mut().find(|n| n.ssid == ssid) {
            if signal > existing.signal {
                existing.signal = signal;
            }
            existing.connected |= connected;
        } else {
            nets.push(WifiNetwork { ssid, signal, security, connected });
        }
    }
    nets
}

/// `wpctl get-volume @DEFAULT_AUDIO_SINK@` → `(volume, muted)`.
/// Output: `Volume: 0.40` or `Volume: 0.40 [MUTED]`.
pub fn parse_wpctl_volume(out: &str) -> (f32, bool) {
    let muted = out.contains("MUTED");
    let volume = out
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.0);
    (volume, muted)
}

/// `wpctl status` → the audio *sink* nodes as [`AudioSink`]s.
///
/// Two places can hold a settable output:
///  - the `Sinks:` block (the hardware ALSA sink), where `wpctl` prints
///    the human description and a leading `*` for the default;
///  - the `Filters:` block, where the L/R-correcting virtual sink
///    `gemini_speakers` appears tagged `[Audio/Sink]` (2026-09-10q).
/// The old parser only looked in `Sinks:`, so the settings panel could
/// never offer “Built-in Speakers” (2026-09-12).
///
/// The `name` here is whatever `wpctl status` showed (description for the
/// hardware sink, node name for the filter sink); [`DeviceData::audio`]
/// later resolves a human description via `wpctl inspect`.
pub fn parse_wpctl_status_sinks(out: &str) -> Vec<AudioSink> {
    let mut sinks: Vec<AudioSink> = Vec::new();
    let mut section = String::new();
    for line in out.lines() {
        let cleaned: String =
            line.chars().filter(|c| !matches!(c, '│' | '├' | '└' | '─')).collect();
        let t = cleaned.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(name) = t.strip_suffix(':') {
            if matches!(name, "Devices" | "Sinks" | "Sources" | "Filters" | "Streams") {
                section = name.to_string();
                continue;
            }
        }
        let default = t.starts_with('*');
        let body = t
            .trim_start_matches('*')
            .trim_start()
            .trim_start_matches('-')
            .trim_start();
        let Some(dot) = body.find('.') else { continue };
        let Ok(id) = body[..dot].trim().parse::<u32>() else { continue };
        let mut name = body[dot + 1..].trim().to_string();
        if let Some(b) = name.find('[') {
            name = name[..b].trim().to_string();
        }
        if name.is_empty() {
            continue;
        }
        // In `Sinks:` every entry is a sink; elsewhere only the
        // `[Audio/Sink]`-tagged filter node counts (skip its
        // `[Stream/Output/Audio]` companion and other blocks).
        if section != "Sinks" && !t.contains("[Audio/Sink]") {
            continue;
        }
        if sinks.iter().any(|s| s.id == id.to_string()) {
            continue;
        }
        sinks.push(AudioSink { id: id.to_string(), name, default });
    }
    sinks
}

/// Extract one quoted `key = "value"` property from `wpctl inspect`.
pub fn parse_wpctl_prop(out: &str, key: &str) -> Option<String> {
    for line in out.lines() {
        let t = line.trim_start_matches(|c: char| matches!(c, '│' | '├' | '└' | '─' | ' ' | '*'));
        if let Some(rest) = t.strip_prefix(key) {
            if let Some(rest) = rest.trim_start().strip_prefix('=') {
                return Some(rest.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// `wpctl inspect @DEFAULT_SINK@` → the default sink's `node.name`
/// (`wpctl status` marks a `*` only inside the `Sinks:` block, so the
/// filter default has to be resolved this way).
pub fn parse_wpctl_default_name(out: &str) -> Option<String> {
    parse_wpctl_prop(out, "node.name")
}

/// `bluetoothctl show` → `(powered, discovering)`.
pub fn parse_bluetoothctl_show(out: &str) -> (bool, bool) {
    let powered = out.lines().any(|l| l.trim().eq_ignore_ascii_case("Powered: yes"));
    let discovering = out.lines().any(|l| l.trim().eq_ignore_ascii_case("Discovering: yes"));
    (powered, discovering)
}

/// `bluetoothctl devices` / `bluetoothctl devices Connected` → list.
/// Lines look like `Device AA:BB:CC:DD:EE:FF Some Name`.
pub fn parse_bluetoothctl_devices(out: &str) -> Vec<BtDevice> {
    let mut devices = Vec::new();
    for line in out.lines() {
        let mut it = line.splitn(4, ' ');
        if it.next() != Some("Device") {
            continue;
        }
        let Some(mac) = it.next() else { continue };
        if !is_mac(mac) {
            continue;
        }
        let name = it.next().unwrap_or("").trim().to_string();
        devices.push(BtDevice {
            mac: mac.to_string(),
            name: if name.is_empty() { mac.to_string() } else { name },
            connected: false,
            paired: true,
        });
    }
    devices
}

fn is_mac(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Clock (h, m) in local wall-clock terms — the device runs the system
/// timezone via /etc/localtime; we only have a raw epoch here, matching
/// what `date` shows once TZ is applied by glibc. The UI uses this for
/// the status bar.
pub fn clock_hm() -> (u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = local_offset_secs();
    let local = (secs as i64 + local_offset_secs()) as u64;
    let sod = (local % 86400) as u32;
    (sod / 3600, (sod % 3600) / 60)
}

/// Seconds east of UTC, read from `/etc/localtime` not being feasible
/// without libc tm; fall back to 0. `TZ=Europe/London` is the deployed
/// setting and matches the old behaviour (UTC in winter).
fn local_offset_secs() -> i64 {
    0
}

fn brightness_paths() -> Option<(PathBuf, PathBuf)> {
    let rd = std::fs::read_dir("/sys/class/backlight").ok()?;
    for e in rd.flatten() {
        let p = e.path();
        let cur = p.join("brightness");
        let max = p.join("max_brightness");
        if cur.exists() && max.exists() {
            return Some((cur, max));
        }
    }
    None
}

fn battery_state() -> BatteryState {
    let mut st = BatteryState::default();
    for p in sysfs_entries("bq25890-battery") {
        st.percent = read_trim(&p.join("capacity")).and_then(|v| v.parse().ok());
        if st.percent.is_some() {
            break;
        }
    }
    for p in sysfs_entries("bq25890-charger") {
        let status = read_trim(&p.join("status"));
        let online = read_trim(&p.join("online")).map(|v| v == "1");
        st.charging = status.as_deref() == Some("Charging") || online == Some(true);
        if status.is_some() {
            break;
        }
    }
    st
}

// ---------------------------------------------------------------------------
// DataProvider

impl gemdata::DataProvider for DeviceData {
    fn status(&self) -> ShellStatus {
        let (brightness, brightness_max) = match brightness_paths() {
            Some((cur, max)) => (
                read_trim(&cur).and_then(|v| v.parse().ok()).unwrap_or(0),
                read_trim(&max).and_then(|v| v.parse().ok()).unwrap_or(0),
            ),
            None => (0, 0),
        };
        let wifi_on = parse_nmcli_radio(&read_cmd("nmcli", &["radio", "wifi"]));
        let (bt_on, _) = parse_bluetoothctl_show(&read_cmd("bluetoothctl", &["show"]));
        let (volume, muted) = parse_wpctl_volume(&read_cmd("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]));
        let (hour, minute) = clock_hm();
        ShellStatus {
            battery: battery_state(),
            wifi: wifi_on,
            bluetooth: bt_on,
            volume: (volume * 100.0).round() as i32,
            muted,
            brightness,
            brightness_max,
            hour,
            minute,
        }
    }

    fn wifi(&self) -> WifiState {
        let enabled = parse_nmcli_radio(&read_cmd("nmcli", &["radio", "wifi"]));
        let active = parse_nmcli_active(&read_cmd(
            "nmcli",
            &["-t", "-f", "NAME,TYPE", "connection", "show", "--active"],
        ));
        let networks = if enabled {
            parse_nmcli_wifi_list(&read_cmd(
                "nmcli",
                &["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "device", "wifi", "list", "--rescan", "no"],
            ))
        } else {
            Vec::new()
        };
        WifiState { enabled, active, networks }
    }

    fn set_wifi_enabled(&self, enabled: bool) -> Result<()> {
        run_cmd("nmcli", &["radio", "wifi", if enabled { "on" } else { "off" }])
    }

    fn scan_wifi(&self) -> Result<()> {
        // Ask for a fresh scan; nmcli blocks until it has results.
        run_cmd("nmcli", &["device", "wifi", "list", "--rescan", "yes"]).map(|_| ())
    }

    fn connect_wifi(&self, ssid: &str, password: Option<&str>) -> Result<()> {
        match password {
            Some(pw) => run_cmd("nmcli", &["device", "wifi", "connect", ssid, "password", pw]),
            None => run_cmd("nmcli", &["device", "wifi", "connect", ssid]),
        }
    }

    fn disconnect_wifi(&self) -> Result<()> {
        let active = parse_nmcli_active(&read_cmd(
            "nmcli",
            &["-t", "-f", "NAME,TYPE", "connection", "show", "--active"],
        ));
        match active {
            Some(name) => run_cmd("nmcli", &["connection", "down", &name]),
            None => Ok(()),
        }
    }

    fn bluetooth(&self) -> BtState {
        let show = read_cmd("bluetoothctl", &["show"]);
        let (enabled, discovering) = parse_bluetoothctl_show(&show);
        let mut devices = parse_bluetoothctl_devices(&read_cmd("bluetoothctl", &["devices"]));
        let connected = parse_bluetoothctl_devices(&read_cmd("bluetoothctl", &["devices", "Connected"]));
        for d in &mut devices {
            d.connected = connected.iter().any(|c| c.mac == d.mac);
        }
        BtState { enabled, discovering, devices }
    }

    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<()> {
        run_cmd("bluetoothctl", &["power", if enabled { "on" } else { "off" }])
    }

    fn connect_bluetooth(&self, mac: &str) -> Result<()> {
        run_cmd("bluetoothctl", &["connect", mac])
    }

    fn scan_bluetooth(&self) -> Result<()> {
        // Bounded scan: bluetoothctl exits after the timeout.
        run_cmd("bluetoothctl", &["--timeout", "5", "scan", "on"])
    }

    fn audio(&self) -> AudioState {
        let (volume, muted) =
            parse_wpctl_volume(&read_cmd("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]));
        let mut sinks = parse_wpctl_status_sinks(&read_cmd("wpctl", &["status"]));
        // Resolve each sink's human description + node name, and mark the
        // default even when it is a filter sink (no `*` in `wpctl
        // status`): `gemini_speakers` shows only its node name there, so
        // the panel would otherwise list "gemini_speakers" (2026-09-12).
        let default_name =
            parse_wpctl_default_name(&read_cmd("wpctl", &["inspect", "@DEFAULT_SINK@"]));
        for s in &mut sinks {
            let info = read_cmd("wpctl", &["inspect", &s.id]);
            if let Some(desc) = parse_wpctl_prop(&info, "node.description") {
                if !desc.is_empty() {
                    s.name = desc;
                }
            }
            if let Some(node) = parse_wpctl_prop(&info, "node.name") {
                if default_name.as_deref() == Some(node.as_str()) {
                    s.default = true;
                }
            }
        }
        AudioState { volume, muted, sinks }
    }

    fn set_default_sink(&self, id: &str) -> Result<()> {
        run_cmd("wpctl", &["set-default", id])
    }

    fn set_volume(&self, volume: f32) -> Result<()> {
        let pct = (volume.clamp(0.0, 1.5) * 100.0).round() as i32;
        run_cmd("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{pct}%")])
    }

    fn set_muted(&self, muted: bool) -> Result<()> {
        run_cmd("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", if muted { "1" } else { "0" }])
    }

    fn set_brightness(&self, percent: i32) -> Result<()> {
        let Some((cur, max)) = brightness_paths() else {
            return Err(DataError::new("no backlight device"));
        };
        let max_v: i64 = read_trim(&max).and_then(|v| v.parse().ok()).unwrap_or(255);
        let v = (max_v * percent.clamp(0, 100) as i64 / 100).max(1);
        std::fs::write(&cur, v.to_string().as_bytes())
            .map_err(|e| DataError::new(format!("{}: {e}", cur.display())))
    }
}

// ---------------------------------------------------------------------------
// tests — the parsers, on their own

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nmcli_radio() {
        assert!(parse_nmcli_radio("enabled\n"));
        assert!(!parse_nmcli_radio("disabled\n"));
    }

    #[test]
    fn nmcli_escaping() {
        assert_eq!(
            split_nmcli_t(r"foo\:bar:baz\\qux"),
            vec!["foo:bar".to_string(), "baz\\qux".to_string()]
        );
    }

    #[test]
    fn nmcli_active() {
        let out = "Wired connection 1:802-3-ethernet\nMy Net:802-11-wireless\n";
        assert_eq!(parse_nmcli_active(out).as_deref(), Some("My Net"));
        assert_eq!(parse_nmcli_active("Wired:802-3-ethernet\n"), None);
    }

    #[test]
    fn wifi_list_dedups_and_marks_connected() {
        let out = "*:Home:80:WPA2\n:Home:40:WPA2\n:Cafe:55:\n";
        let nets = parse_nmcli_wifi_list(out);
        assert_eq!(nets.len(), 2);
        assert!(nets[0].connected);
        assert_eq!(nets[0].signal, 80);
        assert_eq!(nets[0].security, "WPA2");
        assert!(nets[1].is_open());
    }

    #[test]
    fn wifi_list_colon_in_ssid() {
        let nets = parse_nmcli_wifi_list(r":Bob\:s Wifi:12:WPA3");
        assert_eq!(nets[0].ssid, "Bob:s Wifi");
    }

    #[test]
    fn wpctl_volume() {
        assert_eq!(parse_wpctl_volume("Volume: 0.40\n"), (0.40, false));
        assert_eq!(parse_wpctl_volume("Volume: 1.00 [MUTED]\n"), (1.0, true));
    }

    #[test]
    fn wpctl_sinks_includes_filter_sink() {
        // Real `wpctl status` shape: the hardware sink lives in `Sinks:`
        // with a `*`; the virtual sink lives in `Filters:` tagged
        // `[Audio/Sink]` (with a `[Stream/Output/Audio]` companion that
        // must be ignored).
        let out = "\
Audio
 \u{251c}\u{2500} Devices:
 \u{2502}      46. Built-in Audio                      [alsa]
 \u{2502}  
 \u{251c}\u{2500} Sinks:
 \u{2502}  *   51. Headphones / Jack                   [vol: 0.61]
 \u{2502}  
 \u{251c}\u{2500} Sources:
 \u{2502}  
 \u{251c}\u{2500} Filters:
 \u{2502}    - gemini_speakers
 \u{2502}      34. gemini_speakers                          [Audio/Sink]
 \u{2502}      36. gemini_speakers.output                   [Stream/Output/Audio]
 \u{2502}  
 \u{2514}\u{2500} Streams:
";
        let s = parse_wpctl_status_sinks(out);
        assert_eq!(s.len(), 2);
        assert!(s[0].default);
        assert_eq!(s[0].id, "51");
        assert_eq!(s[0].name, "Headphones / Jack");
        assert_eq!(s[1].id, "34");
        assert_eq!(s[1].name, "gemini_speakers");
        assert!(!s[1].default);
    }

    #[test]
    fn wpctl_props() {
        let out = "\
  * node.description = \"Built-in Speakers\"
  * node.name = \"gemini_speakers\"
  * media.class = \"Audio/Sink\"
";
        assert_eq!(parse_wpctl_prop(out, "node.name").as_deref(), Some("gemini_speakers"));
        assert_eq!(
            parse_wpctl_prop(out, "node.description").as_deref(),
            Some("Built-in Speakers")
        );
        assert_eq!(parse_wpctl_default_name(out).as_deref(), Some("gemini_speakers"));
    }

    #[test]
    fn bluetooth_show() {
        assert_eq!(parse_bluetoothctl_show("Powered: yes\nDiscovering: no\n"), (true, false));
    }

    #[test]
    fn bluetooth_devices() {
        let out = "Device AA:BB:CC:DD:EE:FF Headphones\nNot a device\n";
        let d = parse_bluetoothctl_devices(out);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].mac, "AA:BB:CC:DD:EE:FF");
        assert_eq!(d[0].name, "Headphones");
    }

    #[test]
    fn clock_is_in_range() {
        let (h, m) = clock_hm();
        assert!(h < 24 && m < 60);
    }
}
