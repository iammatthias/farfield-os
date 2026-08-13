# Configuration

Every file the bootstrap installs is tracked in `configs/`. To change
behavior durably, edit there and re-run `sudo ./scripts/setup.sh`.
One-off edits on the box work, but they drift from the repo.

## Shell (`configs/zshrc` → `~/.zshrc`)

Put machine-local tweaks in `~/.zshrc.local`. It loads last and wins.
Common adjustments:

```bash
# History (default 1.2M, shared across sessions)
HISTSIZE=1200000

# Pager — bat by default; export PAGER=less if you prefer plain
export PAGER="bat"
```

The modern-CLI aliases (`ls`→eza, `cat`→bat, `tree`→`eza --tree`,
`ports`→`ss -tulpn`) live in the ALIASES section of `configs/zshrc`.

## herdr

herdr (herdr.dev) replaces tmux as the session layer: a background server
keeps terminals alive across ssh drops. `herdr` launches/attaches the
persistent session; `herdr --session <name>` for named ones; config lives
in `~/.config/herdr/config.toml`. From another machine:
`herdr --remote iam@<host>`.

The prompt is hand-rolled zsh (`configs/zsh-prompt.zsh` →
`~/.config/zsh/prompt.zsh`) — override it from `~/.zshrc.local`.

## Kiosk dashboard

- `configs/sway-config` — compositor: brand colors, gaps, tile rules.
- `configs/foot.ini` — terminal: font (IBM Plex Mono, JetBrainsMono Nerd
  fallback) and the farfield palette.
- `configs/zprofile` — auto-starts sway on tty1 only when a display is
  attached (`FF_KIOSK_FONT` scales with output resolution).
- `board/src/main.rs` — the `ff-board` TUI itself; the palette constants
  are near the top of the Rendering section.

`ff-display on|auto|off|status` controls display power;
`ff-kiosk-restart` respawns the tiles after a config change.

## Packages

Edit the `pacman -S` blocks at the top of `scripts/setup.sh` and re-run
setup (`--needed` makes it cheap). One-off installs on the box work too —
they just won't be reproduced by a fresh bootstrap. Language toolchains
are managed by their own tools (`uv`, `rustup`, `npm -g`, `gem`), not
pacman.

## Databases

PostgreSQL and Valkey are host services:

```bash
sudo systemctl status postgresql valkey
psql            # aliased to psql as your user (superuser role created by setup)
valkey-cli      # redis-compatible CLI
```

## Ingress stack

See `stack/README.md` and `HOSTING.md` on the box. The short version:
`/srv/stack/Caddyfile` is live state — manage sites with `add-site`,
`add-public-site`, `add-preview-site`, never by clobbering the file with
repo boilerplate.
