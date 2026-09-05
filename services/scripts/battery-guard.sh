#!/bin/bash
# battery-guard.sh — Gemini PDA battery safety daemon
#
# Motivation (experiments on this device):
#   1. Keep the battery at a safe level: WARN when VBAT is low, and do an
#      orderly poweroff before the Li-ion reaches a dangerous depth of
#      discharge (hard brown-out / preloader-only brick state).
#   2. Verify the device is ACTUALLY charging when USB is connected:
#      alert when VBUS is present but the BQ25896 is not charging (the
#      B-19/B-22 failure mode, where the OTG boost puts the charger IC
#      into source mode so an external charger cannot sink).
#
# Data source: /sys/class/power_supply/bq25890-charger-* (mainline
# bq25890_charger.c driving the TI BQ25896 on i2c0 @ 0x6b — see
# docs/hardware.md "Battery & charging"). Property semantics:
#   status        Charging | Discharging | Full | Not charging
#   online        1 when VBUS is present
#   voltage_now   VBAT in uV (chip ADC: 2.304 V + N * 20 mV). Under load
#                 this sags below OCV; thresholds below account for that.
#   current_now   charge-path current in uA. NEGATIVE while charging in
#                 principle — BUT in the mainline driver it stays 0 while
#                 online (the driver only re-triggers the ADC conversion
#                 when !online/hiz). Unreliable while charging; use
#                 charge_type (from the chip's chrg_status register) as
#                 the primary charge signal.
#   temp          degC * 10, rough (TS pin % -> temperature table).
#
# Thresholds (env-overridable — tune after first on-hardware readings):
#   BATTERY_GUARD_POLL_S=10          poll period
#   BATTERY_GUARD_WARN_LOW_MV=3650   "low battery" on battery power
#   BATTERY_GUARD_CRIT_MV=3500       orderly poweroff on battery power
#                                     (vendor hard power-off is 3400 mV —
#                                     3.18 mt_battery_meter.c:1141; 100 mV
#                                     of headroom so the shutdown completes)
#   BATTERY_GUARD_ALERT_COOLDOWN_S=300  min spacing between repeat alerts
#
# Outputs:
#   - journal:   logger -t battery-guard (journalctl -t battery-guard)
#   - state:     /run/battery-guard/state  (key=value for harnesses)
#   - history:   /var/log/battery-history.csv (one row per poll; rotated
#                 at 5 MiB to battery-history.csv.1)
#
# Safety note: a VBAT read below 2.5 V is treated as a READ ERROR, never
# as "critical" — the ADC floor is 2.304 V, so sub-2.5 V means the read
# failed (I2C glitch) and must not trigger a poweroff. Poweroff requires
# two consecutive CRIT samples.

set -u

PSY_GLOB='/sys/class/power_supply/bq25890-charger-*'
RUNDIR=/run/battery-guard
STATE=$RUNDIR/state
HIST=/var/log/battery-history.csv
POLL_S=${BATTERY_GUARD_POLL_S:-10}
WARN_LOW_MV=${BATTERY_GUARD_WARN_LOW_MV:-3650}
CRIT_MV=${BATTERY_GUARD_CRIT_MV:-3500}
ALERT_COOLDOWN_S=${BATTERY_GUARD_ALERT_COOLDOWN_S:-300}
STUCK_CHARGE_MIN=${BATTERY_GUARD_STUCK_CHARGE_MIN:-15}

mkdir -p "$RUNDIR"
if [ ! -s "$HIST" ]; then
    echo 'ts,online,status,charge_type,vbat_mv,ibat_uA,temp_10c,guard' > "$HIST"
fi

last_alert_ts=0
stuck_since=0
crit_strikes=0
prev_guard=""

now() { date '+%s'; }
stamp() { date '+%F %T'; }
log() { echo "[$(stamp)] $*" | logger -t battery-guard -p daemon.info; }

# alert LEVEL MSG — rate-limited (ALERT_COOLDOWN_S)
alert() {
    local t
    t=$(now)
    if [ "$t" -ge $((last_alert_ts + ALERT_COOLDOWN_S)) ]; then
        log "ALERT $1: $2"
        last_alert_ts=$t
    fi
}

read_psy() {
    local d
    for d in $PSY_GLOB; do
        if [ -e "$d/status" ]; then PSY=$d; return 0; fi
    done
    return 1
}

write_state() {
    cat > "$STATE" <<EOF
ts=$(now)
supply=${PSY##*/}
online=${ONLINE:-?}
status=${STATUS:-?}
charge_type=${CHGT:-?}
vbat_mv=${VBAT_MV:-?}
ibat_uA=${IBAT:-?}
temp_10c=${TEMP:-?}
guard=${GUARD:-?}
EOF
}

rotate_hist() {
    local sz
    sz=$(stat -c %s "$HIST" 2>/dev/null || echo 0)
    if [ "$sz" -gt 5242880 ]; then
        mv "$HIST" "$HIST.1"
        echo 'ts,online,status,vbat_mv,ibat_uA,temp_10c,guard' > "$HIST"
    fi
}

log "battery-guard started (poll=${POLL_S}s warn<${WARN_LOW_MV}mV crit<${CRIT_MV}mV)"

while :; do
    t=$(now)
    GUARD=OK
    ONLINE=?; STATUS=?; VBAT_MV=?; IBAT=?; TEMP=?

    if ! read_psy; then
        GUARD=NOSUPPLY
        alert ERROR "no bq25890-charger power supply under /sys/class/power_supply — charger driver did not probe (check dmesg)"
    else
        ONLINE=$(cat "$PSY/online" 2>/dev/null || echo "?")
        STATUS=$(cat "$PSY/status" 2>/dev/null || echo "Unknown")
        CHGT=$(cat "$PSY/charge_type" 2>/dev/null || echo "?")
        VBAT=$(cat "$PSY/voltage_now" 2>/dev/null || echo 0)
        IBAT=$(cat "$PSY/current_now" 2>/dev/null || echo 0)
        TEMP=$(cat "$PSY/temp" 2>/dev/null || echo 0)
        # validate numerics; fall back to ? on read errors
        case "$VBAT" in (*[!0-9]*|'') GUARD=READERR ;;
            (*) VBAT_MV=$((VBAT / 1000)) ;;
        esac
        case "$IBAT" in (*[!0-9-]*|'') IBAT=? ;; esac
        case "$TEMP" in (*[!0-9]*|'') TEMP=? ;; esac

        if [ "$ONLINE" = 1 ]; then
            # USB present — the whole point: is it charging?
            case "$STATUS" in
                Full)
                    : # holding at full charge: fine
                    ;;
                Charging)
                    # stuck-charge detection: status says Charging but the
                    # chip's charge-status register says NOT charging
                    # (charge_type=NONE) for a long time -> sense fault.
                    # (current_now is NOT usable here: the mainline driver
                    # reads it 0 while online — see header.)
                    if [ "$CHGT" = "None" ]; then
                        [ "$stuck_since" -eq 0 ] && stuck_since=$t
                        if [ $((t - stuck_since)) -gt $((STUCK_CHARGE_MIN * 60)) ]; then
                            alert WARN "status=Charging but charge_type=None for ${STUCK_CHARGE_MIN} min — sense/charger fault?"
                        fi
                    else
                        stuck_since=0
                    fi
                    ;;
                *)
                    GUARD=NOTCHARGING
                    alert WARN "USB present (online=1) but status='${STATUS}' — NOT charging (OTG/boost mode? wrong port/cable? see B-19/B-22)"
                    ;;
            esac
        else
            stuck_since=0
            case "$VBAT_MV" in
                ?)
                    GUARD=READERR
                    alert ERROR "VBAT read failed — cannot assess battery level"
                    ;;
                *)
                    if [ "$VBAT_MV" -lt 2500 ]; then
                        # below ADC floor: read error, NOT a valid reading
                        GUARD=READERR
                        alert ERROR "VBAT ${VBAT_MV} mV below ADC floor — read error, ignoring"
                    elif [ "$VBAT_MV" -lt "$CRIT_MV" ]; then
                        # two consecutive CRIT samples before pulling the plug
                        crit_strikes=$((crit_strikes + 1))
                        if [ "$crit_strikes" -ge 2 ]; then
                            GUARD=CRITICAL
                            log "CRITICAL: VBAT ${VBAT_MV} mV < ${CRIT_MV} mV with no USB — powering off in 10 s (vendor hard-off is 3400 mV)"
                            write_state
                            sleep 10
                            exec systemctl poweroff
                        else
                            GUARD=CRITICAL
                            alert CRIT "VBAT ${VBAT_MV} mV < ${CRIT_MV} mV with no USB — will power off on next sample if still critical (connect USB!)"
                        fi
                    else
                        crit_strikes=0
                        if [ "$VBAT_MV" -lt "$WARN_LOW_MV" ]; then
                            GUARD=LOW
                            alert WARN "low battery on battery power: VBAT ${VBAT_MV} mV (< ${WARN_LOW_MV} mV) — connect USB"
                        fi
                    fi
                    ;;
            esac
        fi

        # temperature sanity (rough TS-pin reading)
        if [ "$TEMP" != ? ] && [ "$TEMP" -gt 450 ]; then
            alert WARN "battery TS pin ~$((TEMP / 10)) C — hot; charging may be throttled (JEITA)"
        fi
    fi

    if [ "$GUARD" != "$prev_guard" ]; then
        log "state: ${prev_guard:-<init>} -> ${GUARD} (online=${ONLINE} status=${STATUS} vbat=${VBAT_MV}mV)"
        prev_guard=$GUARD
    fi
    write_state
    echo "$(date '+%F %T'),${ONLINE},${STATUS},${CHGT:-?},${VBAT_MV},${IBAT},${TEMP},${GUARD}" >> "$HIST"
    rotate_hist
    sleep "$POLL_S"
done
