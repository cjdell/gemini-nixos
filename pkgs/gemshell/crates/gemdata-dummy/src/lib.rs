//! `gemdata-dummy` — an in-memory [`gemdata::DataProvider`].
//!
//! Selected when `GEMSHELL_NESTED=1` (the x86_64 UI dev loop) so the
//! shell can be built and driven without touching the workstation's
//! network/audio. Mutations actually change the fake state, so toggles,
//! connect flows and the password prompt all round-trip realistically.

use std::sync::Mutex;

use gemdata::{
    AudioSink, AudioState, BatteryState, BtDevice, BtState, DataError, Result, ShellStatus,
    WifiNetwork, WifiState,
};

struct Inner {
    wifi_on: bool,
    active: Option<String>,
    networks: Vec<WifiNetwork>,
    bt_on: bool,
    bt_devices: Vec<BtDevice>,
    volume: f32,
    muted: bool,
    sinks: Vec<AudioSink>,
    battery: BatteryState,
    brightness: i32,
    brightness_max: i32,
}

pub struct DummyData {
    inner: Mutex<Inner>,
}

impl Default for DummyData {
    fn default() -> Self {
        Self::new()
    }
}

impl DummyData {
    pub fn new() -> Self {
        DummyData {
            inner: Mutex::new(Inner {
                wifi_on: true,
                active: Some("Gemini-Net".into()),
                networks: vec![
                    WifiNetwork {
                        ssid: "Gemini-Net".into(),
                        signal: 82,
                        security: "WPA2".into(),
                        connected: true,
                    },
                    WifiNetwork {
                        ssid: "Planet HQ".into(),
                        signal: 61,
                        security: "WPA2".into(),
                        connected: false,
                    },
                    WifiNetwork {
                        ssid: "Guest".into(),
                        signal: 44,
                        security: "open".into(),
                        connected: false,
                    },
                    WifiNetwork {
                        ssid: "Coffee:Corner".into(),
                        signal: 30,
                        security: "WPA1 WPA2".into(),
                        connected: false,
                    },
                ],
                bt_on: true,
                bt_devices: vec![
                    BtDevice {
                        mac: "AA:BB:CC:DD:EE:01".into(),
                        name: "Gemini Headset".into(),
                        connected: true,
                        paired: true,
                    },
                    BtDevice {
                        mac: "AA:BB:CC:DD:EE:02".into(),
                        name: "Keyboard K380".into(),
                        connected: false,
                        paired: true,
                    },
                ],
                volume: 0.42,
                muted: false,
                sinks: vec![
                    AudioSink {
                        id: "41".into(),
                        name: "Built-in Audio Analogue Stereo".into(),
                        default: true,
                    },
                    AudioSink {
                        id: "55".into(),
                        name: "gemini_speakers".into(),
                        default: false,
                    },
                ],
                battery: BatteryState { percent: Some(73), charging: false },
                brightness: 40,
                brightness_max: 100,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl gemdata::DataProvider for DummyData {
    fn status(&self) -> ShellStatus {
        let g = self.lock();
        let (h, m) = now_hm();
        ShellStatus {
            battery: g.battery,
            wifi: g.wifi_on,
            bluetooth: g.bt_on,
            volume: (g.volume * 100.0).round() as i32,
            muted: g.muted,
            brightness: g.brightness,
            brightness_max: g.brightness_max,
            hour: h,
            minute: m,
        }
    }

    fn wifi(&self) -> WifiState {
        let g = self.lock();
        WifiState {
            enabled: g.wifi_on,
            active: g.active.clone(),
            networks: if g.wifi_on { g.networks.clone() } else { Vec::new() },
        }
    }

    fn set_wifi_enabled(&self, enabled: bool) -> Result<()> {
        self.lock().wifi_on = enabled;
        Ok(())
    }

    fn scan_wifi(&self) -> Result<()> {
        // Nudge signal strengths a little so a rescan visibly does
        // *something*.
        let mut g = self.lock();
        for n in &mut g.networks {
            n.signal = (n.signal + 7).clamp(5, 99);
        }
        Ok(())
    }

    fn connect_wifi(&self, ssid: &str, password: Option<&str>) -> Result<()> {
        let mut g = self.lock();
        if let Some(n) = g.networks.iter().find(|n| n.ssid == ssid) {
            if !n.is_open() && password.is_none() {
                return Err(DataError::new("password required"));
            }
        } else {
            return Err(DataError::new(format!("no network {ssid}")));
        }
        for n in &mut g.networks {
            n.connected = n.ssid == ssid;
        }
        g.active = Some(ssid.to_string());
        Ok(())
    }

    fn disconnect_wifi(&self) -> Result<()> {
        let mut g = self.lock();
        g.active = None;
        for n in &mut g.networks {
            n.connected = false;
        }
        Ok(())
    }

    fn bluetooth(&self) -> BtState {
        let g = self.lock();
        BtState {
            enabled: g.bt_on,
            discovering: false,
            devices: if g.bt_on { g.bt_devices.clone() } else { Vec::new() },
        }
    }

    fn set_bluetooth_enabled(&self, enabled: bool) -> Result<()> {
        self.lock().bt_on = enabled;
        Ok(())
    }

    fn connect_bluetooth(&self, mac: &str) -> Result<()> {
        let mut g = self.lock();
        if !g.bt_devices.iter().any(|d| d.mac == mac) {
            return Err(DataError::new(format!("unknown device {mac}")));
        }
        for d in &mut g.bt_devices {
            d.connected = d.mac == mac;
        }
        Ok(())
    }

    fn scan_bluetooth(&self) -> Result<()> {
        let mut g = self.lock();
        let n = g.bt_devices.len() + 1;
        g.bt_devices.push(BtDevice {
            mac: format!("AA:BB:CC:DD:EE:{n:02X}"),
            name: format!("Discovered device {n}"),
            connected: false,
            paired: false,
        });
        Ok(())
    }

    fn audio(&self) -> AudioState {
        let g = self.lock();
        AudioState { volume: g.volume, muted: g.muted, sinks: g.sinks.clone() }
    }

    fn set_default_sink(&self, id: &str) -> Result<()> {
        let mut g = self.lock();
        for s in &mut g.sinks {
            s.default = s.id == id;
        }
        Ok(())
    }

    fn set_volume(&self, volume: f32) -> Result<()> {
        self.lock().volume = volume.clamp(0.0, 1.5);
        Ok(())
    }

    fn set_muted(&self, muted: bool) -> Result<()> {
        self.lock().muted = muted;
        Ok(())
    }

    fn set_brightness(&self, percent: i32) -> Result<()> {
        self.lock().brightness = percent.clamp(1, 100);
        Ok(())
    }
}

fn now_hm() -> (u32, u32) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let sod = (s % 86400) as u32;
    (sod / 3600, (sod % 3600) / 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gemdata::DataProvider;

    #[test]
    fn wifi_toggle_and_connect() {
        let d = DummyData::new();
        assert!(d.wifi().enabled);
        d.set_wifi_enabled(false).unwrap();
        assert!(!d.wifi().enabled);
        d.set_wifi_enabled(true).unwrap();

        // wrong password path
        assert!(d.connect_wifi("Planet HQ", None).is_err());
        d.connect_wifi("Planet HQ", Some("hunter2")).unwrap();
        assert_eq!(d.wifi().active.as_deref(), Some("Planet HQ"));
        assert!(d.wifi().networks.iter().find(|n| n.ssid == "Planet HQ").unwrap().connected);
        assert!(!d.wifi().networks.iter().find(|n| n.ssid == "Gemini-Net").unwrap().connected);

        d.disconnect_wifi().unwrap();
        assert_eq!(d.wifi().active, None);
    }

    #[test]
    fn open_network_needs_no_password() {
        let d = DummyData::new();
        d.connect_wifi("Guest", None).unwrap();
        assert_eq!(d.wifi().active.as_deref(), Some("Guest"));
    }

    #[test]
    fn audio_and_bt() {
        let d = DummyData::new();
        d.set_volume(0.8).unwrap();
        d.set_muted(true).unwrap();
        let a = d.audio();
        assert!((a.volume - 0.8).abs() < 1e-6 && a.muted);
        d.set_default_sink("55").unwrap();
        assert!(d.audio().sinks.iter().find(|s| s.id == "55").unwrap().default);

        d.connect_bluetooth("AA:BB:CC:DD:EE:02").unwrap();
        assert!(d.bluetooth().devices.iter().find(|x| x.mac.ends_with("02")).unwrap().connected);
    }
}
