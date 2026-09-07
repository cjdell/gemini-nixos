# gemini-nixos session log

Dated entries of what was actually tried / decided in this repo.
Hardware/boot ground truth lives in the sibling project
(`/home/cjdell/Projects/GeminiPDA/docs/session-log.md`) — cross-reference
when a session touches device behaviour. Latest entry first.

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
