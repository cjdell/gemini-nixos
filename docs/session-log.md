# gemini-nixos session log

Dated entries of what was actually tried / decided in this repo.
Hardware/boot ground truth lives in the sibling project
(`/home/cjdell/Projects/GeminiPDA/docs/session-log.md`) — cross-reference
when a session touches device behaviour. Latest entry first.

## 2026-09-09 — GEMDEMO PURGED TO A SINGLE-FILE GLES 3.1 TEMPLATE (0.3.0), DEPLOYED + CONFIRMED ON GLASS (gens 34-35)

Verdict from the 0.2.0 "GEMINI: EXODUS" on-glass work: the demoscene is
useless except as proof of hardware interaction → stripped to the
skeleton. `pkgs/gemdemo/` is now **ONE Rust file** (`src/main.rs`, ~560
lines incl. the receipt comments) that boots a GLES 3.1 EGL context on a
winit Wayland surface (panfrost fork, Mali-T880) and draws one spinning
per-vertex-shaded triangle while cpal plays a 440 Hz sine through the
`gemini16` plug — the OpenGL-app template going forward (copy file +
pkgs/gemdemo.nix). Deleted: gfx/show/synth/font/cpu/glctx/glutil/audio/
shaders modules (~5200 lines; sources + the 0.2.0/0.1.0 docs live in git
history). libc dep dropped (was only the A72-affinity code). Lockfile
regenerated (212 pkgs; winit 0.29.15 / egl 0.2.7 / gl 0.14 / cpal 0.15.3).

The hardware receipts the purge preserved — now the file-header doc + a
compact docs/gemdemo.md: DSA `glCreate*` silent stubs → `glGen*`;
one interleaved VBO per VAO, no instancing / no multi-buffer VAOs / no
`glDrawElements` (panfrost crashes); `wl_egl_window` required for the
Wayland surface; **audio wire state S16_LE @ 44100 Hz** (S32/48k on the
16-bit MT6351 = noise) — `gemini16` by name (hint-gated) + forced I16
config; uniform typos are silent on GLES → assert.

**Deploys:** `bash bin/deploy.sh deploy` ×2 under run-job (each ~67 s,
rc=0):
- **gen34** `ybrd7qcd…` (commit 6f34d24) — first 0.3.0; on-glass smoke
  FAILED at the shader: `shader tri failed:` with an EMPTY log — the
  inlined compile closure passed the shader-TYPE enum straight to
  glShaderSource (dropped the `glCreateShader` call; invalid object
  handle → compile status 0). Lesson: cargo check can't see GL bugs;
  the panic-with-log helper is only as good as the code before it.
- **gen35** `29afwk7d…` (commit 26746d7, the fix) — **ON GLASS +
  CONFIRMED**: identity `2xd3gy5y6…-gemdemo-0.3.0`, windowed under
  labwc (1025×576): `gemdemo 0.3.0 — Mesa / Mali-T880 (Panfrost) —
  OpenGL ES 3.1 Mesa 25.0.7`; `audio 'gemini16' — 44100 Hz, 1 ch, I16`;
  steady 59-60 fps heartbeat over 12 s (no GL errors, no swap fails);
  user confirmed triangle spins + 440 Hz sine audible.

Docs updated: docs/gemdemo.md (rewritten as the template guide),
README.md row, AGENTS.md row, flake.nix + services/gemini-pda.nix
comments (no longer "demoscene"). Device left: **gen35**, para
unchanged (NORMAL/NixOS), gemwl + lxqt-nested up. Next: nothing owed
for the template itself — future GL work starts from src/main.rs.

## 2026-09-09 — GEMDEMO 0.2.0 DEPLOYED AS GEN33 (old 0.1.0 "AETHER" replaced on the device)

`bash bin/deploy.sh deploy` built + switched gen33 (rc=0, ~42 s — the
only closure delta was gemdemo → 0.2.0). Device now serves
`/run/current-system/sw/bin/gemdemo` →
`/nix/store/xxy3r8rld35a5m97wfv3ly366w52ng2i-gemdemo-0.2.0` (the exact
store path measured at ~60 fps on glass; banner "GEMINI: EXODUS
v0.2.0"). Old AETHER 0.1.0 (`k9njxxs…`) is out of the active profile
(still in the gen32 store — rollback-safe via `deploy.sh rollback`).
Smoke-run of the installed copy under the labwc session: planet maps
bake, frame dump OK, A72s up + pinned.

Rollback: `bash bin/deploy.sh rollback` (one gen) restores 0.1.0 if the
fullscreen QA on glass goes wrong.

## 2026-09-09 — GEMDEMO 0.2.0 ON GLASS (windowed under labwc): 60 fps across the show; S5 planet bake fix measured

Deployed the 0.2.0 build to the PDA (nix copy → /nix/store, run as the
labwc session client) and measured:

- **Audio live**: cpal→gemini16 S16 @ 44.1k opened cleanly; per-cb
  peak logs ~0.92 steady (under the 0.97 ceiling — the old over-limit
  slam is gone; master rms healthy). Beat clock drives the show.
- **A72s**: gemini-a72-up unit requested from inside the demo and the
  render thread pinned to cpu8/cpu9 (confirmed in the banner).
- **FPS**: full show windowed at 1025×576 held 58.9–61 fps through
  S0–S4 and S6 (the old 0.1.0 was single digits).
- **S5 PLANET reveal was the one miss**: the per-pixel fbm planet
  (disc ~44k px at the reveal) dragged fps to ~42. Fixed by baking the
  surface: albedo + cloud maps (256×128) generated ONCE at startup in
  Rust per palette (PAL_EARTH/PAL_PC consts in show.rs, shared with
  the bake), spin = equirect UV scroll, lighting stays per-pixel but
  cheap (no fbm/trig-in-noise in the hot path, spec via repeated
  squaring). Before/after on the same run path: 42 → 60 fps at the
  reveal. No per-frame FBO involved — "bake, don't multipass."
- PPM dumps (window readback RGBA→RGB) land on the device at
  /tmp/s5*.ppm (s5c = S5 reveal ~60 fps build). Region stats confirm
  content (planet mid-bright ~YAVG 134 vs sky ~97–102).
- The 2026-09-09 model couldn't VIEW images — a human should eyeball
  the dumps (title centring, ring depth, palette) before fullscreen.

Files: pkgs/gemdemo/src/gfx.rs (planet bake), src/show.rs (PAL consts),
docs/gemdemo.md (updated status + strategy). Session log entry above
holds the full 0.2.0 rewrite record.

## 2026-09-09 — GEMDEMO 0.2.0 "GEMINI: EXODUS": complete rewrite (assembly-style cinematic spacesynth show) — code green, NOT yet on glass

After the 0.1.0 "AETHER" verdict (2026-09-09: single-digit FPS, flawed
visuals, music broken — master rms ~0.88 from the naive limiter, saw-DC
voices), the whole demo was rewritten in this session:

- **Renderer → direct single-framebuffer layering** (gfx.rs): no post
  chain, no SSAA, no raymarch, no FBOs at scale 1.0. Sky gradient (only
  fullscreen pass) → baked procedural sprite layers (soft/star/ring/
  cloud textures: stars, nebula, halos, particles, shock rings) → warp
  streaks (VS-animated, static VBO) → rotating lit fbm planet + banded
  ring arcs (small-region, low noise budget) → vector ship + engine
  trail → text. Keeps the fork receipts: glGen*, one interleaved
  buffer/VAO, expanded triangles, no instancing/DrawElements.
- **Score engine rewritten** (synth.rs): real mix bus with headroom,
  per-part sends to dotted-8 ping-pong delay + Schroeder reverb,
  sidechain pump, soft-knee ceiling + fast-attack/slow-release limiter;
  chime removed. 126 BPM A-minor, 80 bars / 7 chapters (bar counts
  SHARED with the visual director show.rs). Synth sanity is unit-tested
  (no silence, no clip, no over-limit).
- **Director** (show.rs) turns the beat clock into pure-data
  FrameStates per chapter (S0 EARTH liftoff … S5 PLANET COMPUTERS
  reveal … S6 ORIGIN credits); titles in chapters.
- **A72 cores** (cpu.rs): best-effort — asks gemini-a72-up unit
  (guarded: only on systems where the unit exists), bounded 20 s wait,
  pins the render thread (libc added to Cargo.toml).
- glctx/glutil warnings cleaned; dead modules scenes.rs/post.rs/
  raymarch.rs/sensors.rs deleted; new host check loop
  `bin/gemdemo-host-check.sh` (x86_64 cargo check + unit tests via the
  flake's nixpkgs alsa-lib.dev/pkg-config — seconds).

Verified: host `cargo check` clean; `cargo test --release` 4/4 pass;
aarch64 flake build green (native-aarch64 remote builder, ~80 s,
rc=0). Host kwin/XKB run attempt for frame dumps abandoned (winit
XKBNotFound in the agent's session context) — visual QA is owed ON
GLASS via `--dump` (PPM) + `[`/`]` chapter sweep while watching fps.

Files: pkgs/gemdemo.{nix,src/{main,show,gfx,synth,cpu,audio,font,glctx,
glutil,shaders}.rs}, docs/gemdemo.md (rewritten), Cargo.toml 0.2.0,
bin/gemdemo-host-check.sh.

## 2026-09-09 — BOOT-TIME EXT4 AUTO-REPAIR ON GLASS (defense against hard power-off): initrd e2fsck pre-mount, both paths verified, p22 flashed + sha'd

Follow-up to the boot-panic recovery above. The panic's root cause —
torn ext4 orphan chain that the KERNEL's mount-time journal replay
could not recover (oops every boot, pre-userspace) — is now defended
in the initrd itself:

**Change (commit f45befe):** `devices/planet-geminipda/initrd.nix` runs
`e2fsck -y $ROOT` BEFORE the rw mount, both NixOS (p32) + Debian (p29)
branches; static aarch64 e2fsck (pkgsStatic.e2fsprogs 1.47.4, 1.38 MB)
staged into the cpio. Boot.img 9.37 → 9.52 MiB (< 16 MiB cap), header
geometry + cmdline byte-identical (bootopt intact). Plus
`bin/fsck-p32.sh` — scripted TWRP operator fallback (check/repair/
status; unmounts p32 first — the live-mount spurious-drift lesson).

**Why unconditional -y is free:** e2fsck 1.47.4 `check_if_skip` — on a
HEALTHY fs (VALID set, interval=0, max_mnt_count=-1 as on ours) it
skips in ms (verified locally: 2 ms on a 300 MB test fs). Only journal
RECOVER / ERROR_FS / orphan / VALID-cleared force real work. After a
journal replay e2fsck restarts but the restart re-evaluates and skips
(replay cleared RECOVER) — so a dirty boot costs only the (small)
journal replay, never a full 27 GiB scan (confirmed empirically below).

**On-glass verification (new initrd = boot.img
`yx3fr5qm5…`, sha256 19b299c1effdd950a7699222f9336bb133f009bef7dc554d8e9dc6d2c3aab5bf;
flashed to p22 12:45, backup `stock-dump/boot-20260909-124502.img`):**
1. **Clean boot** — TWRP cycle → NORMAL: boots gen32 in ~34 s
   (unchanged; skip path), no failed units, fs clean, no kernel
   recovery line. p22 read-back sha == image sha.
2. **Dirty boot** — WDT EXRST reboot from the running system (unclean
   shutdown, journal dirty, no TWRP in between): dmesg receipt —
   p32 ro-probe 3.22 s → unmount 3.28 s → **[initrd e2fsck replays
   journal]** → kernel rw mount 3.51 s finds CLEAN journal (NO
   "recovery complete" line — the kernel would have replayed if the
   initrd hadn't) → stage-2 remount 3.87 s. Boot still ~34 s total.
   fs state clean, mount count 3→4.

**Result:** the kernel no longer ever replays a possibly-torn journal
(the incident's oops vector). Clean boots pay ~0; dirty boots (any
hard power-off) self-recover to a clean journal before userspace. The
full torn-orphan repair (the incident's case) is the same -y path
(orphan state forces the full pass — minutes, on an already-broken
boot). TWRP + bin/fsck-p32.sh remain the operator escape hatch.

Note for a future serial/console watcher: the initrd prints
`==> e2fsck -y … (auto-repair; skips fast when clean)` + e2fsck
output on the fbcon console during the ~230 ms window — visible live
but not captured in dmesg/journal (userspace console writes).
Device left: running gen32, para cleared (NORMAL), desktop + wifi up.

## 2026-09-09 — BOOT-PANIC RECOVERY (post-wedge): p32 torn-orphan ext4 from the unclean power-off → repeated pre-mount initrd panics; fixed with offline e2fsck -fy in TWRP, no reflash — device back on gen32, clean cold boot

Follow-up to the wine D3D9 session below (device left WEDGED after the
forced guest-panfrost experiment). User power-cycled; NORMAL boot then
**kernel-panicked every attempt** (fbcon bottom-third only, display mostly
black), device ended up in TWRP. Recovery steps + receipts:

**Read-only diagnosis from TWRP (all clean except the rootfs):**
- p22 `boot` exact-length sha256 = `be6f4d2192d8a95ef762cd17b910fa3af98d3fb98f76a91eef3decd4da4a2e51` — **byte-identical to the verified lean 6.6.0 boot.img** (`boot-img-lean-20260908`); kernel image was never the problem.
- Full 34-partition GPT present; battery 3.98→4.06 V charging (DR rule 3 ok).
- pstore/ramoops empty (`ramoops@44410000`, 896 KiB, IS registered in the lean config/DT — but no crash record survived; the pre-mount panics never reached the backend or left no record).
- p32 (NIXOS_SYSTEM) ext4: **journal dirty + ~50-inode torn orphan linked list (822900-822955) + stale free-block/inode counts** (groups 88/94/96/97/100/106). Pass 2/3/4 (directory structure/connectivity/refcounts) all CLEAN — pure post-crash metadata, no content damage.

**Root-cause chain (receipt):** wedge power-off tore the ext4 orphan
chain; every subsequent NixOS boot died at/before the p32 mount
(journal replay → orphan-list error → mount fail → initrd panic) —
proven by: (a) the journal stayed dirty across all panic attempts
(journald never got to write), (b) `journalctl --list-boots` shows NO
boots between the wedged session (-1) and the recovered boot (0) — the
panics left no journal trace because they were pre-mount.

**Fix (TWRP):** `umount /data /sdcard` → `e2fsck -fy /dev/block/mmcblk0p32`
(orphan list FIXED per inode; counts rebuilt) → verify `e2fsck -fn` rc=0,
all 5 passes clean. ~2 min total. **No reflash, no image change.**

**Result:** para cleared (NORMAL) → single clean cold boot — gen32
`0zjh8q82yb2y3qd4c4iyvk5h19v9xz7i` (current), kernel 7.5 s + userspace
36.7 s, `tune2fs` state=clean mount count 1, `systemctl --failed` empty,
gemwl + lxqt-nested NRestarts=0, renderD128 + card0 up, wifi active,
battery charging 4.14 V. Also serves as the long-owed **cold-boot
confirmation of the dual-boot initrd gen-lookup landing on the newest
generation (gen32): PASS** (first cold boot since gen9 — gens 10-32
were all live deploy.sh switches).

Notes/learned: (1) new receipt — unclean power-off of a wedged box can
tear the ext4 orphan chain so the NEXT boot's journal replay/mount
fails repeatedly with no journal trace; TWRP + offline `e2fsck -fy` is
the fix; the fs was otherwise structurally sound (nix store + profiles
untouched). (2) If a boot-time panic recurs, check pstore from the next
successful boot (`/sys/fs/pstore/dmesg-ramoops-*`) — the region exists
but was empty this time (observation, unverified why).

Device left: **AWAKE on gen32, para cleared (NORMAL/NixOS default),
desktop + wifi up, battery charging.** Pending from the wine session:
re-run `bin/wine-x86-deploy.sh run d3d9test.exe` on the llvmpipe path +
grim screenshot receipt; guest-panfrost stays BANNED without
para=boot-recovery + a clean prefix (per the incident LEARNED note).

## 2026-09-09 (gemdemo audio FIXED-ENOUGH, MUSIC ON GLASS — "awful mix" = the follow-up) — S32 noise trap + engine DC bugs, all committed

Audible music now plays on the device (S16 @ 44.1 k via gemini16). Three
separate root causes were stacked; the last two made the music NOTHING
(audibly) and were found with a host-side render harness
(`/tmp/harness`: `rustc -O harness.rs` beside synth.rs — synth.rs is
pure math, no device needed):

1. **S32-on-wire noise trap** (fix: `hint { show on }` on gemini16 in
   services/pipewire/asound.conf; live via /root/.asoundrc stopgap — /etc
   is read-only): cpal 0.15 enumerates ALSA devices by name hints, so the
   custom plug was invisible and the app fell back to `default`, whose
   F32-in → S32-on-wire plays as noise on the 16-bit-only MT6351.
   (aplay -v A/B: FLOAT→S32 = noise, S16 = clean.)
2. **cpal stream → S16** (audio.rs): after 1, also force an I16 @ 44.1 k
   config when offered (was defaulting to F32; the I16 branch had bugs —
   mono-collapse, wrong frame math — now fixed interleaved-stereo).
3. **saw generator DC bug** (synth.rs, THE silence): `2.0*(p-p.fract())-1.0`
   with p∈[0,1) is ALWAYS -1 (p.fract()==p) — every saw voice (pad, arp/
   pluck, bass osc) output constant -1 DC → tanh/limiter → flat +0.9 DC
   (inaudible). Saw = `2.0*p-1.0` in all three sites.
4. **step decode bug** (synth.rs): `s = bar % STEPS_PER_BAR` froze each
   bar to its first step's arrangement (pad re-triggered every bar, arp
   patterns scrambled). Now `s = step % STEPS_PER_BAR`; `next_step` init
   0 so step 0 plays at t=0.

Harness evidence (8 s RMS/zc per 0.5 s): before fixes — rms 0.27 → 0.90
flat, zc→0 (DC clamp) by t=1 s. After — rms 0.27→0.88 climbing as the
arrangement builds, zc 1.4 k→24 k Hz (real drums/hats/noise). Startup
chime (0.6 s, 880→660→440 Hz) added as an audibility aid — still in
(remove when polishing).

USER-HEARD: 3-note chime then... silence (pre-fix). After the fixes:
"I hear sound, but it's awful" — the mix is over-limited (master rms
~0.88 sustained; master_gain/limiter tuning + the chime removal are the
"fix later"). NOT yet deployed to the running system (device gen31 still
ships the silent build; deploy gen32 when the mix is fixed).

Device state: /root/.asoundrc stopgap present (re-add after root home
wipes); no stray gemdemo processes.

## 2026-09-09 (gemdemo audio: "no music, buzz at open + snap at close" ROOT-CAUSED + FIXED) — S32-on-wire noise trap; gemini16 now discoverable

User report (running gemdemo in the desktop session): no music — only a
brief buzz at start and a snap at exit. Root cause found + fixed at the
alsaLib config level, NOT in the synth (the engine render math is
sound; the stream ran the whole time with no errors):

- **Diagnosis**: `audio.rs` opens `gemini16` (the S16_LE-pinning plug,
  services/pipewire/asound.conf) in front of the 16-bit-only MT6351
  AFE — but cpal 0.15 enumerates ALSA devices via name HINTS, and a
  custom plug is invisible unless it carries `hint { show on }`. Every
  run printed device 'default' → fromenv → sysdefault → hw:0.
- **Live-verified with aplay -v** (2026-09-09): S16_LE in via `default`
  → S16 on the wire (clean). **FLOAT_LE in (the demo's F32 stream) →
  alsa's plug converts to S32_LE on the wire** — and the driver never
  programs the data-width register (mt6797-dai-adda.c), so S32 plays
  as square-wave/white noise (the pre-existing S16/S24/S32 receipt in
  asound.conf's header). The "buzz" was the S32 noise; the "snap" the
  stream open/close against a wrong-width codec state.
- **Fix** (repo, in `services/pipewire/asound.conf`): add `hint { show
  on; description "Gemini PDA MT6351 S16 output" }` to `pcm.gemini16`
  so alsa-lib advertises it; `aplay -L` now lists gemini16 and the demo
  opens device 'gemini16' — F32-in is converted to S16 by the plug's
  pinned slave (the same conversion pipewire's WirePlumber path does),
  clean by construction.
- **Live on the device now**: /etc is NixOS read-only, so the fix was
  applied via `/root/.asoundrc` (root-run sessions read it) — the demo
  prints `audio device 'gemini16' — 44100 Hz, 2 ch, F32`. The next
  system switch carries it in /etc/asound.conf (config change staged in
  services/pipewire/asound.conf). NOTE: `/root/.asoundrc` is a stopgap;
  re-add it if a rebuild wipes root's home.
- audio.rs header + gemdemo.md updated with the receipt. Also confirms
  the demo's audio path = F32 engine → alsa plug → S16 codec is the
  DESIGNED one (pipewire does the same); no engine change needed.

Next: user ears-check (desktop terminal: `gemdemo --windowed --ssaa 1`),
 then switch the system to carry the asound.conf hint in /etc.

## 2026-09-08 (gemdemo on glass — gen31 `cb3b5iv1q6pq8g` builds, NOT deployed) — "AETHER" demoscene/GPU stress test BUILT + RUNNING on the device; every blocker between "green build" and "on glass" closed

`gemdemo` (docs/gemdemo.md) now runs on the live LXQt/labwc session and
directly on the gemwl compositor. Verified via the new `--dump` stage
readback: real scene content through the whole chain (scene FBO → bloom/
feedback post → window), zero panfrost kernel faults, no crash over
60 s+, audio up (pulse/PipeWire, "44100 Hz 2ch F32"). A full receipt +
the complete bug list is in docs/gemdemo.md; highlights:

- Derivation `pkgs/gemdemo.nix` now pins wayland + libxkbcommon on the
  RUNPATH (winit 0.29 dlopens them — the generic rpath fixup only pulled
  the -dev outputs → `NoWaylandLib`).
- glctx: wrap the wl_surface in `wl_egl_window_create` (EGL needs the
  size; bare surface → EGL_BAD_NATIVE_WINDOW); dropped the bogus
  EGL_RENDER_BUFFER/EGL_RGB_BUFFER config key/value pair (BAD_ATTRIBUTE).
- glutil: ALL object creation switched from the 4.5-core DSA `glCreate*`
  to `glGen*` — the DSA entry points are silent "unsupported function"
  stubs on the GLES 3.1 context (ghost objects; mesa: `glBufferData(no
  buffer bound)`).
- Scenes: NO instancing and NO multi-buffer VAOs — the fork's panfrost
  u_vbuf segfaults on instancing and multi-buffer VAOs storm the GPU
  (`panfrost: js fault JOB_BUS_FAULT` + sched timeouts). All draws are
  single interleaved buffers (wlroots-proven pattern). NO DrawElements
  (index-minmax scan crashes). Tunnel verts expanded.
- font.rs: atlas 16×4 → 16×6 (table outgrew it — OOB bake panic) and
  the Writer no longer stores a `*const Font` into a SceneSet that gets
  MOVED (dangling pointer SIGSEGV).
- Shader fixes: `vec5` → pos+uv split, `pal()` duplicated into the
  vortex VS, composite `u_bloom` sampler/float redeclaration renamed
  `u_bloomamt`, dead `u_res` dropped (link-time optimized out →
  uniform() panic).
- Post FBO 1×1 bug: `Post::new()` allocates 1×1 and the init
  `state.resize()` no-ops when sizes already match — fullscreen windows
  that never send a Resized event left the whole post chain at 1×1
  (resolve magnified one texel → flat colour). Force `post.resize`
  after init.
- parse_args rewrite (had never run with flags): flag-only args spun at
  100% CPU, value flags were left at the value position → both fixed.
- Dump tooling: `--dump <frame#> <path>` stage PPMs + `--solid R,G,B`
  target self-test. Readback had to be GL_RGBA (GLES3 rejects
  GL_RGB/UNSIGNED_BYTE for RGBA8 fbos — silent error, zero-filled
  buffers, phantom "everything black"; scene was fine all along).
- Perf truth: fullscreen ssaa2 (4320×2160 scene) ~1–2 fps on the T880;
  ssaa1 ~4 fps; windowed ssaa1 ~15 fps. Stress protocol follows.

To run on the device now (store path in the log tail below):
`XDG_RUNTIME_DIR=/run/gemwl WAYLAND_DISPLAY=wayland-1 gemdemo
--windowed --ssaa 1 --silent` from the LXQt session (add `--section N`
0..6 to jump in). `services/gemini-pda.nix` systemPackages now carries
`gemdemo` — gen31 `cb3b5iv1q6pq8g` (toplevel) built 2026-09-08 but NOT
switched (session was read-only on the live system); deploy when wanted
(`bash bin/deploy.sh build && bash bin/deploy.sh deploy`).

## 2026-09-08 (browsers GL fix, gen28 `1nkzm5nih…`) — REAL CHROME/FIREFOX GL PATH: fork mesa gained the wayland EGL platform; Firefox no-WebGL + Chrome-won't-start root-caused and fixed at build level; wlegltst proves ES 3.1 / Mali-T880 / Panfrost through the nested stack

Follow-up to the browsers-install entry below. User glass test:
"no WebGL in Firefox (not even software); this DID work in Debian.
Chrome won't even start."

Root causes (each verified on device):
1. **Fork mesa had NO wayland EGL platform** (`-Dplatforms=` empty,
surfaceless-only via -Degl-native-platform). Browser GL needs
EGL_PLATFORM_WAYLAND against the nested compositor — with none,
Firefox could not create ANY GL context ("not even software": no
wayland platform AND no swrast/llvmpipe in the fork = nothing to fall
back to). Debian's fork had the wayland platform (es2gears_wayland +
hardware WebRender worked there — legacy GeminiPDA session-log
2026-09-04); the NixOS port had dropped it. `wlegltst.c` (wayland-EGL
smoke client in pkgs/gemwl/) existed but could never pass.
2. **GBM_BACKENDS_PATH not exported**: the fork libgbm has NO baked
backend path (strings-verified) and honors only that env var
(Debian start-lxqt-nested.sh exported it) — browser glxtest/GPU probe
needs dri_gbm.so (which the fork DOES ship at lib/gbm).
3. **Chrome refused to start**: "Running as root without --no-sandbox
is not supported" (zygote_host_impl_linux.cc:102) — the desktop is a
root systemd session; userns/SUID sandbox can't drop root. Fixed with
--no-sandbox in the wrapper (trusted single-user PDA; comment in
config/gemini.nix).

Changes (all in-tree, this session):
- pkgs/mesa-geminipda.nix: `-Dplatforms=wayland` (surfaceless
  preserved via -Degl-native-platform=surfaceless) + wayland build deps.
  Build-discovery gotcha worth recording: wayland's + wayland-scanner's
  .pc live ONLY in their -dev outputs and wayland-protocols' in
  share/pkgconfig, but the nixpkgs pkg-config wrapper role vars don't
  surface all of them to mesa 25's build-time dependency() lookups
  (meson.build:2054 wayland-scanner, :2061 wayland-protocols) — seeded
  env.PKG_CONFIG_PATH + PKG_CONFIG_PATH_FOR_BUILD with the three dirs.
- services/lxqt.nix: GBM_BACKENDS_PATH=${mesaGeminipda}/lib/gbm in the
  session env (port of the Debian env var; mesaGeminipda already in
  scope there).
- config/gemini.nix: google-chrome overridden with commandLineArgs =
  "--no-sandbox" (+ rationale comment).

Version lines (rule 0): mesa-geminipda 25.0.7 now
`mfqyzn3rz6i2w5hliz5w8jr5vylk8h3m` (wayland platform; libEGL_mesa
DT_NEEDED libwayland-client verified on device); relinked
wlroots/labwc/gemwl against it; toplevel gen28
`1nkzm5nih5r39q7wzjf2qv07rbr2r9mi` deployed 2026-09-08 (~78 s
delta — mesa was pre-built, only the relinked drvs + toplevel
shipped). gemwl + lxqt-nested active after activate (no mesa-rebuild
regression). Kernel #329 + boot.img unchanged.

Verification on glass (probe, before user eyes-on): fork's own
wlegltst against the LIVE session prints: wl_drm present (v2),
linux_dmabuf present (v4), eglGetPlatformDisplay(wayland): ok,
EGL 1.5, **GL: OpenGL ES 3.1 Mesa 25.0.7 | Mali-T880 (Panfrost)**, 150+
swaps — i.e. the full client-GL chain (fork EGL wayland -> nested
labwc wl_drm/dmabuf -> panfrost) works end to end. Firefox needs
exactly this chain. Firefox also mapped a window under the new env
(labwc journal: identifier=firefox).

**USER-VERIFIED ON GLASS 2026-09-08: "it works great"** — Firefox
(WebGL) and Chrome both running from the LXQt desktop after the gen28
deploy; entry closed. (Chrome's chrome://gpu mode — ANGLE-on-panfrost
vs SwiftShader — not reported; launcher tuning can follow if a future
session wants it.)


Asked "can we get real Google Chrome on the device" — research + install
session. Findings (web-verified 2026-09-08): Google's official Linux arm64
stable deb exists (dl.google.com …/google-chrome-stable_current_arm64.deb,
133 MB — download page doesn't link it yet, but the URL + apt repo are
live; Widevine + Google sync included, per omgubuntu 2026-07). nixpkgs
removed the old google-chrome path but it lives on at
`pkgs/by-name/go/google-chrome` with **aarch64-linux in platforms** and the
arm64 deb hash at the repo's pinned rev `dc5d91f84032` (v152.0.7977.82).
Firefox 155.0.1 also aarch64-cached at the pin.

GL reasoning (the interesting part): the client GL wiring ALREADY existed
— config/gemini.nix installs the fork's glvnd ICD manifest at
/etc/glvnd/egl_vendor.d/50_mesa.json, and nixpkgs' firefox wrapper ships
libglvnd on LD_LIBRARY_PATH (`withGlvnd` defaults on for Linux), so
Firefox's dlopen of libEGL.so.1 dispatches to mesa-geminipda → panfrost
renderD128 — the same chain Debian's Firefox used for WebGL. The repo was
just missing its first third-party GL client. Chrome's nixpkgs wrapper
only adds ozone/wayland auto-flags when `NIXOS_OZONE_WL` is set (added to
the lxqt-nested env); its default ANGLE path will try Vulkan (absent on
Midgard) then GL/SwiftShader — empirical on glass.

Changes (commit: this session): `config/gemini.nix` systemPackages += [
`pkgs.google-chrome` `pkgs.firefox` ] (comment carries the rationale);
`services/lxqt.nix` unit path += both (bare-name launch in the session
terminal) + `NIXOS_OZONE_WL=1` env; launchers
`config/lxqt/Desktop/{google-chrome,firefox}.desktop` (seed dir — fresh
installs get them via sessionConfig; live device copied now).

Version lines (rule 0): google-chrome 152.0.7977.82
`p20940mi4ir8fk54gi341q72jfajcn2p` (built locally on the Pi — unfree =
never on cache.nixos.org, but the drv is only unpack+patchelf, minutes),
firefox 155.0.1 `d2p0bvy7ap8jqbgsr7drxzyjjh9ckai6` (cache-substituted),
toplevel gen27 `r3hj9x7bcba35qgf8rbhyvw1imd9a7yv`; kernel #329 + boot.img
unchanged. Deploy via `bin/deploy.sh deploy` under run-job: 157 s total
(build → nix copy delta → profile switch + activate); post-activation
gemwl + lxqt-nested active, launcher icons on /root/Desktop.

Next: eyes-on-glass — launch both from the LXQt desktop/menu, read
`chrome://gpu` (panfrost vs SwiftShader?) and open a WebGL page in Firefox
(expectation: works, Debian parity). Log the result here with a [verified
2026-09-08/09] note; if Chrome lands on SwiftShader, try `--use-angle=gl`
(routes ANGLE through the fork EGL).
 (v2, gens 25–26): instant backlight-first sleep/wake (~1-2 s each way), wifi chip teardown removed from the button path (~29 s stall → iface down + daemon kill), press debounce + queue drain in the sleepd daemon — full cycle re-verified on glass incl. wifi re-association

User report: the silver button was "very sluggish" — press once =
nothing, press a few more times = the backlight flickered on/off at
~1 s intervals. Root causes found in the sleepd journal (two bugs):

1. **~29 s stall in the sleep path when wifi was up**: the sleepd
   journal showed `stopping … gemini-wifi-auto` at :44 then ASLEEP
   only at :13 — the gap is `wifi-internal stop` = `echo off >
   /sys/kernel/debug/wcn/pwr` blocking ~29 s in the kernel when the
   chip is fully associated (the WMT whole-chip teardown). The
   backlight (the only visible effect) came LAST in the v1 sequence,
   so the press appeared dead for ~30 s → the user pressed again…
2. **Queued presses cascaded**: each press toggled only after the
   previous ~1-30 s toggle finished, so a flurry of presses produced
   rapid sleep/wake/sleep at ~1 s cadence (the flicker) — and the
   rapid stop/start cycling tripped gemwl's start rate limit
   (`start-limit-hit` → failed, needed reset-failed).

Also learned: the gemini-wifi-* units are RemainAfterExit oneshots
with no ExecStop — `systemctl stop` on them does NOT kill the
wpa_supplicant they spawned (wifi survives a unit stop; only the
`echo off` chip teardown actually stopped it, which is why wlan0
disappeared in the v1 tests).

**v2 fix (pkgs/gemcli/src/sleep.rs, gens 25-26):**

- `sleep on` reordered: backlight off FIRST (instant visible
  acknowledgement), then inputs unbind, cpus 1-7 offline, services
  stop, wifi fast-down (`ip link set wlan0 down` + pkill wpa_supplicant
  + the iface's dhcpcd — the CONSYS chip STAYS powered; the ~29 s
  `echo off` teardown is gone from the button path). Total ~1-2 s
  worst case, backlight in the first ~50 ms.
- `sleep off` reordered the same way (backlight on first); wake
  re-associates by RESTARTING gemini-wifi-auto.service (a plain
  `start` was a no-op — the RemainAfterExit unit stayed "active"
  through sleep). `systemctl start` in the wake path now reset-failed
  first (gemwl start-limit recovery).
- `sleep key` (the daemon): 1 s press debounce (one physical press =
  one toggle even if the polled driver double-reports) + drain of any
  events queued while a toggle ran — mashing can no longer cascade.

On-glass re-verification (gen26): `time gemcli sleep on` = 2.1 s
(backlight off in the first ms; wifi down, wpa dead while asleep),
`time gemcli sleep off` = 1.1 s; wifi re-associated + DHCP
(192.168.49.166) after wake; desktop/audio services all back; asleep
soak ichgr 150 mA (best yet — v1's wifi unit-stop left the radio
alive). Version lines: gemcli 0.1.0; toplevel
`m96fkx88…-nixos-system-gemini` (gc-pinned toplevel-20260909-sleepd).
Kernel untouched. Device left: AWAKE, gen26, desktop + wifi up,
backlight 10 %.

Next: user physical press test on the new daemon (the fix is verified
via ssh toggles; the debounce/drain live path needs a real finger).

## 2026-09-08 (7th) — POWER-SAVING INVESTIGATION + SILVER-BUTTON SLEEP/WAKE ON GLASS (gens 22–24): `gemcli sleep on|off|status|key` light clamshell sleep (backlight off, A53 cpus 1-7 offline, keyboard+touch unbound, services stopped) + `gemini-sleepd.service` KEY_SLEEP daemon — two full sleep/wake round-trips verified; deep-sleep (s2idle) documented as kernel follow-up

Asked-for: investigate power savings (backlight, cores incl. the A53s,
anything else) so the device draws as little battery current as
possible, kill keyboard input while the closed clamshell presses the
keys, and wire the silver side button (mt6351-keys KEY_SLEEP) as
sleep/wake. Outcome: an awake LIGHT sleep is implemented, verified on
glass and now owned by the silver button; TRUE deep sleep needs a
suspend wake source (PMIC/pwrap INT kernel work — no flash happened,
no kernel change). Version lines (rule 0): gemcli 0.1.0 (same crate
version; new subcommands). Gens 22→23→24
`girqg7wp8` → `ip3432lw` → `zbyzpwc` (current). Kernel unchanged 6.6.0
lean; boot partition untouched (pure package/system deploys via
bin/deploy.sh). Device left: AWAKE, gen24, desktop+wifi+audio up,
backlight 10 %, battery fast-charging ~4.08 V, para clear.

**Investigation receipts (docs/power-sleep.md).** The unit still has NO
suspend/resume path: `/sys/power/state` = freeze/mem(s2idle) exists and
CONFIG_SUSPEND=y, but nothing can WAKE s2idle — the side keys are
PMIC-debounced bits in TOPSTATUS 0x220 POLLED by mt6351-keys over
pwrap (no IRQ route in mainline; vendor 3.18 wakes via the PMIC INT →
pwrap EINT status), and the kernel boots clk_ignore_unused /
pd_ignore_unused / regulator_ignore_unused. Power ladder on glass
(USB 500 mA input, ICHGR charge-current proxy, 50 mA ADC steps):
backlight 100 % → off recovers ≥150 mA@5 V (at 100 % the battery
discharges even at full input — vbat 4084→3984); desktop/gemwl idle,
pipewire, CONSYS wifi, and A53 cpus 1-7 each measure ≤50 mA (at/below
ADC resolution). Awake floor with everything off ≈ 400 mA@4 V ≈ 1.6 W
(LCD TDDI panel logic stays on — fbcon kernel can't blank the panel,
rule 5; no cpufreq driver for MT6797; no A53-cluster power-down path).

**Implementation (docs/gemcli.md §sleep + pkgs/gemcli/src/sleep.rs).**
`gemcli sleep on`: stop the heavyweight services that were running
(gemwl/lxqt-nested, pipewire/wireplumber/pipewire-pulse,
gemini-wifi-internal/auto), power the CONSYS chip down via
`wifi-internal stop` (the oneshot units have no ExecStop), offline A53
cpus 1-7 (cpu0 stays), backlight off (bl_power=4, brightness kept),
unbind the clamshell input drivers (matrix-keypad platform `keyboard`
+ novatek-nt36xxx i2c `4-0062`) so the closed lid's key presses make
no input, and record everything in /run/gemcli-sleep.state. `off`
reverses (rebind → backlight → cores → services async). `key` scans
/sys/class/input for the mt6351-keys evdev node (not a hardcoded
eventN), watches for KEY_SLEEP value==1 and toggles — it backs the new
enabled `gemini-sleepd.service` (Restart=always; sshd + battery-guard +
sleepd itself are never stopped). SIGPIPE reset to SIG_DFL in main()
so `gemcli … | head` dies quietly instead of panicking (Broken pipe,
seen during testing).

**On-glass verification.** Full round-trip ×2 via ssh (no button
needed): sleep → cpu online=0, bl_power=4, kbd/touch driver dirs
empty, all 7 units inactive, wlan0 gone (CONSYS powered down), state
file correct, rc=0. Wake → cpus 0-7, bl_power=0, inputs rebound,
services active, wlan0 re-associated (auto still activating a few
seconds), state cleared. **Bug found + fixed during testing:** the
wifi chip power-down was skipped because `systemctl stop --no-block`
raced the `is-active` check (wlan0 stayed up through sleep) — capture
active-ness BEFORE stopping. The "wifi-internal: pwr-off failed
(modules stay loaded)" verdict during sleep is the known whole-chip-
reset no-op (legacy receipt); wlan0 disappearing confirms the teardown.

**Not done this session (next steps):** (1) the physical silver-button
press test is the user's (daemon verified watching event3; toggle logic
verified via ssh commands — a real press is the last check); (2) deep
sleep = kernel follow-up: wire the PMIC HOMEKEY/PWRKEY INT → pwrap
INT_EN → wake-capable IRQ so mt6351-keys can wake s2idle, then probe
s2idle entry (WDT-escaped) and drop the clk/pd/regulator_ignore_unused
flags for the suspend path — recipes in docs/power-sleep.md §Deep
sleep. Host gc-pin: toplevel-20260909-sleepd.

## 2026-09-08 (6th) — GEMCLI ON GLASS (gens 16–21): deployed via deploy.sh, selfcheck ALL PASS, battery/backlight/charger parity byte-identical, a72 up/down round-trip verified — gpio v1 ioctl bug found (v6.6 renumbering) + host disk-full incident

The (5th) entry's next step: get gemcli onto the device. Version
lines (rule 0): gemcli 0.1.0 everywhere; gens 16–21 in order
`ac5hn0p3` → `78lq8ki` → `yqzfd51` → `838j92y` → `ias4zzi` →
`243v8bs` (current). Kernel unchanged (6.6.0 lean, boot partition
touched by nothing — config/package deploys only). Device left:
gen21 current, para clear (NixOS p32 default), A72 cores back offline
(0-7), backlight 10 %, battery fast-charging ~4.06 V.

**Host disk-full incident** (first deploy failed): root fs `/` (which
holds /nix) was 100 % — the gc-pin symlink after the toplevel build
failed ENOSPC. Freed ~7 G of leftover kernel A/B scratch in /tmp
(kfull/kclean/kbase/kernsrc/kobj + kernel dumps from the
borrow-retirement session; nothing referenced them) and re-ran. Host
/nix is 212 G/246 G — a `nix-collect-garbage` is owed soon (the
pinned-deploy gcroots make it safe; deferred to keep this session
focused).

**Deploy mechanism** worked as designed the rest of the way:
run-job + `bash bin/deploy.sh deploy` (~50 s each: build cached,
delta nix copy, profile switch + activate — no flash, no reboot
needed for the new systemPackages entry to land).

**On-glass results**: `gemcli selfcheck` = ALL PASS (8/8: devmem SPM/
WDT reads, bq25890 psy, raw BQ25896 i2c read, backlight 9 %, cpu map
0-7, para present, gpio pads 243/244, gpu regs). Byte-identical
parity: battstat vs `gemcli battery status`, bq25896-raw.sh vs
`gemcli charger raw`, `backlight get`; write round-trip
`set 20` → 20 everywhere → restored 10. `gemcli a72 up both`: cpu8
cold on attempt 1 (DA9214 bus i2c-2, SPM pre-seq, sramldo, WDT-armed
PSCI) + cpu9 warm → 0-9; `a72 down both`: cpu9 per-core, cpu8
last-A72 secure teardown (ISO bit1 re-asserted, PWR_CON bit0 clear),
DA9214 BUCKB rail dropped → 0-7, cold-boot state. rc 0 both ways.

**THE bug**: gpio probes failed EINVAL while the C gpioout succeeded
on the same pads. Bisected with an aarch64 strace (shipped via the
repo's `nix copy --to ssh://10.15.19.82` mechanism from the pinned
rev): strace decoded the C call as GPIO_GET_LINEHANDLE_IOCTL but left
the Rust one raw — and the v6.6 UAPI header shows the v1 ioctl
numbers were REORGANISED after the pre-5.x kernels:
`GPIO_GET_LINEHANDLE_IOCTL` is nr 0x03 (nr 0x02 is now
GPIO_GET_LINEINFO_IOCTL). My nr-0x02 request hit the lineinfo ioctl
with a linehandle struct → EINVAL. Fixed in pkgs/gemcli/src/gpio.rs
(nr 0x03) + a regression test pinning the exact _IOC literals
(8 tests green). Also: chip resolution now scans
GPIO_GET_CHIPINFO_IOCTL ngpio per chip (this kernel: gpiochip0
pinctrl_paris 262 lines + gpiochip1 aw9523b 16) instead of assuming
chip0. Cosmetic: cl2-up warm log now says OK/FAILED (the "rc=1" form
was a success-boolean that read backwards).

Units still ExecStart the scripts — the flips (backlight-default →
wdt/boot/a72 hand-runs → gpu-poweron → battery-guard LAST) are the
remaining step per docs/gemcli.md; nothing in this session flipped
one. strace 7.2 left in the device store (debug tool, harmless).

Next: host nix-collect-garbage (gcroots are in place), then the
lowest-risk unit flip (gemini-backlight-default → `gemcli backlight
set 10`) with a WDT-reboot A/B.

## 2026-09-08 (5th) — GEMCLI LANDED (Rust device-control CLI): clap-based tool with script-parity ports of backlight/battery/charger/power/guard/a72/wdt/boot/gpu/speaker; native aarch64 build rc=0 — nothing flashed, no unit flipped

Asked-for consolidation: one native Rust binary ON the device for the
functions the bring-up shell scripts handle (services/scripts/*), built
in-repo as `pkgs/gemcli.nix` + `pkgs/gemcli/` (the pkgs/* vendored-source
pattern, like gemwl/speaker-amp). Deps are clap 4.5 (derive — the asked-
for "nice CLI parser crate") + libc 0.2 only; access model = /dev/mem mmap
(devmem-equivalent), i2c-dev ioctls incl. DT-base adapter resolution +
`-f` force semantics, gpio chardev v1 for the speaker pads, sysfs for psy/
backlight/cpu. Docs: docs/gemcli.md (map, exit codes, corrections,
parity + flip recipe).

**Scope**: `backlight` (sysfs-first, DISP_PWM0 devmem fallback + clock
gates), `battery status` (battstat exit codes 0/2/3/4/5), `charger raw`
(BQ25896 conversion-trigger + register decode), `power`
(status/watch/charge/dim-to-charge), `guard run` (the safety daemon —
same env knobs, same /run/battery-guard/state + history CSV),
`a72 up/down` (cl2-up/down: DA9214 BUCKB via i2c, SPM pre-sequence,
sramldo SMC, WDT-armed PSCI hotplug/teardown), `gpu poweron/status`
(full MTCMOS vendor sequence), `wdt-reboot`, `boot` (para marker:
recovery/debian/nixos + --no-reboot), `speaker`, `status` aggregate,
`selfcheck` (read-only on-glass parity harness), `version` (rule-0
banner). wifi/wifi-internal + audio-output/defaults deliberately NOT
ported yet (daemon/card orchestrators, not register control) — phase 2.

**Corrections over the scripts (all `[corrected 2026-09-08]`, noted in
module headers + docs/gemcli.md)**: (1) cl2-up.sh always exited 0
(trailing `log` echo rc) even after GAVE UP — gemcli returns the real
outcome; (2) gemini-boot-recovery wrote a SHORT 15-byte para record (no
conv=sync) — gemcli always writes the full padded 32-byte command like
gemini-boot-debian; (3) battery-guard's bash rotation wrote a 7-column
header over 8-field rows — gemcli always writes the 8-column header.

**Build receipt** (build-level only, nothing on the device): run-job
`gemcli-build`, rc=0 in 86 s — rustc/cargo 1.97.1 + llvm substituted
from cache.nixos.org at the pinned rev dc5d91f84032, compiled on the
192.168.49.191 aarch64 builder, out
`ldl1j87ks6s09qzzvbhvhkzqrb00fv53-gemcli-0.1.0` (bin 1.34 MB,
stripped, opt-level s). 6 hardware-free unit tests (civil-time,
cpu-range parse, para decode, charger ichgr parse, CON1 duty->pct) ran
green in-sandbox via buildRustPackage's default checkPhase. Local
`cargo check`/`test` clean on the host too.

**Wiring**: flake package `.#packages.aarch64-linux.gemcli` + eval
verified; services/gemini-pda.nix adds gemcli to
environment.systemPackages NEXT TO the scripts (busybox + i2c-tools
stay for console hand use); README layout table + phase-3 services
table + AGENTS.md where-things-live row; this log. The systemd units
still ExecStart the scripts — flipping is an on-glass job:
`gemcli selfcheck` first, then the lowest-risk-first flip order in
docs/gemcli.md (backlight-default -> wdt/boot/a72 hand-runs ->
gpu-poweron -> battery-guard LAST -> a72-up).

Next: deploy a gen with gemcli in the closure (bin/deploy.sh), run
`gemcli version` + `gemcli selfcheck` on glass, log the version line,
then start the parity diffs.

## 2026-09-08 (4th) — repin closure built + deployed: gen15 (nixpkgs dc5d91f84032) live, desktop healthy — 11-min build vs multi-hour

Executed the (3rd) entry's next step: build the repinned toplevel,
deploy to the device, health-check. Kernel UNCHANGED (6.6.0
#1-mobile-nixos — boot partition untouched, no flash; config-only
deploy like gens 11-14). Eyes-on-glass still owed (see next).

**Build** (run-job `repin-build`, rc=0 in **679 s**): toplevel
`1ia0xwzq8smb88jxk7ig3hfaym7jfsn4-nixos-system-gemini-26.11pre-git`,
gc-pinned `gemini-nixos-toplevel-20260908-1537`. The Qt6/LXQt closure
came from cache.nixos.org (qtbase-6.11.2 `mpgq1xlh…`, lxqt-session
2.4.0 `5ijcyvm3…`, lxqt-panel 2.4.1, pcmanfm-qt 2.4.1, systemd-261.2
`awq6qdiy…`, pipewire 1.6.8 — store hashes EXACTLY match the narinfo-
verified cached paths from the (3rd) entry; no local build logs).
Custom drvs compiled on the Pi as expected: mesa-geminipda 25.0.7
`yb4jq7sx…`, wlroots-geminipda 0.18.2 ×2, labwc-geminipda 0.8.3,
gemwl 1.0 `l53xj1w0n…`. For contrast the same toplevel under the old
pin compiled the whole Qt6/LXQt closure = hours.

**Deploy** (run-job `repin-deploy`, rc=0 in 209 s): `nix copy` of the
new closure over g_ether + `nix-env -p` set + `switch-to-configuration`
switch → device profile **system-15-link**;
`/run/current-system` = the new toplevel. Version float vs old pin
(12th-gen docs): qtbase 6.11.1→6.11.2, lxqt-panel 2.4.0→2.4.1,
systemd 261→261.2, pipewire 1.6.7→1.6.8, ffmpeg 8.x→9.0.1 default —
the documented R2 trade-off; verified pins (kernel/mesa/wlroots/labwc/
gemwl) unchanged.

**Post-switch health (over ssh, no reboot)**: gemwl + lxqt-nested
active, NRestarts=0 both; `systemctl --failed` empty; /run/gemwl
sockets (bus, wayland-0) present; gemwl running from the new store
path. Old generations stay selectable for rollback (profile list).

**Next / owed**: eyes-on-glass DONE (user, gen15 — desktop renders,
no flicker/uninitialised-LCD, core-rule-5 clearance). Still owed: one
cold WDT reboot to confirm the dual-boot initrd gen-lookup lands on
gen15; if green, this is the new cache-healthy baseline (golden rule 9).

## 2026-09-08 (3rd) — NIXPKGS REPIN to the hydra-built channel rev dc5d91f84032 (26.11pre1068949): Qt6/LXQt now substitutes; old-rev workaround overlays pruned

Executed the long-deferred handover/3a decision (repin nixpkgs to a
hydra-built rev). Device UNTOUCHED (still gen14 on p32, para cleared,
Debian p29 intact) — repo-side change only, no build/deploy run.

**What and why.** The old pin came from MNX's npins
(`nixos-26.11pre1031299.0bb7ec54c848`, a releases.nixos.org channel
snapshot); its qtbase 404'd on cache.nixos.org for x86_64 AND aarch64
(base closure only — glibc 200, qtbase/systemd/lxqt 404), so every
Qt6/LXQt compile ran on the 8-core Pi builder. Fix = pin nixpkgs to
the newest rev hydra built the FULL closure for: the nixos-unstable
CHANNEL snapshot behind `channels.nixos.org/nixos-unstable` =
**dc5d91f840324650bac8c379428c7037a416959a** (`26.11pre1068949`, cut
2026-09-07). Raw master commits newer than the channel cut only get
per-commit trunk-combined coverage — exactly the slow situation.

**Receipts (verified 2026-09-08, `nix path-info --store
https://cache.nixos.org` on aarch64 outPaths eval'd at the new rev):**
qtbase/qtwayland/qtsvg/lxqt-session/lxqt-panel/pcmanfm-qt/qterminal/
pavucontrol-qt/qpwgraph/papirus-icon-theme/systemd/nix/pipewire/
alsa-utils/openblas/ffmpeg-headless — narinfos ALL 200. Old rev
contrast: qtbase-6.11.1/lxqt-session-2.4.0/systemd-261 404, glibc 200.
(First probe used curl and wrongly showed all-404 — the sandbox proxy
mangles curl; nix's own HTTP client is the reliable check.)

**flake.nix:** nixpkgs pinned via `builtins.fetchTree` (tarball,
narHash `sha256-VaWGJ6+cIYN2erfSecbRV+4ljI185Ty2wUrXyvQbgOw=`,
`nix flake prefetch`-verified) and handed to the MNX eval shim through
its `pkgs` argument (shim forbids system+pkgs together; system comes
from `pkgs.stdenv.hostPlatform`, module pkgs re-import the same source
via `pkgs.path`). Bump recipe in the flake comment + README Pins:
take the rev behind `channels.nixos.org/nixos-unstable/git-revision`.

**config/gemini.nix:** pruned the old-rev/cross-era workaround
overlays (each forced non-hydra drv hashes down its subtree =
cache misses): systemd `withLibBPF=false`, ffmpeg(-headless)
`withCudaLLVM=false`, openblas `dynamicArch=false`, and the
libfm/libfm-extra/menu-cache autoreconf AM_GLIB_GNU_GETTEXT fix.
Kept: the make_ext4fs shim (R13 — deliberate rootfs-geometry fix).
Rationale: hydra built the un-overridden aarch64 defaults in this
channel (that's why they're cached), so the old bugs are gone (or
were cross-only). If a real build re-hits one, re-add it with a date.

**Measured (dry-run, root `--store local`, new rev):** toplevel
aarch64 — 379 derivations will be built → **242** (after the overlay
prune), 1447 paths (2.7 GiB) will be fetched. The remaining 242 are
NixOS per-config glue (unit-*/etc-*/udev/system-path — never cached)
+ the custom drvs (wlroots-geminipda ×2, labwc-geminipda, mesa fork,
kernel, gemini-firmware/…). No qt/lxqt/systemd/ffmpeg/openblas/libfm
compiles left. Eval green (MNX 2c132754 + device config compatible).

**Cost/risk:** the whole closure re-hashes → one full rebuild + one
full `nix copy` to the device; LXQt/Qt float slightly newer (R2
trade-off — the verified kernel/mesa/wlroots/labwc/gemwl pins are
independent of nixpkgs).

**Next:** DONE — build + deploy + health-check recorded in the
2026-09-08 (4th) entry below (gen15, 1ia0xwzq8s…).

## 2026-09-08 (2nd) — INTERNAL WI-FI (MT6630 CONSYS) WORKING ON NIXOS: wlan0 up, "The Lab" connected, internet ~4.5 ms — the wifi half of handover-2026-09-08-wifi-keyboard is DONE (gens 11-14)

Executed `docs/handover-2026-09-08-wifi-keyboard.md` §2 (wifi
workstream; keyboard §3 was the first session of the day — gen10). All
three boot units now pass: nvram → internal → auto. Kernel unchanged
`6.6.0 #1-mobile-nixos`; no flash, four config-only deploys via
`bin/deploy.sh`: gen11 `ggb0ni0n…`, gen12 `hgakgh7jw…`, gen13
`2qlbx6cb…`, gen14 `zf71qrycr…` (current).

Root causes fixed (all confirmed on glass):
- **R15 (wifi.nix nvram unit, handover §2a):** the unit's ExecStart was a
  multi-line `/bin/sh -c '…'` Nix string — systemd parsed the literal
  newlines as unit directives → bad-setting on EVERY boot since c6afc5c
  (verified: journal "Invalid section header '[ -e
  /etc/wifi/profiles.conf …'"). /data/nvram/APCFG/APRDEB/WIFI and
  /etc/wifi/profiles.conf were never installed → wlan_gen3 probe died on
  nvram_read. Fix: `wifiStateInstall` = pkgs.writeShellScript (unit file
  stays single-line; embeds the firmware + profiles seed store paths).
- **/lib/firmware (wifi.nix, handover §2b):** wlan_gen3's kalFirmwareOpen
  walks a HARDCODED path list (/storage/sdcard0, /vendor/firmware,
  /lib/firmware) — kernel file-open, not request_firmware; none existed
  on NixOS. Fix: `systemd.tmpfiles.rules = [ "L+ /lib/firmware - - - -
  /run/current-system/firmware" ]` + wifi-internal
  `After=systemd-tmpfiles-setup.service`. Verified: dmesg "[wlan]MAC
  address: 00:09:34:5a:af:c1" (factory NVRAM record), "wlanProbe ok",
  "FW OWN" — the factory MAC + TX cal load.
- **NEW NixOS-specific root cause (services/scripts/wifi, gens 12-14):**
  with the two fixes in, wlan0 came up and ASSOCIATED (iw link: "The
  Lab", RSSI -42) but `wifi auto`/`wpa_cli` always failed
  ("Failed to connect to non-global ctrl_ifname … Invalid argument") and
  the auto-connect never got a lease. The wifi CLI was ported verbatim
  from Debian (ctrl_interface=/var/run/wpa_supplicant), but this nixpkgs
  wpa_supplicant is built with the **unprivileged-daemon.patch**, whose
  wpa_cli HARDCODES ctrl dir `/run/wpa_supplicant/control` and client
  dir `/run/wpa_supplicant/client` — and refuses to run without the
  latter (strace: it only `access()`es …/client, never even calls
  socket()). Debug trail: python dgram probes + a copied aarch64 strace
  7.2 (Pi → host → device nix copy) → the patched source in the pinned
  nixpkgs (`pkgs/os-specific/linux/wpa_supplicant/
  unprivileged-daemon.patch`). Fix: write_wpa_conf emits
  `ctrl_interface=/run/wpa_supplicant/control`; wpa_ensure mkdirs
  `…/control` + `…/client`, kills stale daemons via the pid file, drops
  the ctrl dirs, and verifies wpa_cli connectivity 1 s after start (fail
  loudly instead of polling empty for 45 s).
- Verified on glass (gen14, units restarted by the switch in boot
  order): all three units active/Result=success; `wpa_cli -i wlan0
  status`: ssid=The Lab, freq 5180 (5 GHz), key_mgmt=WPA2-PSK,
  **wpa_state=COMPLETED**, ip_address=192.168.49.166, addr
  00:09:34:5a:af:c1; `ping -I wlan0 1.1.1.1` ~4.5 ms (2/2). One cosmetic
  journal note: dhcpcd's "Failed to set DNS configuration … resolve1 …
  unknown unit" (no systemd-resolved; resolv.conf is system-managed —
  harmless).

Next: cold-boot persistence check owed (the deploy-switch restart
exercises the same unit chain; a real power cycle confirms tmpfiles
creates /lib/firmware + unit ordering — do it when the user next
reboots), then the handover's remaining keyboard on-glass typing check
(Fn+K @, shift+3 £) + a `wifi auto` connect test at boot.

## 2026-09-08 — DESKTOP KEYBOARD MAPPINGS + SHELL fixed on glass (gen10): symbols/gemini shipped into the closure; gemwl + labwc now compile layout "gemini"; explicit bash login shell + SHELL for the session

Worked `docs/handover-2026-09-08-wifi-keyboard.md` §3 (keyboard xkb
missing from the closure — the Fn layer / UK layout NEVER worked in
NixOS) + the user's follow-up ask (default shell = bash, not sh). Both
fixed, deployed as **gen10** `y7v5z1sgwlq32812fpvspd5vs7338qh7`
(kernel unchanged `6.6.0 #1-mobile-nixos`, lean self-built) via
`bin/deploy.sh deploy`; no flash, no TWRP. Glass-visible effects land at
the next compositor start / terminal open.

- **xkb layout shipped (root cause from the handover, verified on
glass gen9 first):** gemwl logged `xkbcommon: ERROR: [XKB-338] Couldn't
find file "symbols/gemini"` every boot and fell back to the default US
keymap; labwc logged "Found layout English (US)". Consequence on glass:
Fn (`KEY_RIGHTALT` → `<RALT>`) behaved as Alt (no level3 layer) and
shift+3 gave `#` not `£`. New in-repo artifacts:
  - `config/xkb/symbols/gemini` — vendored byte-identical from
    GeminiPDA `build/rootfs-files/xkb/symbols/gemini` (sha256
    `f56fbab8…`, 5,937 B; provenance README `config/xkb/README.md`;
    same re-copy rule as `config/keymaps/`).
  - `pkgs/gemini-xkb.nix` — packages it as an xkbcommon include dir
    (`$out/symbols/gemini`); exposed as flake package `gemini-xkb`.
  - `services/desktop.nix` (gemwl unit) + `services/lxqt.nix`
    (lxqt-nested unit): `XKB_CONFIG_EXTRA_PATH=${gemini-xkb}`;
    lxqt-nested additionally `XKB_DEFAULT_LAYOUT=gemini` (labwc 0.8.3
    builds its keymap from that env — `src/input/keyboard.c`
    `set_layout`; gemwl hardcodes layout "gemini", gemwl.c:818).
- **Shell:** `users.defaultUserShell = "${pkgs.bashInteractive}/bin/bash"`
  in `config/gemini.nix` (root + gemini now point at the store bash in
  `/etc/passwd`, previously the inherited `/run/current-system/sw/...`
  default — already bash, now pinned), and the lxqt-nested unit exports
  `SHELL=` to the same binary so terminal apps (qterminal — a systemd
  system service has no SHELL env and qterminal falls back to /bin/sh)
  spawn bash.
- **Verified on glass (journal + unit env, gen10):** gemwl restarted
  clean (NRestarts=0) and logs `keyboard keymap: model=pc105
  layout=gemini variant=(default)` ×2 with NO XKB-338 after the
  restart; labwc logs "Found layout **Gemini English (UK)**";
  `systemctl show lxqt-nested -p Environment` carries
  `XKB_CONFIG_EXTRA_PATH=/nix/store/w802hc2bbx…-gemini-xkb-2026-09-08`,
  `XKB_DEFAULT_LAYOUT=gemini`, `SHELL=/nix/store/s6hkkyiy…-bash-
  interactive-5.3p9/bin/bash`; `/etc/passwd` root+gemini = the store
  bash. **User on-glass typing check still owed:** Fn+K → `@`, Fn+L →
  `;`, shift+3 → `£`, shift+' → `~`, shift+. → `?` (qterminal), and
  `echo $0` → bash (was sh before this gen in the desktop terminal).
- **Still open from the handover (not this session's ask):** wifi §2a
  (nvram unit malformed — single-line ExecStart) + §2b (`/lib/firmware`
  symlink) — wifi-internal still fails at boot on gen10; console Fn
  check on the fbcon VT (console.keyMap gemini-uk.map was already wired
  in-repo; on-glass typing of the VT layer untested this session).

## 2026-09-08 (afternoon) — LEAN KERNEL ON GLASS (A/B run done): self-built 6.6.0 boots + desktop verified; boot-log display quirk observed; wifi/keyboard handover written

Executed `docs/handover-2026-09-08-kernel-on-glass.md` — the first boot of the
self-built lean kernel (no #329 borrow). Outcome: **PASS on all §6 criteria**
except wifi-internal — which this session re-diagnosed as rootfs-packaging
gaps, not the deep CONSYS issue previously assumed (see below). Version lines
+ receipts:

- boot.img `/nix/store/b0a7lvxxbq13ryfzh8i1267rzyda7wcs-…_boot.img` —
  8.93 MiB (9,367,552 B), sha256 `be6f4d2192d8a95ef762cd17b910fa3af98d3fb98f76a91eef3decd4da4a2e51`,
  gc-pinned `boot-img-lean-20260908`; cmdline carries
  `bootopt=64S3,32N2,64N2`; header geom (kernel 0x40200000, ramdisk
  0x45000000, tags 0x44000000, pagesize 2048) verified pre-flash via
  `bin/dump-bootimg-header.sh`; **flash bytes verified post-flash from
  TWRP**: image-sized prefix of p22 `boot` sha256 == local image
  (whole-partition sha differs only by leftover bytes of the old
  14.7 MiB #329 image past the 8.93 MiB end — benign).
- generation **gen9** `r2mr8hf2l3k7l3029yhb8dgc27i7m84g-nixos-system-
gemini-26.11pre1031299.0bb7ec54c848` — gc-pinned
  `toplevel-20260908-1342`; deployed via `bin/deploy.sh` (5-path delta:
  `linux-6.6.0` + `linux-6.6.0-modules` + etc + toplevel) while gen8 ran,
  then flashed + rebooted to land kernel+gen together (§3 pairing).
- On glass: `uname -r` = **6.6.0 #1-mobile-nixos** (banner not #329);
  `/run/booted-system` kernel-modules = 6.6.0; `modprobe sramldo-smc`
  OK; panfrost at 17.3 s → renderD128 + card0; gemwl + labwc +
  lxqt-session/panel + pcmanfm running, **NRestarts=0**; battery-guard
  active (charging 4.06 V); boot 7.4 s kernel + 56.3 s userspace.
- Failed unit (systemctl --failed): only `gemini-wifi-internal`;
  additionally `gemini-wifi-nvram` is **bad-setting** (malformed unit —
  never ran on any boot; root-caused below). Old #329 `boot`
  auto-backed-up to `stock-dump/boot-20260908-134552.img` during the
  flash.

**fbcon boot-log display quirk (OBSERVED, unverified regression):**
operator noted the kernel boot logs appear **only in the bottom third of
an otherwise-healthy landscape display** (desktop full-screen and proper
→ panel/rule-5 clean). Evidence it may be *inherited*, not a lean
regression: cmdline is byte-identical to the #329 image
(`fbcon=rotate:3 fbcon=font:TER16x32` in both), fb driver + fbcon config
options identical between lean and full-329 configs (only FB_EFI/
FB_CORE/FB_DEVICE/FB_MODE_HELPERS pruned — `/dev/fb*` now absent, fbcon
unaffected). fbcon took over at 0.29 s at 135×33 (full landscape width in
fbcon's rotated accounting; TER16x32 font), then gemwl released it at
19.7 s. LK fb = 1080×2160 portrait buffer, OVL-scanned to landscape;
fbcon's software-rotation glyph grid lands only partially in the visible
window — the desktop (gemwl) renders correctly because it writes pixels
with full knowledge of the OVL layout. Operator: "I think it started with
the new kernel but I'm not completely sure" — NOT confirmed either way;
re-check against a #329 boot when convenient. Cosmetic only (fbcon
console window pre-gemwl). Follow-up if wanted: compare a #329/gen8 boot
visually, or probe alternate rotate values on a bench boot.

**Wifi/keyboard ROOT-CAUSE SPOTS (both "never worked in NixOS" items
re-diagnosed — they are ROOTFS-PACKAGING gaps, not the deep CONSYS
chip issue the 2026-09-07 log assumed):**

- `gemini-wifi-nvram.service` has been **malformed since the original
  port** (c6afc5c): its multi-line `''/bin/sh -c '…' ''` ExecStart lands
  in the unit file with real newlines/indent → systemd
  "Unbalanced quoting"/"Invalid section header" → **bad-setting, never
  ran on ANY boot** (verified: systemd-analyze verify fails on ALL
  stored gens 2/7/8/9; journal receipts Sep 07 15:41 + every boot).
  Consequence: `/data/nvram/APCFG/APRDEB/WIFI` (factory MAC+TX cal)
  and `/etc/wifi/profiles.conf` were never installed (`/etc/wifi` does
  not even exist on glass).
- wlan_gen3 probe fails on **two missing files**, per this boot's dmesg:
  (1) `nvram_read: failed to open!!` / `glLoadNvram fail` ← the dead
  nvram unit above; (2) `kalFirmwareOpen: Open FW image
  WIFI_RAM_CODE_6797 failed` at all three HARDCODED paths
  `/storage/sdcard0`, `/vendor/firmware`, `/lib/firmware` — none exist
  on NixOS (firmware lives in the nix store; the firmware_class param
  path serves request_firmware — the WMT/ROMv3 leg loaded fine via it
  ("live client re-synced to the patched full-mode MCU") — but
  wlan_gen3's kalFirmwareOpen uses its own hardcoded list, not
  request_firmware). The legacy Debian rootfs satisfied it by
  installing blobs into `/lib/firmware/` (GeminiPDA
  `build/rootfs-files/wifi-consys/install-wifi-consys.sh`). Fix
  direction: symlink `/lib/firmware` → the firmware dir (e.g.
  `/run/current-system/firmware`) via systemd-tmpfiles/activation +
  repair the nvram unit ExecStart (single-line or a script).
  **The CONSYS MCU link itself is healthy on this kernel** (resync OK,
  func-on leg 0) — the "deep CONSYS issue" framing from 2026-09-07
  needs re-testing after the two packaging fixes; the 30 s wlan0
  timeout is downstream (wlan_gen3 probe).
- Keyboard mappings never worked because **the Gemini xkb layout is
  not in the NixOS closure**: gemwl hardcodes layout "gemini"
  (`pkgs/gemwl/gemwl.c`, GEMWL_XKB_LAYOUT override) but xkbcommon
  fails every boot: `[XKB-338] Couldn't find file "symbols/gemini"`
  (include paths = the stock xkeyboard-config-2.47 + `/root/.config/xkb`,
  `/root/.xkb`, `/etc/xkb` — all absent). gemwl then falls back to the
  default keymap → wrong UK keysyms / no Fn (level3) layer. The legacy
  Debian rootfs shipped `symbols/gemini` (GeminiPDA
  `build/rootfs-files/xkb/symbols/gemini`, deploy-xkb-gemini.sh) — no
  NixOS equivalent exists yet. Kernel side is FINE: the matrix device
  is event2 "keyboard" (114-key bitmap), NT36772 touch = event0,
  mt6351-keys = event3, USB mouse = event1. Console keymap
  (`console.keyMap = gemini-uk.map`) is set but only covers the VT
  console, not the gemwl/LXQt desktop path.

Device left: **gen9 on p32, para cleared, NixOS default, booted on the
lean 6.6.0 kernel**, desktop up, wifi-internal failed + wifi-nvram
bad-setting (both root-caused above), Debian p29 untouched. Next:
mediatek wifi + keyboard mappings workstream — see
`docs/handover-2026-09-08-wifi-keyboard.md`.

## 2026-09-08 — Kernel SELF-CONTAINED + LEAN: published-base + delta-tree source model; borrow retired; kernel now builds in-nix (aarch64) in ~6 min

**Goal reached:** the rootfs no longer borrows kernel #329 artifacts from
GeminiPDA — the kernel builds in-repo from the published Linux **v6.6**
base + a tracked file-tree delta, and the config is now a pruned
**device-minimal** config. First lean self-built kernel build:

    linux-6.6.0            /nix/store/cgi059k6lsg14vgd5jl288kjxjp9c5w2-linux-6.6.0
    Image.gz 8,016,646 B   sha256 ace1a67874347d1257f2d4aa28a2d25378f87a6fb117db5af097f6a30ae0addf
    DTB (mt6797-gemini-pda.dtb) sha256 462e7140d6f1a819acf9a06543b1758b9269c7d89bc912f6edc29ff65b2b22b1
                           ^ byte-IDENTICAL to the borrowed #329 DTB (cmp, 2026-09-08)
    release 6.6.0 (moddir 6.6.0); 388 modules (was 1165); sramldo-smc.ko vermagic 6.6.0

Source model (kernel/default.nix, docs/library-deltas.md):
- **base** = kernel.org linux-6.6.tar.gz fetched by hash (sha256-PIj/…);
  byte-identical to `git archive v6.6` of the fork (ffc253263a…). Same
  commit pinned as a git submodule at `kernel/base` (read-only pointer,
  `git submodule update --init --depth 1 kernel/base` to fetch).
- **delta** = `devices/planet-geminipda/kernel/delta/` — the 512 plain
  files (457 A + 55 M, 0 D/R) the bring-up line changes over v6.6;
  copy-replace is exact: v6.6+delta == geminipda-bringup@188aade69
  (the #329 tree) — verified byte-for-byte. NO patch files (repo rule:
  agents edit source). Regenerate with `bin/sync-kernel-delta.sh`
  (replaces bin/snapshot-kernel.sh; no more 225 MB tarball).
- **config** = lean (default): `bin/prune-kernel-config.sh` from
  `config.full-329` (the exact #329 config, kept for A/B). Prune drops
  hardware that can never exist: 51 foreign ARCH_* (only ARCH_MEDIATEK),
  ACPI/EFI/XEN/KVM/PCI/ATA/SATA/NVMe/UFS, media/DVB, BT/NFC/CAN/
  802.15.4, vendor HID/touch/DRM (panfrost-only chain kept), foreign
  SoC clk/pinctrl/gpio/mfd/regulator/phy/rtc/leds/nvmem/iio/etc,
  crypto accelerators, DEBUG_INFO+lockdep. Result: 4,213 → 2,876
  textual → 1,655 enabled after the builder's olddefconfig cascade
  (=y 3,083→1,400, =m 1,021→~250). All 79 keep-symbols verified
  present post-normalization. Rule-5 gate now also asserts the config
  (eval-time, regex-free line scan — builtins.match on the 288 KB file
  stack-overflows the evaluator, found 2026-09-08).

Nix-build fixes discovered (all in kernel/default.nix; the delta stays
byte-identical to the fork):
- mediatek-connectivity Makefiles emit RELATIVE -I$(src)… — fine for
  the fork's in-tree builds, broken under the mobile-nixos O= build
  (wmt_core.c lost osal_typedef.h). postPatch anchors them on
  $(srctree).
- CONFIG_EXTRA_FIRMWARE_DIR in both configs pointed at an absolute
  GeminiPDA host path; now "firmware" (relative → $(srctree)/firmware)
  with the ROMv3 blobs staged into the tree at src-assembly from
  pkgs/gemini-firmware/ (tracked in-repo).
- gpio-aw9523b (keyboard expander) uses gpio_chip.irq, which needs
  CONFIG_GPIOLIB_IRQCHIP — a promptless select-only bool the #329
  config got from other (now-pruned) gpio drivers. postPatch adds the
  select to the fork Kconfig entry.
- sandbox quirk: `mkdir $out/firmware` AFTER the tar+cp steps got
  EACCES on the aarch64 builder; creating it FIRST works.

Timing: lean kernel builds in ~6 min total on the 192.168.49.191
builder (the full-config build had not finished drivers at the ~8.5 min
mark when it failed). Payload 13.47 → 8.02 MB (Image.gz); boot.img
headroom ~1.3 MiB → ~6.5 MiB. Module tree 388 .ko (~288 MB unstripped;
strip/DEBUG_INFO follow-up considered).

NOT flashed. The kernel is unverified on glass (as is any rebuild):
next step = the on-glass A/B run documented in
`docs/handover-2026-09-08-kernel-on-glass.md` (build boot.img + new
nixos gen, flash both in lock-step — §3 pairing rule, para=boot-
recovery discipline, rule-5 eyes check); keep kernel/borrowed +
config.full-329 until then (rollback). Also
still owed: docs sweep (README “Kernel phase”, AGENTS M5/M1 rows,
library-deltas kernel entry, .gitignore snapshot comments) — partially
done this session.

## 2026-09-08 — Handover completed: native-aarch64 MERGED into main (native build model canonical); LXQt desktop first on glass (gen8); two repo bugs fixed on the way

Handover (`docs/handover-2026-09-07-lxqt-native.md`) closed out + the
native branch became canonical — full story below; the worktree
`/home/cjdell/Projects/gemini-nixos-native` was removed once merged
(the handover job log was preserved under `logs/jobs/`).

Native toplevel build finished rc=0 (47790 s ≈ 13.3 h on the
192.168.49.191 builder): `b8hyqdvz…-nixos-system-gemini-26.11pre…`
(the deterministic drv `5br2r178…` from the handover). PINNED twice per
the handover/user requirement — verified with `nix-store -q --roots`
and `gc-pin.sh list`:
- per-user: `/nix/var/nix/gcroots/per-user/cjdell/gemini-nixos-toplevel-20260907-native`
- root-level: `/nix/var/nix/gcroots/gemini-lxqt-native-20260907`

Merge (commit 88a2699): `native-aarch64` folded into `main` — flake
`buildSystem = x86_64-linux → aarch64-linux` (devShells stay x86_64),
`pkgs/speaker-amp.nix` cc via `stdenv.cc.targetPrefix`. Follow-ups in
commit 7a5d6e8: `bin/deploy.sh` build verb = the proven native command
(`sudo nix build --store local .#packages.aarch64-linux.toplevel
--option builders @/etc/nix/machines --fallback`; daemon still has no
`builders =` line); flake.nix/README/AGENTS/feasibility R7 corrected
(cross toplevel ABANDONED — no longer the described model). Flake
outputs are now `packages.aarch64-linux.*` only.

**Desktop on glass — the actual test.** Deployed the native closure to
the device (deploy.sh deploy PATH; `nix copy` 2.1 GiB closure took
160 s over g_ether) and exercised the LXQt session live. Generation
history + version lines (rule 0; kernel #329 + dual-boot boot.img on
p22 UNCHANGED throughout — profile switches only):
- gen6 `b8hyqdvz…` (native toplevel): lxqt-nested.service crash-
  looped — `sessionConfig` in services/lxqt.nix did multi-source `cp
  ${file} ${file} $out/dir`, keeping the STORE basenames
  (`<hash>-lxqt.conf` …), so start-lxqt-nested's seed failed
  (`cannot stat …/lxqt/lxqt.conf`). Fixed: cp each file to its exact
  target name (flat layout $out/lxqt/{lxqt.conf,session.conf},
  $out/labwc/{rc.xml,autostart}, $out/themerc) — commit 7a5d6e8.
- gen7 `svwxm2bz…`: seeding OK, labwc died at startup — "Skipping gbm
  allocator: disabled at compile-time / unable to create allocator".
  wlroots 0.18.2's meson `allocators` option defaults to `['auto']`
  and `mesonAutoFeatures=disabled` resolves it to `[]`. Fixed:
  `-Dallocators=gbm` on the withDrmBackend (labwc) variant
  (pkgs/wlroots-geminipda.nix; gemwl's trimmed build unchanged) —
  commit 9a83d46.
- gen8 `3bqy4v4…` (current): **desktop up**. Rebuilds were fast
  (31 s / 28 s — config+wlroots delta only; the 13 h cost was the one-
  off full closure). Verified after a cold WDT-EXRST reboot
  (device-reboot.sh): booted gen8, gemwl + lxqt-nested active,
  NRestarts=0; session = labwc (nested, wayland-1) + lxqt-session +
  lxqt-panel + pcmanfm-qt --desktop + lxqt-policykit-agent +
  lxqt-notificationd + qterminal (autostart); layer-shell surfaces
  mapped; EGL 1.5 on Mali-T880 (Panfrost) Mesa 25.0.7 fork; gemwl
  compositing continuously (noafbc readback path). Root configs seeded
  to /root/.config/{lxqt,labwc} + LXQt runtime confs (panel.conf etc.).
  No crashes/OOM in the boot journal. Only unit failing = the known
  gemini-wifi-internal (CONSYS, pre-existing). **Eyes on glass
  (2026-09-08, user): the desktop renders correctly — panel/desktop/
  windows visible, no flicker/uninitialised-LCD** (core rule 5
  clearance).

**Still owed / next**: (1) The handover's 3a decision: repin nixpkgs
to a hydra-built unstable rev so future native builds are mostly
cache substitutes (current pin 26.11pre1031299 is 404 on cache for
both platforms — that's why Qt/LXQt compiled). Deferred deliberately
(would re-hash the whole verified closure). (2) Cosmetic: labwc built
without libsfdo → titlebar icon falls back to menu button; 1.5x output
scale is best-effort via the start script's wlr-randr probe. Device
left safe: gen8 NixOS on p32, para cleared, Debian p29 intact.


## 2026-09-07 (night) — LXQt desktop ported in-tree (committed); cross toplevel abandoned at nixpkgs-cross walls; native-aarch64 branch + Pi-builder build running (handover: docs/handover-2026-09-07-lxqt-native.md)

User goal: LXQt running like on the GeminiPDA Debian (labwc-nested).
Repo-side port done + committed on `main` 7a5bf23 (device untouched,
still gen5 `c10qkjdw`); the on-glass/closure build went the native
route. **Check-up doc for tomorrow: `docs/handover-2026-09-07-lxqt-native.md`.**

Done / landed (commit 7a5bf23):
- The black screen from the previous session was root-caused: gemwl ran
  fine but its startup client tinytest-anim SEGV'd ~1 s after map
  (dangling listener vtable — libwayland stores the pointer, the
  function-scope compound literals died before the async release/done
  event → wl_closure_invoke crash). Fixed in pkgs/gemwl/tinytest-anim.c
  + tinytest.c (file-scope statics).
- **LXQt nested desktop in-tree**: pkgs/labwc-geminipda.nix (labwc 0.8.3
  pinned, on wlroots 0.18.2), services/lxqt.nix (lxqt-nested.service:
  labwc -S lxqt-session hosting nixpkgs lxqt 2.4/Qt 6.11; XDG_CONFIG_
  DIRS = lxqt etc/xdg autostart assembly, Papirus icon theme,
  QT_PLUGIN_PATH, HOME=/root env), config/lxqt/ (session configs +
  vendored Gemini openbox themerc, seeded by services/scripts/
  start-lxqt-nested), gemwl now runs with no -s client. desktop.nix/
  config/gemini.nix/README/AGENTS/feasibility updated.
- pkgs/wlroots-geminipda.nix gained withDrmBackend (labwc 0.8.3 needs
  wlr_drm_lease_v1 headers; runs nested, never opens a DRM device).
- Built clean (aarch64): labwc-geminipda (a189p0f7…), gemwl (w70sh6in…).
- Overlays (committed, main): openblas 0.3.33 cross DYNAMIC_ARCH
  ARMV9SME missing-file fix (single ARMV8 when host isAarch64);
  libfm/libfm-extra/menu-cache autoreconf AM_GLIB_GNU_GETTEXT fix
  (native glib.dev/gettext/intltool).
- Host disk cleaned (~14 G on `/`; no GC run — roots intact).

Cross toplevel build (main) ABANDONED after: shiboken6/pyside6 (KF6
python bindings — nixpkgs: "cross is currently very broken") fixed by a
kguiaddons hasPythonBindings=false overrideScope overlay (evaluated
clean, then LOST when config/gemini.nix was git-checkout-ed during the
shutdown — re-derive if cross resumes); final wall Qt6CoreTools missing
for the whole lxqt scope under cross (prototype whole-scope
CMAKE_PREFIX_PATH override recursed — unresolved). See the handover for
the full blocker chain.

Native-aarch64 route (user decision):
- Branch `native-aarch64` + worktree /home/cjdell/Projects/
  gemini-nixos-native (flake: buildSystem = aarch64-linux, native eval;
  devShell stays x86_64). Native toplevel drv 5br2r178… (deterministic).
- speaker-amp.nix cc fix (stdenv.cc.targetPrefix) — commit a689b96.
- Distributed build running: host root `--store local` + `--option
  builders @/etc/nix/machines` → Pi 192.168.49.191 (8-core NixOS)
  compiles, host substitutes from cache.nixos.org (Pi's own cache link
  drops large NARs — HTTP 206 — so Pi-local builds were abandoned).
- **Cache truth (asked):** our nixpkgs pin 26.11pre1031299.0bb7ec54c848
  is NOT on hydra's cache (qtbase narinfo 404 for x86_64-native AND
  aarch64-native) → Qt6/LXQt outputs compile once per platform; 1315
  paths still came from cache.nixos.org during the native build. Fix =
  repin nixpkgs to a hydra-built rev (decision pending; native-aarch64
  is the model that benefits).
- Device state unchanged (gen5 on p32, para cleared, Debian p29 intact;
  nothing flashed).

Next (tomorrow, per handover): poll build-native-host from the native
worktree; on rc=0 PIN the toplevel (per-user gc-pin + root-level
`/nix/var/nix/gcroots/gemini-lxqt-native-20260907` root, verify with
nix-store -q --roots); record the version line; then decide nixpkgs
repin vs deploy-native-closure vs reconcile branches.

## 2026-09-07 — PHASE-2 MILESTONE: FIRST NIXOS BOOT ON GLASS (ssh to a NixOS shell over g_ether); the bootopt discovery; full saga + TODO in docs/phase-2-on-glass.md

The moment of truth happened and mostly worked. **NixOS boots and runs on
the hardware** (p32 userdata; hostname gemini; kernel #329; sshd at
10.15.19.82; g_ether; store re-hydrated; gemini-gpu-poweron + battery-
guard active). Debian (p29) intact throughout. The boot image needed
ONE fix before it would boot at all — see the bootopt discovery below.
Full knowledge capture + the open TODO list: `docs/phase-2-on-glass.md`.

Versions flashed this session (rule 0 lines):
- p22 boot.img sha `3965955f91162467d17e8659c103ac67ee4c79a4950bed38ea3cd456ffc364fb`
  (dual-boot initrd, #329 payload `3a2a7f3a…822`, bootopt cmdline).
- p32 system.img sha `091707d7…` (gen `yl6hkkih…` embedded; flash md5-
  verified + first-MiB readback verified). [corrected: the earlier prep
  entry's `dcfv0nsj` gen came from a separate path-info eval — the
  image's own registration says `yl6hkkih`]
- Debian p29 untouched; para cleared at milestone end (NixOS default).

What happened / what was learned (receipts point at phase-2-on-glass.md):

1. **WDT-EXRST silent no-op from a mid-session A72 bring-up** (the
   cl2-up wdt_disarm trap): the first converge-to-TWRP reboot never
   fired (device uptime 13.9 h unchanged; WDT_MODE 0x10007000 = 0). Fix
   discovered + used: `devmem 0x10007000 32 0x2200005D` (key|0x5D)
   restores LK's mode → `0x48` fires → EXRST. (§2b in the doc.)
2. **THE bootopt discovery**: our boot.img (kernel field sha-identical
   to the working Debian image, geometry identical, ramdisk recipe-
   equivalent) hung on the LK logo ~15 s → WDT boot loop, no kernel
   text, empty pstore (death precedes ramoops/fb). Bisected to the
   HEADER: re-packing our kernel+ramdisk with the old image's header
   (pack-boot-img-custom-ramdisk.py --reference) booted Debian through
   OUR initrd's debian branch. Root cause: LK's
   `platform_parse_bootopt(boot_hdr->cmdline)` (load_image.c:839)
   needs `bootopt=64S3,32N2,64N2 log_buf_len=4M` in the field — the
   field is inert for the KERNEL (CMDLINE_FORCE) but LK reads it first
   ([corrected] boot-process.md §4). Fixed in config/gemini.nix
   kernelParams. (§2a.)
3. **Loop recovery proven ~5×**: para restore via the preloader window
   (`run-mtk.sh w para stock-dump/para-boot-recovery.bin` — the loop
   provides the power-cycles) → TWRP in ~25 s. Also confirmed the
   drills-doc FAC_RESET note is not needed when the preloader path is
   available.
4. **run-mtk.sh hardened**: a second python3.14 mtkclient store path
   broke the python3.13 deps scan (Cryptodome vanishing) — pick_pkg()
   now selects a candidate whose deps resolve. (§2d.)
5. **flash-nixos.sh fixed**: 30 s adb timeout killed the 1.5 GiB rootfs
   push → adb_push (900 s) + wc -c verify; TWRP busybox stat has no -c.
   (§2e.)
6. **Boot attempts + recoveries**: boot-nixos #1 (14:50) looped (pre-
   bootopt image); control tests with the Debian backup boot.img proved
   flash/para/eMMC flows; the P3 header test proved the bootopt cause.
7. **First NixOS boot** (fixed image, ~15:27): fbcon log on the LCD ✓,
   initrd markers ✓, switch_root to gen `yl6hkkih` ✓, store rehydrated,
   systemd up, sshd answering. On-glass checks pass: uname #329,
   hostname gemini, 3.6 GiB RAM, gpu-poweron + battery-guard active.
8. **Not-quite-working (→ TODO in the doc)**: growfs (`/` 3.1 GiB —
   udev by-label coldplug race + resize2fs EINVAL at group #25, kernel
   ext4_resize_fs -22); vconsole (setfont TER16x32 not a kbd font);
   gemini-audio-defaults (status 127); gemini-wifi-internal (mtk_wcn
   modprobe); gemini-a72-up failed at boot (should be opt-in like
   Debian's handoff); nixos-rebuild round-trip untested (phase-2
   criterion second half); Debian-branch re-verify on the final image
   pending.

Device left: NixOS running on p32 (milestone state), para cleared,
Debian p29 intact/bootable, A72s offline, battery charging. Nothing
flashed since the successful boot. Commits pending (14 modified + 2
intent-to-add — see git status). Next: the phase-2-on-glass.md TODO
(P0 growfs first).

## 2026-09-07 — p32 DUAL-BOOT DECIDED + IMPLEMENTED (repo-side): NixOS rootfs → Android userdata, Debian stays on p29; images rebuilt + verified; flash plan updated (awaiting user go-ahead)

User decision this session: "override Android" = take the p32 route of
`docs/repartition-android-space.md`. All §10 decisions made (dated in the
doc): **a)** default OS on para-clear = NixOS; **b)** marker = para
offset-0 command field (byte-exact `cmp` in the initrd); **c)** kernel
stays borrowed #329 (Debian keeps booting the same boot.img); **d)** no
p32 ciphertext backup (`--backup-rootfs` dropped). §9 change list landed:

- **`devices/planet-geminipda/initrd.nix` — dual-boot initrd**: reads the
  32-byte para command (p2 of the largest mmcblk, sysfs size read,
  byte-exact `cmp` vs `boot-debian\0`+20 zeros — cmp-on-files because
  ash vars can't hold NULs); zeros/unknown → NixOS default, marker →
  Debian branch replicating `GeminiPDA/build/initramfs-6.6/init` verbatim
  (A72 opt-in + fstab `/` fix + `switch_root /sbin/init`); mode target
  missing → fall back to the OTHER kind's rootfs → shell only if none.
  Fixed during review: `/tmp` did not exist in the initrd staging dirs
  (would have silently ignored the marker); `${…}` inside the nix `''`
  string is interpolated — replaced with `$var` concatenation +
  `$(basename …)`; added applets dd/cmp/chmod/basename.
- **`devices/planet-geminipda/default.nix`**:
  `system_partition_destination = "userdata"` (p32) + comment.
- **`bin/flash-nixos.sh`**: `rootfs` → `by-name/userdata` (p32, ≥20 GiB
  sanity still passes at 27.3 GiB; prompt "Type 'wipe android'");
  `--backup-rootfs` REMOVED (§10d); NEW `debian` verb (running Linux:
  ssh para-write + WDT EXRST self-boot with read-back verify; TWRP:
  adb). New `twrp_para` helper writes any 32-byte marker.
- **`bin/boot-switch.sh`**: NEW `debian` verb (para=boot-debian + reboot
  from TWRP; no adb wait — Debian has no adbd).
- **NixOS side**: NEW `services/scripts/gemini-boot-debian` (mirror of
  gemini-boot-recovery; 32-byte `conv=sync,fsync` write + read-back
  verify) + hand-started `gemini-boot-debian.service` in
  `services/gemini-pda.nix`. NOTE: new untracked files must be
  `git add -N`ed before building — the flake source export only carries
  tracked paths (hit + fixed this session; the packaged utils lacked the
  script until `git add -N services/scripts/gemini-boot-debian`).
- **Docs**: repartition doc → ✅ decided/implemented (§10 + impl record),
  README (intro, layout rows, Flashing steps, unit table, "do not flash"
  text), AGENTS cheat-sheet boot targets + flash pipeline + where-things-
  live rows, boot-process §3/§5/§6/§7 (selector implemented; fallback =
  other-kind rootfs; gemini-boot-debian exists; size correction — the
  earlier 15,433,728 B / 14,108,276 B figures were an older gzip
  encoding; current sha-verified build is 14,815,232 B / 13,489,966 B
  payload), feasibility §9 phase-2 row annotation, session-log entry.

Images rebuilt + verified (NO FLASH — device untouched, still Debian on
p29, para cleared):

- **`result/boot.img`** 14,815,232 B — sha256
  `016c232351bd5de18c1d855cf9c6c2804ceaf532ad6d96d4a65d96d09152dd47`;
  dual-boot initrd verified inside: /init carries the para selector
  (mkdir /dev/pts /newroot /tmp, dd+cmp marker check, debian branch with
  A72/fstab handoff, NixOS gen lookup); **native busybox `sh -n` clean**;
  the four marker writers (boot-switch debian, flash-nixos twrp_para +
  ssh inline, gemini-boot-debian) all produce byte-identical 32-byte
  commands == the initrd's `cmp` reference (tested on host). Kernel
  field unchanged: payload sha `3a2a7f3a…822` (#329, verified).
- **`result/system.img`** 1,640,378,368 B — sha256
  `091707d716767b31835b821ac6f723b7fb5c4fb8b8058775903b163134b3c056`;
  generation `dcfv0nsj…-nixos-system-gemini-…` — now carries
  `gemini-boot-debian.service` + the packaged CLI
  (`vgrm0ygh…-gemini-pda-utils/bin/gemini-boot-debian`, /bin/sh
  shebang kept under R10). ext4 label NIXOS_SYSTEM re-verified.

**Flash plan (supersedes the earlier p29 entry's next-action; run only
on the user's word):** `flash-nixos.sh status` → `boot` (dual-boot
boot.img → p22; current #329 Debian boot.img auto-backed-up to
stock-dump/) → `rootfs --yes` (system.img → p32 userdata, Android FDE
gone) → `boot-nixos` (para-clear → NixOS p32 first boot) → on-glass
checks (ssh `uname -r`, generation `dcfv0nsj`, growfs). Debian stays
untouched on p29 and boots any time via `boot-debian` (host
`flash-nixos.sh debian` / `boot-switch.sh debian`, or on-device
`gemini-boot-debian`). Note: once the dual-boot boot.img is in p22,
Debian boots ONLY through its initrd's debian branch — rollback of
`boot` = `boot-switch.sh restore` (auto-backup). Nothing flashed yet.
Versions: kernel #329 (payload sha `3a2a7f3a…822`); boot.img sha
`016c2323…`; system.img sha `091707d7…`; generation `dcfv0nsj`; Mesa
25.0.7 fork; wlroots 0.18.2; gemwl 1.0; Mobile NixOS `2c132754`;
nixpkgs `nixos-26.11pre1031299.0bb7ec54c848`.

## 2026-09-07 — PHASE-2 PREP: flash images built + verified (nothing flashed); device state recorded; waiting for the go-ahead

Host-side readiness for the first real NixOS rootfs flash. **No flash,
no write to the device** — all checks read-only.

- **Images rebuilt fresh** (`bash bin/run-job.sh start build-images --
  nix build .#packages.x86_64-linux.default`, rc=0, 21 s — heavy deps
  cached from the 2026-09-07 toplevel rebuild). `result/` now carries:
  - `boot.img` 14,815,232 B — sha256
    `7f346637d69f74744861a993da7aab27ec56900c347f6d587ed62a618a943329`;
    fits p22 (16 MiB) with 1.87 MiB headroom. Kernel field verified:
    `kernel/borrowed/Image.gz` (sha `3f8761a4…`, 13,466,943 B) + 23,023 B
    appended DTB = 13,489,966 B payload, sha `3a2a7f3a…822` — the exact
    documented verified #329 payload (boot-process.md §2); decompressed
    sha `96d0cbbb…`. Ramdisk = minimal initrd, gzip cpio with `/init`
    (1,321,716 B, sha `52c7d580…`). NOTE: old build's 14,108,276 B
    kernel field was a different gzip encoding — identity verified via
    the decompressed sha, so the new image is the same #329 kernel.
  - `system.img` 1,640,366,080 B (1.53 GiB) — sha256
    `cf13e8bc45e3f2b21f2f405bbd213f25ea72cec6db6229b69a6148ecb0ef0952`;
    ext4 label `NIXOS_SYSTEM`; generation inside the image =
    `/nix/store/xy5m38g0…-nixos-system-gemini-26.11pre1031299.0bb7ec54c848`
    == current `.#toplevel` (carries the 2026-09-07 outstanding.md
    fixes: ssh key, hostname, keymap, logind, DRM order, backlight, R10).
- **Device state recorded (live over g_ether, kernel
  `6.6.0-00048-g188aade698dd` = #329)**: eMMC = mmcblk0 58.2 GiB,
  boot0/boot1 **4 MiB each** (live measure — resolves the 2-vs-4 MiB
  open question in inventory.md: DA-log figure confirmed, legacy 2 MiB
  superseded; inventory annotation updated); para (p2) = all zeros
  (cleared → NORMAL boot); p29 = Debian 14 G / 28 G used (54 %);
  battery bq25890 voltage_now = 3.884 V (≥ 3.8 V precondition OK).
- **Tooling re-verified**: devshell closure built (adb, mtkclient store
  pkg fetched); ssh key `~/.ssh/id_ed25519_gemini` present; patched
  mtkclient at `/usr/local/lib/mtkclient-patched`; `flash-nixos.sh
  status` → `device state : linux`, both artifacts present.
- **Flash-safety machinery re-read**: `boot-switch.sh flash` backs up
  the current `boot` to `stock-dump/boot-<ts>.img` before writing;
  para backed up once per session only if `stock-dump/para.bin` is
  absent (it exists — 08-30 readback never clobbered); `restore`
  returns the latest backup. `flash-nixos.sh` leaves para =
  boot-recovery (TWRP sticky) until `boot-nixos` is run.

Versions (targets for the next session's version lines): kernel #329
(borrowed, decompressed sha `96d0cbbb…`); boot.img sha
`7f346637…`; system.img sha `cf13e8bc…`; generation `xy5m38g0`;
Mesa 25.0.7 fork; wlroots 0.18.2; gemwl 1.0; Mobile NixOS `2c132754`;
nixpkgs `nixos-26.11pre1031299.0bb7ec54c848`.

Next action (awaiting user go-ahead): `bash bin/flash-nixos.sh
status` → `boot` → (decide: `--backup-rootfs` of Debian p29 first?)
→ `rootfs --yes` → verify serial/fbcon in TWRP-sticky state →
`boot-nixos` → on-glass checks (ssh `uname -r`, generation, growfs,
§13 items). Optional-but-recommended DR before the flash: gather.md
step 2 (preloader dump via DA session, device off) — step 1 now done
(live 4 MiB boot areas).

## 2026-09-07 — GOLDEN-REPO PIVOT: gemini-nixos declared the primary repo for the whole Gemini PDA project; DR playbook ported here from the sibling (no revert of GeminiPDA)

User decision this session: gemini-nixos will **eventually completely
replace GeminiPDA** — new content goes in THIS repo, GeminiPDA is the
legacy source being folded in (not reverted, not edited for new work).
What was done:

- **AGENTS.md rewritten as the golden charter**: new "Repo status:
  GOLDEN" header + transitional rule (a topic's truth is wherever its
  latest content is; port pointers decay), a **migration plan M1–M7**
  (receipt docs / DR knowledge / stock-dump blobs / recovery tooling /
  kernel+LK trees / services / session history), and all existing rules
  0–8b preserved with authority references changed from
  "sibling is the authority" to "this repo is golden; legacy paths are
  transitional".
- **DR playbook ported here**: `docs/disaster-recovery/{README,
  inventory,gather,drills}.md` — inventory now carries a copy-status
  column (here vs legacy-pending M3) and the bulk-migration rsync
  command; tooling references point at this repo's `bin/`.
- **Recovery tooling ported (M4)**: `bin/run-mtk.sh` (patched-mtkclient
  launcher; version-agnostic store lookup + clear errors when the
  devshell closure / patched copy is missing) and `bin/usb-watch.sh`,
  both from the legacy `build/` originals; `mtkclient` (nixpkgs
  2.1.4.1 — verified present in this repo's nixpkgs pin) added to the
  flake devshell so the store pkg + Loader DAs exist for the launcher.
- **Boot-critical blobs copied (M3 partial)**: `stock-dump/` here now
  holds lk/para/para-boot-recovery/recovery/twrp-noswipe/boot/boot2/
  boot3/logo/proinfo/nvram + gpt txt + 2 representative boot images
  (130 MB); every sha256 re-verified OK against the ledger. Bulk
  (android images, firmware zip, ~85 boot backups) stays in
  `GeminiPDA/stock-dump/` until the documented rsync.
- **Pivot notes added** (dated, non-destructive) to the docs that still
  asserted sibling authority: README (golden banner + local DR row),
  boot-process, repartition-android-space, library-deltas,
  mobile-nixos-port-feasibility (marked historical), outstanding.md.
- Legacy sibling edits from earlier this session (DR folder,
  flashing.md pointer, hardware.md [open question], its session-log
  entry) are **left in place** per "no revert" — they are now legacy
  copies; this repo is the golden ledger.

Versions (nothing flashed): unchanged — kernel #329
`6.6.0-00048-g188aade698dd` (borrowed); Mesa 25.0.7 fork; wlroots 0.18.2;
gemwl 1.0; Mobile NixOS `2c132754`; nixpkgs `nixos-26.11pre1031299.0bb7ec54c848`.

Next action: when the device is next on the bench (phase 2 window), run
`docs/disaster-recovery/gather.md` steps 1–8 BEFORE any flash work —
preloader dump + raw GPT + BROM-entry check are one-time, device-healthy
tasks; then continue M1 (port the receipt docs) whenever a doc session
allows.

## 2026-09-07 — A72 cluster power-DOWN brought over (cl2-down.sh, verbatim; build-level)

## 2026-09-07 — A72 cluster power-DOWN brought over (cl2-down.sh, verbatim; build-level)

Ported the sibling's proven A72 power-down path (GeminiPDA @ 738d19f,
2026-09-07) into this repo's script set:

- **NEW `services/scripts/cl2-down.sh`** — byte-identical copy of the
  sibling's `build/a72-bringup/cl2-down.sh` (md5
  `3b70536b79cee74ee156762e09a6a002`), exec bit set. What it does
  (receipts live in the sibling's session-log/hardware.md): per-core
  PSCI offline is safe (cpu9 while cpu8 up; "psci: CPU9 killed (polled
  0 ms)"); the LAST-A72 branch runs the secure power_off_cl3 teardown
  inside the controller's AFFINITY_INFO SMC (UNBOUNDED waits — WDT 20 s
  armed is the recovery), then — only after B_EXT_BUCK_ISO re-assert
  (0x10006290 bit1) + 0x10006218 bit0 clear are confirmed — drops the
  external DA9214 BUCKB rail (vendor cpu_power_off_buck). Post-down
  state = cold-boot state; re-enable = the existing `cl2-up.sh`
  (verified ×2 cycles on the sibling unit).
- **`services/scripts/cl2-up.sh`** — already carried the sibling's
  bus-wait hardening (diff vs 738d19f: empty).
- **Packaging**: no Nix changes needed — `gemini-utils.nix` copies the
  whole `scripts/` dir, so `cl2-down.sh` now ships in `gemini-pda-utils`
  (on-device PATH via `environment.systemPackages`) and gets the R10
  store-bash shebang rewrite automatically. It is a hand-run CLI only
  (`cl2-down.sh [cpu9|cpu8|both]`), deliberately NOT a systemd unit —
  the down is on-demand and must not race `gemini-a72-up` at boot
  (documented in `services/gemini-pda.nix`). Uses only busybox devmem /
  i2c-tools i2cset / coreutils+gnused+util-linux — all already in the
  service/system PATHs or the base closure.
- **Docs**: README unit/CLI table row, `services/gemini-pda.nix` +
  `gemini-utils.nix` headers, feasibility doc R5 addendum + phase-3
  row (2026-09-07 fragment). No stale "never offline" language existed
  in this repo (sibling corrected its own hardware.md).

Versions: unchanged — kernel still the borrowed #329 (pin
`733c0c7ea74195bd30734f599f37e69febfd38e0`); nothing flashed (still
build-level; device untouched). Kernel tree unchanged in the sibling
commit too.

Next action: unchanged — phase 2 on-glass verification; when the NixOS
rootfs is live, `cl2-down.sh both` then `cl2-up.sh` is the on-device
round-trip to prove the pair on this stack.

## 2026-09-07 — boot-process explainer doc (from the Q&A session; repo-only)

Saved the bootstrapping Q&A (kernel identity / cmdline / rootfs selection /
initramfs builds) as **NEW `docs/boot-process.md`** — a plain-language
explainer layered over the receipt docs. Contents: the one NORMAL boot
slot (only `boot` p22 / `recovery` p1 are loadable by LK), boot.img =
kernel+DTB+ramdisk, the shared borrowed #329 kernel (byte-identity
verified: kernel payload sha256 `3a2a7f3a…822` matches between the
sibling's new_kali_boot.img and `kernel/borrowed/`), the ramdisk `/init`
as the rootfs selector (content markers; NixOS store-only vs Debian
`/etc/os-release`), the four cmdline locations + `CMDLINE_FORCE` (both
OSes boot the identical forced cmdline today; per-OS cmdlines only with
per-OS kernels), the para-marker selector table, the two initramfs builds
vs the one proposed dual-boot initrd, and the size constraints shaping it
all. README layout table got a row for the doc. Nothing flashed; no code
changes.

Next action: unchanged — §10 decisions of `docs/repartition-android-space.md`,
then the §9 implementation.

## 2026-09-07 — Android-space repurpose + dual-boot investigation (repo-only; no device interaction, nothing flashed)

Investigated (per the user, no device changes): can the NixOS rootfs go
where Android currently is, keeping the GeminiPDA Debian rootfs (p29) for
testing, ideally bootable without reflashing `boot`? Also: the 16 MiB
`boot` size-constraint risks + workarounds. Outcome = a proposal doc, no
code changes:

- **NEW `docs/repartition-android-space.md`** — full write-up with
  receipts: partition map + LK boot truth (`boot`/`recovery` are the only
  partitions LK loads — `mt_boot.c:1484/1527`; boot2/boot3 never), the
  p32 `userdata` (27.33 GiB) recommendation over p27/GPT surgery, the
  para-command dual-boot selector (`boot-recovery` reserved for LK→TWRP,
  `boot-debian` → p29, zeros → p32 NixOS default), the Debian handoff to
  replicate from the GeminiPDA initramfs (fstab `/` fix, A72 opt-in
  enforcement), boot-budget measurements (boot.img 14.72 MiB of 16 MiB =
  1.28 MiB headroom; kernel gz 13.45 MiB → 34.3 MiB decompressed; LK is
  zlib/gzip-only; RD_* all =y for future initrd formats), the
  shared-kernel constraint (§8), and the repo-side change list (§9) +
  open decisions (§10).
- **`docs/mobile-nixos-port-feasibility.md`**: two dated [superseded
  2026-09-07] annotations — §7 Non-risks "Android partition layout" row
  and §8 decision 4 (Android p27/p32 fate) — pointing at the new doc
  (both previously assumed p29-only repurpose with Android untouched).
- README not touched (nothing flashed/decided yet; its p29-target text
  stays until §10 decisions land).

Evidence gathered this session (facts for the record, all verified
read-only): current on-device `boot` backup `stock-dump/boot-20260907-
013541.img` carries kernel #328 (00047-g3b3a2b6 — the pre-#329 boot),
#329 is what the device runs now (sibling log 2026-09-07); the repo's
borrowed `kernel/borrowed/Image.gz` = the same #329 payload
(00048-g188aade698dd, version string verified); #329 `.config` has
`CONFIG_CMDLINE_FORCE=y` (boot.img cmdline inert for both OSes); store
build artifacts measured: boot.img 15,433,728 B (kernel 14,108,276 B +
ramdisk 1,321,716 B, 2048 pages) + system.img 1,849,479,168 B; para env
window @ 0x20000 (env.h:36-44) — offset-0 command writes never touch it.

Next action: user decides §10 (default OS, marker location, kernel
phase), then implement §9 (initrd dual-boot branch, flash/boot-switch
re-target to p32, boot-debian verb/unit) — still nothing flashed until
the phase-2 on-glass cycle.

## 2026-09-07 — outstanding.md worked: SSH/logind/keymap/DRM-race/udev/backlight/NAT + R10 shebang fix (build-level; nothing flashed)

Worked the `outstanding.md` rootfs-viability list (§13 order). **No
flash** — device still on the GeminiPDA Debian rootfs / kernel #329; it
was reachable over g_ether for read-only captures + one host-NAT test.

Fixes landed (all build-level, toplevel rebuilt + closure-inspected):

- **§2 SSH root key + §11 hostname** (`config/gemini.nix`):
  `users.users.root.openssh.authorizedKeys.keys` = the
  `id_ed25519_gemini.pub` key; `networking.hostName = "gemini"`.
  Closure: `/etc/ssh/authorized_keys.d/root` carries the key;
  `/etc/hostname` = `gemini`. Toplevel now `nixos-system-gemini-…`.
- **§3 logind side-key policy** (`config/gemini.nix`):
  `services.logind.settings.Login.{HandleSuspendKey,HandleHibernateKey,
  HandlePowerKey} = "ignore"` — CORRECTED the audit's fix sketch:
  `services.logind.extraConfig` is **removed** in this nixpkgs pin
  (module now exposes `settings.Login`). Closure `logind.conf` has the
  `[Login]` section.
- **§4 keymap** (vendor + config): `config/keymaps/gemini-uk.map`
  copied verbatim from GeminiPDA `build/rootfs-files/keyboard/` (+ a
  provenance README); `console.keyMap = ./keymaps/gemini-uk.map`.
  Closure `/etc/vconsole.conf` = `KEYMAP=<store path>`;
  `loadkeys --validate` passes.
- **§5 DRM/panfrost race** (`services/gemini-pda.nix`):
  `boot.kernelModules` += `drm drm_shmem_helper gpu-sched panfrost`
  (+ `mt6351-keys` for §11 determinism). Closure
  `/etc/modules-load.d/nixos.conf` lists them; all four .ko verified
  present in the borrowed #329 module tree.
- **§6 udev USB host-PM rule** (`services/gemini-pda.nix`):
  `services.udev.extraRules` with the B-19 three lines; closure
  `99-local.rules` carries them. Device-only rule captured verbatim
  from the live Debian rootfs first.
- **§7 backlight-default** (`services/gemini-pda.nix`):
  `gemini-backlight-default` oneshot unit (10 %, after udevd); in the
  closure + `multi-user.target.wants`.
- **§8 host NAT** (`bin/usb-tether-nat.sh`, NEW): port of the sibling
  `build/usb-tether-nat.sh` — auto-detected upstream iface + tool
  checks. **Verified live** this session: device `ping -c1 1.1.1.1`
  succeeds (~3 ms) with the NAT rule up.
- **§10 wdt/boot-recovery units** (`services/gemini-pda.nix`):
  `gemini-wdt-reboot.service` + `gemini-boot-recovery.service` as
  hand-started oneshots (no `wantedBy`); both in the closure. AGENTS/
  README unit claims now accurate. boot-recovery comment clarified:
  plain reboot powers off → next power-on (sticky para) lands in TWRP.
- **§11 minors**: hostname + mt6351-keys pin done (above);
  renderD129→renderD128 comments fixed in `services/desktop.nix`;
  serial-getty + wifi-DNS remain on-glass checks.
- **§9 GPU warmup**: DECIDED deferred to the first gemwl boot on glass
  (#329 banding question needs glass); risk + both fix options
  documented in the `services/desktop.nix` header.

**R10 — new port delta found while working the list** (feasibility doc
§7 R10, `services/gemini-utils.nix`): the verbatim Debian scripts shebang
`#!/bin/bash`, but a NixOS rootfs has NO `/bin/bash` (stage-2 only makes
`/bin/sh` via `environment.binsh`) and systemd ExecStart execs scripts
straight (kernel resolves `#!`) → every bash unit (gpu-poweron, a72-up/
cl2-up, battery-guard, audio-defaults, backlight) would have failed on
glass with status=203/EXEC. Fix: package-time shebang rewrite to the
store bash. Verified: packaged scripts now `#!<store>/bin/bash`;
`#!/bin/sh` scripts untouched.

Closure inspected: `/nix/store/pscdi0fn9lan4rcnh2c4g9ksvhd3kh58-
nixos-system-gemini-26.11pre1031299.0bb7ec54c848` (== current
`.#packages.x86_64-linux.toplevel`). Rebuilt under `bin/run-job.sh`
(toplevel-rebuild, rc=0). No image was built/flashed — the boot/rootfs
artifacts are unchanged by this session (config/closure only).

Device-only files captured from the live Debian rootfs (pre-flash
insurance, per outstanding.md §0/§12): the udev rule, logind drop-in,
`/etc/modules-load.d/99-gpu.conf`, `backlight-default.service`, root
`authorized_keys` — all match the inline copies in outstanding.md.

Next action (unchanged): phase 2 — on-glass verification. When the
device is next on the bench: `bin/flash-nixos.sh status` → `boot` →
`rootfs --yes` → verify serial/fbcon → `boot-nixos`; then the on-glass
checks per outstanding.md items (ssh `uname -r`, silver button, keymap
Fn combos, `ls /dev/dri`, backlight get, dongle plug, §9 banding watch).
Log the outcome here with image hashes.

## 2026-09-07 — AGENTS.md + flash/recovery tooling imported (build-level; nothing flashed)

What happened (repo-only; no device interaction — unit untouched, still
running the GeminiPDA Debian rootfs on kernel #329):

- **`AGENTS.md` created** at the repo root, adapted from the sibling's
  AGENTS.md (which was read in full). Brought over, re-contextualised for
  the Mobile NixOS port: version/commit hygiene (rule 0), record-before-
  forget + date + receipts, the LCD-panel safety rule (applies to the
  kernel derivation when the in-repo build is finally used — must stay
  the fbcon/EXCLUDE_DISPLAY build), scripts-over-ad-hoc (rule 6),
  devshell-only CLIs (rule 7 — bare host PATH verified to lack
  python3/adb/make, 2026-09-07), run-job detached runner (rule 8), the
  "where things live"/"when to update what" tables, a device-operations
  cheat sheet, and session start/end discipline. New meta-rule for this
  repo: **the sibling GeminiPDA project is the knowledge authority** —
  receipts live there; this repo records port decisions/deltas.
- **Flash/recovery tooling added under `bin/`**, ported from the
  sibling (GeminiPDA @ abc0afb, 2026-09-07):
  - `bin/net-up.sh`, `bin/device-ssh.sh`, `bin/device-reboot.sh` —
    g_ether host-side link/ssh/WDT-EXRST reboot (near-verbatim ports;
    device 10.15.19.82, key ~/.ssh/id_ed25519_gemini).
  - `bin/boot-switch.sh` — adb/TWRP boot-target state machine
    (status/twrp/android/flash/restore). Deltas vs the sibling: adb/lsusb
    resolved via the flake devshell (self re-exec with a
    `GEMINI_DEVSH_REEXEC` guard); dropped the Gemian `linux` command —
    on this project Linux = the NixOS boot.img in `boot` itself, booted
    by `android` (para-clear + reboot); `xxd` replaced with coreutils
    `od` in `status`.
  - `bin/flash-nixos.sh` — NEW orchestration for THIS repo's artifacts:
    converges to TWRP from ANY device state (running Linux → para write
    over ssh + WDT EXRST self-boot; Android → adb hop; POC/offline →
    prompts), then flashes `boot.img` → `boot` and/or `system.img` → p29
    (`linux`, by-name, sanity-checked ≥20 GiB via /proc/partitions since
    TWRP has no blockdev). Safe default: leaves para = boot-recovery
    (TWRP sticky) — never boots an unverified image unattended. Optional
    `--backup-rootfs` (adb exec-out dd, slow → run-job). Rootfs flash
    prompts unless `--yes`/`wipe p29`.
  - `bin/run-job.sh` — verbatim port (usage strings `bin/`-ified);
    smoke-tested 2026-09-07 (sleep job, rc=0).
- **`flake.nix` devShell** extended: python3+git now also
  `android-tools` (adb 36.0.1) + `usbutils` (lsusb) — the bare host
  PATH has no adb (rule 7). Verified `nix develop` resolves both.
- **`.gitignore`**: `logs/` (run-job state) + `stock-dump/` (device
  partition backups — nvram/IMEI private, never commit).
- **`README.md`**: layout table rows for the new `bin/` scripts;
  "Flashing" section rewritten around `bin/flash-nixos.sh` /
  `bin/boot-switch.sh` with the safety model; "do not flash yet" warning
  retained (still build-level only).

Versions (nothing flashed this session — recorded for the record):
kernel #329 `6.6.0-00048-g188aade698dd` (borrowed, pin
`733c0c7ea74195bd30734f599f37e69febfd38e0`); Mesa 25.0.7 fork; wlroots
0.18.2; gemwl 1.0; Mobile NixOS `2c132754`; nixpkgs
`nixos-26.11pre1031299.0bb7ec54c848`.

Next action: phase 2 — on-glass verification. When the device is next on
the bench: `bin/flash-nixos.sh status` → `boot` → `rootfs --yes` → verify
serial/fbcon → `boot-nixos`; log the outcome here with image hashes.

## 2026-09-07 (evening) — OUTSTANDING-ISSUES SWEEP + DEPLOY MECHANISM: gens 2-5 on glass, every documented failure root-caused; R12/R13/R14 + initrd multi-boot bug + panfrost ordering fixed; rootfs grown to 27.3 GiB; first-ever multi-boot NixOS cycle

Worked the phase-2-on-glass.md TODO from a LIVE gen1 NixOS. Outcome:
**NixOS now boots reliably on every power-on** (a latent initrd bug had
silently made every post-first boot fall back to Debian), rootfs is
27.3 GiB, GPU/audio/vconsole/backlight/battery all green on a clean
boot, and a workstation-style build/switch/deploy loop exists.

Root causes found + fixed (all verified on glass, gens 2-5):
- **R12 — systemd 261 removed the unit `Path=` key.** Every service
  PATH via `serviceConfig.Path = lib.makeBinPath [...]` was ignored
  (journal: `Unknown key 'Path'`): audio (amixer), wifi (modprobe),
  GPU (busybox) all status=127. Migrated 11 uses to the nixpkgs module
  option `path = [ pkgs... ]`. New finding → feasibility doc R12.
- **R13 — make_ext4fs image geometry can't grow past 2x.** Kernel
  online-resize EINVAL at 819200 blocks/25 groups (next sparse_super
  backup group's reserved-GDT entries missing from the resize inode);
  reproduced on the host kernel with the real image. mke2fs geometries
  grow cleanly → `pkgs/make-ext4fs-shim.nix` (make_ext4fs CLI on
  mke2fs) via a `lib.mkAfter` overlay (plain overlay defs lost to mnx's
  overlay list). New system.img = flex_bg/64bit/metadata_csum, grows
  1.5G→27G. Current install grown OFFLINE from TWRP
  (`bin/flash-nixos.sh grow-rootfs` NEW verb: static musl aarch64
  e2fsprogs, e2fsck + resize2fs; fs now 7,164,155 blocks = 27.3 GiB,
  e2fsck -fn clean).
- **R14 — service Type/RemainAfterExit under `unitConfig` ([Unit]) is
  ignored** → oneshots ran Type=simple and deactivated on exit; moved
  to serviceConfig. New finding → feasibility doc R14.
- **panfrost boot ordering**: probed at modules-load (16 s) before GPU
  power-on → "gpu soft reset timed out" -110 → no /dev/dri ever.
  Blacklisted at boot + `services/scripts/panfrost-load.sh` retry
  (rmmod+reprobe until renderD128, up to 60 s) as gemwl ExecStartPre.
  gemwl now runs on glass (renderD128). Desktop = first real on-glass
  GPU chain on NixOS.
- **initrd multi-boot bug**: is_nixos used `[ -e profiles/system ]`,
  which fails in the initrd (nix-env profile chain ends in an ABSOLUTE
  /nix/store path; no /nix/store in the initrd namespace). First boot
  only ever worked via nix-path-registration; EVERY later boot fell
  back to Debian. Fixed with readlink-based resolution. Probe mounts
  also now `-o ro,noload` (no 30x journal replay of dirty partitions
  after WDT resets — killed the "orphan cleanup on readonly fs" flood).
- Minor: console.font TER16x32 removed (kernel font, not kbd); a72-up
  opt-in (no wantedBy); boot.growPartition=false (growpart unit was
  failing on the by-label root); wifi `auto` quiet no-op without an
  interface. wifi-internal REMAINS genuinely broken (deep CONSYS issue:
  modules + WMT pwr-on run, wlan0 never appears — "live client resync
  FAIL"/STP-not-ready; needs its own session).

Mechanism (the "workstation" ask): **bin/deploy.sh** — host cross-
builds the toplevel (bounded --max-jobs 8 --cores 8), pins it
(**bin/gc-pin.sh**, per-user gcroots), ships the delta via
`nix copy --to ssh://10.15.19.82`, switches the device system profile
(`nix-env -p /nix/var/nix/profiles/system --set`) + activates.
Generations 2-5 built + deployed this way; old gens stay bootable /
rollback = `deploy.sh rollback`. Host GC hygiene: the operator's
earlier `nix-collect-garbage` swept the whole cross closure (gen3
silently re-cross-compiled ~259 packages) — every deploy is now pinned.

Version lines (rule 0): gens = gen2 `4glxja3x…`, gen3 `81pdlpvx…`,
gen4 `wlyqyp6…`, gen5 `c10qkjdw…` (current);
boot.img p22 now sha `f3050e06…` (fixed initrd; backed up to
stock-dump/); current rootfs fs = 7,164,155 blocks. Kernel unchanged
#329. Deployed through deploy.sh + reboot-verified; device left:
**gen5 booted on p32 (27.3 GiB), para cleared, NixOS default; only
failed unit = gemini-wifi-internal; Debian p29 untouched.**

Next: wifi-internal deep-dive (CONSYS bringup); commit this session's
changes; consider the self-heal profile unit + native on-device
nixos-rebuild plumbing (flake aarch64 outputs) as follow-ups.
## 2026-09-08 (on-device build/switch, gens 29-30) — THE PDA BUILDS + SWITCHES ITSELF: gen30 built AND switched entirely on-device; `nix-shell -p` works against the flake-pinned nixpkgs; device-rebuild.sh + device-repo.sh

Follow-up to the "native on-device nixos-rebuild plumbing" note from
the 2026-09-07 evening sweep. User ask: iterate the config/add programs
on-the-go (no host) and have `nix-shell -p pkg` work on the PDA.

Facts verified on glass gen28 (before any change):
- `/nix/store` is bind-mounted **ro in the MAIN mount namespace** while
  the socket-activated nix-daemon runs in a **private mount namespace
  that sees it rw** (mountinfo: main `ro,…`, daemon ns `rw,…`, different
  mnt ids — the MNX/NixOS read-only-store design). The daemon is the
  only store writer; **root nix clients auto-connect to the daemon when
  the socket exists** (plain `nix-store --add` as root succeeded on
  gen28 — no `store = daemon` line needed).
- The nix module is enabled (nix.conf is generated) but
  `experimental-features` was EMPTY on gen28 → flake builds failed.

Changes (host commit `6b017bd`, deployed as gen29):
- `config/gemini.nix` new "On-device Nix" section:
  `experimental-features = nix-command flakes`; `max-jobs = 2`,
  `cores = 2` (3.6 GiB RAM bound); `sandbox = false` (trusted
  single-user root PDA); daemon build temp on disk via
  `systemd.services.nix-daemon.environment.TMPDIR = /var/tmp` (/tmp is
  a 1.9 GiB tmpfs — a kernel/mesa build needs GBs). Eval receipt:
  putting it under `serviceConfig.environment` failed ("cannot coerce a
  set to a string") — the option is `systemd.services.X.environment`,
  a SIBLING of serviceConfig.
- `nix.nixPath` + nix.conf `nix-path` → the per-user channels dir
  (root's login-shell NIX_PATH + every nix client).
- systemPackages: `git` 2.55.0 + `micro` (device repo + on-the-go
  editing).
- New `bin/device-rebuild.sh` (runs ON the PDA from
  /root/gemini-nixos; verbs status/build/switch PATH/rollback [N]/
  channels/gc; `build` refuses a dirty repo — rule 0) and new
  `bin/device-repo.sh` (host side; seed/push/pull the repo over g_ether
  as a git BUNDLE — no github round-trip, works offline).
- Gotchas hit + fixed during bring-up (all committed):
  `--extra-experimental-features` takes ONE argv token (multi-feature
  values can't survive word-splitting → drop the flag; nix.conf carries
  the features since gen29); bundle-path fetches need an explicit
  refspec (`git fetch bundle main:refs/remotes/host/main` — git won't
  fetch a bundle's implicit HEAD); amending host commits after seeding
  diverges the device clone (reseeded; device had no unique work).

`nix-shell -p` on the device (the ask): `nix-channel` is BROKEN on this
NixOS 26.11 per-user channels layout (EINVAL/"reading symbolic link
…/channels/nixos" against the dangling ~/.nix-defexpr/channels
symlink — nix-channel abandoned). `device-rebuild.sh channels` installs
the channel MANUALLY: the pinned-rev github tarball fetched by nix
itself (`nix-instantiate --eval` of `builtins.fetchTarball`) → symlink
`/nix/var/nix/profiles/per-user/root/channels/nixpkgs` → the SAME
content-addressed store source the flake's fetchTree unpacks to (no
double download) + a GC root. VERIFIED: `nix-shell -p hello --run …`
substitutes + runs on the PDA (note: legacy `-p` builds a stdenv shell
env, so it pulls gcc/binutils from cache each time — cached, but not
free).

On-device build receipts (rule 0):
- gen29 (host-built via deploy.sh): `b54gw7a…` (config change above;
  87 s host build). Device left running it while the repo was seeded.
- **gen30 (DEVICE-built + DEVICE-switched)**: config change made ON the
  device (`8936db5` "add ripgrep" — device git identity mirrored from
  the host), built with `device-rebuild.sh build` → full flake eval on
  the PDA (mnx tarball fetched into the Git cache; config-glue drvs
  compiled locally) → `8vwpdpz…` in ~4.5 min; `device-rebuild.sh
  switch` activated it. Desktop gemwl + lxqt-nested stayed up through
  the switch (NRestarts=0). ripgrep 15.2.0 live. The device commit was
  pulled back to the host (fast-forward) — host main now contains the
  on-the-go work.
- Kernel/boot.img/flash: NONE this session (profile-only switches;
  para untouched).

Device left: gen30 current (`8vwpdpz…`), repo clone /root/gemini-nixos
@ `b58721d` clean, channels pinned to the flake nixpkgs rev
`dc5d91f84032`, desktop up, gens 27-29 selectable for rollback
(`device-rebuild.sh rollback`).

Next: big custom-drv compiles (kernel/mesa) stay on the host/Pi loop
(deploy.sh) — the PDA compiles them only when their sources change
(expect ~30+ min; RAM-bound 2×2 jobs; a zram/swapfile is the open
improvement for desktop-up compiles). Cold-reboot check of gen30 owed.

## 2026-09-09 — Windows D3D9 on the PDA: wine64+box64 deployed, first D3D9 frame rendered; guest-panfrost run wedged the device (power cycle owed)

Task: "add wine + an x86→arm translator and test a simple Windows D3D9 app".

**Stack (all hydra-cached at the flake nixpkgs rev dc5d91f84032 — verified with `nix path-info --store https://cache.nixos.org`, rule 9):**
- box64 0.4.4 (aarch64 x86-64 translator; 5 paths/81 MB)
- wine64 11.0 (x86_64-linux; 342 paths/1.8 GB) — the closure carries glvnd (libEGL dispatch) but NO GL impl
- mesa 26.2.2 (x86_64-linux; 273 MB) — the GL impl; eglPlatforms x11+wayland; wired in via `__EGL_VENDOR_LIBRARY_FILENAMES` (the env var glvnd 1.7.0 actually implements — verified by grepping the lib; `__EGL_VENDOR_LIBRARY_FILE` does NOT exist)
- d3d9test 1.0 — self-written x86_64-windows PE (pkgs/d3d9test/): rotating vertex-coloured cube + GDI FPS overlay; fixed FVF 0x0042 (D3DFVF_XYZ|D3DFVF_DIFFUSE, mingw-w64 ground truth)
- grim 1.5.0 (screenshots)

**Why box64+wine64, not FEX+i686-wine:** fex-emu is not in this nixpkgs; `pkgsCross` has no i686-linux (i686 *is* a valid import system but its wine NAR is 404 on cache.nixos.org → source compile); wine64+box64 is the only all-cached path. box86 NAR also missing at this pin (not shipped — the test PE is 64-bit).

**Build traps found (all in docs/wine-d3d.md §2/§3):**
- winegcc inside the nixpkgs wine64 package is configured NATIVE (`-dumpmachine` → x86_64-unknown-linux-gnu) — emits an ELF + sh wrapper, not a PE. The PE is built with `pkgsCross.mingwW64` (x86_64-w64-mingw32-g++ 15.3.0 + mingw-w64 14.0.0 headers, needs `allowUnsupportedSystem = true`).
- Wine's headers are partial classic-era D3D9 (no flexible-FVF macros, no DEFAULT_SWISS, D3DCAPS9 in d3d9caps.h, D3DMATRIX = anonymous union, C++ mode for COM classes).
- **gemwl had to gain viewporter**: winewayland refuses to init without wp_viewporter. 2 lines in pkgs/gemwl/gemwl.c (wlr_viewporter_create; wlroots 0.18 handles viewport resources internally — no per-window plumbing). Shipped as **gen32** (deploy.sh; system-32-… on device at session end).

**Deployment:** bin/wine-x86-deploy.sh (status/deploy/init/run/log/shot/kill) — `nix copy` of the 5 paths to the device store + /root/wine-x86 launcher + GC roots (host+device). Not a flake systemPackage (1.8 GB would bloat every deploy delta).

**On-glass results:**
- `wine cmd /c echo` → WINE-HELLO-WORLD (stack proven).
- d3d9test: **FIRST FRAME PRESENTED — D3D9 rendering under wine64+box64**, steady **55–60 FPS** on the llvmpipe path (guest mesa swrast — stable for 10+ min).
- Receipts in device:/root/wine-x86/logs/app.log + the WINEDEBUG=+d3d9 log (7346+ DrawIndexedPrimitive calls).

**BUG 1 (wine, deferred):** GetAdapterDisplayMode/GetDeviceCaps page-fault (garbage-pointer read at 0x9000E000D030F) under box64 on wine 11's wayland driver — gemwl has no xdg_output so wine's screen struct is partly uninitialised. Worked around in d3d9test (identity query gated behind D3D9TEST_IDENTITY, off by default). Adapter string is wine's fallback "NVIDIA GeForce 6800" (GL_RENDERER unavailable via D3D9).

**INCIDENT (unresolved, power cycle owed):** to find out whether the T880 could render the guest, I forced `GALLIUM_DRIVER=panfrost MESA_LOADER_DEBUG=all` on the app. Within ~20 s the **device wedged: kernel alive (ping 0.25 ms, g_ether up, ARP normal) but userspace dead** (sshd banner timeout, TCP:22 connect timeout, flat net counters). Controlled pan_js (kernel panfrost job thread) CPU-time A/B with the llvmpipe app showed +1 jiffie over 27 s — i.e. **the stable path was llvmpipe (CPU), not the T880**; the forced-panfrost run is the suspect (guest x86_64 panfrost 26.2.2 + box64 ioctl path + T880 already held by native gemwl). No software reset available (WDT-EXRST needs an ssh shell; preloader needs buttons). **LEARNED: never force guest panfrost on this stack until root-caused; llvmpipe is the safe renderer; guest-panfrost is a separate, risky experiment (fresh prefix, no other GPU users, WDT safety net via para=boot-recovery first).**

Device left: **WEDGED (userspace dead) — needs physical power cycle.** After power-on: `bash bin/device-ssh.sh 'echo ok'` (auto net-up), verify gen32 + desktop, re-run `bin/wine-x86-deploy.sh run d3d9test.exe` (llvmpipe path), take the grim screenshot receipt, then leave para as-is (normal/NixOS default; system is verified) — or para=boot-recovery if more risky work is planned.

Files: pkgs/wine-x86.nix, pkgs/d3d9test.nix, pkgs/d3d9test/d3d9test.cpp, bin/wine-x86-deploy.sh, docs/wine-d3d.md, pkgs/gemwl/gemwl.c (viewporter).
