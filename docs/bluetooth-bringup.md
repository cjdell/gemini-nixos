# Bluetooth bring-up: MT6630 CONSYS hci_stp (session 2026-09-10)

> Status: 🟡 PARTIAL — BT radio powers on and answers HCI, but the
> CONSYS rx path stalls (multi-second) on BT-channel frames, so a full
> HCI init cannot complete yet. Everything below is dated/committed;
> the code is experimental (not wired into any service).

## TL;DR

- **The Gemini's Bluetooth is the BT half of the same MT6630 CONSYS
  combo that Wi-Fi runs on.** The chip has NO separate BT firmware and
  NO bluetooth USB/serial device: BT HCI rides the WMT/STP fabric as
  the "BT channel" (`BT_TASK_INDX`) of the delta `mtk_wcn` module.
- Ported the vendor's 3.18 **in-kernel BlueZ HCI driver** for this
  exact device (`drv_bt/`, `CONFIG_MTK_COMBO_BT_HCI`, the
  `aeon6797_6m_n_halium_defconfig` path) to 6.6 as `hci_stp` →
  **hci0 is a classic `HCI_UART`-bus BR/EDR+LE controller**.
- Fixed three real bugs found on glass:
  1. **BT power-on crashed the combo chip** (WMT WAKEUP handshake
     timeout → whole-chip assert) whenever BT was turned on after
     ~70 s of host-side inactivity: the CONSYS MCU autonomously sleeps
     even on the DMA transport, and the delta's BTIF "wake" was a
     no-op. Fix: pulse the **BTIF WAK line** (`ap_wakeup_consys`,
     BTIF base `0x1100c000` + `0x64`, vendor
     `hal_btif_send_wakeup_signal`: low > one 32k period then high)
     directly from `mtk_wcn` (self-contained ioremap — deliberately
     not via the builtin spike's ops: the spike is in the kernel Image
     and module↔Image ABI drift oopses until the boot.img catches up).
  2. **Init-script command gating** — made every init command
     best-effort and skippable (`init_script` param, default 0).
  3. hci0 opens clean + fast now (`WMT BT function on OK`, ~0.1 s).
- **Remaining blocker (unsolved):** BT-channel RX delivery from the
  CONSYS rx kthread stalls for *many seconds* (observed ~17 s) —
  `Bluetooth: hci0: Opcode 0xc03 failed: -110` while the chip had
  answered correctly ~ms after TX. The reply is delivered only much
  later (we saw a stale reset-command-complete land 17 s late, during
  the next open). This is in the shared CONSYS rx path
  (`drivers/soc/mediatek/mtk-consys-spike.c` `consys_wmtrxd` /
  `btif_rx_drain`), which Wi-Fi's usage pattern apparently never
  exposes. Until it is tamed, the kernel's 2 s HCI request timeout
  always fires first and every `hciconfig hci0 up` ends with
  "Connection timed out".

## What works / what was verified on glass

- `modprobe bluetooth` + `modprobe hci_stp g_dbg_level=3` → hci0
  registered, auto power-on → `opening hci0` → `OPID(3) type(0) ok`
  (WMT BT func-on, ~0.12 s — the WAK pulse wakes the MCU).
- The chip's BT side answers HCI with correct cmd-completes: RX
  `04 0e 04 01 03 0c 00` = HCI Reset cmd-complete (a reply to the
  *previous* reset attempt — proving the controller is alive and the
  framing works).
- BT func-off works (`OPID(4) type(0) ok`), no crashes on open/close.
- The eFUSE BD-address read command returns nothing on this unit
  (vendor cmd `01 09 10 00`); the driver auto-generates a random
  locally-administered address instead (or `bd_addr=` module param).
- Device/deploy plumbing used: kernel is module-only for these fixes
  (boot.img untouched — `CONFIG_BT=m`, `hci_stp`/`bluetooth.ko` are
  in the nix closure), deployed via `bin/deploy.sh` + WDT reboot.
  Reboot method note: the bare `devmem 0x10007004 32 0x48` write does
  NOT reset the unit anymore — `cl2-up.sh` leaves the WDT
  mode=0 (disabled). Full LK-style sequence works:
  `devmem 0x10007000 32 0x22000015` (KEY|EXTEN|IRQ|EN) +
  `devmem 0x10007004 32 0x48` + `devmem 0x10007008 32 0x1971`
  (RESTART_KEY) — see the session log.

## Kernel deltas (fork → delta, sync-verified each time)

Fork: `/home/cjdell/Projects/GeminiPDA/repos/linux-6.6`
(geminipda-bringup branch). Sync: `bin/sync-kernel-delta.sh`.

| Fork commit | Change |
|---|---|
| `b0c0254e5…` | **BTIF WAK wake fix** — `mtk_wcn_btif_wakeup_consys` pulses the WAK line (module-side ioremap). `drivers/misc/mediatek-connectivity/wcn_hw_glue.c` |
| (before it) | **CONSYS PSM never armed on the SOC spike** — `wmt_ic_soc.c` sw_init now always `wmt_lib_ps_disable()` (kept; harmless, complements the WAK fix) |
| `241f3ca4…` → … | **hci_stp driver port** — `drivers/misc/mediatek-connectivity/drv_bt/` (new dir), Kconfig `MTK_WCN_BT_HCI`, top Makefile hook |
| later commits | hci_stp polish: eFUSE autogen, resilient init, `init_script` param, tracing (currently verbose `BT_INFO` level, gated by `g_dbg_level`) |

Repo-side commits (gemini-nixos, `devices/planet-geminipda/kernel/`):
config + config.full-329 gain `CONFIG_BT=m`, `BT_BREDR/y LE=y`,
`CONFIG_MTK_WCN_BT_HCI=m`; `default.nix` rev header updated per sync.

## Next actions (ordered)

1. **Tame the CONSYS rx stall.** Read `consys_wmtrxd`/`btif_rx_drain`
   in `mtk-consys-spike.c` (rx kthread polls ~50 ms + 2-5 ms usleep —
   that alone cannot stall 17 s; suspect a stuck/long wmt_mtx hold or
   a vFIFO-full condition after the assert-reset cycles). Reproduce
   with back-to-back `hcitool cmd` and watch the kthread state
   (`/proc/<pid>/stack`, `wchan`).
2. Once replies land inside the 2 s HCI timeout: `hciconfig hci0 up`
   → hci0 with BD address → `bluetoothctl`/`bluetoothd` → scan.
3. Wire BlueZ into `config/gemini.nix` only after hci0 inits cleanly.
4. Optionally re-enable the init script (`init_script=1`) after the
   stall is fixed, and try the eFUSE BD read again.
5. When a new boot.img is flashed anyway (future kernel-Image
   changes), the spike's `consys_wmt_ops.wake` op exists as the
   "proper" wake path (module-side pulse remains the portable one).

## Files touched this session

- `pkgs`-side: none (no service/config wiring yet — kernel modules +
  driver only).
- Kernel fork + repo delta (see above); repo `kernel/config`,
  `kernel/config.full-329`, `kernel/default.nix`; this doc; session
  log entry (2026-09-10).
