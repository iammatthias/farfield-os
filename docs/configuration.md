# Configuration

Customization recipes for a farfield os box. Every file the bootstrap
installs is tracked in `configs/` — the durable way to change behavior is
to edit there and re-run `sudo ./scripts/setup.sh` (or copy the file into
place); one-off edits on the box work but drift from the repo.

## Shell (`configs/zshrc` → `~/.zshrc`)

Add machine-local tweaks at the bottom of `~/.zshrc`, or keep them in the
repo if they're worth versioning. Highlights you may want to adjust:

```bash
# Prompt sections (Spaceship)
SPACESHIP_DOCKER_SHOW=true

# History (default 50k, shared across sessions)
HISTSIZE=50000

# Pager — bat by default; export PAGER=less if you prefer plain
export PAGER="bat"
```

The modern-CLI aliases (`ls`→eza, `cat`→bat, `tree`→`eza --tree`,
`ports`→`ss -tulpn`) live in the ALIASES section of `configs/zshrc`.

## Tmux (`configs/tmux.conf` → `~/.tmux.conf`)

The prefix is `Ctrl-a`. Keybindings are listed in `docs/helpers.md`; edit
`configs/tmux.conf` to change them. No plugin manager is installed — the
config is self-contained.

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
