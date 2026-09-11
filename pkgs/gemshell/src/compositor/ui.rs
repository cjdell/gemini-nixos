//! Shell UI drawing — status bar, taskbar, launcher, app switcher, snap
//! previews. All compositor-drawn (no client). Records [`render::Op`]s
//! into a vec (the compositor replays them with the GL renderer).

use crate::compositor::render::{Color, Op, Renderer};
use crate::compositor::{H, STATUS_H, TASKBAR_H, W, Compositor};

const BAR_BG: Color = Color::rgba(0.09, 0.10, 0.12, 1.0);
const BAR_FG: Color = Color::rgba(0.92, 0.93, 0.95, 1.0);
pub const ACCENT: Color = Color::rgba(0.30, 0.62, 0.95, 1.0);
const ACCENT_SOFT: Color = Color::rgba(0.30, 0.62, 0.95, 0.22);
const WIN_BG: Color = Color::rgba(0.11, 0.12, 0.14, 1.0);
const OVERLAY_BG: Color = Color::rgba(0.05, 0.06, 0.08, 0.92);
const MUTED: Color = Color::rgba(0.55, 0.57, 0.60, 1.0);
const ICON_TILE: Color = Color::rgba(0.18, 0.20, 0.24, 1.0);
const BG: Color = Color::rgba(0.055, 0.07, 0.085, 1.0);

fn rect(o: &mut Vec<Op>, x: f32, y: f32, w: f32, h: f32, c: Color) {
    o.push(Op::Rect { x, y, w, h, c });
}
fn circle(o: &mut Vec<Op>, cx: f32, cy: f32, r: f32, c: Color) {
    o.push(Op::Circle { cx, cy, r, c });
}
fn arc(o: &mut Vec<Op>, cx: f32, cy: f32, r0: f32, r1: f32, a0: f32, a1: f32, c: Color) {
    o.push(Op::Arc { cx, cy, r0, r1, a0, a1, c });
}
fn line(o: &mut Vec<Op>, x0: f32, y0: f32, x1: f32, y1: f32, t: f32, c: Color) {
    o.push(Op::Line { x0, y0, x1, y1, t, c });
}

/// A rounded rect (body + 4 corner circles).
fn rounded(o: &mut Vec<Op>, x: f32, y: f32, w: f32, h: f32, rad: f32, c: Color) {
    let rad = rad.min(w / 2.0).min(h / 2.0);
    rect(o, x + rad, y, w - 2.0 * rad, h, c);
    rect(o, x, y + rad, rad, h - 2.0 * rad, c);
    rect(o, x + w - rad, y + rad, rad, h - 2.0 * rad, c);
    circle(o, x + rad, y + rad, rad, c);
    circle(o, x + w - rad, y + rad, rad, c);
    circle(o, x + rad, y + h - rad, rad, c);
    circle(o, x + w - rad, y + h - rad, rad, c);
}

// ---------- status-bar icons (vector) ----------

fn icon_gear(o: &mut Vec<Op>, cx: f32, cy: f32, s: f32, c: Color) {
    arc(o, cx, cy, s * 0.42, s * 0.62, 0.0, std::f32::consts::TAU, c);
    circle(o, cx, cy, s * 0.2, BG);
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        line(
            o,
            cx + s * 0.62 * a.cos(),
            cy + s * 0.62 * a.sin(),
            cx + s * 0.8 * a.cos(),
            cy + s * 0.8 * a.sin(),
            s * 0.16,
            c,
        );
    }
}

fn icon_wifi(o: &mut Vec<Op>, cx: f32, cy: f32, s: f32, on: bool, c: Color) {
    let c = if on { c } else { MUTED };
    arc(o, cx, cy, s * 0.55, s * 0.72, -std::f32::consts::FRAC_PI_4 - 0.5, -std::f32::consts::FRAC_PI_4 + 0.5, c);
    arc(o, cx, cy, s * 0.30, s * 0.47, -std::f32::consts::FRAC_PI_4 - 0.5, -std::f32::consts::FRAC_PI_4 + 0.5, c);
    circle(o, cx, cy, s * 0.14, c);
}

fn icon_bluetooth(o: &mut Vec<Op>, cx: f32, cy: f32, s: f32, on: bool, c: Color) {
    let c = if on { c } else { MUTED };
    let top = cy - s * 0.6;
    let bot = cy + s * 0.6;
    let right = cx + s * 0.35;
    line(o, cx, top, cx, bot, s * 0.14, c);
    line(o, cx, top, right, cy - s * 0.18, s * 0.14, c);
    line(o, cx, bot, right, cy + s * 0.18, s * 0.14, c);
    line(o, right, cy - s * 0.18, cx - s * 0.3, cy + s * 0.28, s * 0.14, c);
    line(o, right, cy + s * 0.18, cx - s * 0.3, cy - s * 0.28, s * 0.14, c);
}

fn icon_speaker(o: &mut Vec<Op>, cx: f32, cy: f32, s: f32, vol: i32, muted: bool, c: Color) {
    let c = if muted { MUTED } else { c };
    rect(o, cx - s * 0.52, cy - s * 0.16, s * 0.22, s * 0.32, c);
    line(o, cx - s * 0.3, cy - s * 0.16, cx + s * 0.1, cy - s * 0.42, s * 0.14, c);
    line(o, cx + s * 0.1, cy - s * 0.42, cx + s * 0.1, cy + s * 0.42, s * 0.14, c);
    line(o, cx + s * 0.1, cy + s * 0.42, cx - s * 0.3, cy + s * 0.16, s * 0.14, c);
    if !muted && vol > 0 {
        arc(o, cx + s * 0.28, cy, s * 0.28, s * 0.40, -std::f32::consts::FRAC_PI_4, std::f32::consts::FRAC_PI_4, c);
        if vol > 55 {
            arc(o, cx + s * 0.28, cy, s * 0.44, s * 0.56, -std::f32::consts::FRAC_PI_4, std::f32::consts::FRAC_PI_4, c);
        }
    }
}

fn icon_battery(o: &mut Vec<Op>, cx: f32, cy: f32, s: f32, pct: Option<i32>, charging: bool, c: Color) {
    let bw = s * 1.2;
    let bh = s * 0.6;
    let bx = cx - bw / 2.0;
    let by = cy - bh / 2.0;
    rect(o, bx, by, bw, bh, BG);
    rounded(o, bx, by, bw, bh, s * 0.1, c);
    rect(o, bx + bw + s * 0.02, by + bh * 0.25, s * 0.1, bh * 0.5, c);
    if let Some(p) = pct {
        let frac = (p.clamp(0, 100) / 100) as f32;
        if frac > 0.01 {
            let fill = if p < 20 {
                Color::rgba(0.9, 0.35, 0.3, 1.0)
            } else if charging {
                Color::rgba(0.35, 0.85, 0.45, 1.0)
            } else {
                Color::rgba(0.35, 0.85, 0.45, 1.0)
            };
            rect(o, bx + s * 0.06, by + s * 0.06, (bw - s * 0.12) * frac, bh - s * 0.12, fill);
        }
    }
    if charging {
        let bx2 = cx - s * 0.08;
        line(o, bx2 + s * 0.12, by + s * 0.08, bx2, cy + s * 0.06, s * 0.14, Color::rgba(1.0, 1.0, 1.0, 1.0));
        line(o, bx2, cy + s * 0.06, bx2 + s * 0.12, by + bh - s * 0.08, s * 0.14, Color::rgba(1.0, 1.0, 1.0, 1.0));
    }
}

fn icon_grid(o: &mut Vec<Op>, cx: f32, cy: f32, s: f32, c: Color) {
    let g = s * 0.26;
    let off = s * 0.42;
    for (dx, dy) in [(-off, -off), (0.0, -off), (off, -off), (-off, 0.0), (0.0, 0.0), (off, 0.0), (-off, off), (0.0, off), (off, off)] {
        rounded(o, cx + dx - g / 2.0, cy + dy - g / 2.0, g, g, g * 0.25, c);
    }
}

/// The app icon for `app` (by index): the PNG icon if uploaded, else a
/// letter tile.
pub fn app_icon(o: &mut Vec<Op>, comp: &Compositor, app: usize, cx: f32, cy: f32, s: f32) {
    if let Some(tex) = comp.app_icon_tex(app) {
        o.push(Op::RectTex {
            x: cx - s / 2.0,
            y: cy - s / 2.0,
            w: s,
            h: s,
            u0: 0.0,
            v0: 0.0,
            u1: 1.0,
            v1: 1.0,
            tex,
            c: Color::rgba(1.0, 1.0, 1.0, 1.0),
            premult: true,
        });
    } else {
        rounded(o, cx - s / 2.0, cy - s / 2.0, s, s, s * 0.2, ICON_TILE);
        let c = comp.apps.get(app).map(|a| a.name.chars().next().unwrap_or('?')).unwrap_or('?');
        o.push(Op::TextCentered {
            x: cx - s / 2.0,
            w: s,
            baseline: cy + s * 0.17,
            s: c.to_string(),
            c: BAR_FG,
        });
    }
}

/// The status-bar tap zones — the single source of truth shared by the
/// draw and the hit-test. (name, cx) — half-width is ZONE_HALF.
pub const ZONE_HALF: f32 = 26.0;
pub fn status_zones() -> Vec<(&'static str, f32)> {
    let mut x = W as f32 - 16.0;
    let mut z = vec![];
    x -= 44.0;
    z.push(("battery", x - 22.0));
    x -= 44.0;
    z.push(("wifi", x - 20.0));
    x -= 44.0;
    z.push(("bluetooth", x - 20.0));
    x -= 44.0;
    z.push(("sound", x - 20.0));
    x -= 44.0;
    z.push(("settings", x - 20.0));
    z
}

pub fn draw_status_bar(o: &mut Vec<Op>, comp: &Compositor) {
    rect(o, 0.0, 0.0, W as f32, STATUS_H, BAR_BG);
    let t = format!("{:02}:{:02}", comp.status.hour, comp.status.minute);
    o.push(Op::Text { x: 14.0, baseline: STATUS_H - 12.0, s: t, c: BAR_FG });

    let cy = STATUS_H / 2.0;
    let s = 15.0;
    for (name, cx) in status_zones() {
        match name {
            "battery" => {
                icon_battery(o, cx, cy, s, comp.status.battery, comp.status.charging, BAR_FG);
                if let Some(p) = comp.status.battery {
                    o.push(Op::TextCentered { x: cx - 38.0, w: 30.0, baseline: cy + 5.0, s: p.to_string(), c: MUTED });
                }
            }
            "wifi" => icon_wifi(o, cx, cy, s, comp.status.wifi, BAR_FG),
            "bluetooth" => icon_bluetooth(o, cx, cy, s, comp.status.bluetooth, BAR_FG),
            "sound" => icon_speaker(o, cx, cy, s, comp.status.volume, comp.status.muted, BAR_FG),
            "settings" => icon_gear(o, cx, cy, s, BAR_FG),
            _ => {}
        }
    }
}

pub const LAUNCHER_X: f32 = 24.0;
pub const LAUNCHER_S: f32 = 56.0;
pub const TILE_X0: f32 = 24.0 + 56.0 + 16.0;
pub const TILE_S: f32 = 56.0;
pub const TILE_GAP: f32 = 14.0;

pub fn draw_taskbar(o: &mut Vec<Op>, comp: &Compositor) {
    let y = H as f32 - TASKBAR_H;
    rect(o, 0.0, y, W as f32, TASKBAR_H, BAR_BG);
    let cy = y + TASKBAR_H / 2.0;
    let s = LAUNCHER_S;

    rounded(o, LAUNCHER_X, y + (TASKBAR_H - s) / 2.0, s, s, s * 0.24, ICON_TILE);
    icon_grid(o, LAUNCHER_X + s / 2.0, cy, s * 0.55, BAR_FG);

    let mut x = TILE_X0;
    for win in comp.visible_windows() {
        if x + TILE_S > W as f32 - 16.0 {
            break;
        }
        let focused = comp.focus == Some(win.id);
        rounded(o, x, y + (TASKBAR_H - TILE_S) / 2.0, TILE_S, TILE_S, TILE_S * 0.24, if focused { ACCENT_SOFT } else { ICON_TILE });
        app_icon(o, comp, win.app.unwrap_or(0), x + TILE_S / 2.0, cy, TILE_S * 0.62);
        circle(o, x + TILE_S - 7.0, y + (TASKBAR_H - TILE_S) / 2.0 + 7.0, 3.5, if focused { ACCENT } else { MUTED });
        x += TILE_S + TILE_GAP;
    }
}

pub fn draw_snap_preview(o: &mut Vec<Op>, comp: &Compositor) {
    let (x, y, w, h) = crate::compositor::snap_rect(comp.snap_preview, comp.snap_win);
    rect(o, x, y, w, h, ACCENT_SOFT);
    rect(o, x, y, w, 3.0, ACCENT);
    rect(o, x, y + h - 3.0, w, 3.0, ACCENT);
    rect(o, x, y, 3.0, h, ACCENT);
    rect(o, x + w - 3.0, y, 3.0, h, ACCENT);
}

/// Launcher grid geometry (shared by draw + hit-test).
pub const LAUNCHER_COLS: usize = 3;
pub const LAUNCHER_MARGIN: f32 = 60.0;
pub const LAUNCHER_TILE: f32 = 120.0;
pub const LAUNCHER_ROW_H: f32 = 184.0;
pub const LAUNCHER_START_Y: f32 = 140.0;

pub fn launcher_tile_pos(i: usize, scroll: f32) -> (f32, f32) {
    let cw = (W as f32 - 2.0 * LAUNCHER_MARGIN) / LAUNCHER_COLS as f32;
    let gap = (cw - LAUNCHER_TILE) / 2.0;
    let row = i / LAUNCHER_COLS;
    let col = i % LAUNCHER_COLS;
    (
        LAUNCHER_MARGIN + col as f32 * cw + gap + LAUNCHER_TILE / 2.0,
        LAUNCHER_START_Y - scroll + row as f32 * LAUNCHER_ROW_H + LAUNCHER_TILE / 2.0,
    )
}

/// Total scrollable content height for `n` apps (used to clamp the
/// scroll so the last row can always be reached).
pub fn launcher_content_h(n: usize) -> f32 {
    let rows = n.div_ceil(LAUNCHER_COLS);
    LAUNCHER_START_Y + rows as f32 * LAUNCHER_ROW_H + 40.0
}

/// Centre of the launcher `Close` button (top-right, inside the status bar).
pub fn launcher_close_pos() -> (f32, f32) {
    (W as f32 - 110.0, 74.0)
}

pub fn draw_launcher(o: &mut Vec<Op>, comp: &Compositor) {
    rect(o, 0.0, 0.0, W as f32, H as f32, OVERLAY_BG);
    o.push(Op::TextCentered { x: 0.0, w: W as f32, baseline: 90.0, s: "Apps".into(), c: BAR_FG });
    for (i, app) in comp.apps.iter().enumerate() {
        let (cx, cy) = launcher_tile_pos(i, comp.launcher_scroll);
        if cy < 40.0 || cy > H as f32 - 40.0 {
            continue;
        }
        app_icon(o, comp, i, cx, cy, 120.0);
        o.push(Op::TextCentered {
            x: cx - 170.0,
            w: 340.0,
            baseline: cy + 66.0,
            s: app.name.clone(),
            c: BAR_FG,
        });
    }
    // Close affordance (a tap anywhere outside a tile also closes).
    let (bx, by) = launcher_close_pos();
    rounded(o, bx - 84.0, by - 34.0, 168.0, 68.0, 34.0, WIN_BG);
    o.push(Op::TextCentered {
        x: bx - 84.0,
        w: 168.0,
        baseline: by + 10.0,
        s: "Close".into(),
        c: BAR_FG,
    });
}

pub fn draw_switcher(o: &mut Vec<Op>, comp: &Compositor) {
    rect(o, 0.0, 0.0, W as f32, H as f32, OVERLAY_BG);
    let wins = comp.switcher_windows();
    let n = wins.len().max(1);
    let cw = 220.0;
    let gap = 24.0;
    let total = n as f32 * cw + (n - 1) as f32 * gap;
    let x0 = (W as f32 - total) / 2.0;
    let y = H as f32 / 2.0 - 120.0;
    for (i, win) in wins.iter().enumerate() {
        let x = x0 + i as f32 * (cw + gap);
        let sel = i == comp.switcher_index;
        rounded(o, x - 6.0, y - 6.0, cw + 12.0, 252.0, 16.0, if sel { ACCENT_SOFT } else { WIN_BG });
        app_icon(o, comp, win.app.unwrap_or(0), x + cw / 2.0, y + 100.0, 96.0);
        let title: String = win.title.chars().take(16).collect();
        o.push(Op::TextCentered {
            x,
            w: cw,
            baseline: y + 226.0,
            s: if title.is_empty() { "window".into() } else { title },
            c: BAR_FG,
        });
    }
    o.push(Op::TextCentered {
        x: 0.0,
        w: W as f32,
        baseline: H as f32 / 2.0 + 180.0,
        s: "Fn+S / 2-finger up — switch".into(),
        c: MUTED,
    });
}

/// Background fill (the whole scene).
pub fn draw_background(o: &mut Vec<Op>) {
    rect(o, 0.0, 0.0, W as f32, H as f32, BG);
}

/// A window (chrome + content) at slide offset `off`.
pub fn draw_window(o: &mut Vec<Op>, comp: &Compositor, win: &crate::compositor::Window, off: f32) {
    let x = win.x + off;
    let y = win.y;
    let focused = comp.focus == Some(win.id);
    if win.is_popup {
        if let Some(tex) = comp.window_texture_id(win.id) {
            let (bx, by, bw, bh) = win.buffer_rect();
            o.push(Op::RectTex { x: bx + off, y: by, w: bw, h: bh, u0: 0.0, v0: 0.0, u1: 1.0, v1: 1.0, tex, c: Color::rgba(1.0, 1.0, 1.0, 1.0), premult: false });
        }
        return;
    }
    rect(o, x, y, win.w, win.h, WIN_BG);
    if let Some(tex) = comp.window_texture_id(win.id) {
        let (bx, by, bw, bh) = win.buffer_rect();
        o.push(Op::RectTex { x: bx + off, y: by, w: bw, h: bh, u0: 0.0, v0: 0.0, u1: 1.0, v1: 1.0, tex, c: Color::rgba(1.0, 1.0, 1.0, 1.0), premult: false });
    }
    // titlebar (server-side decoration only; CSD clients draw their own —
    // otherwise the user sees two title bars / close buttons)
    if !win.csd {
        rect(o, x, y, win.w, crate::compositor::TITLEBAR_H, Color::rgba(0.13, 0.14, 0.16, 1.0));
        if focused {
            rect(o, x, y, win.w, 2.5, ACCENT);
        }
        let title_color = if focused { BAR_FG } else { MUTED };
        let title = if win.title.is_empty() { win.app_id.clone() } else { win.title.clone() };
        o.push(Op::TextClipped { x: x + 12.0, w: win.w - 52.0, baseline: y + crate::compositor::TITLEBAR_H - 11.0, s: title, c: title_color });
        // close (x)
        let cbx = x + win.w - 26.0;
        let cby = y + crate::compositor::TITLEBAR_H / 2.0;
        line(o, cbx - 6.0, cby - 6.0, cbx + 6.0, cby + 6.0, 2.4, Color::rgba(0.8, 0.82, 0.85, 1.0));
        line(o, cbx + 6.0, cby - 6.0, cbx - 6.0, cby + 6.0, 2.4, Color::rgba(0.8, 0.82, 0.85, 1.0));
    } else if focused {
        // A thin accent edge marks focus for undecorated/CSD windows.
        rect(o, x, y, win.w, 2.5, ACCENT);
    }
}

/// Touch-test overlay (`GEMSHELL_TOUCH_TRAIL=1`): draw every finger's
/// path on the scene so touch can be verified on the glass by drawing.
/// A 3-finger touch clears the canvas (see `Compositor::touch_down`).
pub fn draw_touch_trail(o: &mut Vec<Op>, comp: &Compositor) {
    // Distinct colour per finger id so multitouch is visible.
    const PALETTE: [Color; 6] = [
        Color::rgba(0.98, 0.40, 0.42, 1.0),
        Color::rgba(0.40, 0.85, 0.98, 1.0),
        Color::rgba(0.55, 0.95, 0.45, 1.0),
        Color::rgba(0.98, 0.85, 0.35, 1.0),
        Color::rgba(0.80, 0.55, 0.98, 1.0),
        Color::rgba(0.98, 0.60, 0.20, 1.0),
    ];
    let mut prev: Option<(u32, f32, f32)> = None;
    for &(id, x, y) in &comp.trail {
        if id == u32::MAX {
            prev = None; // pen-up break
            continue;
        }
        let c = PALETTE[(id as usize) % PALETTE.len()];
        if let Some((pid, px, py)) = prev {
            if pid == id {
                line(o, px, py, x, y, 7.0, c);
            }
        }
        circle(o, x, y, 5.0, c);
        prev = Some((id, x, y));
    }
    // Legend so the mode is unmistakable on glass.
    o.push(Op::Rect {
        x: 0.0,
        y: 0.0,
        w: W as f32,
        h: 30.0,
        c: Color::rgba(0.0, 0.0, 0.0, 0.45),
    });
    o.push(Op::TextCentered {
        x: 0.0,
        w: W as f32,
        baseline: 21.0,
        s: "TOUCH TEST — draw with your finger · 3 fingers clears".into(),
        c: Color::rgba(0.98, 0.98, 0.98, 1.0),
    });
}

/// Workspace dots above the taskbar.
pub fn draw_workspace_dots(o: &mut Vec<Op>, comp: &Compositor) {
    let dy = H as f32 - TASKBAR_H - 14.0;
    for ws in 0..crate::compositor::N_WORKSPACES {
        let sel = (ws as f32 - comp.ws_target_f()).abs() < 0.5;
        circle(
            o,
            W as f32 / 2.0 + (ws as i32 - 1) as f32 * 22.0,
            dy,
            if sel { 5.0 } else { 3.5 },
            if sel { ACCENT } else { Color::rgba(0.5, 0.52, 0.56, 1.0) },
        );
    }
}
