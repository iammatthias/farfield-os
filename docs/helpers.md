# farfield os helpers

Quick reference for the aliases, functions, and tools that farfield os sets up.
Run `ff-help` on the box for a printed cheat sheet, or `ff-aliases` /
`ff-functions` for an fzf-driven search.

## Tmux

Prefix is **`Ctrl-a`** (not the default `Ctrl-b`).

### Sessions

```bash
t              # tmux
tn <name>      # new named session
ta <name>      # attach
tl             # list sessions
tk <name>      # kill session
```

### Inside tmux (prefix `Ctrl-a`)

```text
Ctrl-a v         split window vertically (pane to the right)
Ctrl-a s         split window horizontally (pane below)
Ctrl-a h/j/k/l   navigate panes (vim-style)
Ctrl-a c         new window
Ctrl-a n / p     next / previous window
Ctrl-a 0-9       jump to window N
Ctrl-a x         close current pane
Ctrl-a z         toggle pane zoom
Ctrl-a d         detach (session keeps running)
Ctrl-a r         reload ~/.tmux.conf
Ctrl-a [         enter copy mode (vim keys)
Ctrl-a ?         list all bindings
```

Sessions survive disconnects — `tmux attach` after re-`ssh` to pick up where
you left off. No plugin manager is installed; if you want one, see
[tpm](https://github.com/tmux-plugins/tpm).

## Caddy site management

Defined as zsh functions; they edit `/srv/stack/Caddyfile` (with a backup)
and reload the caddy container on success. Caddy runs in the `/srv/stack`
docker-compose stack, so reloads go through `docker compose exec caddy
caddy reload --config /etc/caddy/Caddyfile` — the `--config` flag is
required (the image's workdir has no Caddyfile without it).

```bash
add-site <name> <port>         # reverse proxy: name.local:80 -> localhost:port
add-site <name> /path/to/dir   # static files: name.local:80 serves dir
add-public-site <host> <port>  # public site via the Cloudflare Tunnel connector
add-preview-site <name> <port> # tailnet-private preview at <name>.$PREVIEW_APEX
remove-preview-site <name>     # remove a preview site
list-preview-sites             # list preview handles
list-sites                     # list configured virtual hosts
remove-site <name>             # remove a site block
test-caddy                     # caddy validate (inside the container)
caddy-status                   # compose ps + recent container logs
caddy-edit                     # $EDITOR /srv/stack/Caddyfile
caddy-reload                   # caddy reload --config /etc/caddy/Caddyfile (in-container)
caddy-restart                  # docker compose restart caddy
caddy-logs                     # docker compose logs -f caddy
```

## Kiosk dashboard (attached display)

farfield os is headless by default, but `setup.sh` also installs **sway**
(Wayland compositor) + `foot` and configures `getty@tty1` to auto-log
the user in. When a display is plugged into the box, on next login
`~/.zprofile` exec's `sway`, and `ff-kiosk-tiles` arranges the
dashboard as **six tiles** — one foot window per `ff-board` panel,
in a 3×2 grid with gaps and farfield-brand borders (Horizon orange on
the focused tile):

```
╭ HOST ────────╮ ╭ CLAUDE ──────╮ ╭ NET ─────────╮
│ cpu heat +   │ │ live sessions│ │ ↓↑ rates +   │
│ mem, stacked │ │ + projects   │ │ peak graphs  │
╰──────────────╯ ╰──────────────╯ ╰──────────────╯
╭ DISK ────────╮ ╭ CONTAINERS ──╮ ╭ STATUS ──────╮
│ gauge + io   │ │ CPU/MEM      │ │ services ·   │
│ r/w graphs   │ │ sparklines   │ │ sites · procs│
╰──────────────╯ ╰──────────────╯ ╰──────────────╯
```

The compositor owns the mosaic: every tile is a real window you can
focus, swap, zoom, or kill (it respawns), and sway supplies the gaps
and borders. sway (rather than a minimal dwl-style WM) because it
exposes `wl_touch` — on a touch panel, tapping a tile fullscreens it
(back button top-left), and the fullscreen OPS view carries on-screen
action buttons (update / kiosk↻ / stack↻ / prune / reboot, two-tap
confirm on the destructive ones). Each panel process only runs the
samplers it displays. `ff-board` (no args) renders the whole board
in one terminal — that's what `ff-dashboard` runs in tmux for ssh
sessions, where there's no compositor:

Host metrics are sampled natively from `/proc` + `/sys` (CPU total +
per-core, memory/swap, default-route NIC throughput, whole-disk I/O,
hwmon temperature); container CPU/MEM sparklines + net rates come
straight off the Docker socket every 2s. Rendering is diff-based —
no flicker, no full repaints. `q` quits (the kiosk respawns it).

If `ff-board` isn't built (no cargo at setup time), the dashboard
falls back to btop + the shell boards (`ff-metrics-board` +
`ff-status-board`), which remain installed as one-shot CLIs.

You can also run `ff-dashboard` from any shell — it attaches the
same session if it already exists, or builds it. Run over plain ssh
(no tty) it creates/refreshes the session detached, which is how you
rebuild the kiosk layout remotely.

The dashboard helpers are stand-alone too:

```bash
ff-metrics-board     # container CPU/MEM sparklines + net + host (one-shot)
ff-status-board      # the unified dashboard board (one-shot)
ff-services-status   # Caddy sites + service health (one-shot)
ff-docker-status     # docker containers + pm2 processes
ff-claude-stats      # Claude Code sessions + token usage
ff-deploy <name>     # git pull + rebuild a project under ~/projects
ff-bootstrap         # interactive post-install (tailscale, claude login)
```

`ff-claude-stats` reads `~/.claude/projects/*/*.jsonl` to compute
total token usage across all sessions, count active `claude` processes,
and break down sessions by project. Token computation is skipped if
total session data exceeds 50 MB.

To swap the dashboard for something else, edit
`~/.config/sway/config` and replace `exec ff-kiosk-tiles` with e.g.:

```
exec foot --fullscreen -e ff-board       # whole board, one window
exec foot --fullscreen -e btop
```

`ff-kiosk-restart` re-applies the tile layout; `ff-kiosk-shot` grabs a
screenshot of whatever the panel is showing (via grim); `ff-display
on|auto|off|status` drives display power.

In-sway keybindings (only matter if you walk up to the box):

| Keybinding | Action |
|---|---|
| `Super + F` | Toggle fullscreen |
| `Super + Shift + R` | Reload the sway config |
| `Super + Shift + Q` | Quit sway |

## Snapshots (btrfs only)

If your root is btrfs, `setup.sh` configures Snapper and `snap-pac`:

- `snap-pac` auto-snapshots **before and after every pacman transaction** —
  so a bad `pacman -Syu` is recoverable in 30 seconds.
- `snapper-timeline.timer` keeps rolling snapshots: 5 hourly, 7 daily,
  2 weekly, 1 monthly.
- `snapper-cleanup.timer` prunes old snapshots automatically.
- On GRUB systems, `grub-btrfs` adds a "Snapshots" submenu so you can
  boot into any snapshot when an update breaks the system.
- `chattr +C` is set on `/var/lib/postgres`, `/var/lib/valkey`, and
  `/var/lib/docker` to skip CoW on database/container files (these
  get tons of small random writes; CoW makes them slow + bloats
  snapshot sizes).

```bash
snapper -c root list                    # list snapshots
snapper -c root create -d "before X"    # manual snapshot with description
snapper -c root status N..M             # diff between two snapshots
snapper -c root undochange N..M         # selectively revert files
snapper -c root delete N                # delete a specific snapshot
```

If a `pacman -Syu` breaks boot: reboot, hold Shift to enter GRUB, pick
"Arch Linux snapshots" submenu, choose the most recent pre-update entry,
boot into it (read-only), then either `snapper rollback` from there or
`btrfs subvolume set-default` to make it the new root.

systemd-boot users: GRUB-style boot-into-snapshot is GRUB-only. Snapper
itself still works (so `undochange` and timeline retention are useful),
but recovery from a non-bootable system needs a USB.

## PM2

```bash
pm2-start <ecosystem.config.js>   # start an ecosystem file
pm2-add-site <name> <port> <eco>  # pm2-start + add-site in one shot
pm2-remove <name>
pm2-restart <name>
pm2-logs <name>
pm2-status                        # pm2 list + Caddy site list
```

## System status

```bash
system-status      # uptime, load, memory, disk, top procs, listening ports
db-status          # postgresql + valkey systemd status + connection counts
security-status    # ufw + fail2ban + sshd status
port-check <port>  # is anything listening on this port?
ff-info          # fastfetch machine report (TR-100 style)
ff-update        # pacman -Syu + cache clean (--yes: unattended via systemd-run)
ff-help          # full alias / function reference
```

## File operations

```bash
ls / ll / la / lf / lt / tree     # eza variants (icons, --git, --tree, …)
cat                                # bat (paged, syntax-highlighted)
less                               # bat with paging
df / du / free                     # human-readable (-h)
e                                  # nvim
mkcd <dir>                         # mkdir + cd
```

## Navigation

```bash
.. ... .... ..... ......           # cd up N levels
~                                  # cd ~
-                                  # cd -
cdp / cdd / cdt / cdl / cde        # ~/projects, ~/Downloads, /tmp, /var/log, /etc
up <n>                             # cd ../../… n times (function)
```

`zoxide` is initialized — once you've `cd`'d into a directory, `z partial`
will jump back without typing the full path.

## Git

```bash
g gs ga gc gp gl gd gb gco         # short forms
glog                               # git log --oneline --graph --decorate
```

## Docker

```bash
dkr      # docker
dkc      # docker compose
dkps     # docker ps
dkpa     # docker ps -a
dki      # docker images
dkex     # docker exec -it
```

## AUR (yay)

```bash
yay-update / yay-install / yay-remove / yay-search / yay-info
```

## Misc

```bash
ff                     # fastfetch
myip                   # public IP (curl ifconfig.me)
localip                # local IPv4 addresses
ports                  # ss -tulpn
sqlite                 # sqlite3
smart                  # sudo smartctl -a
ufw-status / fail2ban-status
nmap-local             # nmap -sn 192.168.1.0/24
nmap-scan              # nmap -sS -O -F
lsof-port <port>       # lsof -i :<port>
c                      # clear
reload                 # source ~/.zshrc
```

## Backup

```bash
backup-system    # snapshot /srv/stack/Caddyfile, /etc/{ssh,ufw,fail2ban},
                 # ~/.zshrc, ~/.tmux.conf to ~/backups/<timestamp>
```

## Adding your own

Drop functions/aliases into `~/.zshrc` and `source ~/.zshrc`. Or, for a
cleaner override pattern, create `~/.zshrc.local` and add this near the top
of `~/.zshrc`:

```bash
[ -f "$HOME/.zshrc.local" ] && source "$HOME/.zshrc.local"
```

Editing `~/.zshrc` in place works fine — re-running `setup.sh` will back up
the existing file as `~/.zshrc.ff-backup.<timestamp>` before reinstalling.
