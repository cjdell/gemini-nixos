# Handover — first on-glass test of the SELF-BUILT LEAN kernel (2026-09-08)

**Intent:** boot the device on the first kernel that is **built in-repo
by Nix** (no #329 borrow) — the lean-config kernel assembled in commit
`bdca906` — and verify the NixOS rootfs + services still work on it.
This is a *kernel-only* change of the boot path: the boot.img kernel
payload swaps from the borrowed #329 kernel to the self-built one; the
rootfs (p32) and its system generation move in lock-step (see §3).

Precedent for the format: `docs/handover-2026-09-07-lxqt-native.md`.
After the run, fold receipts into `docs/session-log.md` +
`docs/phase-2-on-glass.md` per the golden rules. **Nothing here has
been flashed yet** — this session's work stopped at "kernel builds
green + byte-checks pass".

---

## 1. Artifacts

### 1a. The kernel (BUILT — verified off-device)

| | |
|---|---|
| store path | `/nix/store/cgi059k6lsg14vgd5jl288kjxjp9c5w2-linux-6.6.0` |
| release / modDirVersion | `6.6.0` (no git → no `-00048-g…` suffix; self-consistent) |
| Image.gz | 8,016,646 B — sha256 `ace1a67874347d1257f2d4aa28a2d25378f87a6fb117db5af097f6a30ae0addf` |
| DTB `dtbs/mediatek/mt6797-gemini-pda.dtb` | sha256 `462e7140d6f1a819acf9a06543b1758b9269c7d89bc912f6edc29ff65b2b22b1` — **byte-identical to #329's DTB** (cmp) |
| modules | 388 `.ko` under `lib/modules/6.6.0/` incl. panfrost, drm, drm_shmem_helper, gpu-sched, mtk_wcn, wlan_gen3, rtw88_{core,8821c,8821cu}, sramldo-smc (`extra/`, vermagic `6.6.0 SMP preempt mod_unload aarch64`) |
| config | lean (`devices/planet-geminipda/kernel/config`, from `config.full-329` via `bin/prune-kernel-config.sh`); rule-5 gate asserted at eval |

Source identity: published Linux v6.6 base (kernel.org tarball,
fetch-pinned) + `devices/planet-geminipda/kernel/delta/` (==
geminipda-bringup @ `188aade69`, byte-verified). Banner will read
`6.6.0 … #1-mobile-nixos SMP …` — **not** `#329`.

### 1b. To build this session (NOT yet built)

```sh
# dual-boot boot.img with the LEAN kernel + the (unchanged) minimal initrd
sudo nix build --store local --option builders @/etc/nix/machines --fallback \
  -L --out-link /tmp/bootimg-lean .#packages.aarch64-linux.bootimg
# and the new system generation (module tree 6.6.0 + services) for the device
bash bin/deploy.sh build        # or: run-job start gen-build -- bash bin/deploy.sh build
```
Record in §7 when done: boot.img sha256, total size (expect ~9.3 MiB —
payload 8.0 + initrd 1.26 + headers — vs 14.7 MiB for #329; 16 MiB
partition, ~6.5 MiB headroom), `bootopt` presence (see §4), toplevel
store path + gc-pin (`bin/gc-pin.sh list`).

---

## 2. Current device state (as of the previous session)

- p32 `userdata`: **NixOS**, booted gen8 `3bqy4v4…` — desktop
  (gemwl + lxqt-nested) verified on glass; para cleared; Debian p29
  intact.
- Running kernel: borrowed **#329** `6.6.0-00048-g188aade698dd`; the
  gen8 closure's module tree is the borrowed one (modDirVersion
  `6.6.0-00048-g188aade698dd`).
- Known open items that this run should re-check (from
  `docs/phase-2-on-glass.md` §4): `gemini-a72-up` boot behaviour
  decision ([P2]), `gemini-wifi-auto` quiet no-op ([P2]), serial
  console capture ([P3]), p32 e2fsck health ([P3]).

---

## 3. The kernel ↔ modules pairing rule (READ FIRST)

The NixOS rootfs loads kernel modules from the **booted generation's**
closure (`/run/booted-system/kernel-modules`), and the kernel comes
from the **boot partition**. They must agree on `modDirVersion`:

- new kernel (`6.6.0`) + old gen8 (modules `6.6.0-00048-g…`) →
  modprobe/vermagic mismatch: `sramldo-smc`, the drm chain, wifi… all
  fail to load (boot-critical `=y` still works → the system would boot
  but broken).
- old kernel + new gen → the same mismatch in reverse.

**Therefore:** deploy the new generation **and** flash the new
boot.img, then land on both in **one reboot**. Rollback is the same
pair: old boot.img (automatic backup) + `deploy.sh rollback N`.

---

## 4. Preflight checklist

Host:
- [ ] `bash bin/flash-nixos.sh status` — device state + refresh
      `result/` first: `sudo nix build --store local … -o result
      .#packages.aarch64-linux.default` (the repo `result` symlink is
      stale from an old build).
- [ ] boot.img built (§1b) and **cmdline field contains
      `bootopt=64S3,32N2,64N2`** — LK consumes it via
      `platform_parse_bootopt`; without it the boot hangs on the LK
      logo before any kernel output. Check:
      `bash bin/dump-bootimg-header.sh /tmp/bootimg-lean/boot.img`
      (and that kernel load addr/ramdisk/tags/pagesize match §3.1
      docs).
- [ ] Serial console rig ready (UART0 @ 0x11002000, 921600 8N1 — USB-C
      mux; legacy GeminiPDA receipt) + `script /tmp/boot-serial.log`.
      fbcon starts late; serial is the only full-console view.
- [ ] mtkclient preloader recovery available at the desk
      (`bin/run-mtk.sh`; drills in `docs/disaster-recovery/drills.md`)
      — a hung normal boot has **no software path back**.
- [ ] Battery comfortable (> ~40 % per `bash bin/device-ssh.sh
      'battstat'` when reachable; TWRP battery reads are stale — dmesg
      `[PE+]Ibat=…`).

Device side:
- [ ] Boot backups present (`stock-dump/`; `flash-nixos.sh boot` makes
      one automatically, and `boot-switch.sh restore` rolls back).
- [ ] Rule-5 discipline: this kernel is the fbcon/EXCLUDE_DISPLAY
      build (config gate asserts it), but **judge the glass by eyes**:
      any "uninitialised" LCD flicker → power off immediately, back to
      TWRP, do not continue.

---

## 5. Run plan

1. **Build** (§1b). Long ops under
   `bash bin/run-job.sh start NAME -- …` + `wait NAME` (rc 0 ok / 1
   failed / 2 running — poll, don't sleep).
2. **Deploy the new gen while the OLD system is still running** (the
   only state with ssh/g_ether):
   ```sh
   bash bin/run-job.sh start gen-deploy -- bash bin/deploy.sh deploy
   ```
   Activation is safe under the old kernel (no modules are loaded at
   switch time); the new gen only takes effect on the next boot. First
   `nix copy` of the kernel + module-tree delta over g_ether is the
   long pole (≈300–500 MB) — hence run-job.
3. **Flash the new boot.img** (converges to TWRP from the running
   Linux via para-write + WDT EXRST):
   ```sh
   bash bin/flash-nixos.sh boot /tmp/bootimg-lean/boot.img   # stays in TWRP
   ```
4. **Verify the flash bytes from TWRP before ever booting them** (adb,
   devshell): `adb shell sha256sum
   /dev/block/platform/mtk-msdc.0/11230000.msdc0/by-name/boot` must
   equal §7's boot.img sha256.
5. **First boot of the new kernel** — deliberate, serial + eyes on
   glass:
   ```sh
   bash bin/flash-nixos.sh boot-nixos    # para clear + reboot → normal boot
   ```
   Watch: LK → kernel banner **`Linux version 6.6.0`** → console
   `ttyS0,921600` → minimal initrd scans p32 → stage-2 → fbcon.
6. **Verify** (§6 checklist).
7. **Record version lines** (§7) in `docs/session-log.md` + this doc's
   state table, and say how the device was left.

---

## 6. What "good" looks like (pass criteria)

- [ ] `uname -r` = `6.6.0`; banner shows `#1-mobile-nixos` (not #329).
- [ ] **Glass sane** (rule 5: eyes/TWRP judge — no flicker; fbcon
      rotated as before).
- [ ] g_ether ssh back: `bash bin/device-ssh.sh 'uname -a'` (auto
      net-up).
- [ ] Modules of the booted gen are 6.6.0:
      `ls /run/booted-system/kernel-modules/lib/modules` →
      `6.6.0`; `modprobe sramldo-smc` loads (vermagic match).
- [ ] `gemini-gpu-poweron` active; panfrost chain loads
      (`dmesg | grep -i panfrost`); desktop gen features still up
      (gemwl + lxqt-nested, NRestarts=0) — the gen8 desktop proof
      rerun.
- [ ] Battery/guard + WDT reboot unit sane; `bash bin/device-reboot.sh`
      round-trip works (new kernel WDT EXRST self-boot).
- [ ] Re-check the phase-2 open items (§2): a72-up policy decision,
      wifi-auto quiet, serial log captured ([P3]), p32 e2fsck ([P3]).
- [ ] Timings/size sanity recorded (boot time vs #329, Image 8.0 MiB
      → boot.img ≈ 9.3 MiB).

## 7. Failure modes / recovery

| Symptom | Path |
|---|---|
| Hang at LK logo (no banner) | bootopt missing from cmdline — re-check §4 header before flashing; recover: mtkclient preloader → TWRP → reflash |
| Kernel banner but no userspace / hang later | power-cycle → LK → TWRP (if para was left boot-recovery) or mtkclient (if cleared); restore old boot.img + `deploy.sh rollback` |
| Module-load failures (sramldo/panfrost/wifi) | pairing broken (§3) — both artifacts must move together; fix + reboot |
| LCD flicker / uninitialised panel | power off NOW, TWRP, never continue (rule 5) |
| Anything else | `docs/disaster-recovery/` levels 0–2 playbook |

Rollback pairing: `bin/boot-switch.sh restore` (boot.img backup) +
`bash bin/deploy.sh rollback N`.

---

## 8. Open questions / decisions for this run

- a72-up at boot: keep disabled-by-default like the Debian handoff
  (wedge risk) or opt-in? Decide before/while testing (§2 [P2]).
- Whether the 288 MB unstripped module tree in the closure is
  acceptable (strip in `postInstall` if not).
- Whether to also time-build the full #329 config (`config.full-329`)
  as the byte-parity control (cheap now: ~6 min is the lean build;
  full is the 3,083-=y build — run under run-job if wanted).
- Wifi-internal failed unit on the last on-glass run — re-test under
  the new kernel and either fix or document.

## 9. References

- Kernel session receipt: `docs/session-log.md` 2026-09-08 entry
  (commit `bdca906`).
- Source model: `docs/library-deltas.md` (kernel entry),
  `devices/planet-geminipda/kernel/default.nix`,
  `bin/prune-kernel-config.sh`, `bin/sync-kernel-delta.sh`.
- Boot/flash: `docs/boot-process.md`, `docs/phase-2-on-glass.md`,
  `bin/flash-nixos.sh`, `bin/boot-switch.sh`, `bin/deploy.sh`,
  `bin/dump-bootimg-header.sh`.
- Recovery: `docs/disaster-recovery/` (mtkclient levels 0–2).
