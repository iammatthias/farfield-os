#!/bin/bash
# Converge the fixes from the 2026-08-21 audit that live outside the
# files setup.sh rewrites on every run:
#
#   - ff-firewall.service (new): published container ports bypass UFW
#     entirely, so the actual policy has to live in DOCKER-USER.
#   - ff-stack.service: no longer rebuilds the caddy image on every boot.
#   - the LIVE Caddyfile's trusted_proxies: private_ranges let any tailnet
#     or LAN peer forge X-Forwarded-* / CF-Connecting-IP. cloudflared is
#     always loopback, so loopback is the whole trust set.
#
# The live Caddyfile is state, not a repo artifact — it's edited in place
# here, never replaced, and validated before the reload.
#
# Runs as root via ff-migrate.
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
REAL_USER=${FF_REAL_USER:?ff-migrate must provide FF_REAL_USER}

echo "audit-hardening: installing units + helpers"

install -m 755 "$REPO/bin/ff-firewall" /usr/local/bin/ff-firewall
install -m 755 "$REPO/bin/ff-doctor"   /usr/local/bin/ff-doctor
install -m 644 "$REPO/configs/ff-firewall.service" /etc/systemd/system/ff-firewall.service
install -m 644 "$REPO/configs/ff-stack.service"    /etc/systemd/system/ff-stack.service
systemctl daemon-reload

# Apply the ingress rules now, not just at next boot.
systemctl enable --now ff-firewall.service || {
    echo "audit-hardening: ff-firewall failed to start — is dockerd up?" >&2
    exit 1
}

# --- live Caddyfile: narrow trusted_proxies ---------------------------------
CADDYFILE=/srv/stack/Caddyfile
if [ -f "$CADDYFILE" ] && grep -qE '^\s*trusted_proxies static private_ranges\s*$' "$CADDYFILE"; then
    echo "audit-hardening: narrowing trusted_proxies to loopback"
    cp -a "$CADDYFILE" "$CADDYFILE.bak.pre-trusted-proxies"
    sed -i -E 's|^(\s*)trusted_proxies static private_ranges\s*$|\1trusted_proxies static 127.0.0.0/8 ::1|' "$CADDYFILE"

    # Validate against the running container before reloading. On any
    # doubt, put the original back — a Caddyfile that fails to load takes
    # every site down.
    if docker exec ff-caddy caddy validate --config /etc/caddy/Caddyfile >/dev/null 2>&1; then
        docker exec ff-caddy caddy reload --config /etc/caddy/Caddyfile
        echo "audit-hardening: caddy reloaded"
    else
        mv "$CADDYFILE.bak.pre-trusted-proxies" "$CADDYFILE"
        echo "audit-hardening: Caddyfile failed validation — reverted, no reload" >&2
        exit 1
    fi
else
    echo "audit-hardening: trusted_proxies already narrowed (or Caddyfile absent)"
fi

echo "audit-hardening: done — run ff-doctor to confirm"
