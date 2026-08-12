# farfield os

![farfield os](docs/assets/banner.jpg)

**Opinionated home-server bootstrap for Arch Linux**

A single setup script that turns a fresh Arch install into a home server
for remote development over SSH — with an optional live dashboard if a
display is attached. Spaceship + Zsh + Tmux, Docker, PostgreSQL + Valkey,
a full set of language runtimes, and a docker-compose stack
(Tailscale + Caddy + Cloudflare Tunnel) that ships the network surface.
Day-to-day management is Claude Code over SSH.

This is intentionally opinionated and intentionally heavy — it's a
personal home-server bootstrap, not a minimal TTY distribution.

## What You Get

- **Kiosk Dashboard** — sway (Wayland) + foot tile six `ff-board` panels (CPU/MEM/NET, DISK/CONTAINERS/OPS) on an attached display, with touch support: tap a tile to fullscreen it, on-screen action buttons. A no-op when headless; `ff-dashboard` gives the same board over ssh in tmux
- **Spaceship Prompt** — fast, informative Zsh prompt (git, runtimes, exec time)
- **Zsh with Essential Plugins** — autosuggestions, syntax highlighting, completions
- **Tmux as Default** — tiling terminal multiplexer, `Ctrl-a` prefix, vim keybindings
- **Ingress stack** — docker-compose at `/srv/stack/`: Tailscale (tailnet identity) + Caddy (reverse proxy, LE wildcard certs via DNS-01) + Cloudflare Tunnel (opt-in public sites). `git pull && docker compose up -d --build` to update
- **PM2 Process Management** — Node.js app process management
- **Database Support** — PostgreSQL and Valkey (Redis-compatible)
- **Btrfs + Snapper** — auto-snapshot before/after every pacman transaction; boot-into-snapshot via GRUB submenu when an update breaks the system (only when root is btrfs)
- **Security Features** — UFW firewall, Fail2ban, SSH hardening (drop-in based)
- **System Monitoring** — btop, iotop, nethogs, smartmontools
- **Claude Code** — installed npm-global on the host; ssh in and run `claude` to manage the box or work on projects (`~/CLAUDE.md` gives it system context)
- **Runtime Support** — Node.js, Bun, Python (uv), Ruby, Rust, Go, Java, Docker
- **Development Tools** — eza, bat, fd, fzf, zoxide, ripgrep, and more

## Initial Arch Linux Setup

Before installing, you need a fresh Arch Linux system with basic tools.

### 1. Install Arch Linux

Follow the [Arch Linux Installation Guide](https://wiki.archlinux.org/title/Installation_guide).

### 2. Prerequisites

Ensure your Arch system has:

- A non-root user with sudo access
- SSH access configured
- Basic packages: `git`, `curl`, `wget`, `base-devel`

`setup.sh` installs `yay` (AUR helper) and the rest of the toolchain itself —
you do not need to bootstrap an AUR helper first.

## Quick Start

```bash
# Clone
git clone https://github.com/iammatthias/farfield-os.git
cd farfield-os

# Run setup as root
sudo ./scripts/setup.sh

# Reboot
sudo reboot
```

After reboot, `ff-bootstrap` walks the interactive one-time steps
(tailscale auth, Claude Code login), then start tmux:

```bash
tmux
```

## Core Features

### Spaceship Prompt

- Shows git branch, status, and staging
- Displays active runtime environments (Node, Python, Ruby, etc.)
- Execution time for slow commands
- Clean, fast, and highly customizable

### Zsh Configuration

- **Plugins**: autosuggestions, syntax highlighting, completions, history-substring-search
- **Smart History**: 50k entries, shared across sessions
- **Navigation**: `..`, `...`, `-` for directory jumping; `z <dir>` (zoxide)
- **File Operations**: `ls` with icons (eza), `cat` with syntax highlighting (bat)

### Caddy (containerized)

Caddy runs in the `/srv/stack` compose stack, not as a host package. The
Caddyfile lives at `/srv/stack/Caddyfile`; manage it with the shell
helpers, not by hand:

- **Add Sites**: `add-site myapp 3000` (reverse proxy to a host port) or `add-site static /srv/www` (static files)
- **Public sites**: `add-public-site <name> <host> <port>` (via Cloudflare Tunnel)
- **Preview sites**: `add-preview-site <name> <port|dir>` (HTTPS on the tailnet)
- **Site Management**: `remove-site`, `list-sites`, `test-caddy`
- **Configuration**: `caddy-edit` / `caddy-reload` / `caddy-status`

### Runtime Environments

- **Node.js**: npm, yarn, pnpm, bun, pm2
- **Python**: [`uv`](https://docs.astral.sh/uv/) (replaces pip / pipx /
  pipenv / poetry); preinstalled `uv tool`s: ruff, pytest, black
- **Ruby**: gem, bundler
- **Rust**: cargo, rustup
- **Go**: go, delve debugger
- **Java**: OpenJDK, Maven, Gradle
- **Docker**: docker, compose, buildx
- **Claude Code**: `claude` (Anthropic's CLI; reads `~/CLAUDE.md` for
  system context)

## Essential Commands

### System

```bash
ff-info       # System information (fastfetch machine report)
ff-update     # Update system (--yes for unattended via systemd-run)
ff-help       # Full command reference
ff-dashboard  # The kiosk board, in tmux, over ssh
ff-deploy     # git pull + rebuild a project under ~/projects
```

### Tmux

```bash
t              # Start tmux
tn <name>      # New named session
ta <name>      # Attach to session
tl             # List sessions
Ctrl-a v       # Split vertical
Ctrl-a s       # Split horizontal
Ctrl-a h/j/k/l # Navigate panes
```

### Docker

```bash
dkr             # docker
dkc             # docker compose
dkps            # docker ps
dkpa            # docker ps -a
dki             # docker images
dkex <name>     # docker exec -it
```

### Caddy

```bash
add-site <name> [port|dir]  # Add site (port=reverse proxy, dir=static files)
remove-site <name>          # Remove site from Caddy
list-sites                  # List configured sites
caddy-edit                  # Edit /srv/stack/Caddyfile
caddy-reload                # Reload Caddy (in-container)
test-caddy                  # Validate Caddy configuration
caddy-status                # Check Caddy status and logs
```

### Git

```bash
gs             # git status
ga             # git add
gc             # git commit
gp             # git push
gl             # git pull
glog           # git log --oneline --graph
```

### Navigation

```bash
..             # Go up one directory
...            # Go up two directories
-              # Previous directory
ls             # List with icons
ll             # Long format
tree           # Tree view (aliased to eza --tree)
```

## Configuration

### Spaceship Prompt

Edit `~/.zshrc` to customize the prompt:

```bash
# Show/hide sections
SPACESHIP_NODE_SHOW=true
SPACESHIP_PYTHON_SHOW=true
SPACESHIP_DOCKER_SHOW=true

# Customize symbols
SPACESHIP_CHAR_SYMBOL="❯ "
SPACESHIP_GIT_BRANCH_PREFIX=""
```

### Tmux

Edit `~/.tmux.conf` for custom keybindings (the shipped prefix is `Ctrl-a`).

### Caddy

Use the helpers (`add-site`, `add-public-site`, `add-preview-site`) rather
than editing `/srv/stack/Caddyfile` by hand — they generate blocks that
proxy via `host.docker.internal` (caddy runs inside the stack's network
namespace, so `localhost` would point at the container, not the host):

```caddy
myapp.local:80 {
    reverse_proxy host.docker.internal:3000
}
```

## Development Workflow

1. **Start tmux**: `tmux`
2. **Create project**: `ff-project-init myproject` (or `mkdir` under `~/projects`)
3. **Initialize runtime**: `npm init`, `uv init`, `cargo init`, etc.
4. **Add to Caddy**: `add-site myproject 3000`
5. **Access**: `http://myproject.local` over the tailnet

## Uninstall

```bash
sudo ./scripts/uninstall.sh
```

This removes farfield os configuration (with backups) but keeps system
packages installed.

## Philosophy

farfield os is designed for:

- **Remote Development**: SSH from a laptop to an Arch home server
- **Agent-First Management**: Claude Code on the box, with system context
- **Web Development**: Caddy for reverse proxy and HTTPS
- **Terminal-First**: Tmux as the primary interface; the dashboard is a TUI
- **Runtime Agnostic**: Support for the major language runtimes
- **Versioned Configs**: Every file the bootstrap installs lives under
  `configs/`, so changes are reviewable in diff form rather than buried
  in heredocs

## License

MIT
