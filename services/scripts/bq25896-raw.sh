#!/bin/sh
# bq25896-raw.sh — read the BQ25896 charger's raw ADC/status registers,
# bypassing the kernel driver's stale latches.
#
# Why: the mainline bq25890 driver only re-triggers the chip's ADC
# conversion when !online/hiz (bq25890_update_state), so while charging,
# sysfs current_now stays 0 even though the chip is regulating. This
# script triggers a fresh conversion (REG02 bit 7, auto-clears on
# completion) and reads the results directly over i2c-0 @ 0x6b.
#
# Register map (driver bq25890_charger.c, BQ25896 datasheet):
#   REG00 bits 5-0  IINLIM  = 100 + N*50 mA
#   REG0B bits 7-5  VBUS_STAT, bits 4-3 CHG_STAT (0=none 1=pre 2=fast 3=done)
#   REG0E bits 6-0  BATV    = 2304 + N*20 mV
#   REG0F bits 6-0  SYSV    = 2304 + N*20 mV
#   REG11 bits 6-0  VBUSV   = 2600 + N*100 mV
#   REG12 bits 6-0  ICHGR   = N*50 mA   (measured charge current)
#   REG13 bit 7     VDPM_STAT, bit 6 IDPM_STAT
#
# Usage: bq25896-raw.sh [samples interval_s]   (default: 1 sample)

BUS=0
ADDR=0x6b

conv_and_read() {
    # -f: the kernel bq25890 driver claims 0x6b, so plain -y gets EBUSY;
    # -f uses raw I2C_RDWR (safe: reads/one register write, bus is
    # serialized by the controller)
    r2=$(i2cget -f -y $BUS $ADDR 0x02) || { echo "i2c read failed"; return 1; }
    i2cset -f -y $BUS $ADDR 0x02 $(( r2 | 0x80 )) || return 1
    i=0
    while [ $i -lt 40 ]; do
        sleep 0.05
        r2=$(i2cget -f -y $BUS $ADDR 0x02) 2>/dev/null || continue
        [ $(( r2 & 0x80 )) -eq 0 ] && break
        i=$((i+1))
    done
    r0=$(i2cget -f -y $BUS $ADDR 0x00)
    rb=$(i2cget -f -y $BUS $ADDR 0x0B)
    re=$(i2cget -f -y $BUS $ADDR 0x0E)
    rf=$(i2cget -f -y $BUS $ADDR 0x0F)
    r11=$(i2cget -f -y $BUS $ADDR 0x11)
    r12=$(i2cget -f -y $BUS $ADDR 0x12)
    r13=$(i2cget -f -y $BUS $ADDR 0x13)
    chg=$(( (rb >> 3) & 3 ))
    case $chg in
        0) chgt="none";; 1) chgt="pre";; 2) chgt="fast";; 3) chgt="done";;
    esac
    printf '%s chg=%s vbus_stat=%d vbat=%dmV vsys=%dmV ichgr=%dmA vbus=%dmV iinlim=%dmA vdpm=%d idpm=%d\n' \
        "$(date '+%H:%M:%S')" "$chgt" $(( (rb >> 5) & 7 )) \
        $(( 2304 + (re & 0x7f) * 20 )) \
        $(( 2304 + (rf & 0x7f) * 20 )) \
        $(( (r12 & 0x7f) * 50 )) \
        $(( 2600 + (r11 & 0x7f) * 100 )) \
        $(( 100 + (r0 & 0x3f) * 50 )) \
        $(( (r13 >> 7) & 1 )) $(( (r13 >> 6) & 1 ))
}

n=${1:-1}
int=${2:-0}
i=0
rc=0
while [ $i -lt $n ]; do
    conv_and_read || { rc=1; break; }
    i=$((i+1))
    [ $i -lt $n ] && sleep "$int"
done
exit $rc
