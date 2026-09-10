# Handover — GNOME on the Gemini PDA: fix sluggishness + touch rotation

**Date:** 2026-09-10 (written at the end of the session that landed GNOME).
**State:** GNOME 50.4 is the DEFAULT desktop, boots unattended, **display is
correct-orientation and GPU-capable**, but the user reports two defects to
fix next session:

1. **UI is very sluggish — "feels like software rendering."**
2. **Touchscreen is rotated 90°** (touching the left side activates the
   right side of the UI).

Everything below is from live recon on the device (gen
`3w3xh4hwr52ynd50mn645nl8paiqpwi2-nixos-system-gemini-26.11pre-git`,
kernel `ribq69k94rz3nl88p5vzmjd70d8h2z89-linux-6.6.0`, boot.img sha256
`1d2f350a…`), not speculation. **Nothing in this handover has been changed
yet** — the device is left exactly as the user just saw it.

> **2026-09-10m update — keyboard + touch.**
> - **Keyboard (was not in this handover, reported live): RESOLVED.** The
>   gemini layout is now a first-class xkb layout: a copy of
>   xkeyboard-config with `symbols/gemini` plus a `<layout>` registry
>   entry, pointed at by `XKB_CONFIG_ROOT` (new
>   `pkgs/gemini-xkeyboard-config.nix`). Before that, GNOME Shell's
>   `XkbInfo` (libxkbregistry) could not find "gemini" and silently used
>   `us`. Verified: the keymap mutter sends clients now has
>   `<AE01> = [1, !, |, F1]`, `<AE03> = [3, £, \, F3]`,
>   `<RALT> = ISO_Level3_Shift`. See `docs/session-log.md` 2026-09-10m.
> - **Touch 90°:** the 2026-09-10l fix was right (kernel reports raw
>   portrait; mutter applies the panel-orientation transform) but left a
>   **180°** error — the sensor is mounted 180° relative to the panel.
>   Added `touchscreen-inverted-x` + `-y` (a 180, which commutes with
>   mutter's 90). **Confirmed on glass 2026-09-10m.**

> **2026-09-10k update — Issue 1 root cause FOUND, fixed, deployed and
> verified (RESOLVED).**
> The user's extra clue ("~99 % kworker while scrolling") was reproduced by
> running `gemdemo` as a client of the GNOME session (it rendered at **6
> fps**); `echo l > /proc/sysrq-trigger` then put
> `geminipda_drm_primary_plane_helper_atomic_update+0x1f8/0x228` in
> `kworker/u20:1+events_unbound` (the DRM atomic-commit work). The culprit
> is the driver's per-pixel `writeb()` alpha loop — 2.33 M barriered byte
> stores per full-screen update — and it is also **redundant**: the plane
> advertises XRGB8888 (`format=XR24` in
> `/sys/kernel/debug/dri/0/framebuffer`) and `drm_fb_blit()`'s XRGB→ARGB
> conversion already writes the `0xff` alpha. The loop was deleted. Result:
> `gemdemo` **6 → 64–65 fps** (73 under the hot-swap A/B), commit worker
> **~99 % → ~40–77 %** (fluctuating; the residual is the required
> XRGB→ARGB conversion). Deployed as
> `1m7v0g75nyq6i31q5vl5qx4n6zsr50fg-nixos-system-gemini-26.11pre-git`
> (module `0c83e11a…`), verified across a cold reboot; no boot.img flash was
> needed (module-only change — see `docs/session-log.md` 2026-09-10k).
> Consequently the **Mesa-version-mix hypothesis below is demoted** — the
> responsiveness came back from the kworker fix alone; still worth a look
> before the Mesa 26 rebase, but it is no longer the suspected cause of the
> sluggishness.
> Issue 2 (touch rotation) is unchanged.

---

## Issue 1 — Sluggishness (looks/feels like software rendering)

### What we know (receipts)

- **Two Mesa versions are loaded inside the one gnome-shell process.**
  `/proc/<gnome-shell>/maps` on the device:
  - `libgallium-26.2.2.so` ×6, from `/nix/store/4xnk693i3b3rm7fgxkyjkga4qcwyqr47-mesa-26.2.2`
  - `libgallium-25.0.7.so` ×4, from `/nix/store/mfqyzn3rz6i2w5hliz5w8jr5vylk8h3m-mesa-geminipda-25.0.7`
  - `dri_gbm` ×4, plus `/nix/store/xv6s7zkvnnqjmjfqw09h3099lswrnpyf-mesa-libgbm-26.1.3`
- **Why both are there:**
  - `/etc/glvnd/egl_vendor.d/50_mesa.json` →
    `…-mesa-geminipda-25.0.7/lib/libEGL_mesa.so.0` (the fork, via the
    glvnd ICD).
  - GNOME's `mutter-50.4` / `gnome-shell-50.4`
    (`/nix/store/z4lx3z1mh4h2fqv5y70xir92mrz5720g-mutter-50.4/lib/libmutter-18.so`)
    come from the pinned nixpkgs and drag in **mesa 26.2.2** (+ split
    `mesa-libgbm` 26.1.3).
  - `config/gemini.nix` deliberately keeps `hardware.graphics` **off** and
    puts `mesaGeminipda` + `pkgs.libglvnd` in `systemPackages` — that was
    correct for the nested desktop/browsers, but GNOME is a newer stack
    built against mesa 26.x, so the two collide.
- **panfrost is idle**, i.e. the UI is *not* being drawn on the Mali:
  ```
  /sys/bus/platform/devices/13040000.mali/power/runtime_status = suspended
  runtime_active_time = 78059      (ms)
  runtime_suspended_time = 667252  (ms, ~11 min of 12)
  ```
  It was touched at startup (78 s) then left alone.
- **kmsro itself is proven good** (2026-09-10): `kmscube` on card0 with the
  *fork* mesa reports `OpenGL ES 3.1 Mesa 25.0.7`,
  `renderer: "Mali-T880 (Panfrost)"`. So the kernel device + kmsro pairing
  are not the problem — **the userspace Mesa mix is.**
- mutter logs `Created gbm renderer for '/dev/dri/card0'`, and gnome-shell
  holds both `/dev/dri/card0` and `/dev/dri/renderD128` open — consistent
  with "it *tried* kmsro and then ended up on a degraded/software path."
- gnome-shell average CPU ≈ **9.7 %** at light load; load average ≈ 2.

### Root-cause hypothesis (most likely → least)

1. **Mesa version mismatch inside gnome-shell.** libmutter links mesa
   26.2.2's `libgbm`/`libgallium` while the glvnd ICD injects the fork's
   25.0.7 `libEGL_mesa`; the mixed GL/GBM stack mis-negotiates the kmsro
   path and falls back to llvmpipe/CPU. This is the leading suspect and is
   directly actionable.
2. The fork patch is **required** on the T880 and we can't simply drop it —
   see below — so the fix must make *one* Mesa version carry it.
3. Secondary cost even when the GPU path works: our KMS driver is a
   **shadow plane**, so every atomic update does a CPU `drm_fb_blit` of the
   full 1080×2160×4 ≈ 9.3 MB into the scanout (`geminipda-drm.c`
   `…_primary_plane_helper_atomic_update`). Worth measuring, but it should
   not cause "very sluggish" on its own.

### What the Mesa fork patch actually contains (it is NOT just debug)

`patches/mesa-panfrost-geminipda-25.0.7.patch` (272 lines, 5 files). Read it
fully — only two hunks are functional:

- **`src/gallium/drivers/panfrost/pan_screen.c`** — sets
  `caps->dmabuf = DRM_PRIME_CAP_IMPORT | DRM_PRIME_CAP_EXPORT`. Real fix:
  without it GBM falls back to dumb buffers and dma-buf import is refused
  (breaks kmsro/gemwl/Wayland-GBM). *Check whether upstream mesa 26.2.2
  already sets these caps — it may not need porting.*
- **`src/gallium/drivers/panfrost/pan_cmdstream.c`** — CPU-maps the
  polygon-list BO and `memset`s the **whole** list every batch (instead of
  the upstream `WRITE_VALUE` of the first word), plus a flags change. This
  is a **T880 tiler workaround** (stale empty-bin headers across BO-cache
  reuse → dropped far bins). `PAN_NO_POLYLIST_MEMSET=1` restores upstream
  semantics (the "oracle"). Likely still needed for correct T880 rendering.

Everything else is `getenv`-gated debug/tuning: `PAN_TILERDBG`,
`PAN_TILER_MASK`, `PAN_DUMP_POLYLIST`, `PAN_POLYLIST_FRESH`,
`PAN_FLUSH_POLYLIST`, `PAN_MESA_TILER_HEAP_CPU`.

### Plan for next session

**Step 0 — confirm the renderer (30 min, do this first).**
Run a GL query *inside the GNOME session* with the session's environment
(not the fork env we used for kmscube) and read `GL_RENDERER`:
- launch `eglinfo`/`es2_info` (build from the pinned nixpkgs, copy to the
  device — same trick as `kmscube`/`drm_info` this session) from a
  GNOME terminal / `ssh` into the `cjdell` session with
  `XDG_RUNTIME_DIR=/run/user/1000`, `WAYLAND_DISPLAY=wayland-0`.
- also try `MESA_DEBUG=1 LIBGL_DEBUG=verbose` and `journalctl` on
  gnome-shell, and check whether mutter reports hardware vs software.
- **Expected if hypothesis 1 is right:** `llvmpipe` (or a crash/fallback)
  from the session's default GL, while kmscube-with-fork shows Panfrost.

**Step 1 — make it ONE Mesa (the actual fix).** Two viable shapes:

- **(1a) Preferred — rebase the fork to mesa 26.2.2.** Port the two
  functional hunks (dmabuf caps + polygon-list memset; drop/keep the debug
  hunks as you like) onto the nixpkgs-pinned mesa **26.2.2**, build it as
  the *system* mesa, and have GNOME, the nested desktops and the browsers
  all resolve to it. Result: a single `libgallium-26.2.2.so` in every
  process. This needs `pkgs/mesa-geminipda.nix` to take the version/base
  from the pinned nixpkgs and an overlay/`replaceRuntimeDependencies` (or
  `hardware.graphics.extraPackages` + ICD) so gnome-shell's own `libgbm`
  is the same build. **This is the standards-compliant end state** and
  removes the version skew permanently.
- **(1b) Quick experiment — make the fork the only GL.** Force gnome-shell
  to use the fork's `libEGL`/`libgbm` for *everything* (e.g. point
  `hardware.graphics`/ICD at the fork and provide its `libgbm` +
  `dri_gbm.so` on the session's `LD_LIBRARY_PATH`, as the nested sessions
  already do). Expect a version-ABI fight with mutter (25.0.7 vs 26.2.2);
  if it works it's a shortcut to confirm the diagnosis, but (1a) is the
  keeper.

Diagnostics that will disambiguate fast: after any change, re-check
`/proc/<gnome-shell>/maps` (must show **one** `libgallium`),
`…/mali/power/runtime_active_time` (must climb while you use the UI), and
`GL_RENDERER` (must be `Mali-T880 (Panfrost)`).

**Step 2 — if still slow after a correct single-Mesa GPU path**, measure
the shadow-plane blit: try `gemini-drm`'s `delayed`/damage path or profile
`drm_fb_blit`. Not expected to be the main issue.

### Do NOT

- Do not remove the polygon-list workaround from the fork without an
  on-glass render check (`PAN_NO_POLYLIST_MEMSET=1` is the A/B oracle).
- Do not enable `hardware.graphics` (plain nixpkgs mesa, all drivers) as a
  shortcut — that reintroduces a second Mesa and a much larger closure.

---

## Issue 2 — Touch rotated 90° (left side → right side of UI)

### What we know (receipts)

- Touch HID: **`Novatek NT36772 Touchscreen`**, `/dev/input/event0`.
- Kernel driver: `devices/planet-geminipda/kernel/delta/drivers/input/touchscreen/novatek-nt36xxx.c`.
  Its header says the panel is **native portrait 1080×2160**
  (`abs_x_max 1080`, `abs_y_max 2160`) and that the DT properties map
  `1080×2160 → landscape 2160×1080` as **`X' = y, Y' = 1080 - x`** — i.e.
  it was **tuned for gemwl's landscape framebuffer**.
- The board DTS sets, on `cap_touch@62`:
  ```
  touchscreen-size-x = <1080>;
  touchscreen-size-y = <2160>;
  touchscreen-inverted-x;
  touchscreen-swapped-x-y;
  ```
  (receipt: `…/mt6797-gemini-pda.dts`, i2c4 node.)
- Under GNOME the **DRM mode is portrait 1080×2160** and the connector
  carries **`panel orientation = Left Side Up`**; mutter rotates the
  logical output to landscape 2160×1080 (`crtc-pos=1080x2160+0+0`).
- **Mutter 50.4 does not appear to apply the panel orientation to input**:
  grepping `src/backends/native/meta-input-*.c` and `meta-seat-impl.c` for
  `panel_orientation`/`PANEL_ORIENTATION` finds **nothing** (the property
  only appears in the output/connector/monitor code). So the input mapping
  is expected to come from the device's own coordinate space.

### Root-cause hypothesis

Mutter maps the touch device onto the logical (landscape) monitor and does
**not** rotate it. The driver's DT transform was calibrated for gemwl's
*rotated framebuffer*; in GNOME the same transform is now off by the panel
orientation — hence "left activates right". The exact combination
(rotation vs mirror) needs one **empirical calibration** because the
observed symptom ("left corner → right corner") reads like an axis
inversion on top of the swap, not a plain 90° turn.

Two possibilities to sort out first:

- **(A) Mutter does no input rotation** → the kernel/DT must deliver
  coordinates already matching the logical 2160×1080 landscape; the
  current `swapped-x-y` + `inverted-x` is simply the wrong flavour now
  (e.g. drop `inverted-x`, or invert the other axis).
- **(B) Mutter *does* rotate input** (via the seat/logical-monitor
  transform) → the kernel must deliver **portrait** and the DT
  swap/invert must be removed.

### Plan for next session

**Step 0 — see the numbers.** Get a coordinate readout so you're not
guessing:
- build `libinput` (`libinput debug-events` / `libinput list-devices`) or
  a tiny reader from the pinned nixpkgs and copy it to the device (same
  pattern as `kmscube`/`drm_info`); or use `evtest`.
- Touch a few known screen corners and record the reported `ABS_X/ABS_Y`
  (or libinput mm) values. That immediately tells you whether the device is
  in portrait (0–1080 / 0–2160) or landscape (0–2160 / 0–1080) and whether
  an axis is mirrored.

**Step 1 — iterate WITHOUT reflashing.** Apply a **libinput calibration
matrix** via udev so you can try transforms live (no kernel/boot.img
rebuild):
```
# /etc/udev/rules.d/99-gemini-touch.rules  (temporary, via environment.etc or a test file)
# 90° rotations / flips, e.g.:
SUBSYSTEM=="input", ATTRS{name}=="Novatek NT36772 Touchscreen", ENV{LIBINPUT_CALIBRATION_MATRIX}="0 1 0 -1 0 1"
```
(`LIBINPUT_CALIBRATION_MATRIX` is a 2×3 affine: `a b c d e f`.) Try the 8
combinations and pick the one where all four corners land correctly. This
is a **userspace** change (NixOS `environment.etc` + udev reload, or a
`services.udev.extraRules` entry in `services/gnome.nix`) and is fully
reversible.

**Step 2 — bake the winner into the kernel (the standards fix).** Once you
know the required transform, express it with the standard DT properties on
`cap_touch@62` (`touchscreen-swapped-x-y`, `touchscreen-inverted-x`,
`touchscreen-inverted-y`; ranges via `touchscreen-size-x/y`) and rebuild
the boot.img. Keep the driver's own axis comment in sync (it documents the
gemwl-era transform and will otherwise mislead the next person). A kernel
rebuild + `bin/flash-nixos.sh boot` is required for DT changes; use the
safe cycle and keep a boot backup (the existing pattern).

**Step 3 — if the correct mapping is "portrait, no transform"** (case B),
remove the swap/invert from the DTS and let mutter's own panel-orientation
input handling do the work — but confirm mutter actually rotates by testing;
if it doesn't, keep the transform in the kernel/DT (case A).

### Do NOT

- Rule 5 still applies: touch/DT changes do **not** touch panel
  initialisation (LK still owns the NT36672), so they are safe, but do not
  bundle any display-stack change into this.
- Don't hand-edit the config hard-codes; keep the DT as the single source
  of the touch mapping.

---

## Shared notes for the next session

- **Tooling pattern that worked:** build aarch64 tools from the pinned
  nixpkgs on the builder, `nix copy --to ssh://10.15.19.82`, run on device
  (this is how `kmscube`/`drm_info` were used). `nix build --impure
  --expr 'let p = (builtins.getFlake "…").nixosConfigurations.gemini.pkgs;
  in [ p.kmscube p.drm_info p.libinput ]'`.
- Fork-mesa env that made kmscube work (needed for any GL test):
  `LD_LIBRARY_PATH=<fork>/lib:<libglvnd>/lib`,
  `LIBGL_DRIVERS_PATH=<fork>/lib/dri`,
  `GBM_BACKENDS_PATH=<fork>/lib/gbm`.
- **Deploy loop:** `nix build .#nixosConfigurations.gemini.config.system.build.toplevel`
  → `bin/deploy.sh deploy <path>` (no reflash needed for userspace/rootfs
  changes, including udev rules and session env). Only **kernel/DT**
  changes need `packages.aarch64-linux.bootimg` + `bin/flash-nixos.sh boot`.
- **Watch out:** bumping the Mesa fork's base version will change
  `/nix/store` hashes; re-run `bin/sync-kernel-delta.sh` only for kernel
  delta changes, and update `docs/gnome-feasibility.md` receipts + the
  session log with the new version line.
- The earlier correction (2026-09-10h) still stands: kmsro→panfrost **is**
  the right mechanism; this is not "GNOME can't be accelerated", it is a
  Mesa-version/session-integration bug to finish.

## Definition of done

1. `/proc/<gnome-shell>/maps` shows a **single** `libgallium` version;
   `GL_RENDERER` in the session is `Mali-T880 (Panfrost)`; panfrost
   `runtime_active_time` climbs while interacting; UI no longer sluggish.
2. Touch: all four corners of the UI activate the correct on-screen point,
   with the transform expressed in the touchscreen DT node (or a documented
   libinput matrix if the DT route is impossible).
3. Version line + receipts appended to `docs/session-log.md`; the
   corresponding sections of `docs/gnome-feasibility.md` updated (Mesa
   strategy + touch mapping), and `docs/disaster-recovery/inventory.md` if a
   new boot.img is flashed.
