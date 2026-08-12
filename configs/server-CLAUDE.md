# farfield os server

This machine was bootstrapped by [farfield os](https://github.com/iammatthias/farfield-os).
It is a single-tenant home server intended for remote development over SSH.

## Available tooling

### Shells / multiplexers
- `zsh` (default shell; Zinit + a hand-rolled prompt, mirrors the user's Mac dotfiles)
- `herdr` (persistent agent sessions — replaces tmux; `herdr` attaches, `herdr --remote` from other machines)

### Editors
- `nvim`

### Modern CLI
- `eza` (ls), `bat` (cat/pager), `fd` (find), `ripgrep` / `rg` (grep),
  `fzf`, `zoxide`, `jq`, `yq`, `httpie`, `fastfetch`
- `btop`, `htop`, `iotop`, `nethogs`, `ncdu`, `smartctl`

### Languages / runtimes
- **Node.js** (`node`, `npm`, `yarn`, `pnpm`, `bun`, `pm2`)
- **Python** via **`uv`** — `uv` replaces `pip`/`pipx`/`pipenv`/`poetry`.
  Use `uv venv`, `uv pip install`, `uv tool install <pkg>`, `uv run <cmd>`.
  Installed `uv tool`s: `ruff`, `pytest`, `black`.
- **Ruby** (`gem`, `bundler`)
- **Rust** (rustup, `cargo`)
- **Go** (`go`, `dlv`)
- **Java** (`java`, `mvn`, `gradle`)

### AI / LLM tooling
- **Claude Code** (`claude`, native install in ~/.local/bin) — the primary way this box is
  managed: ssh in, run `claude`. Subscription auth lives at `~/.claude/`.
  You are probably reading this file from inside such a session.
- **New projects** — `ff-project-init <path> [<description>]` creates
  the dir, git-inits it, and drops a starter CLAUDE.md. App/service
  checkouts live in `~/projects/<name>` (what `ff-deploy` expects);
  `/srv/projects` is only for static content served by the caddy
  container via bind-mount (preview sites).

### Web / proxy stack
The network ingress layer runs as a docker-compose stack at `/srv/stack`
(not on host). Containers share the tailscale container's network
namespace:
- `ff-tailscale` — tailnet identity, ingress
- `ff-caddy` — reverse proxy (`add-site <name> <port>` writes to
  `/srv/stack/Caddyfile` and reloads via `docker compose exec caddy
  caddy reload --config /etc/caddy/Caddyfile` — without `--config` the
  image's workdir has no Caddyfile and the reload errors out)
- `ff-cloudflared` — Cloudflare Tunnel connector for opt-in public
  sites (`add-public-site`)

Stack lifecycle is `cd /srv/stack && docker compose <cmd>`. The
`ff-stack` systemd unit runs `docker compose up -d --build` at boot.

### Databases
- `postgresql` (systemd unit `postgresql`, default user matches `$USER`)
- `valkey` (Redis-compatible, systemd unit `valkey`)
- `sqlite3`

### Containers
- `docker`, `docker-compose` (the user is in the `docker` group)

### Display / kiosk dashboard
- `sway` (Wayland) + `foot`. Headless by default. If a display is
  attached, `getty@tty1` auto-logs the user in and `~/.zprofile` exec's
  `sway`, which tiles six `ff-board <panel>` instances into a 3×2
  grid (HOST(cpu+mem)/CLAUDE/NET, DISK/CONTAINERS/OPS). On a touch panel, tapping a
  tile fullscreens it; the fullscreen OPS view has action buttons
  (update / kiosk↻ / stack↻ / prune / reboot, two-tap confirm).
- Config: `~/.config/sway/config`; helpers: `ff-kiosk-restart`,
  `ff-kiosk-shot [out.png]` (screenshot over ssh).
- `ff-dashboard` runs the same board in the current terminal (run it inside herdr to persist).

### Snapshots (btrfs only)
- `snapper` + `snap-pac` — automatic pre/post snapshots for every
  pacman transaction. Recover from a bad `pacman -Syu` via
  `snapper -c root rollback` or by booting an older snapshot from
  GRUB's "Snapshots" submenu.
- Retention: timeline 5 hourly / 7 daily / 2 weekly / 1 monthly /
  0 yearly; pacman pre/post pairs capped at NUMBER_LIMIT=12
  (NUMBER_LIMIT_IMPORTANT=6).
- `/var/lib/postgres`, `/var/lib/valkey`, `/var/lib/docker` are marked
  `chattr +C` (no CoW) — important for write-heavy DB/container files.

### Network / security
- `ufw` (deny incoming except the detected sshd port(s), 80/443,
  8666/udp — kiosk wake push from the CV box — and 172.16.0.0/12,
  so docker containers can reach host-native services)
- `fail2ban` (sshd jail, 3 retries, 1h ban)
- `nmap`, `tcpdump`, `wireshark-cli`

## farfield os helpers (in PATH as `/usr/local/bin/ff-*`)

- `ff-info`   — fastfetch system report
- `ff-update` — `pacman -Syu` + cache clean
- `ff-deploy <project>` — git-pull + rebuild a `~/projects/<project>` stack
- `ff-help`   — full alias / function reference

## Useful shell shortcuts

The full list is `ff-help`, `ff-aliases` (fzf), `ff-functions` (fzf).
Highlights:

- Caddy: `add-site myapp 3000`, `list-sites`, `remove-site myapp`,
  `caddy-edit`, `caddy-reload`, `caddy-logs`, `test-caddy`
- PM2: `pm2-start`, `pm2-add-site`, `pm2-remove`, `pm2-restart`,
  `pm2-logs`, `pm2-status`
- Status: `system-status`, `db-status`, `security-status`,
  `port-check <port>`
- Backup: `backup-system` snapshots configs to `~/backups/<timestamp>`
- Project scaffold: `create-react-hono <name>` (React + Hono boilerplate
  in `~/projects/<name>`)

## Don't do

- Don't change tailscale prefs (`tailscale up --advertise-tags=...`,
  `--advertise-routes=...`, etc.) without explicit user instruction.
  Tags that aren't pre-approved in the tailnet ACL cause auth rejection
  and a restart loop. If you must change network state, ask first.
- Don't reach for Tailscale **Services** (`svc:foo` hostnames,
  `tailscale serve --service=...`) or spin up per-site tailscale
  sidecars. Both work but add admin friction.
- Don't hand-roll subpath routing (`handle_path /<name>*` in the
  shared caddy). Each preview gets its own subdomain on the
  preview apex.
- **Preview sites use `add-preview-site <name> <port-or-dir>`**
  (zsh function). It writes one fragment file at
  `/srv/stack/preview-handles/<name>.caddy` (imported into the
  wildcard HTTPS site block in `/srv/stack/Caddyfile`) and reloads
  caddy. The URL is `https://<name>.$PREVIEW_APEX` —
  **tailnet-private**: DNS for `*.$PREVIEW_APEX` points at this
  box's tailnet IP, so off-tailnet clients can't reach it. Caddy
  serves a real LE wildcard cert obtained via ACME DNS-01 against
  Cloudflare. `$PREVIEW_APEX` lives in `/srv/stack/.env`; read it
  from there if you need the actual value.
- For an **opt-in public preview** (someone off-tailnet needs the
  URL), the cloudflared connector + `add-public-site <hostname>
  <port-or-dir>` handles it. That writes onto caddy's `:8080`
  listener which the tunnel fronts. Don't use it by default —
  previews are tailnet-private unless asked.
- Don't modify `/srv/stack/docker-compose.yml` or `/srv/stack/Caddyfile`
  by hand inside a single agent turn; the user iterates the source-of-
  truth in `~/farfield-os/stack/` and copies into `/srv/stack/`. If you want
  to change stack config, edit the repo and tell the user to pull.

## Conventions

- New web services should live under `~/projects/<name>` and be exposed
  via Caddy with `add-site <name> <port>`; `ff-deploy <name>` pulls +
  rebuilds from there. `/srv/projects` is only for static content served
  by the caddy container via bind-mount (preview sites).
- Long-running Node services use PM2. Use `pm2-add-site` to do both
  steps at once.
- Python projects use `uv` — never `pip install` into the system Python
  (Arch's Python is externally-managed).
- Secrets do not live in the repo. Stack secrets live in
  `/srv/stack/.env` (mode 600); Claude auth in `~/.claude/`.

## Reverting

`sudo /path/to/farfield-os/scripts/uninstall.sh` reverts the farfield os configuration
(stops services, restores stock sshd, backs up user configs as
`*.ff-backup.<timestamp>`). Pacman packages are not removed.
