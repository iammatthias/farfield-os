# farfield os stack

Single-host docker-compose for the network ingress surface.

```
┌────────────── tailscale (network namespace) ──────────────┐
│                                                           │
│  caddy                     cloudflared                    │
│  :80, :443, :8080          tunnel connector               │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

The containers share the `tailscale` container's network namespace
(`network_mode: service:tailscale`). They reach each other on `localhost`,
and the outside world reaches them via the tailnet IP / hostname tailscale
hands out — or, for opt-in public sites, via the Cloudflare tunnel.

## Layout

```
/srv/stack/
├── docker-compose.yml
├── Caddyfile                # bind-mounted into caddy:/etc/caddy/Caddyfile
│                            # LIVE copy accumulates add-site blocks — the
│                            # repo version is boilerplate only
├── .env                     # secrets — copy from .env.example, chmod 600
├── caddy/Dockerfile         # custom image: + caddy-dns/cloudflare module
├── homepage/                # static "online" page (default :80 + apex)
├── preview-https/           # gated :443 wildcard site — only imported
│                            # once CF_API_TOKEN is set (no ACME retries
│                            # on unconfigured installs)
├── preview-handles/         # one .caddy fragment per preview site
└── data/                    # bind-mounted state (persists across `compose down`)
    ├── tailscale/           # tailscale identity
    └── caddy/               # caddy data + config (certs)
```

## Lifecycle

```
docker compose up -d --build      # bring up, build caddy image if needed
docker compose ps                 # what's running
docker compose logs -f tailscale  # follow one service
docker compose pull               # newer base images
docker compose down               # stop everything
```

`ff-stack.service` (a systemd system unit, installed by `setup.sh`) does
`up -d` on boot.

Never restart the `tailscale` container on its own — it owns the network
namespace the other services join, and recreating it alone orphans them.
Always cycle the whole stack with `docker compose up -d`.

## First-boot interactive

One thing has to happen interactively, once:

**Tailscale.** Either fill `TS_AUTHKEY=` in `.env` and let it auto-auth on
first boot, or:
```
docker compose exec tailscale tailscale up
```

`ff-bootstrap` walks this (plus Claude Code login on the host).

Until tailscale is logged in, its healthcheck stays unhealthy and
caddy/cloudflared deliberately wait — they'd be joining a netns with no
tailnet. They start on their own once auth completes.

## Adding a website

Edit `Caddyfile`, then reload caddy without restarting:

```
docker compose exec caddy caddy reload
```

The `add-site myapp 3000` zsh helper does this for you.

## Two listeners: private vs public

Caddy listens on separate ports inside the tailscale netns:

| Port  | Listener  | Reached by             | Helper                |
|-------|-----------|------------------------|-----------------------|
| 80    | private   | tailnet                | `add-site`            |
| 443   | private   | tailnet (LE wildcard)  | `add-preview-site`    |
| 8080  | public    | cloudflared tunnel     | `add-public-site`     |

The split is intentional — but it is configured, not enforced. Nothing
stops cloudflared from reaching `localhost:80` or `:443`; it only doesn't
because every route in the tunnel's dashboard config points at
`http://localhost:8080`. The failure mode to respect: one mistyped
dashboard route to `localhost:80` publishes every private vhost through
the tunnel. Double-check the service URL whenever you touch the tunnel's
Public Hostnames.

## Public sites via Cloudflare Tunnel

1. **Create a tunnel.** Cloudflare Zero Trust dashboard → Networks →
   Tunnels → Create a tunnel. Save the connector token.
2. **Configure routes.** In that tunnel's "Public Hostnames" tab, add
   each public hostname (e.g. `myapp.example.com`) and route each to
   `http://localhost:8080`. They all share the same single route on
   the tunnel side — caddy distinguishes them by Host header.
3. **Set the token AND enable the profile.** In `/srv/stack/.env`:
   ```
   CLOUDFLARED_TOKEN=...
   COMPOSE_PROFILES=cloudflared
   ```
   The connector service is profile-gated so token-less installs don't
   crash-loop it — the token alone does nothing without the profile.
4. **Bring the stack up**: `cd /srv/stack && docker compose up -d`.
   With the profile in `.env`, every future `up -d` (including the boot
   unit) keeps the connector running.
5. **Publish a site.** From a shell on the box:
   ```
   add-public-site myapp.example.com 3000
   ```
   That writes a vhost block to the Caddyfile and reloads caddy. The
   site is now live at `https://myapp.example.com`.

One tunnel covers every site you publish — `add-public-site` per
hostname.
