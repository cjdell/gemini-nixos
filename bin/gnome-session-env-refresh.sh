#!/usr/bin/env bash
# gnome-session-env-refresh.sh — make schema-override changes take effect
# in a RUNNING GNOME session without a logout/reboot.
#
# Why (discovered 2026-09-11): `services.desktopManager.gnome
# .extraGSettingsOverrides` are delivered through NIX_GSETTINGS_OVERRIDES_DIR,
# an `environment.sessionVariables` value captured by the GNOME/systemd
# --user session at LOGIN.  `nixos-rebuild switch` / `bin/deploy.sh`
# rewrite /etc/set-environment but do NOT restart the running user
# session, so the live `systemd --user`, `gsd-*` and `gnome-shell` keep
# the OLD overrides store path.  Symptom (the 2026-09-11 Fn+Q/W/E bug):
# `gsettings get` from a fresh `su -` login shell shows the new value,
# while the running daemon still reads the old one — and restarting
# gsd-media-keys alone does NOT help because it inherits the stale env
# from systemd --user.
#
# What it does:
#   1. reads the expected NIX_GSETTINGS_OVERRIDES_DIR from
#      /etc/set-environment on the device;
#   2. pushes it into the user's `systemd --user` manager
#      (`systemctl --user set-environment`);
#   3. restarts the gsd daemons via their .target units (the services are
#      RefuseManualStart=yes) so they re-read the schema defaults —
#      MediaKeys by default (volume + transport keys), or every
#      org.gnome.SettingsDaemon.* target with --all.
#
# Limits: values read by gnome-shell itself (e.g. the
# org.gnome.shell.keybindings screen-brightness bindings) are cached in
# the running shell and still need a re-login.  A logout/reboot also
# picks /etc/set-environment up permanently, which is the fully clean
# path; this script is the no-reboot workaround.
#
# Usage (from the repo root):
#   bash bin/gnome-session-env-refresh.sh [--all] [USER]
#     USER defaults to cjdell (the device desktop user).
set -euo pipefail
cd "$(dirname "$0")/.."

ALL=0
USER_NAME=""
for a in "$@"; do
  case "$a" in
    --all) ALL=1 ;;
    -h|--help) sed -n '2,40p' "$0"; exit 0 ;;
    -*) echo "!! unknown option: $a" >&2; exit 2 ;;
    *) USER_NAME="$a" ;;
  esac
done
[ -n "$USER_NAME" ] || USER_NAME="${GEMINI_DESKTOP_USER:-cjdell}"

TARGETS="MediaKeys"
if [ "$ALL" = 1 ]; then
  TARGETS="A11ySettings Color Datetime Housekeeping Keyboard MediaKeys Power PrintNotifications Rfkill Sharing Smartcard Sound UsbProtection Wwan"
fi

REMOTE=$(cat <<EOS
set -eu
U="$USER_NAME"
UID_N="\$(id -u "\$U" 2>/dev/null)" || { echo "!! no such user: \$U" >&2; exit 1; }
RD="/run/user/\$UID_N"

NEW="\$(grep -o 'NIX_GSETTINGS_OVERRIDES_DIR="[^"]*"' /etc/set-environment | head -1 | cut -d'"' -f2)"
if [ -z "\$NEW" ]; then
  echo "!! NIX_GSETTINGS_OVERRIDES_DIR not found in /etc/set-environment" >&2
  exit 1
fi
CUR="\$(su - "\$U" -c "export XDG_RUNTIME_DIR=\$RD; systemctl --user show-environment" 2>/dev/null | sed -n 's/^NIX_GSETTINGS_OVERRIDES_DIR=//p' || true)"

echo "expected (system): \$NEW"
echo "running  (session): \${CUR:-<unset>}"

su - "\$U" -c "export XDG_RUNTIME_DIR=\$RD; systemctl --user set-environment NIX_GSETTINGS_OVERRIDES_DIR=\$NEW"
echo "session env updated"

for t in $TARGETS; do
  if su - "\$U" -c "export XDG_RUNTIME_DIR=\$RD; systemctl --user restart org.gnome.SettingsDaemon.\$t.target" 2>/dev/null; then
    echo "  restarted org.gnome.SettingsDaemon.\$t.target"
  else
    echo "  (skip) org.gnome.SettingsDaemon.\$t.target"
  fi
done

NEW_RUN="\$(su - "\$U" -c "export XDG_RUNTIME_DIR=\$RD; systemctl --user show-environment" 2>/dev/null | sed -n 's/^NIX_GSETTINGS_OVERRIDES_DIR=//p' || true)"
echo "now running: \${NEW_RUN:-<unset>}"
[ "\$NEW_RUN" = "\$NEW" ] && echo "OK — gsd daemons now read the current overrides" || echo "!! mismatch: re-login may be required" >&2
EOS
)

printf '%s\n' "$REMOTE" | bash bin/device-ssh.sh 'bash -s'
