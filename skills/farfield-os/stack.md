# The stack — /srv/stack

The ingress layer is a docker compose stack, atomically updatable and isolated
from the host substrate.

```
/srv/stack/
  docker-compose.yml    ff-caddy, ff-cloudflared
  Caddyfile             LIVE STATE — see ingress.md
  .env                  CF_API_TOKEN, CLOUDFLARED_TOKEN (chmod 600, never committed)
  data/caddy/           certs and cache
  preview-handles/      per-preview Caddy fragments
```

## Never cycle caddy alone

`ff-cloudflared` runs with `network_mode: service:caddy` — it has no network
namespace of its own. Restarting caddy by itself leaves the connector attached
to a namespace that no longer exists: the container keeps running, reports
nothing wrong, and every public site is dark.

```bash
# right
cd /srv/stack && docker compose up -d      # or: sudo systemctl restart ff-stack

# wrong — orphans the tunnel
docker restart ff-caddy
docker compose restart caddy
```

Reloading Caddy's *config* without restarting the container is fine, and is
what the site helpers already do:

```bash
docker compose -f /srv/stack/docker-compose.yml exec -T caddy \
  caddy reload --config /etc/caddy/Caddyfile
```

## Updating the stack

```bash
cd /srv/stack && git pull && docker compose up -d --build
```

The caddy image is built locally from `stack/caddy/Dockerfile` because it needs
the `caddy-dns/cloudflare` module compiled in for DNS-01. A stock `caddy` image
will fail to get the wildcard cert.

## systemd

`ff-stack.service` brings the stack up at boot. Prefer
`systemctl restart ff-stack` over ad-hoc compose commands so the unit and the
running state agree.

## Checking it

```bash
ff-doctor              # convergence check, read-only, run this first
ff-docker-status       # containers + pm2 processes
ff-services-status     # service + Caddy-site health
docker compose -f /srv/stack/docker-compose.yml ps
```
