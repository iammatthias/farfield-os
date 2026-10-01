# Ingress — Caddy, the tunnel, sites, and reachability

Caddy runs **in the compose stack**, not on the host. It owns the stack's
network namespace and publishes :80/:443 to the host. Two listeners:

| Listener | Reachable from | Add with |
|---|---|---|
| `:80` | tailnet and LAN (`<name>.local`) | `add-site` |
| `:8080` | the Cloudflare Tunnel only — never published to the host | `add-public-site` |

## Never hand-edit the Caddyfile

`/srv/stack/Caddyfile` is **live state**. It accumulates generated blocks at
runtime between the `PRIVATE-SITES` and `PUBLIC-SITES` markers. The copy in the
repo is fresh-install boilerplate; overwriting the live file with it deletes
every site on the box.

Use the shell functions (defined in `configs/zshrc`, so they exist in an
interactive shell — `bash -c` will not have them):

```bash
add-site <name> <port>          # private reverse proxy → host.docker.internal:<port>
add-site <name> </path>         # private static directory
add-public-site <host> <port>   # public via the tunnel
add-public-site <host> </path>  # public static
remove-site <name-or-hostname>
list-sites
test-caddy                      # validate before trusting a change
```

Names must match `^[a-z0-9-]+$` and hostnames `^[A-Za-z0-9.-]+$` — they are
spliced into the Caddyfile, so they stay boring on purpose.

Reverse-proxy targets are `host.docker.internal:<port>`, never `localhost`:
caddy is inside the stack's namespace, so `localhost` would point at the
container itself.

## Publishing a hostname is two steps, and only one is on the box

`add-public-site` writes the Caddy block. That is necessary and **not
sufficient**. The hostname also needs a public-hostname route
(`→ http://localhost:8080`) in the Cloudflare Zero Trust dashboard, plus DNS
pointing at the tunnel. There is no wildcard, so a new subdomain that has not
been added in the dashboard will not resolve no matter how correct the
Caddyfile is.

If a public site 404s or never connects and `list-sites` shows the block, the
missing half is almost always the dashboard entry.

## Static directories need a bind mount

A static site's directory must be bind-mounted into the caddy container —
edit `/srv/stack/docker-compose.yml`. The helper prints a reminder and does
not do it for you.

## Firewall

UFW filters INPUT. Docker publishes ports with DNAT rules that traffic hits in
FORWARD, so `ufw allow 80/tcp` neither opens nor closes a published container
port. The real policy lives in the `DOCKER-USER` chain, applied by
`ff-firewall`: caddy's published ports serve the tailnet and LAN, everything
else arriving from off-box is dropped.

Public traffic is unaffected by that chain — it arrives through cloudflared,
which reaches caddy over loopback inside the shared namespace.

## Identity

Host `tailscaled` owns the box's single tailnet identity; Tailscale is
deliberately not in the stack. SSH and Caddy's published ports land on its
address. `/var/lib/tailscale/` is that identity — do not clear it casually.

**Tailnet DNS is accepted, and stays split.** Under systemd-resolved,
Tailscale sets its resolver (100.100.100.100) on the `tailscale0` link for the
tailnet's own domains only, with `Default Route: no`; `/etc/resolv.conf` and
every other lookup stay the box's. The box used to join with
`--accept-dns=false`, which protected nothing here and left it unable to resolve
its own `*.ts.net` name while every other device could — the "works from my
phone, not from the box" failure. Don't turn it back off. If the admin console
ever sets the tailnet to *override local DNS*, the link becomes the default
route for everything; `ff-doctor` checks both. Docker containers don't get the
link's DNS — give them IPs, not tailnet names.

## Previews

`*.<preview apex>` is a tailnet-private wildcard with a real Let's Encrypt
cert (DNS-01 via Cloudflare). Each preview is one fragment under
`/srv/stack/preview-handles/<name>.caddy`, imported by the wildcard block.
