#!/usr/bin/env bash
# bt-glass-test.sh — Bluetooth bring-up on-glass test (2026-09-11).
#
# After the mtk_wcn wake-before-send fix (kernel delta 05bbb33d3, module
# only — no boot.img reflash) this drives hci0 through a full HCI init
# and reports where it stalls. Also usable as a manual-WAK probe rig:
#
#   bash bin/bt-glass-test.sh up           hciconfig hci0 up (plain)
#   bash bin/bt-glass-test.sh up-wak       hciconfig up + WAK pulse every
#                                          80 ms (pre-fix proof rig)
#   bash bin/bt-glass-test.sh regs         sample the BTIF RX DMA regs +
#                                          consys_wmtrxd kthread state
#   bash bin/bt-glass-test.sh wak          one manual WAK pulse (releases
#                                          a reply stuck in a sleeping MCU)
#   bash bin/bt-glass-test.sh init1 [N]    set init_script param to N and up
#
# Device access: bin/device-ssh.sh (auto net-up). bluez tools are pulled
# on-device via `nix-shell -p bluez` (pinned to the flake nixpkgs rev).
# Regs: BTIF RX DMA @0x11000a80 (VFF_WPT+0x2c RPT+0x30 VALID+0x3c),
# BTIF LSR @0x1100c014; WAK line @0x1100c064 (pulse = low 64-96us high);
# consys_wmtrxd kthread = the spike RX poller (builtin, pid 89 typically).
set -eu
repo=/home/cjdell/Projects/gemini-nixos
dev=10.15.19.82
devssh() { bash "$repo/bin/device-ssh.sh" "$1"; }

up() {
    devssh 'export PATH=$PATH:/run/current-system/sw/bin
    dmesg -c > /dev/null 2>&1
    echo "=== hciconfig hci0 up ==="
    timeout 15 nix-shell -p bluez --run "hciconfig hci0 up" 2>&1 | tail -2
    echo "--- hci state ---"
    timeout 30 nix-shell -p bluez --run "hciconfig hci0" 2>&1 | head -6
    echo "=== dmesg tail ==="
    dmesg | grep -E "HCI-STP|hci0|Opcode|wmt-exp|WMT-CORE" | tail -15'
}

up_wak() {
    devssh 'export PATH=$PATH:/run/current-system/sw/bin
    dmesg -c > /dev/null 2>&1
    echo "=== hciconfig up + WAK every 80 ms (pre-fix proof rig) ==="
    nix-shell -p bluez --run "hciconfig hci0 up" > /tmp/upwak.log 2>&1 &
    UP=$!
    while kill -0 $UP 2>/dev/null; do
        devmem 0x1100c064 32 0; sleep 0.0003; devmem 0x1100c064 32 1
        sleep 0.08
    done
    cat /tmp/upwak.log | tail -2
    echo "--- hci state ---"
    timeout 30 nix-shell -p bluez --run "hciconfig hci0" 2>&1 | head -6
    echo "=== dmesg tail ==="
    dmesg | grep -E "HCI-STP|hci0|Opcode" | tail -15'
}

regs() {
    devssh 'echo "--- BTIF RX DMA + LSR + kthread (5 x 0.5s) ---"
    for i in 1 2 3 4 5; do
        printf "WPT=%s RPT=%s VALID=%s LSR=%s wchan=%s\n" \
          "$(devmem 0x11000aac 32)" "$(devmem 0x11000ab0 32)" \
          "$(devmem 0x11000abc 32)" "$(devmem 0x1100c014 32)" \
          "$(cat /proc/89/wchan 2>/dev/null)"
        sleep 0.5
    done'
}

wak() {
    devssh 'devmem 0x1100c064 32 0; sleep 0.0003; devmem 0x1100c064 32 1
    echo "WAK pulsed (0x1100c064 low 300us high) - watch dmesg for a stale reply flush"'
}

init1() {
    N="${1:-1}"
    devssh "echo $N > /sys/module/hci_stp/parameters/init_script && echo set init_script=$N"
    up
}

case "${1:-}" in
    up)     up ;;
    up-wak) up_wak ;;
    regs)   regs ;;
    wak)    wak ;;
    init1)  shift; init1 "${1:-1}" ;;
    *) echo "usage: $0 up|up-wak|regs|wak|init1 [N]" >&2; exit 1 ;;
esac
