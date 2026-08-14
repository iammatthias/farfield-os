#!/bin/bash
# Move the tailnet identity from the retired ff-tailscale container to
# host tailscaled, then bring the stack up with caddy as netns owner.
#
# The container state carries the node key, so the tailnet name and IP
# stay put — no re-auth, no orphan node in the admin console. Guarded on
# the old container state existing: fresh installs (and already-migrated
# boxes) no-op straight through.
#
# Runs as root via ff-migrate.
set -euo pipefail

OLD_STATE=/srv/stack/data/tailscale/tailscaled.state

if [ ! -f "$OLD_STATE" ]; then
    echo "host-tailscale: no container tailscale state — nothing to migrate"
    exit 0
fi

if ! command -v tailscale >/dev/null 2>&1; then
    echo "host-tailscale: host tailscale not installed — run setup.sh first" >&2
    exit 1
fi

echo "host-tailscale: adopting the container's tailnet identity on the host"
cd /srv/stack

# Stack down. --remove-orphans clears ff-tailscale, which the current
# compose no longer defines. Public sites are dark until the final up.
docker compose down --remove-orphans

# Adopt the node state. Host tailscaled may be running with an empty,
# never-authenticated state — stop it before swapping.
systemctl stop tailscaled 2>/dev/null || true
install -d -m 700 /var/lib/tailscale
install -m 600 -o root -g root "$OLD_STATE" /var/lib/tailscale/tailscaled.state
systemctl enable --now tailscaled

# Wait for the daemon to come up with the adopted identity.
for i in $(seq 1 30); do
    tailscale status --peers=false >/dev/null 2>&1 && break
    sleep 1
done
tailscale status --peers=false

# The container prefs had Tailscale SSH on; on the host, plain sshd is
# the front door.
tailscale set --ssh=false

# Stack back up, caddy-owned netns.
docker compose up -d --build --remove-orphans

# Retire the old container state (kept for rollback) + stale env vars.
mv /srv/stack/data/tailscale /srv/stack/data/tailscale.retired
sed -i '/^TS_HOSTNAME=/d;/^TS_AUTHKEY=/d;/^TS_EXTRA_ARGS=/d' /srv/stack/.env 2>/dev/null || true

echo "host-tailscale: done — $(tailscale ip -4 | head -1) is now the host's tailnet IP"
