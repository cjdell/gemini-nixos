//! The gemshell compositor core — the main loop, window management,
//! gestures and input. See docs/gemshell.md for the design.

pub mod gbm;
pub mod input;
pub mod render;
pub mod status;
pub mod ui;
pub mod wayland;

use std::collections::HashMap;
use std::os::fd::AsFd;
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::{mpsc, Arc};

use wayland_server::protocol::wl_compositor::WlCompositor;
use wayland_server::protocol::wl_keyboard::{KeyState, KeymapFormat, WlKeyboard};
use wayland_server::protocol::wl_output::WlOutput;
use wayland_server::protocol::wl_seat::WlSeat;
use wayland_server::protocol::wl_shm::WlShm;
use wayland_server::protocol::wl_surface::WlSurface;
use wayland_server::protocol::wl_touch::WlTouch;
use wayland_server::{Client, Display, ListeningSocket, Resource};
use wayland_protocols::xdg::shell::server::xdg_popup::XdgPopup;
use wayland_protocols::xdg::shell::server::xdg_toplevel::{
    State as ToplevelState, XdgToplevel,
};
use wayland_protocols::xdg::shell::server::xdg_wm_base::XdgWmBase;

use crate::common::{apps, font, icons, util};

use wayland::{
    keymap_fd, BufferData, ClientState, CompositorData, OutputData, ShmData, SurfaceData,
    WmBaseData, XdgPopupData, XdgSurfaceData, XdgToplevelData,
};

// scene geometry
pub const W: u32 = 1080;
pub const H: u32 = 2160;
pub const STATUS_H: f32 = 44.0;
pub const TASKBAR_H: f32 = 88.0;
pub const TITLEBAR_H: f32 = 40.0;
pub const N_WORKSPACES: u32 = 3;

// xkb keysyms — the Fn layer of layout "gemini" (config/xkb/symbols/gemini)
// emits exactly these on level 3; values from xkeysym 0.2 (X keysyms).
const XK_Escape: u32 = 0xff1b;
const XK_grave: u32 = 0x60;
const XK_Home: u32 = 0xff50;
const XK_End: u32 = 0xff57;
const XK_Prior: u32 = 0xff55; // page up
const XK_Next: u32 = 0xff56; // page down
const XK_Delete: u32 = 0xffff;
const XF86_Sleep: u32 = 0x1008ff2f; // Fn+Esc
const XF86_Tools: u32 = 0x1008ff81; // Fn+Backspace
const XF86_TaskPane: u32 = 0x1008ff7f; // Fn+A — launcher
const XF86_TopMenu: u32 = 0x1008ffa2; // Fn+S — app switcher
const XF86_AudioLower: u32 = 0x1008ff11; // Fn+C
const XF86_AudioMute: u32 = 0x1008ff12; // Fn+T
const XF86_AudioRaise: u32 = 0x1008ff13; // Fn+V
const XF86_MonBrightDown: u32 = 0x1008ff03; // Fn+B
const XF86_MonBrightUp: u32 = 0x1008ff02; // Fn+N

const MOD5: u32 = 1 << 8;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Snap {
    None,
    Left,
    Right,
    Top,
    Bottom,
    Max,
}

pub struct Window {
    pub id: u32,
    /// set once the client's toplevel/popup resource exists (adopt_*)
    pub toplevel: Option<XdgToplevel>,
    pub surface: WlSurface,
    pub title: String,
    pub app_id: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub buffer: Option<std::sync::Arc<BufferData>>,
    pub workspace: u32,
    pub minimized: bool,
    pub maximized: bool,
    pub snap: Snap,
    /// the launcher app index (matched by title/app_id), if known
    pub app: Option<usize>,
    pub is_popup: bool,
    pub parent: Option<u32>,
}

impl Window {
    /// The client area (below the titlebar), in scene coords.
    pub fn content_rect(&self) -> (f32, f32, f32, f32) {
        (self.x, self.y + TITLEBAR_H, self.w, self.h - TITLEBAR_H)
    }

    /// Where the client buffer is drawn (letterboxed inside the content).
    pub fn buffer_rect(&self) -> (f32, f32, f32, f32) {
        let (cx, cy, cw, ch) = self.content_rect();
        let Some(buf) = &self.buffer else {
            return (cx, cy, cw, ch);
        };
        let bw = buf.w as f32;
        let bh = buf.h as f32;
        if bw <= 0.0 || bh <= 0.0 {
            return (cx, cy, cw, ch);
        }
        let scale = (cw / bw).min(ch / bh);
        let dw = bw * scale;
        let dh = bh * scale;
        (cx + (cw - dw) / 2.0, cy + (ch - dh) / 2.0, dw, dh)
    }
}

struct Finger {
    x: f32,
    y: f32,
    start_x: f32,
    start_y: f32,
    moved: bool,
    down_ms: u64,
}

#[derive(PartialEq, Clone)]
enum Gesture {
    None,
    /// one finger forwarded to the focused client (click/drag)
    Forward { win: u32, finger: u32 },
    /// one finger moving a window (from its titlebar)
    Move { win: u32, finger: u32, off_dx: f32, off_dy: f32 },
    /// two fingers swiping workspaces horizontally
    WorkspaceSwipe { f1: u32, f2: u32, last_x: f32, active: bool },
    /// two fingers swiping up = next app
    NextApp { f1: u32, f2: u32, last_y: f32, active: bool },
    /// three fingers: home (close overlays)
    Home,
    /// one finger scrolling the launcher
    LauncherScroll { finger: u32, last_y: f32 },
}

pub struct Compositor {
    gbm_dev: gbm::Gbm,
    pub renderer: render::Renderer,
    input: input::Input,
    pub font: font::Font,
    icons: icons::IconSet,
    windows: Vec<Window>,
    pub focus: Option<u32>,
    /// continuous workspace camera (integer = that workspace)
    ws_pos: f32,
    ws_target: f32,
    fingers: HashMap<u32, Finger>,
    gesture: Gesture,
    /// a UI tap awaiting resolution on finger-up (status/taskbar/titlebar)
    pending_tap: Option<(f32, f32)>,
    /// the client surface currently receiving wl_touch
    touch_focus: Option<u32>,
    pub launcher_open: bool,
    pub launcher_scroll: f32,
    pub switcher_open: bool,
    pub switcher_index: usize,
    pub apps: Vec<apps::App>,
    /// app index -> GL texture (premultiplied icon)
    app_icons: HashMap<usize, u32>,
    pub settings_open: bool,
    pub status: status::Status,
    status_rx: mpsc::Receiver<status::Status>,
    dirty: bool,
    in_flight: bool,
    serial: u32,
    keymap_str: Option<String>,
    pub snap_preview: Snap,
    pub snap_win: Option<u32>,
    present_time_ms: u64,
    /// connected clients (keyboard fan-out; the backend prunes dead
    /// clients from the protocol state, this list is best-effort)
    tracked_clients: Vec<Client>,
    /// the currently pressed keysyms (for wl_keyboard.enter's keys list)
    pressed_keysyms: Vec<u32>,
    /// last titlebar double-tap bookkeeping
    last_titlebar_tap_ms: u64,
    last_titlebar_win: Option<u32>,
}

impl Compositor {
    /// Builds the compositor state + the (separate) Wayland display.
    /// The display lives OUTSIDE the struct: `dispatch_clients(&mut self)`
    /// needs the display borrowed separately from the state.
    pub fn new() -> Result<(Display<Compositor>, Self), String> {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

        let font_path = find_font().ok_or_else(|| "no system TTF font found".to_string())?;
        log::info!("font: {}", font_path.display());
        let font = font::Font::load(
            font_path
                .to_str()
                .ok_or_else(|| "non-unicode font path".to_string())?,
            26.0,
            2048,
        )
        .map_err(|e| format!("font: {e}"))?;

        let (kbd_opt, touch_opt, kbd_name, touch_name) = input::find_nodes();
        let (kbd_path, touch_path) = match (kbd_opt, touch_opt) {
            (Some(k), Some(t)) => (k, t),
            _ => return Err("no keyboard/touch evdev node (need `input` group)".to_string()),
        };
        let input = input::Input::open(&kbd_path, &touch_path, &kbd_name, &touch_name)?;
        let keymap_str = input.keymap_string();

        let gbm_dev = gbm::Gbm::new("/dev/dri/card0")?;
        let glyph_size = font.w;
        let renderer = render::Renderer::new(gbm_dev.ptr, W, H, &font.pixels, glyph_size)?;

        let display = Display::new().map_err(|e| format!("wayland display: {e}"))?;
        let handle = display.handle();
        let _ = handle.create_global::<Compositor, _, _>(7, CompositorData {}); // wl_compositor
        let _ = handle.create_global::<Compositor, _, _>(3, ShmData {}); // wl_shm
        let _ = handle.create_global::<Compositor, _, _>(11, ()); // wl_seat
        let _ = handle.create_global::<Compositor, _, _>(4, OutputData {}); // wl_output
        let _ = handle.create_global::<Compositor, _, _>(3, WmBaseData {}); // xdg_wm_base

        let (tx, rx) = mpsc::channel();
        status::spawn(tx);
        let status = status::Status::read();

        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let apps = apps::scan(&home);
        log::info!("{} apps from .desktop files", apps.len());
        let icons = icons::load(&home);
        log::info!("{} icons loaded", icons.map.len());

        let compositor = Compositor {
            gbm_dev,
                renderer,
                input,
                font,
                icons,
                windows: Vec::new(),
                focus: None,
                ws_pos: 0.0,
                ws_target: 0.0,
                fingers: HashMap::new(),
                gesture: Gesture::None,
                pending_tap: None,
                touch_focus: None,
                launcher_open: false,
                launcher_scroll: 0.0,
                switcher_open: false,
                switcher_index: 0,
                apps,
                app_icons: HashMap::new(),
                settings_open: false,
                status,
                status_rx: rx,
                dirty: true,
                in_flight: false,
                serial: 1,
                keymap_str,
                snap_preview: Snap::None,
                snap_win: None,
                present_time_ms: util::now_ms(),
                tracked_clients: Vec::new(),
                pressed_keysyms: Vec::new(),
                last_titlebar_tap_ms: 0,
                last_titlebar_win: None,
            };
        Ok((display, compositor))
    }

    pub fn ws_target_f(&self) -> f32 {
        self.ws_target
    }

    /// Upload app icon textures (premultiplied) once at startup.
    pub fn upload_app_icons(&mut self) {
        // snapshot the app list: the loop below mutably borrows
        // self.renderer while reading app/icon data
        let apps = self.apps.clone();
        let iconset = &self.icons;
        for (i, app) in apps.iter().enumerate() {
            let Some(icon) = icons::for_app(iconset, &app.icon, &app.path) else {
                continue;
            };
            let scale = 96.0f32 / icon.w.max(icon.h) as f32;
            let tw = (icon.w as f32 * scale).round().max(2.0) as u32;
            let th = (icon.h as f32 * scale).round().max(2.0) as u32;
            let mut rgba = vec![0u8; (tw * th * 4) as usize];
            for y in 0..th {
                for x in 0..tw {
                    let sx = (x as f32 / scale).min(icon.w as f32 - 1.0) as u32;
                    let sy = (y as f32 / scale).min(icon.h as f32 - 1.0) as u32;
                    let s = ((sy * icon.w + sx) * 4) as usize;
                    let d = ((y * tw + x) * 4) as usize;
                    let a = icon.rgba[s + 3] as f32 / 255.0;
                    rgba[d] = (icon.rgba[s] as f32 * a) as u8;
                    rgba[d + 1] = (icon.rgba[s + 1] as f32 * a) as u8;
                    rgba[d + 2] = (icon.rgba[s + 2] as f32 * a) as u8;
                    rgba[d + 3] = (a * 255.0) as u8;
                }
            }
            if let Ok(tex) = self.renderer.make_texture_pub(tw, th, &rgba) {
                self.app_icons.insert(i, tex);
            }
        }
        self.dirty = true;
    }

    pub fn app_icon_tex(&self, app: usize) -> Option<u32> {
        self.app_icons.get(&app).copied()
    }

    pub fn run(mut self, mut display: Display<Compositor>) -> i32 {
        let socket = match ListeningSocket::bind("wayland-0") {
            Ok(s) => s,
            Err(e) => {
                log::error!("wayland socket bind: {e}");
                return 1;
            }
        };
        let sock_name = socket
            .socket_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "wayland-0".into());
        log::info!("wayland socket: {sock_name}");
        self.upload_app_icons();

        let wl_fd = display.as_fd().as_raw_fd();
        let gbm_fd = self.gbm_dev.fd;
        let kbd_fd = self.input.kbd_fd;
        let touch_fd = self.input.touch_fd;
        let sock_fd = socket.as_raw_fd();
        let mut pfd = [
            pollfd(wl_fd, libc::POLLIN),
            pollfd(gbm_fd, libc::POLLIN),
            pollfd(kbd_fd, libc::POLLIN),
            pollfd(touch_fd, libc::POLLIN),
            pollfd(sock_fd, libc::POLLIN),
        ];

        loop {
            let timeout = if self.dirty || self.animating() { 0 } else { 16 };
            let rc =
                unsafe { libc::poll(pfd.as_mut_ptr(), pfd.len() as libc::nfds_t, timeout as libc::c_int) };
            if rc < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                log::error!("poll: {}", std::io::Error::last_os_error());
                return 1;
            }

            while let Ok(s) = self.status_rx.try_recv() {
                if s != self.status {
                    self.status = s;
                    self.dirty = true;
                }
            }

            if pfd[0].revents & libc::POLLIN != 0 {
                if let Err(e) = display.dispatch_clients(&mut self) {
                    log::error!("dispatch: {e}");
                }
                let _ = display.flush_clients();
            }

            if pfd[4].revents & libc::POLLIN != 0 {
                while let Ok(Some(stream)) = socket.accept() {
                    match display.handle().insert_client(stream, Arc::new(ClientState::default())) {
                        Ok(client) => {
                            log::info!("client connected");
                            self.note_client(client);
                            self.dirty = true;
                        }
                        Err(e) => {
                            log::error!("client insert: {e}");
                            break;
                        }
                    }
                }
            }

            if pfd[1].revents & libc::POLLIN != 0 {
                self.gbm_dev.consume_flip();
                self.in_flight = false;
                self.present_time_ms = util::now_ms();
                self.dirty = true;
            }

            if pfd[2].revents & libc::POLLIN != 0 || pfd[3].revents & libc::POLLIN != 0 {
                for ev in self.input.read_events(W as f32, H as f32) {
                    match ev {
                        input::Event::Key {
                            code,
                            keysym,
                            pressed,
                            mods,
                            ..
                        } => {
                            self.serial += 1;
                            self.handle_key(code, keysym, pressed, mods);
                        }
                        input::Event::TouchDown { id, x, y } => {
                            self.serial += 1;
                            self.touch_down(id, x, y);
                        }
                        input::Event::TouchMotion { id, x, y } => {
                            self.serial += 1;
                            self.touch_motion(id, x, y);
                        }
                        input::Event::TouchUp { id } => {
                            self.serial += 1;
                            self.touch_up(id);
                        }
                    }
                }
            }

            self.step_animation();

            if self.dirty && !self.in_flight {
                self.render_frame();
                self.dirty = false;
                self.in_flight = true;
                let _ = display.flush_clients();
            }
        }
    }

    fn animating(&self) -> bool {
        (self.ws_pos - self.ws_target).abs() > 0.002 || self.launcher_scroll.abs() > 0.5
    }

    fn step_animation(&mut self) {
        if (self.ws_pos - self.ws_target).abs() > 0.002 {
            self.ws_pos += (self.ws_target - self.ws_pos) * 0.35;
            if (self.ws_pos - self.ws_target).abs() <= 0.002 {
                self.ws_pos = self.ws_target;
            }
            self.dirty = true;
        }
        if self.launcher_scroll.abs() > 0.5 && !matches!(self.gesture, Gesture::LauncherScroll { .. }) {
            self.launcher_scroll *= 0.8;
            self.dirty = true;
        }
    }

    fn note_client(&mut self, client: Client) {
        self.tracked_clients.push(client);
        if self.tracked_clients.len() > 32 {
            self.tracked_clients.remove(0);
        }
    }

    // -----------------------------------------------------------------
    // keys

    fn handle_key(&mut self, code: u32, keysym: u32, pressed: bool, mods: u32) {
        if !pressed {
            self.pressed_keysyms.retain(|k| *k != keysym);
            if keysym == XF86_TopMenu && self.switcher_open {
                // release = activate the selection
                self.switcher_open = false;
                self.activate_switcher();
                self.dirty = true;
            }
            // key up: forward to the focused client
            self.forward_key(code, keysym, false);
            return;
        }
        if !self.pressed_keysyms.contains(&keysym) {
            self.pressed_keysyms.push(keysym);
        }
        // ---- compositor shortcuts (consume) ----
        let consumed = match keysym {
            XF86_TaskPane => {
                self.launcher_open = !self.launcher_open;
                self.switcher_open = false;
                true
            }
            XF86_TopMenu => {
                if self.switcher_open {
                    // each press cycles the selection
                    let n = self.switcher_windows().len();
                    if n > 0 {
                        self.switcher_index = (self.switcher_index + 1) % n;
                    }
                } else {
                    self.switcher_open = true;
                    self.launcher_open = false;
                    self.switcher_index = self
                        .switcher_windows()
                        .iter()
                        .position(|w| Some(w.id) == self.focus)
                        .unwrap_or(0);
                }
                self.dirty = true;
                true
            }
            XF86_Tools | XK_Delete if self.switcher_open == false => {
                if let Some(f) = self.focus {
                    self.close_window(f);
                }
                true
            }
            XF86_AudioLower => {
                self.volume_step(-10);
                true
            }
            XF86_AudioRaise => {
                self.volume_step(10);
                true
            }
            XF86_AudioMute => {
                self.volume_mute();
                true
            }
            XF86_MonBrightDown => {
                self.brightness_step(-10);
                true
            }
            XF86_MonBrightUp => {
                self.brightness_step(10);
                true
            }
            XF86_Sleep => {
                self.launcher_open = false;
                self.switcher_open = false;
                true
            }
            XK_Prior if mods & MOD5 != 0 => {
                self.switch_workspace(self.ws_target - 1.0);
                true
            }
            XK_Next if mods & MOD5 != 0 => {
                self.switch_workspace(self.ws_target + 1.0);
                true
            }
            XK_Home if mods & MOD5 == MOD5 => {
                self.snap_focused(Snap::Left);
                true
            }
            XK_End if mods & MOD5 == MOD5 => {
                self.snap_focused(Snap::Right);
                true
            }
            XK_grave if mods & MOD5 == MOD5 => {
                self.toggle_maximize_focused();
                true
            }
            XK_Escape => {
                if self.launcher_open || self.switcher_open {
                    self.launcher_open = false;
                    self.switcher_open = false;
                    self.dirty = true;
                }
                true
            }
            _ => false,
        };
        if consumed {
            return;
        }
        self.forward_key(code, keysym, true);
    }

    /// Forward a key event to the focused client's wl_keyboard.
    fn forward_key(&mut self, code: u32, _keysym: u32, pressed: bool) {
        let Some(focus) = self.focus else { return };
        let Some(win) = self.windows.iter().find(|w| w.id == focus) else { return };
        let Some(kbd) = self.kbd_res_for(win) else { return };
        let xkb_code = code + 8; // evdev keycode -> xkb keycode
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        let _ = kbd.key(self.serial, self.present_time_ms as u32, xkb_code, state);
        let (d, l, k) = self.input.mods_masks();
        let _ = kbd.modifiers(self.serial, d, l, k, 0);
    }


    fn volume_step(&mut self, delta: i32) {
        let target = (self.status.volume + delta).clamp(0, 100);
        spawn_cmd(
            "wpctl",
            vec![
                "set-volume".to_string(),
                "@DEFAULT_AUDIO_SINK@".to_string(),
                format!("{target}%"),
            ],
        );
        self.status.volume = target;
        self.dirty = true;
    }

    fn volume_mute(&mut self) {
        let m = !self.status.muted;
        spawn_cmd(
            "wpctl",
            vec![
                "set-mute".to_string(),
                "@DEFAULT_AUDIO_SINK@".to_string(),
                if m {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
            ],
        );
        self.status.muted = m;
        self.dirty = true;
    }

    fn brightness_step(&mut self, delta: i32) {
        let max = self.status.brightness_max.max(1);
        let cur_pct = self.status.brightness * 100 / max;
        let target = (cur_pct + delta).clamp(0, 100);
        if let Some(v) = status::set_brightness(target, max) {
            self.status.brightness = v;
            self.dirty = true;
        }
    }

    // -----------------------------------------------------------------
    // touch + gestures

    fn touch_down(&mut self, id: u32, x: f32, y: f32) {
        self.dismiss_popups_outside(x, y);

        let f = Finger {
            x,
            y,
            start_x: x,
            start_y: y,
            moved: false,
            down_ms: util::now_ms(),
        };
        let n = self.fingers.len();
        self.fingers.insert(id, f);

        match n {
            0 => self.gesture_begin_single(id, x, y),
            1 => self.gesture_begin_double(id),
            2 => {
                self.cancel_forwarded();
                if self.launcher_open || self.switcher_open {
                    self.launcher_open = false;
                    self.switcher_open = false;
                    self.dirty = true;
                }
                self.gesture = Gesture::Home;
            }
            _ => {
                self.cancel_forwarded();
            }
        }
    }

    fn gesture_begin_single(&mut self, id: u32, x: f32, y: f32) {
        if self.launcher_open {
            self.gesture = Gesture::LauncherScroll { finger: id, last_y: y };
            return;
        }
        if self.switcher_open {
            self.gesture = Gesture::None; // tap resolved on up
            return;
        }
        if y < STATUS_H || y > H as f32 - TASKBAR_H {
            self.pending_tap = Some((x, y));
            self.gesture = Gesture::None;
            return;
        }
        let ws = self.ws_target.round() as u32;
        // copy hit info out first: the branches below mutably borrow self
        let mut hit = None;
        for win in self.windows.iter().rev() {
            if win.minimized || win.workspace != ws {
                continue;
            }
            if x >= win.x && x <= win.x + win.w && y >= win.y && y <= win.y + win.h {
                hit = Some((win.id, win.is_popup, win.x, win.y, win.w));
                break;
            }
        }
        let Some((wid, is_popup, wx, wy, ww)) = hit else {
            self.launcher_open = false;
            self.switcher_open = false;
            self.gesture = Gesture::None;
            return;
        };
        self.raise(wid);
        if !is_popup && y - wy < TITLEBAR_H {
            // close button (top-right 44px)
            if x >= wx + ww - 44.0 {
                self.close_window(wid);
                return;
            }
            // double-tap = toggle maximize
            let now = util::now_ms();
            let double = self.last_titlebar_win == Some(wid) && now - self.last_titlebar_tap_ms < 320;
            self.last_titlebar_tap_ms = now;
            self.last_titlebar_win = Some(wid);
            if double {
                let was_max = self.windows.iter().find(|w| w.id == wid).map(|w| w.maximized).unwrap_or(false);
                self.set_maximized(wid, !was_max);
                self.gesture = Gesture::None;
                return;
            }
            self.gesture = Gesture::Move { win: wid, finger: id, off_dx: x - wx, off_dy: y - wy };
            self.snap_preview = Snap::None;
            self.snap_win = Some(wid);
        } else {
            self.begin_forward(wid, id);
            self.gesture = Gesture::Forward { win: wid, finger: id };
        }
    }

    fn gesture_begin_double(&mut self, id: u32) {
        let first = self.fingers.iter().find(|(k, _)| **k != id).map(|(k, _)| *k);
        self.cancel_forwarded();
        self.snap_preview = Snap::None;
        self.snap_win = None;
        let Some(f1) = first else {
            self.gesture = Gesture::None;
            return;
        };
        let (f1d, f2d) = match (self.fingers.get(&f1), self.fingers.get(&id)) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                self.gesture = Gesture::None;
                return;
            }
        };
        let dx = f2d.start_x - f1d.start_x;
        let dy = f2d.start_y - f1d.start_y;
        if dx.abs() > dy.abs() {
            self.gesture = Gesture::WorkspaceSwipe {
                f1,
                f2: id,
                last_x: (f1d.start_x + f2d.start_x) / 2.0,
                active: true,
            };
        } else {
            self.gesture = Gesture::NextApp {
                f1,
                f2: id,
                last_y: (f1d.start_y + f2d.start_y) / 2.0,
                active: true,
            };
        }
    }

    fn touch_motion(&mut self, id: u32, x: f32, y: f32) {
        let Some(f) = self.fingers.get_mut(&id) else {
            return;
        };
        if (x - f.start_x).abs() > 18.0 || (y - f.start_y).abs() > 18.0 {
            f.moved = true;
        }
        f.x = x;
        f.y = y;

        // clone the gesture: the arms below mutably borrow self
        match self.gesture.clone() {
            Gesture::Move { win, finger, off_dx, off_dy } if finger == id => {
                let needs_unsnap = self
                    .windows
                    .iter()
                    .find(|w| w.id == win)
                    .map(|w| w.maximized || w.snap != Snap::None)
                    .unwrap_or(false);
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == win) {
                    w.x = (x - off_dx).clamp(0.0, W as f32 - w.w);
                    w.y = (y - off_dy).clamp(STATUS_H, H as f32 - TASKBAR_H - 40.0);
                }
                if needs_unsnap {
                    self.unmaximize(win);
                }
                self.snap_preview = self.snap_zone(x, y);
                self.dirty = true;
            }
            Gesture::WorkspaceSwipe { f1, f2, last_x, active } if f2 == id || f1 == id => {
                let Some(cx) = self.pair_center_x(f1, f2) else {
                    return;
                };
                if active {
                    self.ws_pos = (self.ws_pos - (cx - last_x) / W as f32)
                        .clamp(0.0, (N_WORKSPACES - 1) as f32);
                }
                if let Gesture::WorkspaceSwipe { last_x: lx, .. } = &mut self.gesture {
                    *lx = cx;
                }
                self.dirty = true;
            }
            Gesture::NextApp { f1, f2, last_y, .. } if f2 == id || f1 == id => {
                let Some(cy) = self.pair_center_y(f1, f2) else {
                    return;
                };
                let mut should_next = false;
                if let Gesture::NextApp { active: a, last_y: ly, .. } = &mut self.gesture {
                    if *a && cy - *ly < -80.0 {
                        should_next = true;
                        *a = false;
                    }
                    *ly = cy;
                }
                if should_next {
                    self.next_app();
                }
                self.dirty = true;
            }
            Gesture::LauncherScroll { finger, last_y } if finger == id => {
                self.launcher_scroll -= (y - last_y) * 0.9;
                self.launcher_scroll = self.launcher_scroll.clamp(-600.0, 0.0);
                if let Gesture::LauncherScroll { last_y: ly, .. } = &mut self.gesture {
                    *ly = y;
                }
                self.dirty = true;
            }
            Gesture::Forward { win, .. } => {
                self.forward_motion(win, x, y);
            }
            _ => {}
        }
    }

    fn touch_up(&mut self, id: u32) {
        let was_moved = self.fingers.get(&id).map(|f| f.moved).unwrap_or(false);
        let pos = self.fingers.get(&id).map(|f| (f.x, f.y));
        self.fingers.remove(&id);

        match self.gesture.clone() {
            Gesture::None => {
                if !was_moved && self.fingers.is_empty() {
                    if let Some((x, y)) = pos {
                        self.handle_tap(x, y);
                    }
                }
                self.pending_tap = None;
            }
            Gesture::Move { win, .. } => {
                if self.snap_preview != Snap::None && self.snap_win == Some(win) {
                    self.apply_snap(win, self.snap_preview);
                }
                self.snap_preview = Snap::None;
                self.snap_win = None;
                if self.fingers.is_empty() {
                    self.gesture = Gesture::None;
                }
            }
            Gesture::WorkspaceSwipe { .. } => {
                self.ws_target = self.ws_pos.round().clamp(0.0, (N_WORKSPACES - 1) as f32);
                if self.fingers.len() < 2 {
                    self.gesture = Gesture::None;
                }
            }
            Gesture::NextApp { .. } | Gesture::Home => {
                if self.fingers.len() < 2 {
                    self.gesture = Gesture::None;
                }
            }
            Gesture::LauncherScroll { .. } => {
                if self.fingers.is_empty() {
                    self.gesture = Gesture::None;
                }
            }
            Gesture::Forward { win, .. } => {
                self.forward_up(win, id);
                if self.fingers.is_empty() {
                    self.gesture = Gesture::None;
                }
            }
        }
    }

    fn pair_center_x(&self, a: u32, b: u32) -> Option<f32> {
        let (fa, fb) = (self.fingers.get(&a)?, self.fingers.get(&b)?);
        Some((fa.x + fb.x) / 2.0)
    }
    fn pair_center_y(&self, a: u32, b: u32) -> Option<f32> {
        let (fa, fb) = (self.fingers.get(&a)?, self.fingers.get(&b)?);
        Some((fa.y + fb.y) / 2.0)
    }

    fn snap_zone(&self, x: f32, y: f32) -> Snap {
        let edge = 26.0;
        if x < edge {
            return Snap::Left;
        }
        if x > W as f32 - edge {
            return Snap::Right;
        }
        if y < STATUS_H + edge {
            return Snap::Top;
        }
        if y > H as f32 - TASKBAR_H - edge {
            return Snap::Bottom;
        }
        Snap::None
    }

    // -----------------------------------------------------------------
    // taps (UI hit-testing)

    fn handle_tap(&mut self, x: f32, y: f32) {
        if self.launcher_open {
            let n = self.apps.len();
            for i in 0..n {
                let (cx, cy) = ui::launcher_tile_pos(i, self.launcher_scroll);
                if x > cx - 70.0 && x < cx + 70.0 && y > cy - 70.0 && y < cy + 80.0 {
                    self.launch_app(i);
                    self.launcher_open = false;
                    return;
                }
            }
            return;
        }
        if self.switcher_open {
            let n = self.switcher_windows().len().max(1);
            let cw = 220.0;
            let gap = 24.0;
            let total = n as f32 * cw + (n - 1) as f32 * gap;
            let x0 = (W as f32 - total) / 2.0;
            let y0 = H as f32 / 2.0 - 120.0;
            for i in 0..n {
                let tx = x0 + i as f32 * (cw + gap);
                if x > tx - 6.0 && x < tx + cw + 6.0 && y > y0 - 6.0 && y < y0 + 252.0 {
                    self.switcher_index = i;
                    self.switcher_open = false;
                    self.activate_switcher();
                    self.dirty = true;
                    return;
                }
            }
            return;
        }
        if y < STATUS_H {
            for (name, cx) in ui::status_zones() {
                if (x - cx).abs() < ui::ZONE_HALF {
                    match name {
                        "settings" | "sound" | "wifi" | "bluetooth" => self.open_settings(),
                        "battery" => {}
                        _ => {}
                    }
                    return;
                }
            }
            return;
        }
        if y > H as f32 - TASKBAR_H {
            if x < ui::LAUNCHER_X + ui::LAUNCHER_S + 8.0 {
                self.launcher_open = !self.launcher_open;
                self.switcher_open = false;
                self.dirty = true;
                return;
            }
            let tiles: Vec<(u32, u32)> =
                self.visible_windows().iter().map(|w| (w.id, w.workspace)).collect();
            let mut tx = ui::TILE_X0;
            for (wid, ws) in tiles {
                if tx + ui::TILE_S > W as f32 {
                    break;
                }
                if x >= tx && x < tx + ui::TILE_S {
                    if self.focus == Some(wid) {
                        self.minimize(wid);
                    } else {
                        self.ws_target = ws as f32;
                        self.raise(wid);
                    }
                    return;
                }
                tx += ui::TILE_S + ui::TILE_GAP;
            }
            return;
        }
        // (window titlebars are handled by the Move gesture on touch-down)
    }

    // -----------------------------------------------------------------
    // client touch forwarding

    fn touch_res_for(&self, win: &Window) -> Option<WlTouch> {
        let client = win.toplevel.as_ref()?.client()?;
        let cs = client.get_data::<ClientState>()?;
        let out = cs.inner.lock().ok()?.touch.clone();
        out
    }

    fn kbd_res_for(&self, win: &Window) -> Option<WlKeyboard> {
        let client = win.toplevel.as_ref()?.client()?;
        let cs = client.get_data::<ClientState>()?;
        let out = cs.inner.lock().ok()?.keyboard.clone();
        out
    }

    fn begin_forward(&mut self, win: u32, finger: u32) {
        let Some(w) = self.windows.iter().find(|w| w.id == win) else {
            return;
        };
        let Some(touch) = self.touch_res_for(w) else {
            return;
        };
        self.touch_focus = Some(win);
        let (x, y) = self.finger_pos(finger).unwrap_or((0.0, 0.0));
        let (lx, ly) = self.local_coords(w, x, y);
        let _ = touch.down(self.serial, self.present_time_ms as u32, &w.surface, finger as i32, lx as f64, ly as f64);
        let _ = touch.frame();
        self.dirty = true;
    }

    fn forward_motion(&mut self, win: u32, x: f32, y: f32) {
        let Some(w) = self.windows.iter().find(|w| w.id == win) else {
            return;
        };
        let Some(touch) = self.touch_res_for(w) else {
            return;
        };
        let finger = self.gesture_finger() as i32;
        let (lx, ly) = self.local_coords(w, x, y);
        let _ = touch.motion(self.present_time_ms as u32, finger, lx as f64, ly as f64);
        let _ = touch.frame();
        self.dirty = true;
    }

    fn forward_up(&mut self, win: u32, finger: u32) {
        let Some(w) = self.windows.iter().find(|w| w.id == win) else {
            return;
        };
        let Some(touch) = self.touch_res_for(w) else {
            return;
        };
        let _ = touch.up(self.serial, self.present_time_ms as u32, finger as i32);
        let _ = touch.frame();
        if self.fingers.is_empty() {
            self.touch_focus = None;
        }
        self.dirty = true;
    }

    fn cancel_forwarded(&mut self) {
        if let Gesture::Forward { win, finger } = self.gesture.clone() {
            let Some(w) = self.windows.iter().find(|w| w.id == win) else {
                return;
            };
            let Some(touch) = self.touch_res_for(w) else {
                return;
            };
            let _ = touch.cancel();
            let _ = touch.frame();
            self.touch_focus = None;
        }
    }

    fn gesture_finger(&self) -> u32 {
        if let Gesture::Forward { finger, .. } = self.gesture {
            return finger;
        }
        self.fingers.keys().next().copied().unwrap_or(0)
    }

    fn finger_pos(&self, finger: u32) -> Option<(f32, f32)> {
        self.fingers.get(&finger).map(|f| (f.x, f.y))
    }

    /// The pressed keysyms as a wl_keyboard enter `keys` array (u32
    /// little-endian bytes — the 0.31 codegen maps the array to Vec<u8>).
    fn pressed_keys_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for k in &self.pressed_keysyms {
            out.extend_from_slice(&k.to_le_bytes());
        }
        out
    }

    /// Surface-local logical coords (scene buffer rect -> client pixels;
    /// clients with 1:1 buffers get exact pixels).
    fn local_coords(&self, w: &Window, x: f32, y: f32) -> (f32, f32) {
        let (bx, by, bw, _bh) = w.buffer_rect();
        let Some(buf) = &w.buffer else {
            return (x - w.x, y - w.y - TITLEBAR_H);
        };
        let scale = (bw / buf.w as f32).max(1e-6);
        ((x - bx) / scale, (y - by) / scale)
    }

    // -----------------------------------------------------------------
    // app launching

    fn launch_app(&mut self, idx: usize) {
        let Some(app) = self.apps.get(idx) else {
            return;
        };
        let Some(argv) = apps::exec_argv(&app.exec) else {
            log::warn!("no Exec= for {}", app.name);
            return;
        };
        log::info!("launch: {} ({})", app.name, app.exec);
        let prog = argv[0].clone();
        spawn_cmd(&prog, &argv[1..]);
    }

    fn open_settings(&mut self) {
        let has = self
            .windows
            .iter()
            .any(|w| w.app_id == "gemsettings" || w.title.contains("Settings"));
        if has {
            if let Some(w) = self.windows.iter().find(|w| w.app_id == "gemsettings" || w.title.contains("Settings")) {
                let wid = w.id;
                self.raise(wid);
                return;
            }
        }
        self.settings_open = true;
        spawn_cmd("gemsettings", vec!["--section=wifi".to_string()]);
    }

    // -----------------------------------------------------------------
    // window management

    pub fn visible_windows(&self) -> Vec<&Window> {
        self.windows
            .iter()
            .filter(|w| !w.minimized && !w.is_popup && w.workspace as f32 == self.ws_target)
            .collect()
    }

    pub fn switcher_windows(&self) -> Vec<&Window> {
        self.windows.iter().filter(|w| !w.minimized && !w.is_popup).collect()
    }

    fn raise(&mut self, id: u32) {
        if let Some(pos) = self.windows.iter().position(|w| w.id == id) {
            let w = self.windows.remove(pos);
            self.windows.push(w);
            self.focus = Some(id);
            self.send_focus_change();
            self.dirty = true;
        }
    }

    fn switch_workspace(&mut self, target: f32) {
        self.ws_target = target.clamp(0.0, (N_WORKSPACES - 1) as f32);
        self.dirty = true;
    }

    fn next_app(&mut self) {
        let mut wins = self.visible_windows();
        if wins.is_empty() {
            return;
        }
        let cur = self
            .focus
            .and_then(|f| wins.iter().position(|w| w.id == f))
            .unwrap_or(0);
        let next = (cur + 1) % wins.len();
        self.raise(wins[next].id);
    }

    fn activate_switcher(&mut self) {
        let picked = self
            .switcher_windows()
            .get(self.switcher_index)
            .map(|w| (w.workspace, w.id));
        if let Some((ws, wid)) = picked {
            self.ws_target = ws as f32;
            self.raise(wid);
        }
    }

    /// Register a new toplevel for a surface (called from the
    /// xdg_surface.get_toplevel dispatch). Returns the window id.
    pub fn new_toplevel(&mut self, surface: WlSurface) -> Option<u32> {
        if self.windows.iter().any(|w| w.surface.id() == surface.id()) {
            return None;
        }
        let id = self.next_id();
        let n = self.windows.len();
        let x = 40.0 + (n % 4) as f32 * 36.0;
        let y = STATUS_H + 40.0 + (n % 4) as f32 * 36.0;
        self.windows.push(Window {
            id,
            toplevel: None,
            surface,
            title: String::new(),
            app_id: String::new(),
            x,
            y,
            w: 540.0,
            h: 960.0,
            buffer: None,
            workspace: self.ws_target as u32,
            minimized: false,
            maximized: false,
            snap: Snap::None,
            app: None,
            is_popup: false,
            parent: None,
        });
        self.dirty = true;
        Some(id)
    }

    /// The toplevel resource is in (data_init created it in the dispatch).
    pub fn adopt_toplevel(&mut self, win: u32, toplevel: XdgToplevel) {
        self.focus = Some(win);
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == win) {
            w.toplevel = Some(toplevel);
        }
        self.send_focus_change();
        self.request_configure(win);
        self.dirty = true;
    }

    pub fn new_popup(&mut self, surface: WlSurface, parent: Option<u32>, w: f32, h: f32, x: f32, y: f32) -> Option<u32> {
        if self.windows.iter().any(|w2| w2.surface.id() == surface.id()) {
            return None;
        }
        let id = self.next_id();
        self.windows.push(Window {
            id,
            toplevel: None,
            surface,
            title: String::new(),
            app_id: String::new(),
            x: x.clamp(0.0, W as f32 - 10.0),
            y: y.clamp(STATUS_H, H as f32 - TASKBAR_H - 10.0),
            w: w.clamp(40.0, W as f32 as f32),
            h: h.clamp(40.0, H as f32 as f32),
            buffer: None,
            workspace: self.ws_target as u32,
            minimized: false,
            maximized: false,
            snap: Snap::None,
            app: None,
            is_popup: true,
            parent,
        });
        self.dirty = true;
        Some(id)
    }

    pub fn adopt_popup(&mut self, win: u32, popup: XdgPopup) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == win) {
            w.toplevel = None;
            let _ = popup; // popups carry no toplevel; kept alive via the popup resource in XdgPopupData
        }
    }

    /// The anchor rect for popup placement (parent window rect, or a
    /// default in the middle).
    pub fn popup_anchor(&self, parent: Option<u32>) -> (f32, f32, f32, f32) {
        parent
            .and_then(|p| self.windows.iter().find(|w| w.id == p))
            .map(|w| (w.x, w.y, w.w, w.h))
            .unwrap_or((W as f32 / 2.0, H as f32 / 2.0, 200.0, 200.0))
    }

    pub fn set_title(&mut self, win: u32, title: String) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == win) {
            w.title = title;
            if w.app_id.is_empty() {
                w.app_id = w.title.clone();
            }
            w.app = self
                .apps
                .iter()
                .position(|a| a.name == w.title || a.name == w.app_id);
            self.dirty = true;
        }
    }

    pub fn set_app_id(&mut self, win: u32, app_id: String) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == win) {
            w.app_id = app_id.clone();
            w.app = self
                .apps
                .iter()
                .position(|a| a.name == app_id || a.name == w.title);
            self.dirty = true;
        }
    }

    pub fn close_window(&mut self, win: u32) {
        let idx = match self.windows.iter().position(|w| w.id == win) {
            Some(i) => i,
            None => return,
        };
        let w = self.windows.remove(idx);
        drop(w.toplevel);
        if self.focus == Some(win) {
            self.focus = self.windows.iter().rev().find(|w| !w.minimized && !w.is_popup).map(|w| w.id);
            self.send_focus_change();
        }
        if w.app_id == "gemsettings" {
            self.settings_open = false;
        }
        self.renderer.drop_window_texture(win);
        self.dirty = true;
    }

    pub fn minimize(&mut self, win: u32) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == win) {
            w.minimized = true;
            if self.focus == Some(win) {
                self.focus = self.windows.iter().rev().find(|w| !w.minimized && !w.is_popup).map(|w| w.id);
                self.send_focus_change();
            }
            self.dirty = true;
        }
    }

    pub fn set_maximized(&mut self, win: u32, maximized: bool) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == win) {
            if maximized {
                w.maximized = true;
                w.snap = Snap::Max;
                w.x = 0.0;
                w.y = STATUS_H;
                w.w = W as f32;
                w.h = H as f32 - STATUS_H - TASKBAR_H;
            } else {
                self.unmaximize(win);
                return;
            }
        }
        self.request_configure(win);
        self.dirty = true;
    }

    fn unmaximize(&mut self, win: u32) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == win) {
            w.maximized = false;
            w.snap = Snap::None;
            w.y = w.y.max(STATUS_H);
            self.request_configure(win);
        }
    }

    fn toggle_maximize_focused(&mut self) {
        let Some(f) = self.focus else { return };
        let Some(m) = self.windows.iter().find(|w| w.id == f).map(|w| !w.maximized) else {
            return;
        };
        self.set_maximized(f, m);
    }

    fn snap_focused(&mut self, snap: Snap) {
        if let Some(f) = self.focus {
            self.apply_snap(f, snap);
        }
    }

    fn apply_snap(&mut self, win: u32, snap: Snap) {
        let Some(w) = self.windows.iter_mut().find(|w| w.id == win) else {
            return;
        };
        let sw = W as f32;
        let st = STATUS_H;
        let sb = H as f32 - TASKBAR_H;
        w.maximized = false;
        match snap {
            Snap::Left => {
                w.snap = Snap::Left;
                w.x = 0.0;
                w.y = st;
                w.w = sw / 2.0;
                w.h = sb - st;
            }
            Snap::Right => {
                w.snap = Snap::Right;
                w.x = sw / 2.0;
                w.y = st;
                w.w = sw / 2.0;
                w.h = sb - st;
            }
            Snap::Top => {
                w.snap = Snap::Top;
                w.x = 0.0;
                w.y = st;
                w.w = sw;
                w.h = (sb - st) / 2.0;
            }
            Snap::Bottom => {
                w.snap = Snap::Bottom;
                w.x = 0.0;
                w.y = (st + sb) / 2.0;
                w.w = sw;
                w.h = (sb - st) / 2.0;
            }
            Snap::Max => {
                w.snap = Snap::Max;
                w.maximized = true;
                w.x = 0.0;
                w.y = st;
                w.w = sw;
                w.h = sb - st;
            }
            Snap::None => {
                w.snap = Snap::None;
            }
        }
        self.request_configure(win);
        self.dirty = true;
    }

    fn request_configure(&mut self, win: u32) {
        let Some(w) = self.windows.iter().find(|w| w.id == win) else {
            return;
        };
        let Some(t) = w.toplevel.as_ref() else {
            return;
        };
        // NOTE: no ToplevelState::Active — the xdg-shell State enum at
        // this protocol version has no `active` member; focus is
        // communicated via wl_keyboard.enter instead.
        let states = match w.snap {
            Snap::Left => vec![ToplevelState::TiledLeft],
            Snap::Right => vec![ToplevelState::TiledRight],
            Snap::Top => vec![ToplevelState::TiledTop],
            Snap::Bottom => vec![ToplevelState::TiledBottom],
            Snap::Max => vec![ToplevelState::Maximized],
            Snap::None => Vec::new(),
        };
        let (cw, ch) = if w.maximized || w.snap != Snap::None {
            (w.w as i32, (w.h - TITLEBAR_H) as i32)
        } else {
            (0, 0)
        };
        let states_bytes: Vec<u8> = states.into_iter().map(|s| s as u8).collect();
        // NOTE: generated order is (width, height, states)
        let _ = t.configure(cw, ch, states_bytes);
    }

    // -----------------------------------------------------------------
    // keyboard focus (wl_keyboard fan-out)

    fn send_focus_change(&mut self) {
        let focus = self.focus;
        // snapshot (client, owns-focus-window) — the loop below
        // immutably borrows self.windows per client
        let owned: Vec<(Client, Option<u32>)> = self
            .tracked_clients
            .iter()
            .cloned()
            .map(|client| {
                let owns = focus.and_then(|f| {
                    self.windows
                        .iter()
                        .find(|w| w.id == f)
                        .and_then(|w| w.toplevel.as_ref())
                        .and_then(|t| t.client())
                        .filter(|c| c.id() == client.id())
                        .map(|_| f)
                });
                (client, owns)
            })
            .collect();
        for (client, new_focus) in owned {
            let Some(cs) = client.get_data::<ClientState>() else {
                continue;
            };
            let mut inner = match cs.inner.lock().ok() {
                Some(i) => i,
                None => continue,
            };
            let Some(kbd) = inner.keyboard.as_ref() else {
                continue;
            };
            let old = inner.kbd_focus;
            if old == new_focus {
                continue;
            }
            let old_surf = old.and_then(|id| self.windows.iter().find(|w| w.id == id)).map(|w| w.surface.clone());
            let new_surf = new_focus.and_then(|id| self.windows.iter().find(|w| w.id == id)).map(|w| w.surface.clone());
            if let Some(ow) = old_surf {
                let _ = kbd.leave(self.serial, &ow);
            }
            if let Some(nw) = new_surf {
                let keys = self.pressed_keys_bytes();
                let _ = kbd.enter(self.serial, &nw, keys);
                let (d, l, k) = self.input.mods_masks();
                let _ = kbd.modifiers(self.serial, d, l, k, 0);
            }
            inner.kbd_focus = new_focus;
        }
    }

    /// Called from the wl_seat.get_keyboard dispatch: send the keymap
    /// (and repeat info) to the new keyboard, and enter if it has focus.
    pub fn keyboard_bound(&mut self, client: &Client, kbd: &WlKeyboard) {
        let Some(km) = &self.keymap_str else {
            return;
        };
        if let Some((fd, size)) = keymap_fd(km) {
            let _ = kbd.keymap(KeymapFormat::XkbV1, fd.as_fd(), size);
            let _ = kbd.repeat_info(25, 500);
        }
        // if this client already owns the focus, enter now
        let focus_win = self.focus.and_then(|f| {
            self.windows
                .iter()
                .find(|w| w.id == f)
                .map(|w| (w.surface.clone(), w.id))
        });
        let has_focus = focus_win
            .as_ref()
            .and_then(|(s, _)| {
                self.windows
                    .iter()
                    .find(|w| w.surface.id() == s.id())
                    .and_then(|w| w.toplevel.as_ref())
                    .and_then(|t| t.client())
                    .map(|c| c.id() == client.id())
            })
            .unwrap_or(false);
        if has_focus {
            if let Some((surf, _)) = focus_win {
                let keys = self.pressed_keys_bytes();
                let _ = kbd.enter(self.serial, &surf, keys);
                let (d, l, k) = self.input.mods_masks();
                let _ = kbd.modifiers(self.serial, d, l, k, 0);
            }
            if let Some(cs) = client.get_data::<ClientState>() {
                if let Ok(mut inner) = cs.inner.lock() {
                    inner.kbd_focus = self.focus;
                }
            }
        }
    }

    // -----------------------------------------------------------------
    // wm-base

    pub fn wm_base_pong(&mut self, _serial: u32) {}

    // -----------------------------------------------------------------
    // surface commits

    pub fn surface_commit(&mut self, surface: &WlSurface) {
        let Some(data) = surface.data::<SurfaceData>() else {
            return;
        };
        let (buf, win) = {
            let inner = data.inner.lock().unwrap();
            (inner.pending_buffer.clone(), inner.window)
        };
        let Some(buf) = buf else {
            return;
        };
        let Some(win) = win else {
            return;
        };
        let map: &[u8] = &buf.map[..];
        let tex = self
            .renderer
            .window_texture(win, buf.w, buf.h, buf.stride, buf.format, map);
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == win) {
            w.buffer = Some(buf.clone());
            if !w.maximized && w.snap == Snap::None && !w.is_popup {
                let bw = (buf.w as f32).min(W as f32 - 16.0);
                let bh = (buf.h as f32).min(H as f32 - STATUS_H - TASKBAR_H - TITLEBAR_H - 16.0);
                if (bw - w.w).abs() > 1.0 || (bh - w.h).abs() > 1.0 {
                    w.w = bw;
                    w.h = bh;
                    w.x = w.x.clamp(0.0, W as f32 - bw);
                    w.y = w.y.clamp(STATUS_H, H as f32 - TASKBAR_H - bh);
                }
            }
            let _ = tex;
        }
        self.dirty = true;
    }

    pub fn surface_detached(&mut self, surface: &WlSurface) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.surface.id() == surface.id()) {
            w.buffer = None;
            self.renderer.drop_window_texture(w.id);
        }
        self.dirty = true;
    }

    pub fn xdg_surface_detached(&mut self, surface: WlSurface) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.surface.id() == surface.id()) {
            w.buffer = None;
            self.renderer.drop_window_texture(w.id);
        }
        self.dirty = true;
    }

    pub fn window_texture_id(&self, win: u32) -> Option<u32> {
        self.renderer.window_texture_id(win)
    }

    fn send_frame_callbacks(&mut self) {
        for w in &self.windows {
            let Some(data) = w.surface.data::<SurfaceData>() else {
                continue;
            };
            let cbs = std::mem::take(&mut data.inner.lock().unwrap().frame_cbs);
            for cb in cbs {
                let _ = cb.done(self.present_time_ms as u32);
            }
        }
    }

    // -----------------------------------------------------------------
    // rendering

    fn render_frame(&mut self) {
        let mut ops: Vec<render::Op> = Vec::with_capacity(256);
        ui::draw_background(&mut ops);

        // windows per visible workspace (with the slide offset)
        let p = self.ws_pos;
        for ws in 0..N_WORKSPACES {
            let off = (ws as f32 - p) * W as f32;
            if off < -(W as f32) || off > W as f32 {
                continue;
            }
            for win in self.windows.iter().filter(|w| w.workspace == ws && !w.minimized) {
                ui::draw_window(&mut ops, self, win, off);
            }
        }

        if self.snap_preview != Snap::None {
            ui::draw_snap_preview(&mut ops, self);
        }
        ui::draw_status_bar(&mut ops, self);
        ui::draw_taskbar(&mut ops, self);
        if self.launcher_open {
            ui::draw_launcher(&mut ops, self);
        }
        if self.switcher_open {
            ui::draw_switcher(&mut ops, self);
        }
        ui::draw_workspace_dots(&mut ops, self);

        self.renderer.begin_frame();
        self.renderer.replay(&self.font, &ops);
        if let Err(e) = self.renderer.swap() {
            log::error!("swap: {e}");
        }
        self.send_frame_callbacks();
    }

    fn next_id(&mut self) -> u32 {
        self.windows.iter().map(|w| w.id).max().unwrap_or(0) + 1
    }

    fn dismiss_popups_outside(&mut self, x: f32, y: f32) {
        let mut close = Vec::new();
        for w in self.windows.iter().filter(|w| w.is_popup) {
            if !(x >= w.x && x <= w.x + w.w && y >= w.y && y <= w.y + w.h) {
                close.push(w.id);
            }
        }
        for id in close {
            self.close_window(id);
        }
    }
}

// ---- helpers ----

/// The snap rect for a zone (drawn by ui, applied to windows by the
/// compositor) — one source of truth.
pub fn snap_rect(snap: Snap, _win: Option<u32>) -> (f32, f32, f32, f32) {
    let sw = W as f32;
    let st = STATUS_H;
    let sb = H as f32 - TASKBAR_H;
    match snap {
        Snap::Left => (0.0, st, sw / 2.0, sb - st),
        Snap::Right => (sw / 2.0, st, sw / 2.0, sb - st),
        Snap::Top => (0.0, st, sw, (sb - st) / 2.0),
        Snap::Bottom => (0.0, (st + sb) / 2.0, sw, (sb - st) / 2.0),
        _ => (0.0, st, sw, sb - st),
    }
}

fn pollfd(fd: RawFd, events: libc::c_short) -> libc::pollfd {
    libc::pollfd { fd, events, revents: 0 }
}

fn find_font() -> Option<std::path::PathBuf> {
    for dir in [
        "/usr/share/fonts/dejavu",
        "/usr/share/fonts/truetype",
        "/usr/share/fonts/TTF",
        "/usr/share/fonts",
    ] {
        if let Some(p) = walk(dir, |p| p.extension().and_then(|e| e.to_str()) == Some("ttf")) {
            return Some(p);
        }
    }
    None
}

fn walk(dir: &str, f: impl Fn(&std::path::Path) -> bool) -> Option<std::path::PathBuf> {
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if f(&p) {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn spawn_cmd<P: AsRef<std::ffi::OsStr>, A: AsRef<std::ffi::OsStr>, I: IntoIterator<Item = A>>(
    prog: P,
    args: I,
) {
    let prog = prog.as_ref().to_os_string();
    let args: Vec<std::ffi::OsString> =
        args.into_iter().map(|a| a.as_ref().to_os_string()).collect();
    std::thread::Builder::new()
        .name("spawn".into())
        .spawn(move || {
            let _ = std::process::Command::new(&prog).args(&args).spawn();
        })
        .ok();
}
