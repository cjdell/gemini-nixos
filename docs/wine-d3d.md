# Wine on the Gemini PDA — x86-64 Windows apps via box64 + wine64

Last updated: 2026-09-11 (**wine-wow64 is now the deployed/scripted stack;
runtime moved to `/var/lib/wine-x86` so the desktop user can run it**).
Previous: 2026-09-09 (32-bit guests via wine-wow64; glprobe32;
panfrost-not-llvmpipe correction).

Run the **first real Windows Direct3D 9 program on the PDA**: a
self-written `d3d9test` (rotating vertex-coloured cube + GDI FPS
overlay) under **wine 11.0 (x86-64) emulated by box64 0.4.4 (aarch64)**,
rendering D3D9→OpenGL through wined3d onto the **gemwl Wayland
compositor**.

Tooling: `bin/wine-x86-deploy.sh` (status/deploy/init/run/log/shot/kill),
`pkgs/wine-x86.nix` (stack pin), `pkgs/d3d9test.nix` + `pkgs/d3d9test/`
(the PE). Not part of the flake system on purpose — see §5.

## 1. Stack choice (why box64 + wine64, not FEX + i686-wine)

Verified against the flake's nixpkgs pin (`dc5d91f84032`,
26.11pre1068949, channel rev) on 2026-09-09:

| Candidate | Status at this pin |
|---|---|
| `fex-emu` | **not in nixpkgs** (no attribute; no source under pkgs) |
| cross wine for i686 (`pkgsCross.i686-linux`) | **not exposed** — `pkgsCross` has `mingwW64`/`mingw-ucrt-*`/`x86_64-windows`… but no i686-linux; `i686-linux` *is* a valid `import` system (wine exists there, glibc 2.42) but its wine NAR is **not on cache.nixos.org** (404 on the wine outPath) → would compile from source |
| `wine64` 11.0 (x86_64-linux) | **hydra-cached** (outPath narinfo 200) — 342-path / 1.8 GB closure |
| `box64` 0.4.4 (aarch64) | **hydra-cached** — 5-path / 81 MB closure |
| `box86` 0.3.8 (aarch64) | in nixpkgs but its NAR **missing** from cache at this pin; NOT shipped (d3d9test is a 64-bit PE, wine64 prefix is 64-bit-only) |

So: **64-bit wine under box64** is the only all-cached path, and it
matches the test target (a 64-bit PE). Box64 implements its own x86-64
ELF loader (PT_INTERP is ignored) and re-execs itself for guest
`execve` of x86 binaries — which is why `wine64` can spawn its
`wineserver` coprocess under emulation.

Cache checks used `nix path-info --store https://cache.nixos.org`
(rule 9 — curl narinfo 404s are proxy noise, nix's client is truth).

## 2. Building the D3D9 test PE — the winegcc trap

`d3d9test` is compiled to a **x86_64-windows PE** with the
**mingw-w64 cross toolchain** (`pkgsCross.mingwW64` stdenv =
`x86_64-w64-mingw32-g++` 15.3.0 + mingw-w64 14.0.0 headers/libs, all
hydra-cached). `pkgs/d3d9test.nix` is called from `pkgsCross.mingwW64`,
so `stdenv` inside it IS the cross stdenv.

**Trap (verified 2026-09-09):** the `winegcc` shipped inside the
nixpkgs `wine64` package is configured for a **native x86_64-linux**
target — `winegcc -dumpmachine` → `x86_64-unknown-linux-gnu`. It
compiles fine, but emits an **ELF** (plus a sh wrapper named
`d3d9test.exe` that execs `wine d3d9test.exe.so`) — wine cannot load an
ELF. Do not use that winegcc for PEs; use the mingw-w64 cross compiler
(mingw-w64 also carries the *real* Microsoft D3D9 headers, which wine's
headers only partially mirror — see §3).

## 3. D3D9 header ground truth (wine vs mingw-w64)

Wine's headers (`wine64/include/wine/windows/`) are a **partial,
classic-era** API:

- `D3DMATRIX` = anonymous union `{ float m[4][4]; struct{_11.._44}; }`
  (DUMMYSTRUCTNAME/DUMMYUNIONNAME are empty macros) — access via
  `m->m[row][col]` in C++.
- **no** flexible-FVF `D3DFMT_*` macros, **no** `D3DFVF` flexible
  constants, **no** `DEFAULT_SWISS`, **no** `GetDesktopDisplayMode`,
  **no** `D3DSWAPE_EFFECT_DEFAULT`, `D3DCAPS9` only in the separate
  `d3d9caps.h` (and without the `D3DDEVCAPS_*` enum).
- C mode: COM interfaces are opaque structs needing
  `IDirect3D9_Release(p)`-style macros; **C++ mode gives real classes**
  → the app is C++ (`->Release()` works).

Fixed FVF constants (the safe, classic path — used by d3d9test):
`D3DFVF_XYZ | D3DFVF_DIFFUSE` = `0x0002 | 0x0040` = **0x0042**
(D3DVECTOR + D3DCOLOR, 20-byte stride) — from mingw-w64 14.0.0
`d3d9types.h` (the source of truth for the classic constants; verified
identical in both header sets).

## 4. Running on the PDA

```
box64  →  wine64 loader (.wine, x86-64 ELF)  →  wineserver (spawned)
wine64 →  winewayland.drv → wayland-0 (gemwl, /run/gemwl)
wined3d→  EGL/GBM (x86-64 mesa from the wine closure) → panfrost T880
```

- Launcher: `/var/lib/wine-x86/wine-wow` (installed by `deploy`; the
  system `wine`/`wine64` wrappers exec the same path). It sets
  `WINEPREFIX=$HOME/.wine-x86` (per-user), `WINEDEBUG=warn-all`
  (override), and the guest-mesa EGL vendor manifest. It does NOT force
  a display: under GNOME it inherits the session's
  `XDG_RUNTIME_DIR`/`WAYLAND_DISPLAY` (the old gemwl-era hardcode to
  `/run/gemwl` is gone). [moved 2026-09-11: the old
  `/root/wine-x86/wine-x86` launcher was unreachable from the `cjdell`
  session — `/root` is 0700, `wine` died with "Permission denied"]
- **gemwl needed viewporter** (2026-09-09): winewayland refuses to
  init without `wp_viewporter` (`waylanddrv:wayland_process_init Wayland
  compositor doesn't support wp_viewporter` → `nodrv_CreateWindow`).
  Fix = 2 lines in `pkgs/gemwl/gemwl.c` (`wlr_viewporter_create`;
  wlroots 0.18 handles the viewport resources internally — the 0.17-era
  `new_request` API is gone; the scene renderer already accounts for
  viewports). The wlroots 0.18.2 build already contained the
  `wlr_viewporter_create` symbol — gemwl simply never registered the
  global.
- Prefix init: `bash bin/wine-x86-deploy.sh init` (wineboot -u; first
  run takes a while under emulation).
- Run: `bash bin/wine-x86-deploy.sh run` (detached; log
  `/root/wine-x86/logs/app.log`), `… log`, `… shot out.png` (grim via
  the gemwl screencopy protocol), `… kill`.
- WINEDEBUG for receipts: `WINEDEBUG=+d3d9 run` shows wined3d's GL
  renderer line (proves panfrost-T880 vs llvmpipe fallback).

## 5. Deployment model (why not a flake systemPackage)

- wine64 closure = **1.8 GB / 342 paths**; adding it to the flake's
  `systemPackages` would bloat every `bin/deploy.sh` delta copy and the
  device system profile. Instead `deploy` ships the four paths to the
  device store with `nix copy --to ssh://10.15.19.82` (same mechanism
  as deploy.sh) and GC-pins them with a `/nix/var/nix/gcroots/wine-x86/`
  directory (device + host).
- The device store is separate from the system profile — rolling the
  system back never touches the wine stack; `nix gc` on the device
  never collects it.
- Re-deploy after a nixpkgs repin: just `bash bin/wine-x86-deploy.sh
  deploy` again (paths change; the launcher is rewritten with the new
  store paths).

## 6. On-glass results

First D3D9 frame + receipts in the 2026-09-09 session-log entry (d3d9test
55-60 fps cube). NOTE [corrected 2026-09-09]: that run and every wine GL
run is **guest panfrost on the T880**, NOT llvmpipe — see §7. grim
screenshots were never proven on glass (gemwl lacks screencopy).

## 7. 32-bit Windows apps: wine-wow64 (wineWow64Packages.full) + the
panfrost-not-llvmpipe correction (2026-09-09)

32-bit PEs (most 2003-era demos — dsd_lm.exe is PE32 i386) need the
32-bit payload that `wine64` (nixpkgs, 64-bit-only) lacks. The flake-pin
has `wineWow64Packages.full` = **wine-wow64 11.0** — the new-WoW64
single-loader build: `bin/.wine` + `lib/wine/{i386-windows (1072 PEs),
x86_64-unix, x86_64-windows}`, NO i686-linux needed, hydra-cached at
dc5d91f84032 (`5qkxzvcr5k4pglxsnzvqz308sn6cs02m`, 706 MB closure). box64
runs its 32-bit x86 code in most cases (experimental per box64 README;
the known-broken case is wined3d/D3D — GL is fine). A wineboot -u in a
fresh prefix installs syswow64; 64-bit apps run unchanged.

**Now scripted (2026-09-11):** `bin/wine-x86-deploy.sh deploy` ships
`wineWow64Packages.full` (706 MB closure) + box64 + mesa + grim, installs
`/var/lib/wine-x86/wine-wow` (0755, world-readable) and GC-roots
`wine-wow64`; `pkgs/wine-cli.nix` wraps it as `wine`/`wine64` in
`systemPackages`; the prefix is per-user `$HOME/.wine-x86`
(`… init [user]` runs `wineboot`). The legacy 64-bit-only `wine64`
(1.8 GB) is still an attribute in `pkgs/wine-x86.nix` but is no longer
deployed by default.

**glprobe32** (pkgs/glprobe32) is the 32-bit GL present probe: two shared
contexts + display list built on B executed on A + colored clears +
gdi32 SwapBuffers + glReadPixels→BMP. On glass it shows a flashing
coloured triangle — 32-bit guest GL present under box64+wow64 works.
Build note: `#include <GL/gl.h>` (uppercase dir — mingw-w64 layout,
case matters when cross-compiling on Linux), `-mwindows`,
`pkgsCross.mingw32` (i686) cross stdenv, NOT winegcc.

**CORRECTION — guest GL is panfrost-T880, env-llvmpipe does NOT stick:**
glGetString(GL_RENDERER) under the guest reports "Mali-T880 MC4
(Panfrost)", GL 3.1 Mesa 26.2.2 — with BOTH `GALLIUM_DRIVER=llvmpipe`
and `LIBGL_ALWAYS_SOFTWARE=1` exported. The x86_64 guest mesa's
EGL-wayland platform opens the T880 render node via GBM and loads
panfrost_dri regardless of those env vars (DRI device loading ignores
them). Implication: the earlier session's "llvmpipe was the stable/safe
path" claim (d3d9test cube) was wrong — it was the hardware all along,
and the 2026-09-09 wedge's attribution to "forced guest panfrost" is
unsupported (long guest-panfrost probe runs on 2026-09-09 were stable).
Treat guest-panfrost as the DEFAULT path for all wine GL work;
re-root-cause the wedge before risky runs (session-log handover).

**dsd_lm.exe status (2026-09-09):** runs (music via fmod/winmm), window
shows (small, top-left — ChangeDisplaySettings unsupported on
winewayland), scene BLACK with ~69 presents/s (not stalled). Suspect:
its fixed-function GL 1.x scene (glLightfv/glMaterialfv/glFog/glTexGen/
display lists) against panfrost GL 3.1's weaker fixed-function path.
Next: render it on a true llvmpipe guest mesa (build with panfrost
disabled) to confirm — full handover in docs/session-log.md (top entry).
