#!/bin/bash
# Stop the shared access log from recording response Location headers.
#
# farfield's library now answers an authorized book download with a 302 to a
# presigned R2 URL — a short-lived bearer credential. Caddy's JSON access log
# records every response header, and access.log is mode 644, so without this
# each download would leave a working URL in a world-readable file. Nothing
# (ff-board included) reads resp_headers from the log.
#
# The live Caddyfile is state, not a repo artifact — edited in place, never
# replaced, validated before the reload, and restored on any doubt. A reload
# keeps the container (and the cloudflared that shares its namespace) up.
#
# Runs as root via ff-migrate.
set -euo pipefail

CADDYFILE=/srv/stack/Caddyfile
if [ ! -f "$CADDYFILE" ]; then
    echo "access-log-drop-location: no Caddyfile — nothing to do"
    exit 0
fi
if grep -q 'resp_headers>Location delete' "$CADDYFILE"; then
    echo "access-log-drop-location: already applied"
    exit 0
fi

# Rewrite only the `format json` inside the (ff_access_log) snippet.
cp -a "$CADDYFILE" "$CADDYFILE.bak.pre-drop-location"
awk '
    /^\(ff_access_log\) \{/ { in_snippet = 1 }
    in_snippet && /^[[:space:]]*format json[[:space:]]*$/ {
        match($0, /^[[:space:]]*/); ind = substr($0, 1, RLENGTH)
        print ind "format filter {"
        print ind "    wrap json"
        print ind "    fields {"
        print ind "        resp_headers>Location delete"
        print ind "    }"
        print ind "}"
        done = 1; in_snippet = 0; next
    }
    { print }
    END { if (!done) exit 3 }
' "$CADDYFILE.bak.pre-drop-location" > "$CADDYFILE.new" || {
    rm -f "$CADDYFILE.new"
    echo "access-log-drop-location: ff_access_log snippet not in the expected shape — left alone" >&2
    exit 1
}
cat "$CADDYFILE.new" > "$CADDYFILE"   # keep the inode caddy bind-mounts
rm -f "$CADDYFILE.new"

if docker exec ff-caddy caddy validate --config /etc/caddy/Caddyfile >/dev/null 2>&1; then
    docker exec ff-caddy caddy reload --config /etc/caddy/Caddyfile
    echo "access-log-drop-location: caddy reloaded"
else
    cat "$CADDYFILE.bak.pre-drop-location" > "$CADDYFILE"
    echo "access-log-drop-location: Caddyfile failed validation — reverted, no reload" >&2
    exit 1
fi
