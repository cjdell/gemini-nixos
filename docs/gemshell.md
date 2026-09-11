# gemshell — the native Gemini PDA Wayland compositor + desktop shell

Last updated: 2026-09-12

A custom, Rust-based, Wayland-native desktop for the Gemini PDA: the
`gemshell` compositor (KMS + Mesa OpenGL, a handful of shell services)
plus an **in-process egui settings panel** (Wi-Fi / Bluetooth / Audio).
Registered as a choosable session — `gemcli session set gemshell` —
alongside gnome / cosmic / niri / console
(docs/desktop-selection.md).

Status: **on glass (2026-09-11)** — the compositor boots, imports the LK
framebuffer (GPU-direct present), renders the shell UI, and runs clients;
the 2026-09-12 rework replaced the separate `gemsettings` wl_shm client
with a GPU-tessellated egui overlay and moved all system-data access
behind the `gemdata::DataProvider` trait (see below). Nested x86_64 dev
loop: `bash bin/gemshell-nested.sh` (host EGL/GBM + wl_shm, seconds per
iteration; it opens the settings panel by default). Host check:
`bash bin/gemshell-host-check.sh`. **Owed: a human look at the glass to
confirm the orientation/rotation direction + touch calibration** (the
2026-09-12 present() fix corrected a left-right mirror; see Orientation).
Full receipt: session-log 2026-09-11 "gemshell ON GLASS" + 2026-09-12.

## Orientation (landscape product / portrait fb)

The Gemini is a landscape clamshell but the LK framebuffer is a
**portrait 1080x2160** buffer. gemwl renders its landscape scene with
`WL_OUTPUT_TRANSFORM_90`; `geminipda-drm` advertises panel-orientation
`LEFT_UP` (=90) for the same reason (driver header, 2026-09-10). gemshell
follows: the logical scene is **2160x1080** (`compositor::W/H`), and
`present()` counter-rotates 90 into the fb in the compute copy
(`COPY_CS`, `fbSize`/`srcSize`/`rotation` uniforms). Touch evdev is
portrait like the fb; `Input::touch_to_scene` applies the matching
inverse rotation.

- `GEMSHELL_ROTATE` (default **270**) — display counter-rotation.
- `GEMSHELL_TOUCH_ROTATE` (default = `GEMSHELL_ROTATE`).

Because display and touch use the same transform, an incorrect direction
would look wrong but still hit the right controls; confirm visually.

**2026-09-12 mirror fix.** The on-glass report was "left-right mirrored,
top/bottom correct". Cause: the compute copy sampled the scene FBO with
`texelFetch`, which indexes the texture's BOTTOM-UP storage directly, so
the intended 90° rotation was actually a transpose (a rotation composed
with a horizontal reflection — exactly a left-right mirror). `COPY_CS`
now flips the scene Y (`t = ivec2(s.x, srcSize.y - 1 - s.y)`) before the
fetch; the mapping dst(p.x,p.y) = scene(p.y, fbW-1-p.x) is now
orientation-preserving. It also matches `touch_to_scene` rotation 90
(`(ny*scene_w, (1-nx)*scene_h)`), so touch and display finally agree.
The rotation DIRECTION (90 vs 270) is still the calibration knob:
`GEMSHELL_ROTATE`/`GEMSHELL_TOUCH_ROTATE`.

**2026-09-12 on-glass result:** `90` rendered the scene **180° out**, so
both defaults moved to **270** (the 180° counterpart, and the exact
inverse of the touch-270 mapping) — confirmed correct.

### Touch (Protocol-B) — the dead-touch root cause

The NT36772 driver reports Protocol B as
`ABS_MT_SLOT` → `input_mt_report_slot_state` (`ABS_MT_TRACKING_ID`) →
`ABS_MT_POSITION_X/Y` (`novatek-nt36xxx.c nvt36xxx_report`). Two bugs
kept touch dead; both are fixed (2026-09-12):

1. **`ABS_MT_SLOT` was `57`** — 57 is `ABS_MT_TRACKING_ID`; SLOT is
   `0x2f = 47`. The match lists SLOT first, so every tracking-id event
   was consumed as a slot change and a finger-down was never
   registered. This was *the* cause (touch was dead even before any
   transform issue).
2. **Emit at SYN_REPORT, not at TRACKING_ID.** Because TRACKING_ID
   precedes `POSITION_X/Y`, the old code emitted the down with the
   previous contact's (or `0,0`) coordinates. `read_touch` now records
   per-slot down/up + changed flags and emits `TouchDown`/`Motion`/`Up`
   at `SYN_REPORT`, when positions are final.

Also `read_events`' evdev fds MUST be `O_NONBLOCK` — the drain loop's
follow-up `read()` otherwise blocks and freezes the whole compositor on
the first event.

### Touch test mode (`GEMSHELL_TOUCH_TRAIL`)

Set `GEMSHELL_TOUCH_TRAIL=1` (the system service sets it by default for
the bring-up) and every finger draws a coloured trail on the scene
(`ui::draw_touch_trail`), one colour per finger id, with pen-up breaks;
normal gestures are bypassed while the mode is on, and a **3-finger
touch clears** the canvas. This is how touch is verified on the glass —
remove the env from `services/gemshell.nix` to restore normal gestures.
For scripted strokes use the repo injector's new multi-point mode:
`tapxy X Y [X2 Y2 …]` (built from `bin/touch-inject.c`).

## Nested mode on x86_64 (development)

`GEMSHELL_NESTED=1` (via `bin/gemshell-nested.sh`) runs gemshell as a
**Wayland client** under the host compositor: headless EGL/GBM on
`/dev/dri/renderD128`, the scene FBO read back and presented into an
`xdg_toplevel` via `wl_shm` (nearest-neighbour downscale,
`GEMSHELL_NESTED_SCALE`, default 0.5), and the host's
pointer/keyboard/touch forwarded into gemshell's own seat. Children
connect to the `wayland-gemshell` socket. This is the fast UI loop — no
device flash. The script sets `GEMSHELL_OPEN_SETTINGS=1` so the egui
panel is visible immediately (any other client via `GEMSHELL_AUTOSTART`).
Nested mode selects `gemdata-dummy` as its `DataProvider`, so the UI is
exercised against in-memory Wi-Fi/BT/audio state without touching the
workstation's network. Build: `nix build .#packages.x86_64-linux.gemshell`.

**egui host-side validation (2026-09-12):** the nested binary also works
headless-ish for verification — run it with `GEMSHELL_SCREENSHOT=/tmp/
x.png GEMSHELL_SCREENSHOT_DELAY_MS=2500 GEMSHELL_OPEN_SETTINGS=1` under a
host Wayland session and it writes the composited scene FBO to a PNG
(useful for checking text regressions without the device).

Build gotchas found the hard way (2026-09-11, aarch64 nix build —
the host `cargo check` never links, so all of these died only on the
real build):

- **libglvnd's `libEGL.so.1` does NOT export the platform entry
  points** (`eglGetPlatformDisplayEXT`, `eglCreatePlatformSurface`) —
  verified by readelf on the store lib. They are resolved through
  `eglGetProcAddress` (the EGL 1.5 way), which libglvnd's dispatch
  answers from the mesa-geminipda ICD (src/compositor/render.rs).
- **The store's xkbcommon 1.13.1 (aarch64) exports only the V_0.5.0-era
  state API**: `xkb_state_mods_for_key`, `xkb_state_key_get_mods`,
  `xkb_state_mods_get_mask`, `xkb_state_mod_get_*` and
  `xkb_state_group_get_index` are all absent from its dynamic symbol
  table (each died with an undefined reference in turn). The wl
  modmap masks are therefore built per slot with
  `xkb_state_mod_index_is_active` (src/compositor/input.rs — the FFI
  block carries the note).
- **`c_char` on aarch64 is `u8`**, `i8` on x86_64 — the hand-rolled
  FFI must say `c_char` (or cast at the call), never `i8`.
- **Link set declared in `pkgs/gemshell/build.rs`**
  (wayland-server/client, xkbcommon, gbm, EGL, GLESv2, png, z) —
  deterministic, and the reason the build doesn't depend on each
  crate's own link emission.

Why this exists: GNOME works but is sluggish on the T880/A72 (GNOME 50
+ gdm + full app suite). gemshell targets the same KMS/GL stack GNOME
proved (geminipda-drm card0 + panfrost via Mesa kmsro, 2026-09-10) but
with a minimal, single-process compositor that draws its own shell UI —
no shell app suite, no daemon zoo.

Status: **build-level** (2026-09-11). On-glass bring-up checklist at the
bottom.

## Components

| Piece | Where | Notes |
|---|---|---|
| `gemshell` (compositor + shell) | `pkgs/gemshell/` (bin `gemshell`) | single process: wayland server + GL renderer + input + chrome + egui settings panel |
| `shell` (egui UI) | `pkgs/gemshell/src/shell.rs` | the settings panel: layout, fonts, widgets; emits GPU triangle meshes |
| `gemdata` | `pkgs/gemshell/crates/gemdata/` | the `DataProvider` trait + data types (Wi-Fi/BT/audio/battery/status) |
| `gemdata-device` | `pkgs/gemshell/crates/gemdata-device/` | real impl + all gemcli device functions (nmcli/bluetoothctl/wpctl/sysfs/i2c/devmem); unit-tested parsers |
| `gemdata-dummy` | `pkgs/gemshell/crates/gemdata-dummy/` | in-memory fake for nested mode |
| `gemcli` | `pkgs/gemshell/crates/gemcli/` (bin `gemcli`) | thin clap frontend over `gemdata-device` (no logic of its own) |
| derivation | `pkgs/gemshell.nix`, `pkgs/gemcli.nix` | rustPlatform, native aarch64, release profile; both build the same workspace |
| session module | `services/gemshell.nix` | `services.gemshellDesktop.enable` (co-installed, marker-selected) |
| session wiring | `services/desktop-select.nix`, `services/scripts/gemini-desktop-apply`, `pkgs/gemshell/crates/gemdata-device/src/session.rs` | mode `gemshell` |

## Architecture (compositor)

Single thread, one `poll()` loop over: the wayland display fd, the
gbm device fd (flip events), the two evdev nodes (keyboard, touch),
with a 16 ms timeout. No async runtime, no threads in the hot path
(one background thread polls status: battery/wifi/bt/volume).

- **Display path** — `/dev/dri/card0` (geminipda-drm, the LK
  framebuffer as DRM/KMS; single fixed mode 1080×2160 portrait, XRGB8888
  shadow plane). gbm device on card0 → gbm surface (XRGB8888) →
  `eglCreatePlatformSurface(EGL_PLATFORM_GBM_MESA)`. eglSwapBuffers
  page-flips through gbm; the flip event on the gbm device fd paces the
  next frame (34 ms timer fallback if the driver never signals).
  Mesa kmsro pairs card0 with renderD128 (panfrost) — exactly the GNOME
  path. GL = GLES 3.x, one textured-quad program, one 2048² glyph atlas,
  icon textures.
- **Wayland** — `wayland-server` 0.31 (system/libwayland backend) +
  `wayland-protocols` 0.32 (server). Globals: `wl_compositor`,
  `wl_shm` (hand-implemented pool/buffer state), `wl_seat`
  (keyboard+pointer+touch), `wl_output`, `xdg_wm_base`
  (xdg-shell v1, incl. minimal xdg-popup for menus), no
  linux-dmabuf in v1 (clients ship wl_shm buffers — the gemdemo-
  verified path; dmabuf/EGLImage import is the follow-up for
  GPU-direct apps).
- **Clients** — any standard wl_shm xdg-shell client: GTK apps (via
  GTK's wayland backend), Qt (ozone-wayland), gemdemo,
  labwc? (labwc is a compositor — no). Chrome/Firefox work when
  launched inside the session (their GL goes through the same
  /run/opengl-driver ICD; launched with LD_LIBRARY_PATH carrying the
  glvnd client libs so dlopen-based loaders find them).
- **Input** — raw evdev (protocol-B multitouch from the NT36772;
  EV_KEY from the AW9523 gpio-matrix keyboard), xkbcommon keymap from
  layout `gemini` (XKB_CONFIG_EXTRA_PATH, same as every other desktop
  here). The compositor consumes gestures itself and forwards
  one-finger interactions as wl_touch / synthetic wl_pointer to
  clients.

### Windows, workspaces, snapping

- One window = one xdg_toplevel. Compositor-drawn titlebar (36 px):
  title, maximize button, close button. Window size follows the client
  buffer; on maximize/snap the compositor requests a new size via
  xdg_toplevel configure and letterboxes the client buffer if the
  client does not follow.
- Workspaces: 3 horizontal pages (1 = center, 2 = left, 3 = right of
  the gesture direction). A 2-finger horizontal swipe drags the
  page strip (both adjacent pages visible while swiping) and switches
  on release.
- Snapping: drag a window by its titlebar — edges light up a translucent
  preview (left/right half, top/bottom half, center-top = maximize);
  release applies it. Keyboard: Fn+←/→/↑-drag etc. (table below).
- Focus: raise-on-tap, no focus-follows. The focused window gets a
  highlighted titlebar; Escape/3-finger-down drops overlays.

### Shell UI (compositor-drawn)

- **Status bar** (top, 44 px): time (left); right side tap targets:
  gear (opens the settings panel), speaker, bluetooth, wifi, battery
  (% + charge bolt). Tapping gear/speaker/wifi/bt opens the in-process
  egui settings panel on the matching section.
- **Settings panel** (egui, full-screen modal): Wi-Fi / Bluetooth /
  Audio tabs. See "Settings panel (egui)" below. There is no separate
  settings client any more (the old `gemsettings` binary was removed
  2026-09-12).
- **Taskbar** (bottom, 84 px): launcher button (grid) + one tile per
  running app (icon or letter tile, title under it, running dot).
  Tap = focus + bring its workspace to front; tap the focused one again
  = hide (minimize); the focused app shows a highlight bar.
- **Launcher**: full-screen app grid (icons + names, 1-finger scroll),
  built from the XDG desktop dirs (`/usr/share/applications`,
  `/home/cjdell/.local/share/applications`, `/root/.local/share/…`).
  Tap launches (Exec= parsing, `%u %U %f` stripped, `Terminal=`/
  `NoDisplay=` respected).
- **App switcher** (Fn+Tab / 2-finger up-swipe): center row of running
  apps, Tab cycles, release activates; 2-finger up-swipe = next app.

### Input model (multitouch, no mouse)

The touch panel is the only pointer (10-pt multitouch, raw portrait —
identity mapping to the 1080×2160 scene, confirmed 2026-09-10).
Gestures are decided by the compositor BEFORE forwarding:

| Fingers | Gesture | Action |
|---|---|---|
| 1 | tap status-bar/taskbar/titlebar/launcher | UI action |
| 1 | tap window content | forward to app (click) |
| 1 | drag from titlebar | move window (+ snap previews) |
| 1 | drag window content | forward to app (scroll) |
| 1 | double-tap titlebar | maximize/restore |
| 1 | drag in launcher | scroll |
| 2 | horizontal swipe | swipe workspaces (live slide) |
| 2 | up-swipe | next app (switcher) |
| 3 | up-swipe | open launcher |
| 3 | down-swipe | close overlay / home |

When a 2nd/3rd finger lands during a forwarded touch, the compositor
sends `wl_touch.up` for the forwarded finger(s) first (clients must
cancel their gesture), then the compositor gesture takes over.

### Keyboard shortcuts (layout `gemini`, Fn = level-3 mod)

| Action | Keysym | Physical |
|---|---|---|
| App switcher | Mod5+Tab (Tab level 0) | Fn+Tab |
| Previous workspace | Mod5+Prior | Fn+↑ |
| Next workspace | Mod5+Next | Fn+↓ |
| Snap left / right | Mod5+Home / Mod5+End | Fn+← / Fn+→ |
| Maximize / restore | Mod5+grave | Fn+M |
| Close focused window | Mod5+Delete | Fn+Backspace |
| Launcher | XF86TaskPane | Fn+A |
| Volume − / + / mute | XF86AudioLower/RaiseVolume, XF86AudioMute | Fn+C / Fn+V / Fn+T |
| Brightness − / + | XF86MonBrightnessDown/Up | Fn+B / Fn+N |
| Close overlay | Escape (level 0) | Esc |

Media XF86 keysyms are matched regardless of modifier mask (the Fn
layer carries Mod5 through the ISO_Level3_Shift modifier).

## Data access (`gemdata`)

All system-data reads/writes go through ONE trait, `gemdata::
DataProvider` (`pkgs/gemshell/crates/gemdata/src/lib.rs`): `status()`,
`wifi()` / `set_wifi_enabled` / `scan_wifi` / `connect_wifi` /
`disconnect_wifi`, `bluetooth()` / `set_bluetooth_enabled` /
`connect_bluetooth` / `scan_bluetooth`, `audio()` / `set_default_sink` /
`set_volume` / `set_muted`, `set_brightness`. The trait and its data
types (`WifiState`, `BtState`, `AudioState`, `ShellStatus`, …) are UI-,
GL- and wayland-free.

Two implementations:

- **`gemdata-device`** — the real one. Each subsystem is a thin
  command/sysfs wrapper around a *pure parser* (`parse_nmcli_wifi_list`,
  `parse_wpctl_status_sinks`, `parse_bluetoothctl_show`, …), so the
  text formats are unit-tested with plain `cargo test` off-device. It
  also carries ALL the gemcli device functions (a72, backlight,
  battery, boot, devmem, gpio, gpu, guard, i2c, power, profile,
  session, sleep, speaker, status, wdt) — moved out of the gemcli
  crate 2026-09-12 so `gemcli` and gemshell share ONE implementation.
- **`gemdata-dummy`** — in-memory fake (selected in
  `GEMSHELL_NESTED` mode) whose mutations actually change the fake state,
  so toggles/connect/password flows round-trip on the workstation.

`gemcli` is now a thin clap frontend over `gemdata-device` (parse →
call; no logic). The gemshell status poller
(`compositor/status.rs`) reads `DataProvider::status()` every 2 s; the
settings panel reads the full snapshots on open / after a mutation /
every 3 s. The actual interfaces:

- **Battery** — sysfs `/sys/class/power_supply/bq25890-battery-*`
  (capacity is voltage-derived, no fuel gauge); `bq25890-charger-*`
  `status`/`online` for the charge bolt.
- **Wi-Fi** — `nmcli` (NetworkManager): `radio wifi`, `device wifi
  list`, `device wifi connect`, `connection down`.
- **Bluetooth** — `bluetoothctl show/devices/connect/scan`.
- **Volume / sinks** — `wpctl get-volume` / `wpctl status` /
  `set-default` / `set-volume` / `set-mute` (PipeWire at /run/gemwl-audio).
- **Brightness** — `/sys/class/backlight/*/brightness` (world-writable
  via the plumbing udev rule).

## Settings panel (egui)

An **in-process, GPU-tessellated** overlay drawn by the compositor
(`src/shell.rs` + `compositor/render.rs::draw_egui`). No separate client,
no CPU rasterization:

- egui does font loading/shaping, layout, scrolling, focus and text
  editing. It emits premultiplied triangle meshes + a font-atlas texture
  delta; `draw_egui` uploads the atlas and draws the meshes with a GLES
  index shader (`EGUI_VERT`/`EGUI_FRAG`), clip rects → GL scissor. The
  UI runs at `PPP = 2.0` (1080×540 points for the 2160×1080 scene).
- Screens: **Wi-Fi** (radio toggle, network cards with signal/security,
  Connect / password dialog, Disconnect), **Bluetooth** (power, known
  devices, Connect, scan), **Audio** (volume slider, mute, output/sink
  radio list → `set_default_sink`).
- Fonts: `GEMSHELL_FONT` (DejaVu on device) is registered first with
  egui's bundled fonts as fallback, so text never disappears.
- Open with the status-bar gear / wifi / bt / speaker zones;
  Escape or Close dismisses. `GEMSHELL_OPEN_SETTINGS=1` opens it at
  startup (dev).

**2026-09-12 text/UI fix.** The previous hand-rolled glyph atlas had a
double-origin bug in `common/font.rs`: `OutlinedGlyph::draw` already
yields raster-local pixel coordinates, but the code subtracted
`bounds.min` again, pushing every glyph above the baseline out of its
raster — the all-blank text. The atlas build now uses the coordinates
directly. (The chrome text in `ui.rs` uses the same fixed atlas.)

## Session wiring

`gemcli session set gemshell [--apply]` writes the marker
(`/var/lib/gemini/desktop`); on the next boot
`gemini-desktop-apply` (Before=display-manager.service) creates the
`/run/gemini-console` sentinel (GDM skipped) WITHOUT starting a tty1
getty (no chvt — the compositor owns the panel; a console on tty1
would fight the shadow-plane blit for the same scanout memory).
`gemini-gemshell.service` (User=cjdell) runs the compositor after the
GPU power-on + panfrost-load (same ordering as the GNOME module) and
force-disables the nested gemwl/phosh/LXQt stack (they would double-
drive the panel via /dev/gemfb).

cjdell groups gain `input` (/dev/input evdev) and `bluetooth`
(bluetoothctl) — `video`/`audio`/`networkmanager` already there.

## Footprint (target, honest numbers owed on glass)

The GL floor is Mesa/panfrost: expect the compositor RSS in the
~40–60 MB range (comparable to any GL app; weston ≈ 50 MB), NOT
literally "a handful of MB" — that budget is unreachable once the GPU
driver is loaded, same as for GNOME's mutter. The point vs GNOME: one
~50 MB compositor replaces gdm + mutter + gsd-* + gnome-shell's app
suite (hundreds of MB), and nothing else runs by default. Measure
`smem`/`/proc/*/status` on glass and record here.

## Build

- x86_64 fast loop: `bash bin/gemshell-host-check.sh` (cargo check/test
  of the whole workspace in a nix shell: cargo/rustc/pkg-config +
  wayland/libxkbcommon/libglvnd). `--test` runs the data-crate unit
  tests (the gemshell bin can't link on the host).
- aarch64 (real): `sudo nix build --store local .#packages.aarch64-linux.gemshell`
  (or the full toplevel; long builds under `bin/run-job.sh`). `gemcli`
  builds from the same workspace via `pkgs/gemcli.nix`
  (`buildAndTestSubdir = crates/gemcli`).
- Cargo.lock is committed (crate set pinned, incl. egui 0.29).
- The nix `src` excludes the host `target/` tree (`cleanSourceWith`).

## On-glass bring-up checklist (owed)

1. Boot with `gemcli session set gemshell --reboot`; confirm the panel
   shows the gemshell scene (NOT the uninitialised flicker — rule 5:
   if it flickers, stop → TWRP → fix the image).
2. Status bar: time advances; battery % ≈ UPower's; wifi icon tracks
   the NM connection; tapping gear/wifi/bt/sound opens the settings panel.
3. Settings panel (egui): connect a known Wi-Fi (SSID+password over the
   hardware keyboard), toggle BT, pick a sink, move the volume slider.
4. Touch: tap-launch from the launcher; drag a window by its titlebar
   to an edge (snap preview + snap); double-tap titlebar maximize;
   2-finger swipe switches workspaces (slide); 2-finger up = next app;
   3-finger up/down launcher/home; one-finger scroll inside an app.
5. Keyboard: Fn+Tab switcher, Fn+←/→ snap, Fn+↑/↓ workspaces,
   Fn+M maximize, Fn+⌫ close, Fn+B/N brightness (panel dims),
   Fn+C/V/T volume.
6. Run gemdemo under gemshell (GL client path) at 60 fps.
7. Record: RSS (compositor), fps, boot time, anything that needed a fix
   here + in the session log.
