//! Device telemetry for the HUD (the stress-test readout). Reads the
//! power-supply + thermal + cpu-hotplug sysfs nodes at ~2 Hz. All reads
//! are best-effort: a missing node just shows "-" (the demo must not die
//! over a sensor).

#[derive(Clone)]
pub struct Sensors {
    pub battery_present: bool,
    pub battery_pct: Option<i32>,
    pub battery_mv: Option<i32>,
    pub battery_ma: Option<i32>, // negative = discharging
    pub charger_state: String,
    pub charger_mv: Option<i32>,
    pub temp_mk: Option<i32>,
    pub cpus_online: Option<i32>,
}

impl Default for Sensors {
    fn default() -> Self {
        Sensors {
            battery_present: false,
            battery_pct: None,
            battery_mv: None,
            battery_ma: None,
            charger_state: String::from("-"),
            charger_mv: None,
            temp_mk: None,
            cpus_online: None,
        }
    }
}

fn read_int(path: &str) -> Option<i32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().split_whitespace().next()?.parse::<i32>().ok())
}

fn read_str(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

impl Sensors {
    pub fn poll(&mut self) {
        let b = "/sys/class/power_supply/battery";
        self.battery_present = std::path::Path::new(&format!("{b}/capacity")).exists();
        if self.battery_present {
            self.battery_pct = read_int(&format!("{b}/capacity"));
            self.battery_mv = read_int(&format!("{b}/voltage_now"));
            self.battery_ma = read_int(&format!("{b}/current_now")).map(|u| u / 1000);
        }
        let c = "/sys/class/power_supply/bq25890-charger-0";
        if let Some(st) = read_str(&format!("{c}/status")) {
            self.charger_state = st;
        }
        self.charger_mv = read_int(&format!("{c}/voltage_now"));
        self.temp_mk = read_int("/sys/class/thermal/thermal_zone0/temp")
            .or_else(|| read_int("/sys/class/thermal/thermal_zone1/temp"));
        if let Some(online) = read_str("/sys/devices/system/cpu/online") {
            // "0-7" or "0-3,5"
            let n = online.split(',').filter_map(|range| {
                let mut it = range.split('-');
                match (it.next(), it.next()) {
                    (Some(a), None) => a.parse::<i32>().ok().map(|v| v + 1),
                    (Some(a), Some(b)) => {
                        let lo = a.parse::<i32>().ok()?;
                        let hi = b.parse::<i32>().ok()?;
                        Some(hi - lo + 1)
                    }
                    _ => None,
                }
            }).sum::<i32>();
            self.cpus_online = Some(n);
        }
    }
}
