# Troubleshooting

## SSH

### New connections drop at the handshake. Existing sessions work.
An openssh upgrade ran without an sshd restart. The old listener cannot
start the new per-session binary. `ff-update` restarts sshd after
openssh upgrades. If you upgraded by hand, run this from a still-open
session:
```bash
sudo systemctl try-restart sshd   # does NOT drop existing sessions
```
No open session? Physical console (or reboot).

### Typed input garbles over ssh
The box does not have your terminal's terminfo entry. Setup installs
`ghostty-terminfo`. For other terminals, push the entry from the client:
`infocmp -x | ssh box 'tic -x -'`.

## Ingress stack

```bash
cd /srv/stack
docker compose ps                  # tailscale must be healthy first —
                                   # caddy + cloudflared join its netns
docker compose logs tailscale caddy cloudflared --tail 50
```

- **Everything down at once**: never restart the tailscale container
  alone — caddy/cloudflared orphan when its netns dies. Always
  `docker compose up -d` the whole stack (`ff-stack.service` does this at
  boot).
- **Caddy rejects config**: `test-caddy` validates; `caddy-status` tails
  logs. Site helpers keep a `.bak` of the previous Caddyfile.
- **Public site 502s**: check cloudflared is running and
  `COMPOSE_PROFILES=cloudflared` is set in `/srv/stack/.env`.
- **Container can't reach a host service**: host ports are proxied via
  `host.docker.internal`; UFW must allow docker subnets
  (`ufw allow from 172.16.0.0/12` — setup does this).

## Kiosk dashboard

```bash
ff-kiosk-shot            # screenshot what the panel is showing (grim)
ff-kiosk-restart         # respawn the six tiles
ff-display status        # display power state
```

- **Blank display**: `ff-display on` forces power on; presence daemon and
  wake listener both fail-open, so a blank screen usually means sway
  isn't running — check tty1 auto-login (`systemctl status getty@tty1`)
  and `~/.zprofile`'s DRM guard.
- **Tiles missing after an update**: `pkill -x ff-board` — the tiles
  respawn; or `ff-kiosk-restart`.

## herdr

Persistent sessions live in herdr's background server:
```bash
herdr status server             # is the server up?
herdr session list              # what sessions exist
herdr                           # launch/attach the persistent session
```
If the server wedges: `herdr server stop`, then run `herdr` again.

## Zsh

```bash
chmod 600 ~/.zsh_history        # history not saving
chsh -s /usr/bin/zsh            # shell didn't change
rm ~/.zcompdump*; exec zsh      # stale completion cache
```

## System

```bash
sudo pacman -Sy                 # refresh db if installs fail
df -h /                         # disk space
systemctl --failed              # anything red
journalctl -p err -b            # errors this boot
```

If an update breaks the system on a btrfs root: reboot, open the GRUB
**Snapshots** submenu, and boot the pre-transaction snapshot. snap-pac
makes one around every pacman run.

## Recovery

### Reset configuration
```bash
cd ~/farfield-os
sudo ./scripts/setup.sh         # re-applies configs (backs up ~/.zshrc first)
```

### Emergency console access
1. `Ctrl+Alt+F2` for a spare TTY (tty1 belongs to the kiosk when a
   display is attached)
2. Log in, fix, switch back with `Ctrl+Alt+F1`
