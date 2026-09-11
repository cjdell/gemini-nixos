# Vanilla GNOME on the Gemini PDA — feasibility (the KMS path)

Last updated: 2026-09-12. Status: ✅ **ON GLASS — GNOME is the default desktop,
GPU-accelerated**. Verified 2026-09-10 on the device (generation 66): the
geminipda-drm KMS device binds, mutter selects it as primary from the
built-in DSI panel, and renders through **panfrost via Mesa kmsro**
(`kmscube` on card0 → `Mali-T880 (Panfrost)`). GNOME survives a clean
reboot with zero failed units. The on-glass checks and receipts are at the
end of this doc.

> **2026-09-12 update:** the nested Phosh and LXQt desktops (and the
> COSMIC/niri GDM sessions) were removed from the project. GNOME,
> `gemshell` and the fbcon console are the supported set now; references
> below to Phosh/LXQt as a rollback are historical. The KMS work and the
> GNOME receipts remain current.

Background: GNOME 50 (the pinned nixpkgs' `mutter`/`gnome-shell` **50.4**)
cannot run on the pre-KMS graphics stack at all, which is why the kernel
work below was the prerequisite.

This doc answers: "Phosh is essentially built on GNOME, so vanilla GNOME
should be possible — make it the default desktop, no X11/Xwayland."

## TL;DR

You are right that it is *possible* — but only if the LCD is exposed as a
real **DRM/KMS** device. GNOME Shell (`gnome-shell` ⇄ `mutter`) is not a
shell that runs on a compositor; it **is** the compositor, and its only
non-KMS backend is `--headless` (no output). Phosh is a different animal:
a GTK4/libadwaita shell **client** plus its own wlroots compositor
(`phoc`), which is why it slots into gemwl. "Phosh reuses GNOME tech" ≠
"GNOME Shell can nest the way Phosh does".

So the choice is:

1. **Expose the LK framebuffer as DRM/KMS** (`simpledrm`-style driver) →
   GNOME Shell runs Wayland-native, no X11/Xwayland, no gemwl/nesting.
   **Implemented 2026-09-10** — see "What was built" below. Still
   requires on-glass verification (rule 5: it touches the display path,
   but never re-initialises the panel — LK keeps that job).
2. **GNOME 49 + Xwayland-nested** — rejected: you said no X11/Xwayland,
   and it would require a second nixpkgs pin just for GNOME 49 (golden
   rule 9) plus the fragile nested stack.
3. **Stay on a wlroots shell + GNOME apps** — LXQt (already in-tree) is
   the most complete shell today; Phosh is the mobile one you find WIP.
   Zero risk. `services.geminiGnomeApps` gives you calculator/calendar/
   maps/… in either.

## Evidence: GNOME Shell 50 has no non-KMS, non-headless backend

Read from the exact pinned source, **mutter 50.4**
(`nix build .#nixosConfigurations.gemini.pkgs.mutter.src`,
`/nix/store/qy48nabmz4k5gv0bpwi442pnvj613abj-mutter-50.4.tar.xz`, 2026-09-10):

- `src/backends/` contains the shared code and **`native/`**. There is
  **no `src/backends/x11/` directory**, and no `x11/` nested subbackend
  (GNOME 49 disabled X11 by default; GNOME 50 removed it — Phoronix,
  heise, 2026).
- Grepping the whole `src/` tree for `META_TYPE_BACKEND_X11`,
  `BACKEND_X11_NESTED`, `x11_nested` → **no matches**. The nested backend
  (`create_nested_backend`) is gone with the X11 backend.
- `src/core/meta-context-main.c` offers exactly two backends:
  `create_headless_backend()` (`--headless`, "Run as a headless display
  server", no scanout) and the default native backend. There is no
  `--nested`.
- `src/backends/native/` is KMS in its bones: `meta-crtc-kms.c`,
  `meta-gpu-kms.c`, `meta-drm-buffer.c/.h`, `meta-drm-buffer-gbm.c`,
  `meta-drm-buffer-dumb.c`, `meta-drm-lease.c`, `meta-device-pool.c`.
  The native backend needs a `/dev/dri/cardN` with a modesetting
  pipeline + GBM.

The device has **no display-capable KMS card**: the panel is a raw
LK-initialized framebuffer (`geminipda-fb` → fbdev `/dev/fb0` +
`/dev/gemfb` dma-buf; `devices/planet-geminipda/kernel/delta/drivers/
video/fbdev/geminipda-fb.c`), and panfrost is a DRM device with
`DRIVER_RENDER` only — it owns `/dev/dri/card0` *and* `renderD128` (a
card minor is allocated even without `DRIVER_MODESET`, verified from
`drm_dev_register()`), but has **no display connectors**. So the missing
piece is a display-controller KMS card, not a render device. (The
MediaTek display DRM drivers are deliberately excluded — rule 5.)

## Why gemwl can't host GNOME (the "Phosh works, GNOME should too" gap)

The nesting pattern that works here is: *a full wlroots compositor as a
Wayland client of gemwl*. gemwl (`pkgs/gemwl/gemwl.c`) is an xdg-shell
kiosk that owns `/dev/gemfb`; labwc and phoc implement wlroots' `wayland`
backend and so can be its clients. GNOME Shell/mutter has no "Wayland
client" backend at all (only KMS-native or headless), so it cannot be a
client of gemwl — independent of any protocol gemwl does or does not
implement. This is the same reason `mutter --nested` historically needed
X11: nested mutter was `MetaBackendX11Nested`, i.e. an X11 client.

## The Wayland-native path: make the LK framebuffer a KMS device

Linux already has the exact pattern for "firmware left a framebuffer, we
have no display driver": **`simpledrm`** (`drivers/gpu/drm/tiny/
simpledrm.c`, verified in the v6.6 base). It:

- binds a `simple-framebuffer` platform device (OF node or platform
  data), reading width/height/stride/format and a memory region;
- owns the fixed framebuffer memory (`devm_aperture_acquire_from_firmware`
  + `devm_ioremap_wc`/`devm_memremap`);
- exposes a normal atomic KMS pipeline (primary plane + CRTC + encoder +
  connector) and a generic fbdev for the console;
- uses the **shadow-plane** helpers
  (`DRM_GEM_SHADOW_PLANE_HELPER_FUNCS`, `drm_gem_fb_shadow_plane`): every
  atomic update blits the GEM framebuffer into the fixed scanout memory
  (`simpledrm_primary_plane_helper_atomic_update` → `drm_fb_blit` →
  `sdev->screen_base`). That is exactly the "render elsewhere, copy into
  the LK OVL region" model, in-kernel.

Concretely, the delta we would build:

1. **Kernel config** (lean config): `CONFIG_DRM_KMS_HELPER=y`,
   `CONFIG_DRM_GEM_SHMEM_HELPER` (already `=m`),
   `CONFIG_DRM_SIMPLEDRM=y`, `CONFIG_DRM_FBDEV_EMULATION=y` (and the
   sysfb/aperture bits simpledrm needs).
2. **Register the LK fb as a `simple-framebuffer` platform device.**
   Reuse the already-written geometry parser
   (`geminipda_fb_get_geometry()` in `geminipda-fb.c`, which decodes
   `/chosen/atag,videolfb`) and hand the region to simpledrm — either as
   `struct simplefb_platform_data` (`width=1080, height=2160,
   stride=4352, format="a8r8g8b8"`; 4352 from the OVL 1088-px row pitch)
   plus an `IORESOURCE_MEM`, or as an OF `simple-framebuffer` node with a
   `memory-region`. The existing fbdev/dma-buf registration in
   `geminipda-fb.c` then retires (one owner of the region; otherwise
   `devm_aperture_acquire_from_firmware`/`request_mem_region` collide).
3. **Render split**: mutter renders through panfrost and presents to the
   new KMS card. This is standard on ARM (Mali is 3D-only) and is handled
   by Mesa's **kmsro** layer, which is already compiled into the fork —
   see the corrected rendering section below for the full receipt chain.
   The on-glass unknown is not mutter but whether kmsro's dumb-buffer
   sharing works between the shmem KMS card and panfrost.
4. **GNOME session integration**: this is *not* a drop-in for the current
   model. The desktop today is a **systemd system service with no logind
   session** (services/*.nix, cjdell user). GNOME wants a real
   graphical session: logind seat, `gnome-session`, session bus, dconf,
   `gnome-settings-daemon`, xdg-desktop-portal(-gnome), possibly GDM.
   (In the event the standard NixOS `services.desktopManager.gnome` +
   `displayManager.gdm` modules were used directly and went Wayland-native
   on the KMS card — see "What was built" below.)
5. **Retire gemwl + the nested desktops** once GNOME is the default:
   with KMS, nothing needs to own the framebuffer in userspace. (Keep
   them buildable as the rollback path.)

### Risks / guardrails (rule 5)

- This touches the **display path**. The panel init stays entirely in LK;
  a shadow-plane driver only writes the already-initialized region — it
  does not re-init the NT36672. Still: build + flash only behind the
  normal safe cycle (para = `boot-recovery` until verified), keep the
  current boot.img/rootfs as rollback, and judge the glass by eye/TWRP
  (never a capture).
- Two framebuffer owners during the transition is the main footgun; the
  gemfb fbdev must not race simpledrm for the region.
- Real work, not an afternoon: kernel delta + config + a new session
  module + on-glass iteration. It should be its own dated session(s).

## App support (what is installed now)

`services/gnome-apps.nix` (default on) puts the GNOME applications on the
system profile: gnome-calculator, gnome-calendar, gnome-maps,
gnome-clocks, gnome-weather, gnome-contacts, gnome-characters,
gnome-text-editor, gnome-system-monitor, gnome-disk-utility,
gnome-connections, gnome-usage, baobab, seahorse, file-roller, papers,
loupe, snapshot, gnome-screenshot, gnome-console; plus
adwaita-icon-theme, gsettings-desktop-schemas and
gnome-online-accounts. They are ordinary Wayland clients and appear in
the app grid of Phosh or LXQt.

Known app-service gaps on this stack (same root cause: no gnome-session):

- **Location** (Maps/Weather/Clocks): `services.geoclue2` is enabled, but
  geoclue needs a shell-side location *agent* to authorise requests;
  gnome-shell provides one on vanilla GNOME, Phosh/LXQt do not. Without a
  fix, Maps opens but cannot place you.
- **Online accounts** (Calendar/Contacts sync): `gnome-online-accounts`
  is installed and D-Bus-activatable, but no session manager starts the
  daemon eagerly; local calendars/contacts work.

## What was built (2026-09-10)

### 1. Kernel: `geminipda-drm`, a DRM/KMS driver for the LK framebuffer

`devices/planet-geminipda/kernel/delta/drivers/gpu/drm/tiny/geminipda-drm.c`,
modelled on upstream `simpledrm`:

- binds the DT node `planet,geminipda-drm` (added to the board DTS next to
  the existing `planet,geminipda-fb` node); reads the LK scanout base/size
  from `/chosen` with the same `atag,videolfb` parser as `geminipda-fb.c`;
- one CRTC + one primary **shadow plane** (GEM buffer blitted into the
  fixed scanout memory on every atomic update) and one
  `DRM_MODE_CONNECTOR_DSI` connector (DSI makes mutter treat it as a
  built-in panel);
- fixed mode **1080x2160**, stride **4352** (`ALIGN(1080,32)*4`), format
  **ARGB8888** (LK `eBGRA8888`);
- advertises the standard **panel orientation** property, default
  **Left Side Up** (90°), so mutter renders the landscape desktop rotated
  into the portrait buffer — the same transform gemwl applies with
  `-t 90`. Module parameter `panel_orientation` allows 0..3 for on-glass
  tuning;
- forces the alpha byte to `0xff` after each blit (the `ARGB8888 renders
  BLACK` receipt in `pkgs/gemwl/gemwl.c`).

Kconfig/Makefile live in the delta (`drivers/gpu/drm/tiny/`), and
`bin/prune-kernel-config.sh` now emits `CONFIG_DRM_GEMINIPDA=m` +
`CONFIG_DRM_KMS_HELPER=m` (kept past its DRM prune). The rule-5 gate in
`kernel/default.nix` still passes: no mediatek-drm/mtk-mmsys/DSI-PHY is
enabled.

Coexistence: `FB_GEMINIPDA` (the fbdev/`/dev/gemfb` view gemwl uses) stays
enabled, so the same kernel still supports the nested desktops as a
rollback. Only one desktop stack drives the screen at a time.

### 2. Userspace: `services/gnome.nix`

The **standard NixOS modules** — `services.desktopManager.gnome` +
`services.displayManager.gdm` + autologin — rather than a bespoke session.
It force-disables the legacy gemwl compositor (mutually exclusive), keeps
this repo's custom PipeWire (`services.pipewire.enable = lib.mkForce false`; the
custom `pipewire.service` still satisfies GNOME), loads panfrost after
`gemini-gpu-poweron` (gemwl used to do this), and sets the gemini xkb
layout. Option: `services.gnomeDesktop.enable` (**default true** in
`config/gemini.nix`).

**Rendering: GPU-accelerated via Mesa `kmsro` (corrected 2026-09-10).** An
earlier draft of this doc claimed GNOME could only run software-rendered.
That was wrong, and the `softwareRendering` option it produced (which sets
`LIBGL_ALWAYS_SOFTWARE=1`) would actively defeat the real path. The correct
mechanism, read end-to-end from source:

- The live device shows **panfrost owns `/dev/dri/card0`** (plus
  `renderD128`) — `ls -l /dev/dri` on the PDA, 2026-09-10. Panfrost is a
  DRM device with `DRIVER_RENDER` and no `DRIVER_MODESET`; the kernel's
  `drm_dev_register()` (`drivers/gpu/drm/drm_drv.c`) still allocates and
  registers the **primary (card) minor** for it, so mutter *does* see it.
  "Render-only" here means "no display connectors", not "no card node".
- Mali is 3D-only (no display controller), so Mesa's **`kmsro`** layer
  exists precisely to pair a render-only GPU with a display controller.
  It is **already built into `pkgs/mesa-geminipda.nix`**: the mesa build
  auto-enables kmsro whenever a renderonly driver (panfrost) is enabled
  (`meson.build`: `with_gallium_kmsro = … contains(true)`), and the fork's
  `libgallium-25.0.7.so` exports `kmsro_drm_screen_create`,
  `panfrost_drm_screen_create_renderonly` and `pipe_kmsro_create_screen`
  (verified in the store, 2026-09-10).
- For our unknown KMS driver name, Mesa's pipe-loader falls back to kmsro:
  `pipe_loader_drm_probe_fd_nodup()` does `get_driver_descriptor(<name>)`,
  fails, then `get_driver_descriptor("kmsro")` — "kmsro supports lots of
  drivers, try as a fallback" (`src/gallium/auxiliary/pipe-loader/
  pipe_loader_drm.c`). `dri2_init_screen()` reaches exactly that path
  (`src/gallium/frontends/dri/dri2.c`).
- `kmsro_drm_screen_create()` then pairs the display device with a render
  device via `pipe_loader_get_compatible_render_capable_device_fds()`, which
  **only requires the display device to be on the platform bus** ("For
  platform display-only devices, we try to find a render-capable device on
  the platform bus …", then `loader_open_render_node_platform_devices()`
  over `[panfrost, panthor, …]`). Our `geminipda-drm` is a platform device
  (DT node). Mesa's EGL DRM path also calls this directly as
  `dri_query_compatible_render_only_device_fd()` (`platform_drm.c`
  `get_fd_render_gpu_drm()`).
- Net effect: EGL on `geminipda-drm` is a **panfrost-backed renderonly
  screen**; `GL_RENDERER` is `Mali-T880 (Panfrost)`. Mutter's
  `meta-render-device.c` marks a render device hardware-accelerated unless
  `GL_RENDERER` starts with `llvmpipe`/`softpipe`/`swrast`. So
  `choose_primary_gpu_unchecked()` (which prefers a GPU with a connected
  built-in panel *and* hardware rendering) selects `geminipda-drm` as its
  primary GPU and renders through panfrost — GPU-accelerated, no nesting,
  no bespoke compositor, and mutter scans out on the same card.

So `services/gnome.nix` does **not** set software rendering by default;
`softwareRendering = true` is a debug fallback only if kmsro ever fails to
pair on glass. **The one genuine, still-unproven risk** is the kmsro
sharing path at runtime: `panfrost_create_kms_dumb_buffer_for_resource`
allocates a dumb buffer on the `geminipda-drm` (shmem) card and imports it
into panfrost for rendering, then the shadow plane blits it back. Both
sides support it in principle (`DRM_GEM_SHMEM_DRIVER_OPS` provides
`dumb_create` + prime import/export), but it must be confirmed on glass
(see the checklist).

## On-glass results (2026-09-10 — all verified on the device)

1. ✅ **KMS boot.img flashed** (sha256 `1d2f350a…`, 9.52 MiB). Previous
   boot backed up to `stock-dump/boot-20260910-134318.img`. Flashed via
   `bin/flash-nixos.sh boot`, safe cycle kept para = `boot-recovery`
   until verified, then `boot-nixos`.
2. ✅ **The device**: `/dev/dri/card0` (geminipda-drm) + `/dev/dri/card1`
   (panfrost) + `renderD128`. `drm_info`: connector DSI,
   **`Status: connected`**, mode **1080×2160@60 preferred**,
   **`panel orientation … = Left Side Up`**. `crtc-0: active=1`,
   `crtc-pos=1080x2160+0+0` (the rotated scanout). geminipda-fb keeps the
   console; the two views coexist.
2b. ✅ **kmsro pairing confirmed — this was the make-or-break check.**
   `kmscube` on card0 (with the fork mesa env):
   `OpenGL ES 3.1 Mesa 25.0.7`, **`renderer: "Mali-T880 (Panfrost)"`**.
   GPU acceleration is real, not software.
3. ✅ **Orientation correct with the default** (`panel_orientation` =
   Left Side Up); gemwl's old `-t 90` transform is now expressed by the
   standard DRM property and rendered by mutter.
4. ✅ **GNOME deployed** (`services.gnomeDesktop.enable = true`, gen 66)
   and **boots unattended**: after a `systemctl reboot` the unit
   `display-manager` came back **active** in ~48 s with no manual step.
5. ✅ **GNOME Shell on panfrost**: journal —
   `Added device '/dev/dri/card0' (geminipda-drm) using atomic mode setting.`
   `Created gbm renderer for '/dev/dri/card0'`
   `GPU /dev/dri/card0 selected primary from builtin panel presence`;
   gnome-shell holds **both** `/dev/dri/card0` and `/dev/dri/renderD128`
   open (the kmsro pair); **no** llvmpipe/swrast/"software rendering"
   warnings; `systemctl --failed` = **0 units**. Auto-login as `cjdell`.

Known, expected limitations (not regressions):

- **Xwayland**: no X server is running (Wayland-native). Mutter advertises
  `Using public X11 display :0` and spawns Xwayland only on demand, so
  nothing X11 is active. A `mutter` built with `xwaylandSupport = false`
  would remove it entirely (an overlay + rebuild of gnome-shell; not done
  because nothing needs it).
- **Geolocation**: `Error looking up permission: … No entry for
  geolocation` — the geoclue/portal location agent gap noted below; Maps
  opens but cannot place you yet.
- **Camera**: `Failed to start camera monitor` — no camera present; the
  Snapshot app cannot work (hardware limitation, unrelated to GNOME).
- Benign noise: `Failed to obtain high priority context`, `g_close(fd:0)
  EBADF`, and dbus "Ignoring duplicate name" lines from overlapping
  portal/ibus store paths.

## Options if the kmsro pairing does not hold on glass

The primary path needs no new code. If step 2b shows swrast instead of
panfrost, in rough order of preference:

1. **Fix the kmsro pairing inputs.** Most likely causes are recoverable:
   our card not detected as a platform device, or
   `loader_open_render_node_platform_devices()` not matching panfrost. Both
   are debuggable with `MESA_DEBUG=1` and are the cheapest first move.
2. **Give the pair an explicit name.** If the generic "unknown driver →
   kmsro" fallback is bypassed, add `geminipda-drm` to the kmsro driver
   list / provide a `geminipda-drm_dri.so → kmsro` alias in the mesa fork
   (one line; we already carry the fork).
3. **Let mutter use panfrost as the render primary and `geminipda-drm` as
   the scanout secondary.** Mutter supports this (`init_secondary_gpu_data`,
   `meta-onscreen-native.c` cross-GPU copy); it needs the secondary to be
   hardware-accelerated too, which kmsro provides. Same pairing, expressed
   at the mutter layer.
4. **Implement real MediaTek OVL/MMSYS KMS** (mainline `mediatek-drm` for
   MT6797) instead of the LK-framebuffer driver. The most "standard"
   long-term form, and Mesa's kmsro already knows the `mediatek` display
   name — but it is a much larger project and it touches the display/DSI
   path that rule 5 guards (LK owns panel init). Only worth it if the
   simpledrm-style card proves limiting.
5. **Debug fallback, not a solution:**
   `services.gnomeDesktop.softwareRendering = true` (llvmpipe) to get a
   usable session while working the above.

Nothing here changes the conclusion that the KMS device is the right
foundation: it is what makes options 2/3/4 possible at all.

## Recommendation

The KMS driver plus kmsro is the standards-compliant, GPU-accelerated path
and it needs no further software work before testing. Until the new
boot.img is flashed and step 2b passes, keep the nested desktop as the
default; flip `services.gnomeDesktop.enable` only after the kmsro pairing
is confirmed on glass. The GNOME apps are already available under the
current shells regardless.
