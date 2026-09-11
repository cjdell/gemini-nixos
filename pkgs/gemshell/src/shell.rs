//! The egui-powered shell UI.
//!
//! Why a UI library (2026-09-12): the hand-rolled glyph atlas kept
//! producing blank/misplaced text, and the settings client had to
//! hand-draw every widget. egui does font loading, shaping, layout,
//! scrolling, focus and text editing, and emits GPU-ready triangle
//! meshes — so all the heavy lifting is a library's job and the drawing
//! stays hardware accelerated (`compositor::render::Renderer::draw_egui`
//! uploads the atlas and draws the meshes with a GLES triangle shader).
//!
//! This module owns the *UI only*: it is handed a `&dyn DataProvider`
//! (see `gemdata`) and never shells out itself. The compositor owns the
//! egui input plumbing and the data snapshots.

use egui::{
    Align, Align2, Color32, Context, FontData, FontDefinitions, FontFamily, Layout, Margin,
    RichText, Rounding, ScrollArea, TextEdit, Vec2,
};
use gemdata::{AudioState, BtState, DataProvider, WifiState};

/// Logical points per scene pixel. The scene is 2160x1080 physical, so
/// the UI sees 1080x540 points — the right density for this panel.
pub const PPP: f32 = 2.0;

const BG: Color32 = Color32::from_rgb(18, 21, 26);
const CARD: Color32 = Color32::from_rgb(32, 37, 45);
const CARD_HI: Color32 = Color32::from_rgb(46, 54, 66);
const FG: Color32 = Color32::from_rgb(235, 237, 242);
const MUTED: Color32 = Color32::from_rgb(150, 156, 165);
const ACCENT: Color32 = Color32::from_rgb(77, 158, 242);
const GREEN: Color32 = Color32::from_rgb(89, 217, 115);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Wifi,
    Bluetooth,
    Audio,
    Display,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Wifi => "Wi-Fi",
            Section::Bluetooth => "Bluetooth",
            Section::Audio => "Audio",
            Section::Display => "Display",
        }
    }
}

/// Mutable settings-panel state, owned by the compositor across frames.
pub struct SettingsState {
    pub section: Section,
    /// SSID awaiting a password, if the password dialog is open.
    pub password_for: Option<String>,
    pub password: String,
    pub status: String,
    /// A mutation happened this frame; reload the snapshots afterwards.
    pub need_refresh: bool,
    /// The user hit Close.
    pub close: bool,

    // cached snapshots (refreshed on open / mutation / timer)
    pub wifi: WifiState,
    pub bt: BtState,
    pub audio: AudioState,
    /// Screen backlight, raw sysfs value + its max (Display tab).
    pub brightness: i32,
    pub brightness_max: i32,
    /// Current compositor UI scale (Display tab), and the value the user
    /// picked this frame (the compositor applies it).
    pub ui_scale: f32,
    pub requested_scale: Option<f32>,
}

impl Default for SettingsState {
    fn default() -> Self {
        SettingsState {
            section: Section::Wifi,
            password_for: None,
            password: String::new(),
            status: String::new(),
            need_refresh: true,
            close: false,
            wifi: WifiState::default(),
            bt: BtState::default(),
            audio: AudioState::default(),
            brightness: 0,
            brightness_max: 100,
            ui_scale: 1.0,
            requested_scale: None,
        }
    }
}

/// One frame of UI output for the GL renderer.
pub struct UiFrame {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    /// The user picked a new UI scale this frame (Settings > Display);
    /// the compositor applies it (`Compositor::set_ui_scale`).
    pub requested_scale: Option<f32>,
}

pub struct ShellUi {
    ctx: Context,
    pub state: SettingsState,
}

impl Default for ShellUi {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellUi {
    pub fn new() -> Self {
        let ctx = Context::default();
        install_fonts(&ctx);
        ctx.set_visuals(egui::Visuals::dark());
        ctx.set_pixels_per_point(PPP);
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(14.0, 9.0);
        ctx.set_style(style);
        ShellUi { ctx, state: SettingsState::default() }
    }

    /// Set egui's pixels-per-point. Includes the compositor UI scale so a
    /// 150/200% panel is rasterized at full resolution (crisp), not
    /// upscaled from the 100% atlas.
    pub fn set_ppp(&mut self, ppp: f32) {
        if ppp > 0.0 {
            self.ctx.set_pixels_per_point(ppp);
        }
    }

    /// Reload every snapshot from the provider (shell-outs; call on open,
    /// after a mutation, or on a slow timer — never per frame).
    pub fn refresh(&mut self, data: &dyn DataProvider, ui_scale: f32) {
        self.state.wifi = data.wifi();
        self.state.bt = data.bluetooth();
        self.state.audio = data.audio();
        let s = data.status();
        self.state.brightness = s.brightness;
        self.state.brightness_max = s.brightness_max;
        self.state.ui_scale = ui_scale;
        self.state.need_refresh = false;
    }

    /// Run one egui pass and return the GPU-ready meshes.
    pub fn run(&mut self, data: &dyn DataProvider, input: egui::RawInput) -> UiFrame {
        let Self { ctx, state } = self;
        let full = ctx.run(input, |ctx| build(ctx, data, state));
        if state.need_refresh {
            state.wifi = data.wifi();
            state.bt = data.bluetooth();
            state.audio = data.audio();
            let s = data.status();
            state.brightness = s.brightness;
            state.brightness_max = s.brightness_max;
            state.need_refresh = false;
        }
        let primitives = ctx.tessellate(full.shapes, full.pixels_per_point);
        let requested_scale = state.requested_scale.take();
        UiFrame { primitives, textures: full.textures_delta, requested_scale }
    }
}

// ---------------------------------------------------------------------------
// fonts

fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    // Prefer the device's DejaVu (set by services/gemshell.nix) so UK
    // glyphs and the £/° symbols are present; egui's bundled fonts are the
    // fallback so text never disappears.
    if let Some(path) = std::env::var_os("GEMSHELL_FONT") {
        if let Ok(bytes) = std::fs::read(&path) {
            fonts.font_data.insert("system".to_owned(), FontData::from_owned(bytes));
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "system".to_owned());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .push("system".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------------------
// the panel

fn build(ctx: &Context, data: &dyn DataProvider, st: &mut SettingsState) {
    let frame = egui::Frame::none()
        .fill(BG)
        .inner_margin(Margin::same(26.0));
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        header(ui, st);
        ui.add_space(2.0);
        tabs(ui, st);
        ui.add_space(12.0);
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match st.section {
                Section::Wifi => wifi_ui(ui, data, st),
                Section::Bluetooth => bt_ui(ui, data, st),
                Section::Audio => audio_ui(ui, data, st),
                Section::Display => display_ui(ui, data, st),
            });
    });

    if st.password_for.is_some() {
        password_dialog(ctx, data, st);
    }
}

fn header(ui: &mut egui::Ui, st: &mut SettingsState) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Settings").size(30.0).strong().color(FG));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button(RichText::new("Close").size(17.0)).clicked() {
                st.close = true;
            }
        });
    });
}

fn tabs(ui: &mut egui::Ui, st: &mut SettingsState) {
    ui.horizontal(|ui| {
        for s in [Section::Wifi, Section::Bluetooth, Section::Audio, Section::Display] {
            if ui
                .selectable_label(st.section == s, RichText::new(s.title()).size(19.0))
                .clicked()
            {
                st.section = s;
                st.need_refresh = true;
            }
        }
    });
}

fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).size(15.0).color(MUTED));
}

// ---------- Wi-Fi ----------

fn wifi_ui(ui: &mut egui::Ui, data: &dyn DataProvider, st: &mut SettingsState) {
    let mut on = st.wifi.enabled;
    if ui
        .checkbox(&mut on, RichText::new("Wi-Fi radio").size(19.0))
        .changed()
    {
        if let Err(e) = data.set_wifi_enabled(on) {
            st.status = e.to_string();
        }
        st.need_refresh = true;
    }

    ui.horizontal(|ui| {
        match &st.wifi.active {
            Some(name) => {
                ui.label(RichText::new(format!("Connected: {name}")).size(17.0).color(GREEN));
                if ui.button("Disconnect").clicked() {
                    let _ = data.disconnect_wifi();
                    st.need_refresh = true;
                }
            }
            None => muted(ui, if st.wifi.enabled { "Not connected" } else { "Wi-Fi is off" }),
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Scan").clicked() {
                st.status = "Scanning…".into();
                // `nmcli --rescan yes` blocks until it has results; the
                // panel is briefly unresponsive, then shows them.
                if let Err(e) = data.scan_wifi() {
                    st.status = e.to_string();
                }
                st.need_refresh = true;
            }
        });
    });

    if !st.status.is_empty() {
        muted(ui, st.status.clone());
    }
    ui.add_space(4.0);
    ui.separator();

    if !st.wifi.enabled {
        ui.add_space(8.0);
        muted(ui, "Turn the radio on to see networks.");
        return;
    }
    if st.wifi.networks.is_empty() {
        ui.add_space(8.0);
        muted(ui, "No networks found — tap Scan.");
        return;
    }

    for n in st.wifi.networks.clone() {
        let fill = if n.connected { CARD_HI } else { CARD };
        egui::Frame::none()
            .fill(fill)
            .rounding(Rounding::same(12.0))
            .inner_margin(Margin::symmetric(16.0, 12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let bar = signal_glyph(n.signal);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("{bar}  {}", n.ssid)).size(20.0).color(FG));
                        muted(ui, format!("{}%  ·  {}", n.signal, n.security));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if n.connected {
                            ui.label(RichText::new("connected").size(16.0).color(GREEN));
                        } else if ui.button("Connect").clicked() {
                            if n.is_open() {
                                if let Err(e) = data.connect_wifi(&n.ssid, None) {
                                    st.status = e.to_string();
                                }
                                st.need_refresh = true;
                            } else {
                                st.password_for = Some(n.ssid.clone());
                                st.password.clear();
                            }
                        }
                    });
                });
            });
        ui.add_space(6.0);
    }
}

fn signal_glyph(signal: i32) -> &'static str {
    match signal {
        s if s >= 75 => "▂▄▆█",
        s if s >= 50 => "▂▄▆ ",
        s if s >= 25 => "▂▄  ",
        _ => "▂   ",
    }
}

// ---------- Bluetooth ----------

fn bt_ui(ui: &mut egui::Ui, data: &dyn DataProvider, st: &mut SettingsState) {
    let mut on = st.bt.enabled;
    if ui
        .checkbox(&mut on, RichText::new("Bluetooth").size(19.0))
        .changed()
    {
        if let Err(e) = data.set_bluetooth_enabled(on) {
            st.status = e.to_string();
        }
        st.need_refresh = true;
    }
    ui.horizontal(|ui| {
        muted(ui, format!("{} known device(s)", st.bt.devices.len()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Scan (5 s)").clicked() {
                if let Err(e) = data.scan_bluetooth() {
                    st.status = e.to_string();
                }
                st.need_refresh = true;
            }
        });
    });
    if !st.status.is_empty() {
        muted(ui, st.status.clone());
    }
    ui.add_space(4.0);
    ui.separator();

    if !st.bt.enabled {
        ui.add_space(8.0);
        muted(ui, "Bluetooth is off.");
        return;
    }
    for d in st.bt.devices.clone() {
        let fill = if d.connected { CARD_HI } else { CARD };
        egui::Frame::none()
            .fill(fill)
            .rounding(Rounding::same(12.0))
            .inner_margin(Margin::symmetric(16.0, 12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&d.name).size(19.0).color(FG));
                        muted(ui, d.mac.clone());
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if d.connected {
                            ui.label(RichText::new("connected").size(16.0).color(GREEN));
                        } else if ui.button("Connect").clicked() {
                            if let Err(e) = data.connect_bluetooth(&d.mac) {
                                st.status = e.to_string();
                            }
                            st.need_refresh = true;
                        }
                    });
                });
            });
        ui.add_space(6.0);
    }
}

// ---------- Audio ----------

fn audio_ui(ui: &mut egui::Ui, data: &dyn DataProvider, st: &mut SettingsState) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Volume").size(19.0).color(FG));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            muted(ui, format!("{}%", (st.audio.volume * 100.0).round() as i32));
        });
    });
    let mut vol = st.audio.volume.clamp(0.0, 1.0);
    if ui
        .add(egui::Slider::new(&mut vol, 0.0..=1.0).show_value(false))
        .changed()
    {
        st.audio.volume = vol;
        let _ = data.set_volume(vol);
    }
    let mut muted_flag = st.audio.muted;
    if ui.checkbox(&mut muted_flag, "Mute").changed() {
        st.audio.muted = muted_flag;
        let _ = data.set_muted(muted_flag);
    }
    ui.add_space(4.0);
    ui.separator();
    ui.label(RichText::new("Output").size(19.0).color(FG));
    if st.audio.sinks.is_empty() {
        muted(ui, "No output devices found.");
    }
    for s in st.audio.sinks.clone() {
        let selected = s.default;
        // Prefer the human description; fall back to the node name. The
        // device impl resolves `gemini_speakers` -> "Built-in Speakers".
        let label = s.name.clone();
        if ui.radio(selected, RichText::new(label).size(18.0)).clicked() && !selected {
            // The amp actually flips in `gemini-speakerd`, which follows
            // the PipeWire default sink (services/gemini-pda.nix); the
            // panel only has to select the sink.
            if let Err(e) = data.set_default_sink(&s.id) {
                st.status = e.to_string();
            }
            st.audio = data.audio();
        }
    }
    ui.add_space(6.0);
    muted(ui, "Internal speakers and the 3.5 mm jack share the codec outputs; picking a sink switches the speaker amp.");
}

// ---------- Display ----------

fn display_ui(ui: &mut egui::Ui, data: &dyn DataProvider, st: &mut SettingsState) {
    // ---- UI scale ----
    ui.label(RichText::new("Interface scale").size(19.0).color(FG));
    ui.horizontal(|ui| {
        for (label, value) in [("100%", 1.0f32), ("150%", 1.5), ("200%", 2.0)] {
            let selected = (st.ui_scale - value).abs() < 0.01;
            if ui.selectable_label(selected, RichText::new(label).size(18.0)).clicked() && !selected {
                st.ui_scale = value;
                st.requested_scale = Some(value);
            }
        }
    });
    muted(ui, "Applies to the whole desktop (chrome, launcher and text).");
    ui.add_space(6.0);
    ui.separator();

    // ---- brightness ----
    let max = st.brightness_max.max(1);
    let mut pct = ((st.brightness.max(0) as i64 * 100) / max as i64).clamp(0, 100) as i32;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Screen brightness").size(19.0).color(FG));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            muted(ui, format!("{pct}%"));
        });
    });
    if ui
        .add(egui::Slider::new(&mut pct, 0..=100).suffix("%").show_value(false))
        .changed()
    {
        if let Err(e) = data.set_brightness(pct) {
            st.status = e.to_string();
        }
        st.brightness = pct * max / 100;
    }
    if !st.status.is_empty() {
        muted(ui, st.status.clone());
    }
    ui.add_space(6.0);
    muted(ui, "Keyboard: Fn+B / Fn+N dim / brighten, Fn+C / Fn+V volume.");
    ui.add_space(4.0);
    ui.separator();
    ui.label(RichText::new("Power").size(19.0).color(FG));
    muted(ui, "Silver button sleeps/wakes (gemini-sleepd).");
}

// ---------- password dialog ----------

fn password_dialog(ctx: &Context, data: &dyn DataProvider, st: &mut SettingsState) {
    let ssid = st.password_for.clone().unwrap_or_default();
    let mut open = true;
    egui::Window::new("Wi-Fi password")
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(RichText::new(format!("Password for “{ssid}”")).size(19.0));
            let resp = ui.add(
                TextEdit::singleline(&mut st.password)
                    .password(true)
                    .desired_width(420.0)
                    .hint_text("type on the keyboard…"),
            );
            resp.request_focus();
            // Enter submits (the on-screen Connect button also works).
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut connect = enter;
            ui.horizontal(|ui| {
                connect |= ui.button("Connect").clicked();
                if ui.button("Cancel").clicked() {
                    st.password_for = None;
                }
            });
            if connect {
                let pw = st.password.clone();
                if let Err(e) = data.connect_wifi(&ssid, Some(&pw)) {
                    st.status = e.to_string();
                }
                st.password_for = None;
                st.need_refresh = true;
            }
            ui.label(
                RichText::new("Enter = connect  ·  Esc = cancel")
                    .size(14.0)
                    .color(MUTED),
            );
        });
    if !open {
        st.password_for = None;
    }
}
