# farfield os

![farfield os](docs/assets/banner.jpg)

**An opinionated home-server bootstrap for Arch Linux**

One script turns a fresh Arch install into my home server. The result: a
sharp shell, persistent sessions, the runtimes I use, and an ingress
stack that decides what the world can see. If a display is attached, the
box becomes a live dashboard. Day to day, Claude Code manages the box
over SSH.

This is intentionally opinionated and intentionally complete. Every
default is a decision I made for my own house. It is a personal
bootstrap, not a minimal TTY distribution. If your decisions differ,
fork it and make them yours. That is the intended path, not a
workaround.

## What you get

- **Kiosk dashboard** — sway + foot tile six `ff-board` panels on an
  attached display: HOST, CLAUDE, NET / DISK, CONTAINERS, OPS. Touch
  works: tap a tile to make it full screen. Headless, `ff-dashboard`
  shows the same board over SSH.
- **Hand-rolled prompt** — zsh-native. ⊙ host, path, git, command
  duration, clock. No prompt binary.
- **Zsh via Zinit** — fzf-tab, autosuggestions, syntax highlighting,
  history search. Mirrors my dotfiles.
- **herdr sessions** — persistent terminals from
  [herdr.dev](https://herdr.dev). Sessions survive SSH drops. Attach
  from any device.
- **Ingress stack** — Tailscale on the host for private access: the box
  is one tailnet node, and ssh lands on it from anywhere. Caddy and
  Cloudflare Tunnel in docker compose at `/srv/stack` for HTTPS,
  routing, and the public sites. No open ports on the router.
- **Snapshots** — Snapper photographs the system before and after every
  pacman transaction. A bad update is a reboot into yesterday. Btrfs
  roots only.
- **Security** — UFW, Fail2ban, hardened SSH. The public surface is
  opt-in, one hostname at a time.
- **Claude Code** — native install on the host, with system context in
  `~/CLAUDE.md`. The box is built to be worked on by an agent.
- **Runtimes** — Node, Bun, Python (uv), Ruby, Rust, Go, Java, Docker.
  PostgreSQL and Valkey for data. PM2 for Node processes.
- **Modern CLI** — eza, bat, fd, fzf, zoxide, ripgrep.

## Before you start

Install Arch Linux. Follow the
[installation guide](https://wiki.archlinux.org/title/Installation_guide).

You need:

- A non-root user with sudo
- SSH access
- `git`, `curl`, `wget`, `base-devel`

`setup.sh` installs `yay` and everything else itself. Do not bootstrap
an AUR helper first.

Read `setup.sh` before you run it. It is the fastest way to know whether
you want this.

## Quick start

```bash
git clone https://github.com/iammatthias/farfield-os.git
cd farfield-os
sudo ./scripts/setup.sh
sudo reboot
```

After the reboot, run `ff-bootstrap`. It walks the one-time steps:
Tailscale auth and Claude login. Run it again any time — it skips what
is already done.

Then start your session:

```bash
herdr
```

## The shell

The prompt is hand-rolled zsh. It shows ⊙, the hostname, the path, the
git state, the duration of slow commands, and the clock. Override any of
it from `~/.zshrc.local` — that file loads last and wins.

History is large (1.2M lines) and shared across sessions. Completions
use fzf-tab. Bare `ls` and `tree` stay unaliased, so scripts and agents
see stock tools. `ll`, `la`, and `lt` are the eza views.

## Caddy

Caddy runs in the `/srv/stack` compose stack, not on the host. The live
Caddyfile is state — manage it with the helpers, never by hand:

```bash
add-site <name> <port>       # private reverse proxy on the tailnet
add-site <name> </path>      # private static site
add-public-site <host> <port># public site through the Cloudflare Tunnel
remove-site <name>           # remove a site
list-sites                   # list sites
test-caddy                   # validate the config
```

The helpers proxy through `host.docker.internal`. Caddy lives inside the
stack's network namespace, so `localhost` would point at the container.

## Essential commands

```bash
ff-info       # system report
ff-update     # update the system (--yes: unattended, survives SSH drops)
ff-help       # the full command reference
ff-dashboard  # the board in your terminal (run inside herdr to persist)
ff-deploy     # pull + rebuild a project under ~/projects
```

```bash
t              # herdr — launch or attach the persistent session
tn <name>      # named session
ta <name>      # attach to a session
tl             # list sessions
tk <name>      # stop a session
herdr --remote iam@<host>   # attach to this box from another machine
```

```bash
gs ga gc gp gl # git status / add / commit / push / pull
dkr dkc        # docker / docker compose
dkps dki dkex  # docker ps / images / exec -it
z <dir>        # zoxide jump
```

## Configuration

Every file the bootstrap installs lives in `configs/`. Edit there and
re-run setup, and the change is versioned. Machine-local tweaks go in
`~/.zshrc.local`.

- Prompt: `~/.config/zsh/prompt.zsh`
- herdr: `~/.config/herdr/config.toml`, then `herdr server reload-config`
- Packages: the `pacman -S` blocks at the top of `scripts/setup.sh`
- Docs pages: edit `README.md` or `docs/*.md`, then run
  `go run ./scripts/build-os-docs . stack/homepage`

## Workflow

1. Start a session: `herdr`
2. Create a project: `ff-project-init myproject`
3. Initialize a runtime: `npm init`, `uv init`, `cargo init`
4. Route it: `add-site myproject 3000`
5. Open it: `http://myproject.local` on the tailnet

## Uninstall

```bash
sudo ./scripts/uninstall.sh
```

It reverts the configuration and keeps backups. Packages stay installed
— remove those yourself if you want them gone.

## Philosophy

- **One machine, whole.** The box is a single-tenant home server, not a
  general-purpose distribution.
- **Agent-first.** Claude Code lives on the host with a map of the
  system. Most administration is a conversation.
- **Terminal-first.** Persistent herdr sessions are the interface. The
  dashboard is a TUI.
- **Configs are tracked.** Every installed file is reviewable in diff
  form, not buried in heredocs.
- **No secrets in the repo.** Stack secrets land in `/srv/stack/.env` at
  runtime, mode 600, never committed.

## License

MIT
