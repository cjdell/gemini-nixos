#!/bin/sh
# device-repo.sh — seed + sync the gemini-nixos git repo between the
# HOST (golden copy) and the device clone at /root/gemini-nixos, over
# the g_ether link, as a git BUNDLE (no github round-trip needed; works
# offline; reuses the existing host<->device ssh trust via
# bin/device-ssh.sh). The repo also carries a github origin — both
# hosts may use it, but this script is the offline bench path.
#
# Model [2026-09-08]: the device clone is for ON-THE-GO iteration:
# edit -> `git commit` on the device -> device-rebuild.sh build/switch.
# Sync is explicit and directional so nothing is ever silently lost:
#   seed  first clone onto the device (creates /root/gemini-nixos;
#         needs git on the device = in the system closure since gen29)
#   push  host -> device: device main is reset to host main; refused if
#         the device repo is dirty OR has commits the host lacks (use
#         pull first — the device is authoritative for its own edits)
#   pull  device -> host: fetch the device's commits into the host repo
#         as refs/remotes/gemini-device/main and fast-forward main;
#         if the host moved ahead too, merge manually
#
# Usage (from the host repo root):
#   bash bin/device-repo.sh seed|push|pull
set -eu

repo=/home/cjdell/Projects/gemini-nixos
dev=10.15.19.82
dev_repo=/root/gemini-nixos
bundle=/var/tmp/gemini-repo.bundle

device_ssh() { bash "$repo/bin/device-ssh.sh" "$1"; }

seed() {
    echo "device-repo: seeding $dev_repo from host git..."
    git -C "$repo" bundle create - --all | device_ssh "cat > $bundle"
    device_ssh "
        set -e
        [ -e $dev_repo ] && { echo '!! $dev_repo already exists — remove it first if you really want to reseed' >&2; exit 2; }
        git clone -q $bundle $dev_repo
        rm -f $bundle
        echo \"device-repo: seeded at \$(git -C $dev_repo rev-parse --short HEAD)\"
    "
}

push() {
    echo "device-repo: push host -> device..."
    git -C "$repo" bundle create - main | device_ssh "cat > $bundle"
    device_ssh "
        set -e
        cd $dev_repo || { echo '!! no device repo — run seed first' >&2; exit 2; }
        [ -z \"\$(git status --porcelain)\" ] || { echo '!! device repo dirty — commit/stash first (rule 0)' >&2; exit 3; }
        # bundle-path remotes need an explicit refspec (git won't fetch
        # their implicit HEAD — verified 2026-09-08)
        git fetch -q $bundle main:refs/remotes/host/main
        git merge-base --is-ancestor HEAD refs/remotes/host/main \
            || { echo '!! device main has commits the host lacks — pull them back first (device-repo.sh pull)' >&2; exit 4; }
        git reset --hard -q refs/remotes/host/main
        rm -f $bundle
        echo \"device-repo: device main -> \$(git rev-parse --short HEAD)\"
    "
}

pull() {
    echo "device-repo: pull device -> host (fast-forward)..."
    device_ssh "
        cd $dev_repo || { echo '!! no device repo on $dev' >&2; exit 2; }
        [ -z \"\$(git status --porcelain)\" ] || { echo '!! device repo dirty — commit/stash first' >&2; exit 3; }
        git bundle create - main 2>/dev/null
    " > /var/tmp/gemini-device.bundle
    git -C "$repo" fetch -q /var/tmp/gemini-device.bundle main:refs/remotes/gemini-device/main
    rm -f /var/tmp/gemini-device.bundle
    if git -C "$repo" merge --ff-only refs/remotes/gemini-device/main >/dev/null 2>&1; then
        echo "device-repo: host main -> $(git -C "$repo" rev-parse --short HEAD) (device commits merged)"
    else
        echo "!! host main has moved ahead of the device — merge refs/remotes/gemini-device/main manually" >&2
        exit 1
    fi
}

cmd=${1:-usage}
case "$cmd" in
seed) seed ;;
push) push ;;
pull) pull ;;
*)
    echo "usage: device-repo.sh seed|push|pull" >&2
    exit 2
    ;;
esac
