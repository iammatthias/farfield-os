# farfield os stack

Single-host docker-compose for the network ingress surface.

```
┌─────────────── caddy (network namespace) ─────────────────┐
│                                                           │
│  caddy                     cloudflared                    │
│  :80, :443 published       tunnel connector               │
│  :8080 internal only       → localhost:8080               │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

Tailscale is not in the stack — the tailnet identity is host tailscaled
(installed by `setup.sh`, authenticated by `ff-bootstrap`). cloudflared
shares caddy's network namespace (`network_mode: service:caddy`), so the
tunnel's dashboard routes to `http://localhost:8080` land on caddy.
Private clients reach caddy's published :80/:443 on the host's tailnet
or LAN IP; public sites arrive via the Cloudflare tunnel only.

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
    └── caddy/               # caddy data + config (certs)
```

## Lifecycle

```
docker compose up -d --build      # bring up, build caddy image if needed
docker compose ps                 # what's running
docker compose logs -f caddy      # follow one service
docker compose pull               # newer base images
docker compose down               # stop everything
```

`ff-stack.service` (a systemd system unit, installed by `setup.sh`) does
`up -d` on boot.

Never restart the `caddy` container on its own — it owns the network
namespace cloudflared joins, and recreating it alone orphans the
connector. Always cycle the whole stack with `docker compose up -d`.
(`docker compose exec caddy caddy reload` is always safe — that's a
config reload inside the running container.)

## First-boot interactive

One thing has to happen interactively, once — on the HOST, not in the
stack:

**Tailscale.** `sudo tailscale up` (or run `ff-bootstrap`, which walks
this plus Claude Code login). The stack itself needs no tailnet auth.

## Adding a website

Edit `Caddyfile`, then reload caddy without restarting:

```
docker compose exec caddy caddy reload
```

The `add-site myapp 3000` zsh helper does this for you.

## Two listeners: private vs public

Caddy listens on separate ports:

| Port  | Listener  | Reached by             | Helper                |
|-------|-----------|------------------------|-----------------------|
| 80    | private   | tailnet / LAN          | `add-site`            |
| 443   | private   | tailnet (LE wildcard)  | `add-preview-site`    |
| 8080  | public    | cloudflared tunnel     | `add-public-site`     |

:80 and :443 are published to the host; :8080 is not — only cloudflared
can reach it, via `localhost` in the shared netns.

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
