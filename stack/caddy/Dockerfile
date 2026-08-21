# farfield os — caddy image with the Cloudflare DNS module compiled in.
#
# Stock caddy:latest can't do ACME DNS-01, which is what we need for a
# wildcard cert on $PREVIEW_APEX (LetsEncrypt won't issue wildcards via
# HTTP-01, only DNS-01).
#
# xcaddy is Caddy's standard plugin-build harness; it rebuilds the caddy
# binary with the modules listed below. CF_API_TOKEN is read at runtime
# from the environment (set in /srv/stack/.env).

# Major-pinned to caddy 2.x — a surprise 3.x jump shouldn't arrive via
# a routine `docker compose build`. Minor/patch updates ride the tag.
#
# The DNS module is version-pinned too: unpinned, `xcaddy build` resolves
# to whatever the module's default branch holds at build time, so the
# binary serving ingress would depend on when it was last built rather
# than on anything in this repo. Bump deliberately.

FROM caddy:2-builder AS builder

ARG CADDY_DNS_CLOUDFLARE=v0.2.4

RUN xcaddy build \
    --with github.com/caddy-dns/cloudflare@${CADDY_DNS_CLOUDFLARE}

FROM caddy:2

COPY --from=builder /usr/bin/caddy /usr/bin/caddy
