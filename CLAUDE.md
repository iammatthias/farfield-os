# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

farfield os is an opinionated home-server bootstrap for Arch Linux. One
script provisions an Arch box for remote development over SSH: enhanced
zsh, tmux, Docker, PostgreSQL + Valkey, a broad set of language runtimes
(Node, Python via uv, Ruby, Rust, Go, Java), plus a docker-compose stack
under /srv/stack for the network-ingress layer (Tailscale, Caddy,
Cloudflare Tunnel). Day-to-day management is Claude Code over SSH.

It also installs sway (Wayland compositor) + foot so an optional attached
display becomes a live kiosk dashboard (auto-login on tty1 → sway → six
`ff-board <panel>` foot tiles — host(cpu+mem), claude, net / disk,
containers, status — arranged 3×2 by `ff-kiosk-tiles`;
`bin/ff-dashboard` runs the same board as a single tmux view for ssh
sessions). The DRM-status guard in `~/.zprofile` is a no-op on truly
headless boxes. sway is used (not a minimal dwl-style WM) because
it exposes `wl_touch` — the rack touch panel's tap interactions
(tap a tile → fullscreen; action buttons in the fullscreen OPS view) only
work under a compositor that delivers touch to clients.

### Container stack (`/srv/stack`)

The network ingress layer is a docker-compose stack — atomically updatable
via `git pull && docker compose up -d --build` and isolated from the host
substrate.

- `ff-tailscale` (image: `tailscale/tailscale`) — tailnet identity. Other
  services share its network namespace (`network_mode: service:tailscale`)
  so they reach the tailnet directly + can talk to one another on
  `localhost`.
- `ff-caddy` (image: built from `stack/caddy/Dockerfile`, adds the
  Cloudflare DNS module) — reverse proxy. Caddyfile lives at
  `/srv/stack/Caddyfile`, mounted into the container.
  `add-site`/`remove-site` shell helpers edit it and reload via
  `docker compose exec`.
- `ff-cloudflared` (image: `cloudflare/cloudflared`) — Cloudflare Tunnel
  connector for opt-in public sites (`add-public-site`).

State on host:
- `/srv/stack/Caddyfile` — caddy config. The LIVE copy accumulates
  add-site blocks at runtime — never clobber it with the repo boilerplate.
- `/srv/stack/.env` — TS_AUTHKEY, CF_API_TOKEN, CLOUDFLARED_TOKEN (chmod 600)
- `/srv/stack/data/tailscale/` — tailnet identity
- `/srv/stack/data/caddy/{data,config}/` — caddy data + cert cache
- `/srv/projects/` — project checkouts (served by preview sites)

When root is btrfs, the script installs Snapper + snap-pac (auto-snapshot
on every pacman transaction) and grub-btrfs (boot-into-snapshot from GRUB).
`/var/lib/{postgres,valkey,docker}` get `chattr +C` to skip CoW on
high-churn database/container files.

The AI surface is **Claude Code on the host** (`claude`, installed
npm-global): ssh in and run `claude` to manage the box or work on
projects. Auth (subscription OAuth) is interactive — `ff-bootstrap`
walks it once post-install.

It is intentionally heavy — this is a personal home-server bootstrap, not a
"minimal TTY" distribution.

## Repository Structure

```
farfield-os/
├── README.md
├── LICENSE
├── CLAUDE.md
├── .gitignore
├── scripts/
│   ├── setup.sh          # Bootstrap (run as root)
│   └── uninstall.sh      # Revert configuration
├── configs/              # Files installed verbatim by setup.sh
│   ├── zshrc, tmux.conf, foot.ini, sway-config, zprofile, btop.conf
│   ├── fastfetch.jsonc, fail2ban-jail.local, getty-autologin.conf
│   ├── ff-stack.service, ff-docker-prune.{service,timer}
│   ├── journald-farfield.conf, tmpfiles-farfield.conf, udev-rapl.rules
│   └── server-CLAUDE.md             # installed to ~/CLAUDE.md (system context for Claude Code)
├── bin/                  # ~20 ff-* helpers installed to /usr/local/bin
│   └── ff-{info,update,help,deploy,dashboard,bootstrap,display,
│         kiosk-*,*-board,*-status,project-init,claude-stats}
├── board/                # ff-board — fullscreen ratatui kiosk TUI
│   ├── Cargo.toml        #   (built by setup.sh; host + container graphs)
│   └── src/main.rs
├── stack/                # /srv/stack boilerplate (compose, Caddyfile,
│   └── ...               #   caddy Dockerfile, homepage, preview-https)
└── docs/
    ├── configuration.md
    ├── helpers.md
    └── troubleshooting.md
```

## Key Commands

### Install / Update / Uninstall

```bash
sudo ./scripts/setup.sh        # Bootstrap a fresh Arch system
ff-update                      # pacman -Syu + cache clean (--yes: unattended)
sudo ./scripts/uninstall.sh    # Revert configuration (with backups)
```

### Post-install reference

```bash
ff-info     # fastfetch report (TR-100 style)
ff-help     # Full command reference
```

## Architecture

### setup.sh — three phases

1. **System packages** via `pacman -S`: shells/editors, Docker, runtimes,
   databases, security tooling, modern CLI replacements. (Caddy is NOT a host
   package — it lives in the /srv/stack compose stack.)
2. **System configuration**: install configs from `configs/` to their canonical
   locations, configure UFW + fail2ban + SSH hardening, init Postgres cluster,
   enable systemd units (ff-stack, Docker, Postgres, Valkey).
3. **Per-user tooling** (run as `$REAL_USER` via `sudo -u`): Oh My Zsh, plugins,
   Spaceship prompt, npm globals (yarn/pnpm/pm2/eslint/prettier/jest +
   `@anthropic-ai/claude-code`), Bun, `uv tool install` (ruff/pytest/black),
   Ruby bundler, rustup, Go delve.

Setup is run as root. The script derives the target user via `logname`, and
all per-user work runs through `sudo -u "$REAL_USER"`.

### Helper scripts (`/usr/local/bin/`)

- `ff-info`  — `exec fastfetch` (uses the installed `~/.config/fastfetch/config.jsonc`)
- `ff-update`— `pacman -Syu` + cache clean
- `ff-help`  — printed reference of installed aliases / functions

### Generated user files

- `~/.zshrc`                          — copied from `configs/zshrc`
- `~/.tmux.conf`                      — copied from `configs/tmux.conf`
- `~/.config/fastfetch/config.jsonc`  — copied from `configs/fastfetch.jsonc`
- `~/CLAUDE.md`                       — copied from `configs/server-CLAUDE.md` (only if not already present); gives Claude Code system context

## Design principles

- **Opinionated, not minimal** — assumes a single-tenant home server, not a
  general-purpose distribution.
- **Idempotent-ish** — re-running setup.sh re-applies configs, backing up
  existing `~/.zshrc` first. Most steps tolerate already-configured state.
- **No secrets in repo** — stack secrets land in `/srv/stack/.env`
  (chmod 600) at runtime, never committed.
- **Configs are tracked** — every file the bootstrap installs lives under
  `configs/` so changes are reviewable in diff form rather than buried in
  heredocs.

## Editing tips

- To change shell behavior, edit `configs/zshrc` and re-run setup, or just
  copy it onto `~/.zshrc` (it overrides existing).
- To add a Caddy site at runtime, use the `add-site` shell function from
  `configs/zshrc`; don't hand-edit `/srv/stack/Caddyfile`.
- To change the package set, edit the two `pacman -S` blocks at the top of
  `scripts/setup.sh`.
- After editing `README.md` or `docs/*.md`, regenerate the os.farfield.systems
  doc pages so they don't drift: `python3 scripts/build-os-docs.py . stack/homepage`
  and commit the refreshed `stack/homepage/docs/`.

## Documentation

- **README.md** — user-facing install + usage guide
- **docs/configuration.md** — customization recipes
- **docs/helpers.md** — full alias / keybinding reference
- **docs/troubleshooting.md** — common issues
