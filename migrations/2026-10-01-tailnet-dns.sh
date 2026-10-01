#!/bin/bash
# Give an existing box split DNS for the tailnet's names (*.ts.net) — see
# bin/ff-tailnet-dns. setup.sh installs this on a fresh box; a box set up
# before it could not resolve its own tailnet name, so anything on it that
# followed a ts.net URL failed while every other device reached it.
#
# Runs as root via ff-migrate.
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
DROPIN=/etc/systemd/system/tailscaled.service.d/ff-tailnet-dns.conf

if ! command -v tailscale >/dev/null 2>&1; then
    echo "tailnet-dns: host tailscale is not installed — nothing to configure"
    exit 0
fi
if cmp -s "$REPO/configs/tailscaled-tailnet-dns.conf" "$DROPIN" &&
    cmp -s "$REPO/bin/ff-tailnet-dns" /usr/local/bin/ff-tailnet-dns; then
    echo "tailnet-dns: already installed"
    exit 0
fi

echo "tailnet-dns: installing split DNS for ts.net on tailscale0"
install -m 755 "$REPO/bin/ff-tailnet-dns" /usr/local/bin/ff-tailnet-dns
install -d "$(dirname "$DROPIN")"
install -m 644 "$REPO/configs/tailscaled-tailnet-dns.conf" "$DROPIN"
systemctl daemon-reload
# Apply now rather than restarting tailscaled: a restart drops every tailnet
# session into the box, this one's SSH possibly included.
/usr/local/bin/ff-tailnet-dns

# Prove it: the box's own tailnet name must resolve.
self=$(tailscale status --json 2>/dev/null | jq -r ".Self.DNSName // empty" | sed "s/\.$//")
if [ -n "$self" ] && ! getent hosts "$self" >/dev/null; then
    echo "tailnet-dns: $self still does not resolve" >&2
    exit 1
fi
echo "tailnet-dns: done${self:+ ($self resolves)}"
