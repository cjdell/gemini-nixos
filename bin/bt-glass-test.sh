#!/usr/bin/env bash
# bt-glass-test.sh — Bluetooth bring-up on-glass test (2026-09-09).
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
#   bash bin/bt-glass-test.sh le-on        btmgmt le on (needs the 7af6ee9c
#                                          local-record fix to report 'le')
#   bash bin/bt-glass-test.sh find [sec]   btmgmt find (mgmt discovery,
#                                          no daemon needed); finds real
#                                          LE devices on glass
#   bash bin/bt-glass-test.sh btd-scan     bluetoothd + bluetoothctl scan
#                                          on a private system dbus under
#                                          systemd-run units (survives ssh)
#
# NOTE: long discovery/scan runs (>1 ssh lifetime) go under
# bin/run-job.sh:  bash bin/run-job.sh start bt-scan -- bash bin/bt-glass-test.sh find 40
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

le_on() {
    devssh 'export PATH=$PATH:/run/current-system/sw/bin
    setsid nix-shell -p bluez --run "btmgmt le on" 2>&1 | head -2
    setsid nix-shell -p bluez --run "btmgmt info" 2>&1 | grep -a "current settings"'
}

find_devices() {
    SECS="${1:-25}"
    devssh "export PATH=\$PATH:/run/current-system/sw/bin
    setsid timeout $((SECS + 20)) nix-shell -p bluez --run \"btmgmt --timeout $SECS find\" 2>&1
    echo '--- transport health ---'
    dmesg | grep -cE 'stp_do_tx_timeout|resync probe|TX retry limit'"
}

btd_scan() {
    # bluetoothd + bluetoothctl on a private system dbus (org.bluez policy
    # is missing from the stock NixOS /etc/dbus-1 static tree). Daemons run
    # under systemd-run so they survive the ssh session; the scan itself
    # must stay inside ONE ssh call (bluetoothctl --timeout).
    devssh 'export PATH=$PATH:/run/current-system/sw/bin
    BLUEZ=$(ls -d /nix/store/*-bluez-5.* | head -1)
    DBUS_BIN=$(nix-shell -p dbus --run "which dbus-daemon" 2>/dev/null)
    BTC=$BLUEZ/bin/bluetoothctl
    systemctl stop gemini-bt-bus.service gemini-btd.service 2>/dev/null; sleep 1
    mkdir -p /run/gemini-bt-dbus
    cat > /tmp/gemini-bt-bus.conf <<EOF
<busconfig><type>system</type>
<listen>unix:path=/run/gemini-bt-dbus/socket</listen><auth>EXTERNAL</auth>
<policy context="default">
<allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/>
<allow send_type="method_call"/><allow send_type="signal"/><allow send_type="method_return"/><allow send_type="error"/>
<allow receive_type="method_call"/><allow receive_type="signal"/><allow receive_type="method_return"/><allow receive_type="error"/>
</policy></busconfig>
EOF
    systemd-run --unit=gemini-bt-bus --collect "$DBUS_BIN" --config-file=/tmp/gemini-bt-bus.conf --nofork >/dev/null 2>&1
    sleep 2
    systemd-run --unit=gemini-btd --collect --setenv=DBUS_SYSTEM_BUS_ADDRESS=unix:path=/run/gemini-bt-dbus/socket $BLUEZ/bin/bluetoothd -n >/dev/null 2>&1
    sleep 3
    systemctl is-active gemini-bt-bus gemini-btd
    DBUS_SYSTEM_BUS_ADDRESS=unix:path=/run/gemini-bt-dbus/socket $BTC --timeout 15 power on 2>&1 | tail -1
    echo "=== bluetoothctl scan on (45 s) ==="
    DBUS_SYSTEM_BUS_ADDRESS=unix:path=/run/gemini-bt-dbus/socket timeout 90 $BTC --timeout 45 scan on 2>&1 | grep -avE "^$" | head -10
    echo "--- transport health ---"
    dmesg | grep -cE "stp_do_tx_timeout|resync probe|TX retry limit"'
}

case "${1:-}" in
    up)     up ;;
    up-wak) up_wak ;;
    regs)   regs ;;
    wak)    wak ;;
    init1)  shift; init1 "${1:-1}" ;;
    le-on)  le_on ;;
    find)   shift; find_devices "${1:-25}" ;;
    btd-scan) btd_scan ;;
    *) echo "usage: $0 up|up-wak|regs|wak|init1 [N]|le-on|find [sec]|btd-scan" >&2; exit 1 ;;
esac
