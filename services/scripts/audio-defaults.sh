#!/bin/bash
# audio-defaults.sh — apply the known-good ALSA playback route + output mode
# at boot (run by gemini-audio-defaults.service, after alsa-restore).
#
# The mainline mt6797 machine driver does NOT route DL1->ADDA->DAC->HPL/HPR
# until the route switches AND the HPL/HPR muxes are set (they boot at
# 'Open' = silent — handover-2026-09-06-h recipe):
#     amixer sset 'ADDA_DL_CH1 DL1_CH1' on
#     amixer sset 'ADDA_DL_CH2 DL1_CH2' on
#     amixer sset 'HPL Mux' 'Audio Playback'
#     amixer sset 'HPR Mux' 'Audio Playback'
# DAPM only powers these widgets while a stream runs, so leaving the
# switches on at boot costs no battery.  Volume ('Headphone Volume' = 5)
# is owned by alsa-restore.service (/var/lib/alsa/asound.state) — this
# script does not touch it.  Then the persisted manual output mode
# (speaker/headphone — /etc/gemini/audio-output-mode) is re-applied so
# the boot state matches the user's last choice instead of the pads'
# cold LOW default.
#
# (Ported verbatim from the GeminiPDA project; audio-output lives next to
# this script in the gemini-pda-utils bin dir, substituted at package
# build time.)
set -eu
export LANG=C
export LC_ALL=C

CARD=0
amixer -c "$CARD" sset 'ADDA_DL_CH1 DL1_CH1' on >/dev/null
amixer -c "$CARD" sset 'ADDA_DL_CH2 DL1_CH2' on >/dev/null
amixer -c "$CARD" sset 'HPL Mux' 'Audio Playback' >/dev/null
amixer -c "$CARD" sset 'HPR Mux' 'Audio Playback' >/dev/null

mode="$(cat /etc/gemini/audio-output-mode 2>/dev/null || echo speaker)"
__GEMINI_UTILS__/audio-output "$mode" >/dev/null 2>&1 || true

logger -t audio-defaults "playback route set; output mode: $mode"
