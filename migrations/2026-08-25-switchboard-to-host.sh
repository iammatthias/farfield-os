#!/bin/bash
# Move switchboard from the compose stack to a host systemd unit.
#
# It has to hand messages to an agent, which means exec'ing a binary and
# reaching the user's agent config and credentials. A distroless container with
# one data volume cannot, so it stops being a container. This cannot live in
# setup.sh because it has to UNDO a running container a fresh install would
# never have created.
#
# Runs as root via ff-migrate, once per box.
set -euo pipefail

REAL_USER=${FF_REAL_USER:?ff-migrate must provide FF_REAL_USER}
REAL_HOME="/home/$REAL_USER"
REPO=$(cd "$(dirname "$0")/.." && pwd)
PROJECT="$REAL_HOME/projects/farfield"
DATA="$PROJECT/data"
UNIT=ff-switchboard.service

# Guard: nothing to converge if the unit is already running and the container
# is gone. A fresh box reaches that state through setup.sh, not through here.
container=$(docker ps -a --filter 'name=farfield-switchboard' --format '{{.Names}}' 2>/dev/null | head -1 || true)
if [ -z "$container" ] && systemctl is-enabled --quiet "$UNIT" 2>/dev/null; then
    echo "switchboard-to-host: already converged"
    exit 0
fi

[ -d "$PROJECT/apps/switchboard" ] || {
    echo "switchboard-to-host: no switchboard source at $PROJECT — pull farfield first" >&2
    exit 1
}

echo "switchboard-to-host: converging"

# ---------------------------------------------------------------------------
# 1. Build first. Nothing is torn down until there is something to replace it.
# ---------------------------------------------------------------------------
echo "  building"
sudo -u "$REAL_USER" bash -lc "cd '$PROJECT/apps/switchboard' && go build -o '$PROJECT/bin/switchboard' ." || {
    echo "switchboard-to-host: build failed — leaving the container running" >&2
    exit 1
}
install -m 755 "$PROJECT/bin/switchboard" /usr/local/bin/switchboard

# ---------------------------------------------------------------------------
# 2. Host-process overrides. The fleet's .env is shared with the containers and
#    says nothing about how a host process should bind, so the difference lives
#    in its own file rather than being edited into the shared one.
#
#    HOST is the docker0 gateway, not loopback: caddy runs in a container and
#    reaches the host through host.docker.internal, so a service bound to
#    127.0.0.1 is invisible to it while looking perfectly healthy locally.
# ---------------------------------------------------------------------------
bind_ip=$(grep -E '^FARFIELD_BIND_IP=' "$PROJECT/.env" 2>/dev/null | tail -1 | cut -d= -f2- | tr -d '"' || true)
bind_ip=${bind_ip:-172.17.0.1}
install -d -m 755 /etc/farfield
cat >/etc/farfield/switchboard.env <<EOF
# Written by migrations/2026-08-25-switchboard-to-host.sh.
# Only what differs because switchboard is a host process, not a container.
HOST=$bind_ip
SWITCHBOARD_PORT=8802
SWITCHBOARD_DB_PATH=$DATA/switchboard.sqlite
KEYS_DB_PATH=$DATA/keys.sqlite
EOF
chmod 644 /etc/farfield/switchboard.env

# ---------------------------------------------------------------------------
# 3. A shared group over the data directory.
#
#    The containers write as nonroot uid/gid 65532 and own everything in there.
#    SQLite does not only write its database file — it creates -wal and -shm
#    beside it — so the host process needs write access to the DIRECTORY, which
#    a chown of the files alone does not give. Both sides therefore share a
#    group: setgid so new files stay in it, group-writable so either can create.
#
#    The group must be NAMED. systemd rejects a bare gid in SupplementaryGroups
#    with "failed to determine supplementary groups" and the unit never starts.
# ---------------------------------------------------------------------------
getent group 65532 >/dev/null || groupadd -g 65532 farfield-data
chmod 2775 "$DATA" "$DATA/pulse" 2>/dev/null || true
for f in "$DATA"/keys.sqlite* "$DATA"/switchboard.sqlite* "$DATA"/pulse/switchboard.sqlite*; do
    [ -e "$f" ] || continue
    chgrp 65532 "$f" 2>/dev/null || true
    chmod g+w "$f" 2>/dev/null || true
done

# ---------------------------------------------------------------------------
# 4. Stop the container. It cannot be put back by compose — the same commit
#    that adds this migration removes switchboard from docker-compose.yml — so
#    the failure path below stops the unit and says so rather than pretending.
# ---------------------------------------------------------------------------
on_failure() {
    echo "switchboard-to-host: FAILED — the line is DOWN" >&2
    systemctl stop "$UNIT" 2>/dev/null || true
    systemctl disable "$UNIT" 2>/dev/null || true
    echo "  the container cannot be restored: this commit removed switchboard" >&2
    echo "  from docker-compose.yml. To get the line back, either fix forward" >&2
    echo "  (journalctl -u $UNIT, then ff-switchboard deploy) or check out the" >&2
    echo "  previous farfield commit and 'docker compose up -d switchboard'." >&2
}
trap on_failure ERR

if [ -n "$container" ]; then
    echo "  stopping $container"
    docker stop "$container" >/dev/null
    docker rm "$container" >/dev/null
fi

# ---------------------------------------------------------------------------
# 5. The database and its telemetry sidecar were written by the container's
#    nonroot uid (65532). The host process runs as the real user and cannot
#    open them until they change hands.
# ---------------------------------------------------------------------------
for f in "$DATA"/switchboard.sqlite* "$DATA"/pulse/switchboard.sqlite*; do
    [ -e "$f" ] || continue
    chown "$REAL_USER:$REAL_USER" "$f"
    echo "  chown $(basename "$f")"
done

# ---------------------------------------------------------------------------
# 6. Install and start the unit.
# ---------------------------------------------------------------------------
install -m 644 "$REPO/configs/$UNIT" "/etc/systemd/system/$UNIT"
systemctl daemon-reload
systemctl enable --now "$UNIT"

# ---------------------------------------------------------------------------
# 7. Prove it. `active` only means the process started; /status means the
#    database opened and the service is answering.
# ---------------------------------------------------------------------------
for _ in $(seq 1 15); do
    if curl -fsS -m 3 "http://$bind_ip:8802/status" >/dev/null 2>&1; then
        trap - ERR
        echo "switchboard-to-host: converged — $(curl -fsS -m 3 "http://$bind_ip:8802/status")"
        exit 0
    fi
    sleep 1
done

echo "switchboard-to-host: unit started but /status never answered on $bind_ip:8802" >&2
false
