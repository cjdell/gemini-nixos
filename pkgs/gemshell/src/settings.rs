//! gemsettings — the gemshell settings client (binary 2 of 2).
//!
//! A plain `xdg_toplevel` + `wl_shm` Wayland client (no toolkit): one
//! full-screen ARGB8888 buffer, CPU-drawn (rects + the shared font
//! atlas), redrawn on change. Three screens — Wi-Fi / Bluetooth /
//! Audio — driven by the standard system tools (`nmcli`,
//! `bluetoothctl`, `wpctl`), which is exactly what the compositor's
//! status-bar gear launches.
//!
//! Input: `wl_touch` (the compositor forwards taps as touch; there is
//! no mouse) plus `wl_keyboard` for text entry (Wi-Fi passwords) and
//! Esc/Enter. Keyboard keys are translated through the keymap the
//! compositor ships (xkbcommon).
//!
//! Design + receipts: docs/gemshell.md. Build: pkgs/gemshell.nix.

mod common;

use std::os::fd::AsFd;
use std::process::Command;

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_keyboard, wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
    wl_touch,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

use common::font::Font;
use common::util;

// ---------------------------------------------------------------------------
// geometry + theme

const W: i32 = 1080;
const H: i32 = 2160;
const STRIDE: i32 = W * 4;
const HEADER_H: i32 = 132;
const TAB_H: i32 = 120;
const ROW_H: i32 = 140;
const PAD: i32 = 28;

type Rgb = [u8; 3];
const BG: Rgb = [14, 18, 22];
const HEADER: Rgb = [22, 26, 32];
const CARD: Rgb = [30, 35, 42];
const CARD_HI: Rgb = [44, 51, 62];
const FG: Rgb = [235, 237, 242];
const MUTED: Rgb = [140, 145, 153];
const ACCENT: Rgb = [77, 158, 242];
const GREEN: Rgb = [89, 217, 115];
const DANGER: Rgb = [235, 100, 100];

fn rgb(c: Rgb) -> [u8; 4] {
    // wl_shm ARGB8888 on little-endian is [B, G, R, A]
    [c[2], c[1], c[0], 255]
}

// ---------------------------------------------------------------------------
// CPU painter

struct Painter {
    px: Vec<u8>,
}

impl Painter {
    fn new() -> Self {
        Painter { px: vec![0u8; (W * H * 4) as usize] }
    }

    fn blend(&mut self, x: i32, y: i32, c: Rgb, cov: u8) {
        if cov == 0 || x < 0 || y < 0 || x >= W || y >= H {
            return;
        }
        let o = ((y * W + x) * 4) as usize;
        let a = cov as u32;
        let ia = 255 - a;
        self.px[o] = ((c[2] as u32 * a + self.px[o] as u32 * ia) / 255) as u8;
        self.px[o + 1] = ((c[1] as u32 * a + self.px[o + 1] as u32 * ia) / 255) as u8;
        self.px[o + 2] = ((c[0] as u32 * a + self.px[o + 2] as u32 * ia) / 255) as u8;
        self.px[o + 3] = 255;
    }

    fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if x < 0 || y < 0 || x >= W || y >= H {
            return;
        }
        let o = ((y * W + x) * 4) as usize;
        let b = rgb(c);
        self.px[o..o + 4].copy_from_slice(&b);
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        for yy in y..(y + h).min(H) {
            for xx in x..(x + w).min(W) {
                self.set(xx, yy, c);
            }
        }
    }

    /// Rounded rectangle (radius r), filled.
    fn round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: Rgb) {
        let r = r.min(w / 2).min(h / 2).max(0);
        for yy in 0..h {
            for xx in 0..w {
                // corner test
                let inside = if xx < r && yy < r {
                    let dx = r - xx;
                    let dy = r - yy;
                    dx * dx + dy * dy <= r * r
                } else if xx >= w - r && yy < r {
                    let dx = xx - (w - r - 1);
                    let dy = r - yy;
                    dx * dx + dy * dy <= r * r
                } else if xx < r && yy >= h - r {
                    let dx = r - xx;
                    let dy = yy - (h - r - 1);
                    dx * dx + dy * dy <= r * r
                } else if xx >= w - r && yy >= h - r {
                    let dx = xx - (w - r - 1);
                    let dy = yy - (h - r - 1);
                    dx * dx + dy * dy <= r * r
                } else {
                    true
                };
                if inside {
                    self.set(x + xx, y + yy, c);
                }
            }
        }
    }

    fn font_text(&mut self, f: &Font, x: i32, baseline: i32, s: &str, c: Rgb) {
        let mut pen = x as f32;
        for ch in s.chars() {
            let Some(g) = f.glyph(ch) else {
                pen += f.size * 0.6;
                continue;
            };
            if g.w > 0 && g.h > 0 {
                let gx = (pen + g.x_off).round() as i32;
                let gy = (baseline as f32 - g.y_top).round() as i32;
                let ax = (g.u0 * f.w as f32) as u32;
                let ay = (g.v0 * f.h as f32) as u32;
                for yy in 0..g.h {
                    for xx in 0..g.w {
                        let so = (((ay + yy) * f.w + (ax + xx)) * 4 + 3) as usize;
                        let cov = f.pixels[so];
                        self.blend(gx + xx as i32, gy + yy as i32, c, cov);
                    }
                }
            }
            pen += g.advance;
        }
    }

    /// Centered text in [x, x+w).
    fn text_center(&mut self, f: &Font, x: i32, w: i32, baseline: i32, s: &str, c: Rgb) {
        let tw = f.text_width(s) as i32;
        self.font_text(f, x + (w - tw) / 2, baseline, s, c);
    }
}

// ---------------------------------------------------------------------------
// model + actions

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Wifi,
    Bluetooth,
    Audio,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Wifi => "Wi-Fi",
            Section::Bluetooth => "Bluetooth",
            Section::Audio => "Audio",
        }
    }
}

#[derive(Clone)]
struct Row {
    label: String,
    sub: String,
    /// right-side toggle state (None = no toggle)
    toggle: Option<bool>,
    /// highlighted (the active/default item)
    active: bool,
    action: Action,
}

#[derive(Clone)]
enum Action {
    Tab(Section),
    WifiToggle,
    WifiDisconnect,
    WifiSelect(String),
    BtToggle,
    BtSelect(String),
    BtScan,
    AudioDefault(String),
    VolUp,
    VolDown,
    Mute,
    PassSubmit,
    PassCancel,
    Close,
}

#[derive(Clone)]
struct Hit {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    action: Action,
}

// ---------------------------------------------------------------------------
// app state

struct App {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    seat: Option<wl_seat::WlSeat>,
    touch: Option<wl_touch::WlTouch>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    surface: Option<wl_surface::WlSurface>,
    xdg_surface: Option<xdg_surface::XdgSurface>,
    buffer: Option<wl_buffer::WlBuffer>,
    configured: bool,
    dirty: bool,
    shm_map: Option<memmap2::MmapMut>,

    painter: Painter,
    font: Font,
    hits: Vec<Hit>,
    touch_active: Option<(f64, f64, Action)>,
    touch_pos: (f64, f64),

    section: Section,
    rows: Vec<Row>,
    status: String,
    scroll: f32,

    // Wi-Fi
    wifi_radio: bool,
    wifi_current: String,
    // Bluetooth
    bt_powered: bool,
    // Audio
    volume: f32,
    muted: bool,
    // password entry
    password_for: Option<String>,
    password: String,

    xkb_ctx: Option<Xkb>,
}

/// xkbcommon state for decoding the compositor's keymap.
struct Xkb {
    state: xkbcommon::xkb::State,
}

impl App {
    fn new() -> Result<Self, String> {
        let font_path = util::find_font().ok_or_else(|| "no system TTF font found".to_string())?;
        let font = Font::load(font_path.to_str().unwrap(), 40.0, 2048)?;
        let mut app = App {
            compositor: None,
            shm: None,
            wm_base: None,
            seat: None,
            touch: None,
            keyboard: None,
            surface: None,
            xdg_surface: None,
            buffer: None,
            configured: false,
            dirty: true,
            shm_map: None,
            painter: Painter::new(),
            font,
            hits: Vec::new(),
            touch_active: None,
            touch_pos: (0.0, 0.0),
            section: Section::Wifi,
            rows: Vec::new(),
            status: String::new(),
            scroll: 0.0,
            wifi_radio: false,
            wifi_current: String::new(),
            bt_powered: false,
            volume: 1.0,
            muted: false,
            password_for: None,
            password: String::new(),
            xkb_ctx: None,
        };
        app.refresh();
        Ok(app)
    }

    // -----------------------------------------------------------------
    // system commands

    fn run(prog: &str, args: &[&str]) -> String {
        match Command::new(prog).args(args).output() {
            Ok(o) => {
                let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                s.push_str(&String::from_utf8_lossy(&o.stderr));
                s
            }
            Err(e) => format!("error: {e}"),
        }
    }

    fn refresh(&mut self) {
        match self.section {
            Section::Wifi => self.refresh_wifi(),
            Section::Bluetooth => self.refresh_bt(),
            Section::Audio => self.refresh_audio(),
        }
        self.dirty = true;
    }

    fn refresh_wifi(&mut self) {
        let radio = Self::run("nmcli", &["radio", "wifi"]);
        self.wifi_radio = radio.trim().eq_ignore_ascii_case("enabled");

        // active connection name
        self.wifi_current.clear();
        let active = Self::run("nmcli", &["-t", "-f", "NAME,TYPE", "connection", "show", "--active"]);
        for line in active.lines() {
            let mut it = line.splitn(2, ':');
            if let (Some(name), Some(ty)) = (it.next(), it.next()) {
                if ty.starts_with("802-11-wireless") {
                    self.wifi_current = unescape_nmcli(name);
                }
            }
        }

        let mut rows = Vec::new();
        rows.push(Row {
            label: "Wi-Fi".into(),
            sub: if self.wifi_radio { "radio on".into() } else { "radio off".into() },
            toggle: Some(self.wifi_radio),
            active: false,
            action: Action::WifiToggle,
        });
        if self.wifi_radio {
            let list = Self::run(
                "nmcli",
                &["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "device", "wifi", "list", "--rescan", "no"],
            );
            let mut seen = std::collections::HashSet::new();
            for line in list.lines() {
                // IN-USE:SSID:SIGNAL:SECURITY (nmcli escapes ':' in values)
                let parts: Vec<&str> = line.splitn(4, ':').collect();
                if parts.len() < 4 {
                    continue;
                }
                let in_use = parts[0].trim() == "*";
                let ssid = unescape_nmcli(parts[1]);
                if ssid.is_empty() || !seen.insert(ssid.clone()) {
                    continue;
                }
                let signal: i32 = parts[2].trim().parse().unwrap_or(0);
                let sec = parts[3].trim();
                let sec = if sec.is_empty() { "open" } else { sec };
                rows.push(Row {
                    label: ssid.clone(),
                    sub: format!("{signal}%  ·  {sec}"),
                    toggle: None,
                    active: in_use,
                    action: Action::WifiSelect(ssid),
                });
            }
            if self.wifi_current.is_empty() {
                self.status = "Not connected".into();
            } else {
                self.status = format!("Connected: {}", self.wifi_current);
            }
        } else {
            self.status = "Wi-Fi is off".into();
        }
        self.rows = rows;
    }

    fn refresh_bt(&mut self) {
        let show = Self::run("bluetoothctl", &["show"]);
        self.bt_powered = show.lines().any(|l| l.trim() == "Powered: yes");
        let mut rows = vec![Row {
            label: "Bluetooth".into(),
            sub: if self.bt_powered { "powered".into() } else { "off".into() },
            toggle: Some(self.bt_powered),
            active: false,
            action: Action::BtToggle,
        }];
        let devices = Self::run("bluetoothctl", &["devices"]);
        let mut count = 0;
        for line in devices.lines() {
            let mut it = line.splitn(3, ' ');
            let _ = it.next(); // "Device"
            let Some(mac) = it.next() else { continue };
            let name = it.next().unwrap_or(mac);
            count += 1;
            rows.push(Row {
                label: name.to_string(),
                sub: mac.to_string(),
                toggle: None,
                active: false,
                action: Action::BtSelect(mac.to_string()),
            });
        }
        rows.push(Row {
            label: "Scan".into(),
            sub: format!("{count} known device(s) — tap to scan 5 s"),
            toggle: None,
            active: false,
            action: Action::BtScan,
        });
        self.status = if self.bt_powered { "Bluetooth on".into() } else { "Bluetooth off".into() };
        self.rows = rows;
    }

    fn refresh_audio(&mut self) {
        let vol = Self::run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]);
        self.volume = vol
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        self.muted = vol.contains("MUTED");

        let mut rows = Vec::new();
        rows.push(Row {
            label: "Volume".into(),
            sub: format!("{}%{}", (self.volume * 100.0).round() as i32, if self.muted { " (muted)" } else { "" }),
            toggle: None,
            active: false,
            action: Action::VolUp,
        });
        // sinks from `wpctl status` (the "Sinks:" block)
        let status = Self::run("wpctl", &["status"]);
        let mut in_sinks = false;
        for line in status.lines() {
            let t = line.trim_end();
            if t.trim_start().starts_with("Sinks:") {
                in_sinks = true;
                continue;
            }
            if in_sinks {
                if t.trim().is_empty() {
                    break;
                }
                if let Some((id, name, def)) = parse_wpctl_sink(t) {
                    rows.push(Row {
                        label: name,
                        sub: format!("sink {id}"),
                        toggle: None,
                        active: def,
                        action: Action::AudioDefault(id),
                    });
                }
            }
        }
        rows.push(Row {
            label: "Mute".into(),
            sub: if self.muted { "muted".into() } else { "unmuted".into() },
            toggle: Some(self.muted),
            active: false,
            action: Action::Mute,
        });
        self.status = format!("Output {}%", (self.volume * 100.0).round() as i32);
        self.rows = rows;
    }

    // -----------------------------------------------------------------
    // actions

    fn activate(&mut self, action: Action) {
        match action {
            Action::Tab(s) => {
                self.section = s;
                self.refresh();
            }
            Action::Close => std::process::exit(0),
            Action::WifiToggle => {
                let on = !self.wifi_radio;
                Self::run("nmcli", &["radio", "wifi", if on { "on" } else { "off" }]);
                self.refresh();
            }
            Action::WifiDisconnect => {
                if !self.wifi_current.is_empty() {
                    let name = self.wifi_current.clone();
                    Self::run("nmcli", &["connection", "down", &name]);
                }
                self.refresh();
            }
            Action::WifiSelect(ssid) => {
                // Secured network -> ask for a password (keyboard input).
                if let Some(row) = self.rows.iter().find(|r| matches!(&r.action, Action::WifiSelect(s) if *s == ssid)) {
                    if row.sub.contains("open") {
                        self.run_wifi_connect(&ssid, None);
                    } else {
                        self.password_for = Some(ssid);
                        self.password.clear();
                        self.dirty = true;
                    }
                }
            }
            Action::PassSubmit => {
                if let Some(ssid) = self.password_for.clone() {
                    let pw = self.password.clone();
                    self.password_for = None;
                    self.password.clear();
                    self.run_wifi_connect(&ssid, Some(&pw));
                }
            }
            Action::PassCancel => {
                self.password_for = None;
                self.password.clear();
                self.dirty = true;
            }
            Action::BtToggle => {
                let on = !self.bt_powered;
                Self::run("bluetoothctl", &["power", if on { "on" } else { "off" }]);
                self.refresh();
            }
            Action::BtSelect(mac) => {
                self.status = format!("Connecting {mac}…");
                self.dirty = true;
                Self::run("bluetoothctl", &["connect", &mac]);
                self.refresh();
            }
            Action::BtScan => {
                self.status = "Scanning…".into();
                self.dirty = true;
                // btmon/scan is async; run a bounded scan
                let _ = Command::new("bluetoothctl").args(["--timeout", "5", "scan", "on"]).output();
                self.refresh();
            }
            Action::AudioDefault(id) => {
                Self::run("wpctl", &["set-default", &id]);
                self.refresh();
            }
            Action::VolUp => {
                Self::run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", "5%+"]);
                self.refresh();
            }
            Action::VolDown => {
                Self::run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"]);
                self.refresh();
            }
            Action::Mute => {
                Self::run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]);
                self.refresh();
            }
        }
    }

    fn run_wifi_connect(&mut self, ssid: &str, pw: Option<&str>) {
        self.status = format!("Connecting to {ssid}…");
        self.dirty = true;
        let out = match pw {
            Some(p) => Self::run("nmcli", &["device", "wifi", "connect", ssid, "password", p]),
            None => Self::run("nmcli", &["device", "wifi", "connect", ssid]),
        };
        let last = out.lines().last().unwrap_or("").to_string();
        self.status = if last.to_lowercase().contains("success") {
            format!("Connected to {ssid}")
        } else {
            format!("Failed: {}", last.chars().take(60).collect::<String>())
        };
        self.refresh();
    }

    fn key(&mut self, code: u32, pressed: bool) {
        let Some(xkb) = self.xkb_ctx.as_mut() else { return };
        use xkbcommon::xkb::{KeyDirection, Keycode, Keysym};
        // wl_keyboard.key is the raw evdev scancode; xkbcommon adds 8.
        let kc = Keycode::new(code + 8);
        let dir = if pressed { KeyDirection::Down } else { KeyDirection::Up };
        xkb.state.update_key(kc, dir);
        if !pressed {
            return;
        }
        let sym = xkb.state.key_get_one_sym(kc);
        if sym == Keysym::Escape {
            if self.password_for.is_some() {
                self.password_for = None;
                self.password.clear();
            } else {
                std::process::exit(0);
            }
            self.dirty = true;
        } else if sym == Keysym::Return || sym == Keysym::KP_Enter {
            if self.password_for.is_some() {
                self.activate(Action::PassSubmit);
            }
        } else if sym == Keysym::BackSpace {
            if self.password_for.is_some() {
                self.password.pop();
                self.dirty = true;
            }
        } else if self.password_for.is_some() {
            let s = xkb.state.key_get_utf8(kc);
            let printable: String = s.chars().filter(|c| !c.is_control()).collect();
            if !printable.is_empty() {
                self.password.push_str(&printable);
                self.dirty = true;
            }
        }
    }

    fn touch_down(&mut self, x: f64, y: f64) {
        self.touch_pos = (x, y);
        if let Some(action) = self.hit_test(x, y) {
            self.touch_active = Some((x, y, action));
        }
    }

    fn touch_motion(&mut self, x: f64, y: f64) {
        self.touch_pos = (x, y);
    }

    fn touch_up(&mut self) {
        let Some((dx, dy, action)) = self.touch_active.take() else { return };
        let (x, y) = self.touch_pos;
        // only fire when the finger stayed near the down point
        if ((x - dx).powi(2) + (y - dy).powi(2)).sqrt() < 40.0 {
            if let Some(a) = self.hit_test(x, y) {
                if std::mem::discriminant(&a) == std::mem::discriminant(&action) {
                    self.activate(a);
                }
            }
        }
    }

    fn hit_test(&self, x: f64, y: f64) -> Option<Action> {
        let (x, y) = (x as i32, y as i32);
        for h in self.hits.iter().rev() {
            if x >= h.x && x < h.x + h.w && y >= h.y && y < h.y + h.h {
                return Some(h.action.clone());
            }
        }
        None
    }

    // -----------------------------------------------------------------
    // drawing

    fn draw(&mut self) {
        self.hits.clear();
        let p = &mut self.painter;
        p.rect(0, 0, W, H, BG);
        // header
        p.rect(0, 0, W, HEADER_H, HEADER);
        p.font_text(&self.font, PAD, 86, "Settings", FG);
        // close button
        let cx = W - 96;
        p.round_rect(cx, 30, 68, 68, 16, CARD);
        p.font_text(&self.font, cx + 25, 78, "✕", FG);
        self.hits.push(Hit { x: cx, y: 30, w: 68, h: 68, action: Action::Close });

        // tabs
        let tab_y = HEADER_H;
        let tab_w = W / 3;
        for (i, s) in [Section::Wifi, Section::Bluetooth, Section::Audio].iter().enumerate() {
            let x = i as i32 * tab_w;
            let active = *s == self.section;
            p.rect(x, tab_y, tab_w, TAB_H, if active { CARD } else { HEADER });
            if active {
                p.rect(x, tab_y + TAB_H - 6, tab_w, 6, ACCENT);
            }
            p.text_center(&self.font, x, tab_w, tab_y + 78, s.title(), if active { FG } else { MUTED });
            self.hits.push(Hit { x, y: tab_y, w: tab_w, h: TAB_H, action: Action::Tab(*s) });
        }

        let mut y = HEADER_H + TAB_H + PAD;
        // status line
        if !self.status.is_empty() {
            p.font_text(&self.font, PAD, y + 34, &self.status, MUTED);
            y += 70;
        }
        for row in &self.rows {
            let r = row;
            let bg = if r.active { CARD_HI } else { CARD };
            p.round_rect(PAD, y, W - 2 * PAD, ROW_H - 12, 18, bg);
            p.font_text(&self.font, PAD + 28, y + 58, &r.label, FG);
            if !r.sub.is_empty() {
                p.font_text(&self.font, PAD + 28, y + 102, &r.sub, MUTED);
            }
            if let Some(on) = r.toggle {
                let tx = W - PAD - 28 - 110;
                let (col, knob) = if on { (GREEN, tx + 78) } else { (CARD_HI, tx + 8) };
                p.round_rect(tx, y + 34, 110, 60, 30, col);
                p.round_rect(knob, y + 34, 52, 60, 26, FG);
            }
            if r.active {
                p.rect(PAD, y, 8, ROW_H - 12, ACCENT);
            }
            self.hits.push(Hit {
                x: PAD,
                y,
                w: W - 2 * PAD,
                h: ROW_H - 12,
                action: r.action.clone(),
            });
            y += ROW_H;
        }

        // audio quick controls
        if self.section == Section::Audio {
            let by = H - 260;
            let bw = (W - 2 * PAD - 2 * 24) / 3;
            for (i, (label, act)) in [
                ("Vol −", Action::VolDown),
                ("Mute", Action::Mute),
                ("Vol +", Action::VolUp),
            ]
            .iter()
            .enumerate()
            {
                let x = PAD + i as i32 * (bw + 24);
                p.round_rect(x, by, bw, 120, 22, CARD_HI);
                p.text_center(&self.font, x, bw, by + 76, label, FG);
                self.hits.push(Hit { x, y: by, w: bw, h: 120, action: act.clone() });
            }
        }

        // password overlay
        if let Some(ssid) = self.password_for.clone() {
            p.rect(0, 0, W, H, [8, 10, 12]);
            let bx = PAD;
            let bw = W - 2 * PAD;
            let by = H / 2 - 260;
            p.round_rect(bx, by, bw, 520, 28, CARD);
            p.font_text(&self.font, bx + 40, by + 90, &format!("Password for “{ssid}”"), FG);
            p.round_rect(bx + 40, by + 140, bw - 80, 96, 16, BG);
            let shown = if self.password.is_empty() {
                "type on the keyboard…".to_string()
            } else {
                "•".repeat(self.password.chars().count())
            };
            let col = if self.password.is_empty() { MUTED } else { FG };
            p.font_text(&self.font, bx + 68, by + 202, &shown, col);
            let (ba, bb) = (bx + 40, bx + 40 + (bw - 80) / 2 + 20);
            let hw = (bw - 100) / 2;
            p.round_rect(ba, by + 300, hw, 110, 20, ACCENT);
            p.text_center(&self.font, ba, hw, by + 372, "Connect", FG);
            p.round_rect(bb, by + 300, hw, 110, 20, CARD_HI);
            p.text_center(&self.font, bb, hw, by + 372, "Cancel", FG);
            p.font_text(&self.font, bx + 40, by + 470, "Enter = connect   ·   Esc = cancel", MUTED);
            self.hits.push(Hit { x: ba, y: by + 300, w: hw, h: 110, action: Action::PassSubmit });
            self.hits.push(Hit { x: bb, y: by + 300, w: hw, h: 110, action: Action::PassCancel });
        }
    }

    fn upload(&mut self) {
        if let Some(buf) = self.buffer.as_ref() {
            if let Some(surf) = self.surface.as_ref() {
                surf.attach(Some(buf), 0, 0);
                surf.damage(0, 0, W, H);
                surf.commit();
            }
        }
        self.dirty = false;
    }

    fn redraw(&mut self) {
        self.draw();
        // copy the painter into the shm mapping
        if let Some(mmap) = self.shm_map.as_mut() {
            let n = self.painter.px.len().min(mmap.len());
            mmap[..n].copy_from_slice(&self.painter.px[..n]);
        }
        self.upload();
    }
}

/// Parse a `wpctl status` sink line:
/// `  *   41. Built-in Audio Analogue Stereo [vol: 0.40]` -> (id, name, default)
fn parse_wpctl_sink(line: &str) -> Option<(String, String, bool)> {
    let t = line.trim_start();
    let def = t.starts_with('*');
    let rest = t.trim_start_matches('*').trim_start();
    // "41. name…"
    let dot = rest.find('.')?;
    let id: String = rest[..dot].trim().parse().ok()?;
    let mut name = rest[dot + 1..].trim().to_string();
    if let Some(b) = name.find('[') {
        name = name[..b].trim().to_string();
    }
    Some((id, name, def))
}

fn unescape_nmcli(s: &str) -> String {
    // nmcli -t escapes ':' as '\:' and '\' as '\\'
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// wayland dispatch

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for App {
    fn event(
        _state: &mut Self,
        _: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

macro_rules! empty_dispatch {
    ($iface:ty) => {
        impl Dispatch<$iface, ()> for App {
            fn event(
                _state: &mut Self,
                _: &$iface,
                _: <$iface as wayland_client::Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    };
}

empty_dispatch!(wl_compositor::WlCompositor);
empty_dispatch!(wl_shm::WlShm);
empty_dispatch!(wl_shm_pool::WlShmPool);
empty_dispatch!(wl_buffer::WlBuffer);

impl Dispatch<wl_surface::WlSurface, ()> for App {
    fn event(
        _state: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            let caps = match capabilities {
                wayland_client::WEnum::Value(c) => c,
                _ => return,
            };
            if caps.contains(wl_seat::Capability::Touch) && state.touch.is_none() {
                state.touch = Some(seat.get_touch(qh, ()));
            }
            if caps.contains(wl_seat::Capability::Keyboard) && state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(qh, ()));
            }
        }
    }
}

impl Dispatch<wl_touch::WlTouch, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_touch::WlTouch,
        event: wl_touch::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_touch::Event::Down { x, y, .. } => state.touch_down(x, y),
            wl_touch::Event::Motion { x, y, .. } => state.touch_motion(x, y),
            wl_touch::Event::Up { .. } => state.touch_up(),
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { fd, size, .. } => {
                use std::io::Read;
                use xkbcommon::xkb;
                // The crate is built default-features=false, so
                // `Keymap::new_from_fd` (behind the `wayland` feature) is
                // unavailable — read the memfd text and use
                // new_from_string instead.
                let mut file = std::fs::File::from(fd);
                let mut raw = vec![0u8; size as usize];
                if file.read_exact(&mut raw).is_ok() {
                    let text = String::from_utf8_lossy(&raw).into_owned();
                    let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
                    if let Some(km) = xkb::Keymap::new_from_string(
                        &ctx,
                        text,
                        xkb::KEYMAP_FORMAT_TEXT_V1,
                        xkb::KEYMAP_COMPILE_NO_FLAGS,
                    ) {
                        state.xkb_ctx = Some(Xkb { state: xkb::State::new(&km) });
                    }
                }
            }
            wl_keyboard::Event::Key { key, state: ks, .. } => {
                use wayland_client::WEnum;
                let pressed = matches!(ks, WEnum::Value(wl_keyboard::KeyState::Pressed));
                state.key(key, pressed);
            }
            _ => {}
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for App {
    fn event(
        _state: &mut Self,
        wm: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for App {
    fn event(
        state: &mut Self,
        surf: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            surf.ack_configure(serial);
            state.configured = true;
            state.redraw();
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for App {
    fn event(
        _state: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Close => std::process::exit(0),
            xdg_toplevel::Event::Configure { width, height, .. } => {
                log::debug!("toplevel configure {width}x{height}");
            }
            _ => {}
        }
    }
}

// `shm_map` lives here rather than in App's literal field list so the
// memfd/mmapping can be created after the pool exists.
impl App {
    fn create_shm(&mut self, shm: &wl_shm::WlShm, qh: &QueueHandle<Self>) {
        let size = (W * H * 4) as usize;
        let file = match memfd(size) {
            Ok(f) => f,
            Err(e) => {
                log::error!("memfd: {e}");
                return;
            }
        };
        let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
        let buf = pool.create_buffer(0, W, H, STRIDE, wl_shm::Format::Argb8888, qh, ());
        let map = unsafe {
            memmap2::MmapOptions::new()
                .len(size)
                .map_mut(&file)
                .ok()
        };
        self.shm_map = map;
        self.buffer = Some(buf);
        // keep the pool + file alive by leaking them into the shm state
        // (process-lifetime objects; the compositor owns the buffer copy)
        std::mem::forget(pool);
        std::mem::forget(file);
    }
}

fn memfd(size: usize) -> Result<std::fs::File, std::io::Error> {
    use std::os::fd::FromRawFd;
    let name = std::ffi::CString::new("gemsettings-shm").unwrap();
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    file.set_len(size as u64)?;
    Ok(file)
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if let Err(e) = run() {
        eprintln!("gemsettings: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("connect: {e}"))?;
    let (globals, mut queue) = registry_queue_init(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = queue.handle();

    let mut app = App::new()?;
    let compositor = globals
        .bind::<wl_compositor::WlCompositor, _, _>(&qh, 1..=4, ())
        .map_err(|e| format!("wl_compositor: {e}"))?;
    let shm = globals
        .bind::<wl_shm::WlShm, _, _>(&qh, 1..=1, ())
        .map_err(|e| format!("wl_shm: {e}"))?;
    let wm_base = globals
        .bind::<xdg_wm_base::XdgWmBase, _, _>(&qh, 1..=1, ())
        .map_err(|e| format!("xdg_wm_base: {e}"))?;
    app.seat = globals.bind::<wl_seat::WlSeat, _, _>(&qh, 1..=5, ()).ok();

    app.compositor = Some(compositor.clone());
    let surface = compositor.create_surface(&qh, ());
    let xdg = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("Settings".into());
    toplevel.set_app_id("gemsettings".into());
    surface.commit();

    app.surface = Some(surface);
    app.xdg_surface = Some(xdg);
    app.wm_base = Some(wm_base);
    app.create_shm(&shm, &qh);

    // initial roundtrip so the seat capabilities + keymap arrive
    queue
        .blocking_dispatch(&mut app)
        .map_err(|e| format!("blocking_dispatch: {e}"))?;

    loop {
        if app.dirty && app.configured {
            app.redraw();
        }
        queue
            .blocking_dispatch(&mut app)
            .map_err(|e| format!("blocking_dispatch: {e}"))?;
    }
}
