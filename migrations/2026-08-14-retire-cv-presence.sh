#!/bin/bash
# Retire the CV-based presence subsystem: the poll daemon
# (ff-kiosk-presence), the UDP wake listener (ff-kiosk-wake-listener),
# their sway respawn loops, and the 8666/udp firewall rule. The CV
# pipeline it depended on is gone; ff-display stays as a plain manual
# power toggle and a tap still wakes a dark panel (ff-board).
#
# Runs as root via ff-migrate. Guarded: fresh installs no-op.
set -euo pipefail

[ -x /usr/local/bin/ff-kiosk-presence ] || {
    echo "retire-cv-presence: nothing to remove"
    exit 0
}

REPO=$(cd "$(dirname "$0")/.." && pwd)
REAL_USER=${FF_REAL_USER:-}

echo "retire-cv-presence: removing the presence daemon + wake listener"

# Kill the sway respawn loops and their children (the -f pattern matches
# the wrapper shells too, so nothing respawns).
pkill -f ff-kiosk-presence 2>/dev/null || true
pkill -f ff-kiosk-wake-listener 2>/dev/null || true
pkill -f "ncat -ulk 8666" 2>/dev/null || true

rm -f /usr/local/bin/ff-kiosk-presence /usr/local/bin/ff-kiosk-wake-listener

# The wake-push port is no longer listened on.
ufw delete allow 8666/udp >/dev/null 2>&1 || true

# Refresh the sway config (exec loops removed) so the next kiosk start
# doesn't relaunch the daemons.
if [ -n "$REAL_USER" ] && [ -d "/home/$REAL_USER/.config/sway" ]; then
    install -m 644 -o "$REAL_USER" -g "$REAL_USER" \
        "$REPO/configs/sway-config" "/home/$REAL_USER/.config/sway/config"
fi

# Stale runtime plumbing (mode/override/push/status were daemon files;
# state stays — ff-display and the tap-wake still share it).
if [ -n "$REAL_USER" ]; then
    uid=$(id -u "$REAL_USER" 2>/dev/null || true)
    [ -n "$uid" ] && rm -f "/run/user/$uid/ff-display/"{mode,override,push,status,listener.lock,lock}
fi

echo "retire-cv-presence: done"
