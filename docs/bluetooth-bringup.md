# Bluetooth bring-up: MT6630 CONSYS hci_stp

> Status: ✅ **hci0 inits clean on glass; BR/EDR + LE both enabled;
> discovery finds real devices over the air** (2026-09-09l). Transport
> rx-stall root-caused and fixed (MCU autonomous sleep).
> **GUI + CLI tools WORKING** (2026-09-09m): persistent bluetoothd on
> the system bus (gens 52-53, `services/bluetooth.nix`), auto-powered at
> boot (Privacy=off receipt below), bluetoothctl/btmgmt/hcitool CLIs on
> PATH, blueman-manager window + SNI tray applet in the LXQt session.
> Remaining: the LE event-mask acceptance-range quirk, an RF/pairing
> test with a device close by, PAN tethering (no CONFIG_BT_BNEP).
>
> **A2DP (audio) is a separate layer, now FIXED** (2026-09-10r) — the
> HCI link above inits and connects, but streaming used to stutter-stall.
> Root cause was the **STP PSM**, whose deep-idle backend is an
> unimplemented stub; it desynced the BTIF link every ~1.5 s. The delta now
> forces it off (`wmt_lib.c`). See **`docs/bluetooth-a2dp.md`** for the
> measurement table and on-glass verification.

## TL;DR

- **The Gemini's Bluetooth is the BT half of the same MT6630 CONSYS
  combo that Wi-Fi runs on.** No separate BT firmware, no bluetooth
  USB/serial device: BT HCI rides the WMT/STP fabric as the "BT
  channel" (`BT_TASK_INDX`) of the delta `mtk_wcn` module.
- Ported the vendor's 3.18 **in-kernel BlueZ HCI driver**
  (`drv_bt/`, `CONFIG_MTK_COMBO_BT_HCI`) to 6.6 as `hci_stp` →
  hci0 is a classic `HCI_UART`-bus BR/EDR+LE controller.
- **The CONSYS rx "stall" was ROOT-CAUSED (2026-09-09l) and fixed: it
  was the CONSYS MCU's AUTONOMOUS SLEEP, not a vFIFO deadlock.** With
  the device idle, a HCI reset's reply sat INSIDE the asleep MCU and
  surfaced only at the next open's func-on WAK pulse — measured 17 s
  late (83 min in one case). The RX vFIFO was empty at every register
  sample while the reply was "missing" (`0x11000aac` WPT=RPT,
  VALID=0) and a bare WAK pulse with NO traffic released the stuck
  reply instantly. Wi-Fi never exposed this because its traffic is
  continuous.
- Three kernel-side fixes, all **module-only (no boot.img reflash)**:
  1. `05bbb33d3` — **wake-before-send**: `mtk_wcn_btif_write` pulses
     the BTIF WAK line before every transport write.
  2. `16e6d817` (write-triggered, replaced) → `440be2a1` —
     **open-gated WAK keep-awake heartbeat**: pulse WAK every ~30 ms
     from `mtk_wcn_btif_open` until `mtk_wcn_btif_close`. A
     write-triggered hold was too short: a scan/connection runs for
     seconds with NO host writes, the MCU dozed mid-session, the STP
     TX layer hit its retry limit and the spike live client resynced
     (seen during the first bluetoothd discovery).
  3. `7dce5d17`→`f6b135a3` — **HCI_QUIRK_EXT_INIT_BEST_EFFORT**: the
     MT6630 OVER-ADVERTISES capabilities and refuses the matching
     commands (LE Set Event Mask 0x20, Write LE Host Supported 0x12,
     GET_MWS_TRANSPORT_CONFIG...), which aborted every hci0 open and
     kept BR/EDR down too. The le_init3 + hci_init4 + le_init4 stages
     now run best-effort (log + continue).
  4. `7af6ee9c` — **record LMP_HOST_LE + HCI_LE_ENABLED locally**:
     the kernel's whole LE-enabled state hangs off the refused 0x200d
     (its completion handler sets the flags); without it mgmt LE ops
     are REJECTED (0x0b). Under the quirk the host state is applied
     directly and the command skipped. After this, `btmgmt le on`
     reports "powered br/edr le" and discovery works.

## Verified on glass (2026-09-09l)

- hci0 **UP RUNNING** — full HCI init completes (init_script=1 radio
  config works; eFUSE BD 00:00:46:02:79:01 read back; controller
  self-name "MTK MT0279 #1").
- Every command answered on a clean ~40-80 ms cadence (the 2 s HCI
  timeout no longer fires). The WAK heartbeat keeps the MCU awake for
  whole sessions; 0 STP timeouts / 0 spike resyncs across repeated
  40 s discoveries.
- BR/EDR inquiry runs its full window cleanly (no BR/EDR devices in
  range to report).
- `btmgmt le on` → settings "powered br/edr le".
- `btmgmt find` (mgmt discovery) found **real LE devices over the
  air**: `4A:A8:59:57:1B:65` and `41:BE:81:BB:F9:E4` (LE Random,
  rssi -96..-98 — distant beacons).
- bluetoothctl end-to-end works on a private system dbus (dbus
  policy: org.bluez is NOT in the stock NixOS `/etc/dbus-1` static
  tree — a test bus on its own socket with a permissive policy was
  used, daemons kept alive via `systemd-run` units):
  `power on` succeeded, `scan on` → "Discovery started /
  Controller ... Discovering: yes". (No NEW lines in the 60 s capture
  — the only in-range devices are the two very weak beacons above.)

## Known quirks of this controller (all observed with MCU held awake)

- **LE Set Event Mask (0x2001) is content-gated**: accepts only
  low-bit masks (0x00/0x01/0x02 = adv-report ok), refuses 0xd0 05 /
  0xff ff ff ff ff ff ff ff (0x20). Kernel's standard mask d0 05 is
  refused at open → best-efforted. Follow-up: map the kernel LE mask
  to the accepted low bits, or the adv-report bit alone suffices for
  scanning.
- **Write LE Host Supported (0x200d) refused (0x12)** even though LE
  runs fine — fixed by the local-record patch (7af6ee9c).
- **LE Set Scan Enable/Params + LE reads all work** at runtime.
- **LE Set Random Address (0x2005) works**, scan enable 0x200c works.
- Feature/command over-advertising: GET_MWS_TRANSPORT_CONFIG refused
  with the MWS bit set, some LE 5.x cmds unknown (0x01).

## Transport / sleep model (why the fixes look the way they do)

The vendor wakes the chip through the negotiated WMT sleep path
(SLEEP/WAKEUP/HOST_AWAKE commands); this port disables CONSYS PSM (the
spike never arms it — `wmt_ic_soc.c` sw_init does
`wmt_lib_ps_disable()`), so the MCU's autonomous sleep is only
defeated by the BTIF WAK line. While BT is open we hold the MCU awake
with a ~30 ms WAK pulse train (a 30-50 ms pulse costs nothing
measurable; waking an awake MCU is harmless). Closing hci0 lets it
sleep again; the next session's first write wakes it.

BTIF regs (MT6797): base 0x1100c000, **WAK = 0x64** (WO, pulse low >
one 32k period ~64-96 µs then high); RX DMA 0x11000a80 area:
VFF_WPT +0x2c, RPT +0x30, VALID +0x3c; UART LSR 0x1100c014.
consys_wmtrxd = the builtin spike RX kthread (pid 89 typically).

## Remaining follow-ups (ordered)

1. **LE event-mask quirk** (0x2001 low-bit acceptance) so the kernel
   receives LE events with its standard flow; today the mgmt path
   still delivers DeviceFound (verified) but the mapping is worth
   doing for bluez.
2. **RF/pairing test with a real device** (the beacons were at -96 dBm
   through walls). Keyboard/mouse or phone visible at <5 m confirms
   BR/EDR inquiry + pairing/connection next (nothing has been PAIRED
   yet — only scanned).
3. **PAN tethering**: kernel lacks CONFIG_BT_BNEP (bluetoothd:
   "kernel lacks bnep-protocol support") — module-only kernel config
   change if wanted.
4. `hci_stp` module-vanish quirk (failed opens can drop bluetooth +
   hci_stp from lsmod while the controller keeps working; re-modprobe
   recreates hci0) — cleanup TODO in the failed-open path (deep-idle
   stub WARNs, mtk_wcn_stub_alps.c).
5. Cosmetic: bluetoothd-start mgmt failures on this controller
   ("Failed to clear/add UUID" 0x03, adv-monitor reset) — SDP/UUID
   records via mgmt; watch whether pairing is affected.
6. When a new boot.img is flashed anyway, the spike's
   `consys_wmt_ops.wake` op exists as the "proper" wake path
   (module-side pulse remains the portable one).

## Kernel deltas (fork → delta, sync-verified each time)

Fork: `/home/cjdell/Projects/GeminiPDA/repos/linux-6.6`
(geminipda-bringup branch). Sync: `bin/sync-kernel-delta.sh`.
Repo kernel rev header (`devices/planet-geminipda/kernel/default.nix`)
tracks the fork HEAD each sync.

| Fork commit | Change |
|---|---|
| `05bbb33d3` | **wake-before-send on every BTIF write** (rx-stall fix 1) |
| `440be2a1` | **open-gated WAK keep-awake heartbeat** (rx-stall fix 2; replaced the write-triggered `16e6d817`) |
| `f6b135a3` | **HCI_QUIRK_EXT_INIT_BEST_EFFORT** (le_init3 + hci_init4 + le_init4 tolerance; renamed from the earlier `7dce5d17` LE-only version) |
| `7af6ee9c` | **local LMP_HOST_LE + HCI_LE_ENABLED record** when 0x200d is refused (LE mgmt unlock) |
| (earlier) | BTIF WAK wake fix, CONSYS PSM never armed, hci_stp driver port + polish (see git log / session log 2026-09-09k) |

## Persistent service + GUI/CLI tools (2026-09-09m)

Follow-up #1 closed: `config/gemini.nix` now imports `services/
bluetooth.nix` (gen52 first cut, gen53 + Privacy=off):

- **Option name correction**: the MNX eval DOES import the full nixpkgs
  module list (mobile-nixos `lib/release-tools.nix` evalWith:
  `../modules/module-list.nix ++ nixos/modules/module-list.nix`), so
  the nixpkgs bluetooth module is in scope — under its CURRENT name
  `hardware.bluetooth` (`services.bluetooth` was renamed away before
  this nixpkgs rev; the earlier "option does not exist" was the rename,
  not the module list). Enabling it gives: bluetoothd
  (dbus-org.bluez.service alias) wanted at boot via
  `systemd.targets.bluetooth`, bluez in systemPackages (CLIs), the
  org.bluez dbus policy (bluez in `services.dbus.packages` — the
  bring-up's missing-policy problem is gone), udev rules and
  `/etc/bluetooth/main.conf`.
- `gemini-bt-hci.service`: `modprobe hci_stp` after the internal-wifi
  CONSYS bring-up (module tree ships it in current-system).
- **Privacy = off is REQUIRED for the boot path**: bluez 5.87 runs mgmt
  set-privacy during AutoEnable at daemon start and the MT6630 rejects
  it ("Failed to set privacy: Rejected (0x0b)") -> controller stays
  Powered: no after every reboot (interactive `power on` always
  worked). Verified with a throwaway `bluetoothd -f` conf first.
- **GUI**: blueman 2.4.6 — `blueman.desktop` autostart via the merged
  profile etc/xdg/autostart; applet + blueman-tray run in the session.
  The LXQt panel had NO tray -> added the `[statusnotifier]` plugin
  group to `config/lxqt/panel.conf` (seeded) + the live file. Proof:
  StatusNotifierWatcher `RegisteredStatusNotifierItems =
  [":1.x/org/blueman/sni"]`. `blueman-manager` opens a window on the
  nested labwc (grim captures at /tmp/bt-mgr-final.png + a pre-gen53
  /tmp/bt-glass-labwc.png on the host). lxqt.nix adds blueman to the
  session lxqtApps when `config.hardware.bluetooth.enable`.
- Cosmetic: bluetoothd logs mgmt "clear/add UUID" 0x03 + adv-monitor
  failures on this controller at start; kernel lacks CONFIG_BT_BNEP
  (PAN tethering follow-up).

## On-glass test rig

`bin/bt-glass-test.sh` — up / up-wak / regs / wak / init1 verbs
(device-side `nix-shell -p bluez` supplies hciconfig/hcitool/btmgmt/
bluetoothctl; dbus-daemon from `nix-shell -p dbus`). bluetoothd is
under `libexec`/`bin` of the bluez store path (not on the nix-shell
PATH) — call it by store path. Discovery/daemon runs longer than one
ssh session go under `bin/run-job.sh`.

## Files touched 2026-09-09l / 2026-09-09m

- Kernel fork + repo delta (`mtk_btif` wake+heartbeat in
  `wcn_hw_glue.c`, `hci_sync.c`/`hci.h` best-effort init + LE record,
  `hci_stp.c` quirk wiring); `kernel/default.nix` rev header.
- `bin/bt-glass-test.sh` (new); this doc; session logs 2026-09-09k/l.
- 2026-09-09m (config-only, gens 52-53): `services/bluetooth.nix`
  (new), `config/gemini.nix` (import), `services/lxqt.nix` (blueman in
  the session when bluetooth enabled), `config/lxqt/panel.conf` (new,
  SNI tray) + `services/scripts/start-lxqt-nested` (seed it). The
  earlier "bluetoothd wiring is follow-up #1" note is obsolete — it is
  DONE (see the section above).
