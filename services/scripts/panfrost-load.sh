#!/bin/sh
# panfrost-load.sh — load the panfrost DRM driver only once the GPU is
# actually powered on (run as gemwl.service ExecStartPre).
#
# WHY (2026-09-07, on glass): panfrost probes at module-load time, and a
# probe that runs too close after gemini-gpu-poweron times out
# ("gpu soft reset timed out", -110) and leaves the module loaded-but-
# failed — /dev/dri never appears and nothing retries. The successful
# probes happened ~1 min after power-on. panfrost is blacklisted at boot
# (services/gemini-pda.nix) so nothing else probes the un-powered mali
# early; this script is the only loader and it rmmod+reprobes until the
# GPU answers (30 tries x 2 s = up to 60 s).
#
# Uses: modprobe/rmmod (kmod), seq/sleep (coreutils) — provide via the
# unit's `path`.
set -u

try=0
while [ "$try" -lt 30 ]; do
    try=$((try + 1))
    if [ -e /dev/dri/renderD128 ]; then
        echo "panfrost-load: render node present (try $try)"
        exit 0
    fi
    modprobe panfrost 2>/dev/null || true
    if [ -e /dev/dri/renderD128 ]; then
        echo "panfrost-load: render node present after modprobe (try $try)"
        exit 0
    fi
    # probe failed — unload so the next iteration re-probes (a loaded-but-
    # failed module makes modprobe a no-op).
    rmmod panfrost 2>/dev/null || true
    sleep 2
done

echo "panfrost-load: /dev/dri/renderD128 never appeared after $((try * 2))s" >&2
exit 1
