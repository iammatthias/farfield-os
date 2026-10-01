#!/bin/bash
# Let an existing box resolve the tailnet's names (*.ts.net), its own included.
#
# Boxes joined with --accept-dns=false so a server would keep its own
# resolv.conf. Under systemd-resolved that flag never protected resolv.conf —
# Tailscale only ever sets its resolver on the tailscale0 link, for the
# tailnet's domains — so all it did was leave the box unable to resolve
# *.ts.net while every other device could. ff-bootstrap now accepts DNS
# where resolved runs it; this brings existing boxes along, and removes the
# ExecStartPost hook a short-lived first attempt installed (tailscaled resets
# its link's DNS after start, so the hook lost every time).
#
# Runs as root via ff-migrate.
set -euo pipefail

# the short-lived hook, if this box got it
if [ -e /etc/systemd/system/tailscaled.service.d/ff-tailnet-dns.conf ]; then
    rm -f /etc/systemd/system/tailscaled.service.d/ff-tailnet-dns.conf
    rmdir --ignore-fail-on-non-empty /etc/systemd/system/tailscaled.service.d
    systemctl daemon-reload
fi
rm -f /usr/local/bin/ff-tailnet-dns

if ! command -v tailscale >/dev/null 2>&1 || ! tailscale ip -4 >/dev/null 2>&1; then
    echo "tailnet-accept-dns: not on a tailnet — nothing to do"
    exit 0
fi
# Only under systemd-resolved: anywhere else, accepting DNS rewrites resolv.conf.
if ! systemctl is-active --quiet systemd-resolved || ! grep -q '^nameserver 127\.0\.0\.53' /etc/resolv.conf; then
    echo "tailnet-accept-dns: resolv.conf is not systemd-resolved's — leaving DNS alone"
    exit 0
fi

if tailscale debug prefs 2>/dev/null | jq -e '.CorpDNS == true' >/dev/null; then
    echo "tailnet-accept-dns: already accepting tailnet DNS"
else
    before=$(md5sum /etc/resolv.conf)
    tailscale set --accept-dns=true
    sleep 3
    if [ "$(md5sum /etc/resolv.conf)" != "$before" ]; then
        echo "tailnet-accept-dns: resolv.conf changed — reverting" >&2
        tailscale set --accept-dns=false
        exit 1
    fi
fi

self=$(tailscale status --json 2>/dev/null | jq -r '.Self.DNSName // empty' | sed 's/\.$//')
if [ -n "$self" ] && ! getent hosts "$self" >/dev/null; then
    echo "tailnet-accept-dns: $self still does not resolve" >&2
    exit 1
fi
echo "tailnet-accept-dns: done${self:+ ($self resolves)}"
