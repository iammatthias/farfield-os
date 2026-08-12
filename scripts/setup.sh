#!/bin/bash
#
# farfield os - Home Server Bootstrap for Arch
# Zsh (Zinit, hand-rolled prompt) + herdr + Caddy stack + runtimes
#

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

if [[ $EUID -ne 0 ]]; then
   echo -e "${RED}Run as root: sudo ./setup.sh${NC}"
   exit 1
fi

REAL_USER="${SUDO_USER:-$(logname 2>/dev/null || echo "")}"
if [ -z "$REAL_USER" ] || [ "$REAL_USER" = "root" ]; then
    echo -e "${RED}Could not determine the target user. Run as: sudo ./setup.sh (not as root directly).${NC}"
    exit 1
fi
REAL_HOME="/home/$REAL_USER"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIGS="$REPO_ROOT/configs"
BIN="$REPO_ROOT/bin"

# Track services that fail to start so we can warn at the end.
FAILED_SERVICES=()

# Snapshot files we modify so uninstall.sh can restore the original.
# Only created on first run — re-running setup mustn't clobber the original snapshot.
snapshot() {
    local f=$1
    [ -f "$f" ] && [ ! -f "$f.ff-orig" ] && cp -a "$f" "$f.ff-orig" || true
}
snapshot /etc/locale.gen
snapshot /etc/locale.conf
snapshot /etc/ssh/sshd_config
snapshot /etc/docker/daemon.json

echo -e "${GREEN}farfield os - Home Server Bootstrap${NC}"
echo

# On cloud images, first-boot cloud-init may still be running pacman in the
# background and hold /var/lib/pacman/db.lck. No-op on non-cloud Arch.
if command -v cloud-init &>/dev/null; then
    echo -e "${YELLOW}Waiting for cloud-init to settle...${NC}"
    cloud-init status --wait &>/dev/null || true
fi

# -----------------------------------------------------------------------------
# System packages
# -----------------------------------------------------------------------------
echo -e "${GREEN}Updating system...${NC}"
pacman -Syu --noconfirm

if ! grep -q "en_US.UTF-8 UTF-8" /etc/locale.gen 2>/dev/null; then
    echo "en_US.UTF-8 UTF-8" >> /etc/locale.gen
fi
locale-gen
echo "LANG=en_US.UTF-8" > /etc/locale.conf
export LANG=en_US.UTF-8
export LC_ALL=en_US.UTF-8

# Persistent journal — without /var/log/journal, journald falls back to
# RAM-only and logs vanish on reboot, which is hostile for a home server.
mkdir -p /var/log/journal
systemd-tmpfiles --create --prefix /var/log/journal &>/dev/null || true
systemctl kill --kill-whom=main -s USR1 systemd-journald &>/dev/null || true

echo -e "${GREEN}Installing core packages...${NC}"
# caddy + tailscale + cloudflared live in the /srv/stack docker compose,
# not on the host. Docker is the only network-layer thing we install
# directly.
pacman -S --noconfirm --needed \
  zsh neovim git curl wget unzip \
  docker docker-compose \
  nodejs npm \
  python uv \
  ruby \
  go jdk-openjdk maven gradle \
  base-devel man-db man-pages

echo -e "${GREEN}Installing development tools...${NC}"
# ghostty-terminfo: ssh clients on Ghostty send TERM=xterm-ghostty; without
# the entry, zle redraws garble every keystroke.
pacman -S --noconfirm --needed \
  ghostty-terminfo \
  eza bat fd fzf zoxide ripgrep jq yq \
  fastfetch htop btop iotop nethogs lsof ncdu \
  tree bc rsync rclone 7zip imagemagick httpie \
  net-tools openssh mosh speedtest-cli ufw fail2ban nmap tcpdump wireshark-cli \
  postgresql valkey sqlite smartmontools pacman-contrib arch-audit

echo -e "${GREEN}Installing display stack (sway kiosk dashboard)...${NC}"
# Wayland compositor + terminal + fonts for the optional attached-display
# dashboard. sway is used (not a minimal dwl fork) because it exposes
# wl_touch — required for the rack touch panel's tap interactions. Fonts:
# IBM Plex Mono is the brand's technical voice (foot.ini leads with it);
# JetBrainsMono Nerd supplies the box-drawing + icon + braille glyphs that
# btop and the shell prompt rely on; noto covers unicode
# fallback so foreign glyphs render instead of tofu. grim backs
# ff-kiosk-shot screenshots.
pacman -S --noconfirm --needed sway foot grim \
    ttf-ibm-plex ttf-jetbrains-mono-nerd \
    noto-fonts noto-fonts-emoji

# Re-run locale-gen post-install. pacman -Syu earlier may have replaced
# glibc; locale-archive needs to be regenerated against the new libraries
# or postgres rejects "en_US.UTF-8" at startup.
locale-gen &>/dev/null || true

# -----------------------------------------------------------------------------
# Btrfs + Snapper (only if root filesystem is btrfs)
# -----------------------------------------------------------------------------
ROOT_FS=$(stat -f -c %T / 2>/dev/null || echo unknown)
if [ "$ROOT_FS" = "btrfs" ]; then
    echo -e "${GREEN}Detected btrfs root — configuring Snapper...${NC}"

    # snap-pac auto-snapshots before/after every pacman transaction; once
    # it's installed, every subsequent pacman call below will snapshot.
    pacman -S --noconfirm --needed snapper snap-pac inotify-tools

    # grub-btrfs adds a "Snapshots" submenu to GRUB so you can boot into
    # any snapshot when an update breaks the system. Only meaningful on
    # GRUB; systemd-boot has no equivalent.
    if command -v grub-mkconfig &>/dev/null; then
        pacman -S --noconfirm --needed grub-btrfs
    else
        echo -e "${YELLOW}Not on GRUB — skipping grub-btrfs.${NC}"
        echo -e "${YELLOW}For boot-into-snapshot on systemd-boot, see https://wiki.archlinux.org/title/Snapper${NC}"
    fi

    # archinstall ships @.snapshots mounted at /.snapshots (per fstab).
    # snapper wants to own /.snapshots itself. Preserve the archinstall
    # subvolume but get snapper's config files in place.
    if [ ! -f /etc/snapper/configs/root ]; then
        if mountpoint -q /.snapshots; then
            umount /.snapshots
            rmdir /.snapshots 2>/dev/null || true
            snapper -c root create-config /
            # snapper just made a fresh subvol at /.snapshots; ditch it
            # and re-mount archinstall's @.snapshots in its place.
            btrfs subvolume delete /.snapshots 2>/dev/null || true
            mkdir -p /.snapshots
            mount -a
        else
            snapper -c root create-config /
        fi

        snapper -c root set-config "TIMELINE_LIMIT_HOURLY=5"
        snapper -c root set-config "TIMELINE_LIMIT_DAILY=7"
        snapper -c root set-config "TIMELINE_LIMIT_WEEKLY=2"
        snapper -c root set-config "TIMELINE_LIMIT_MONTHLY=1"
        snapper -c root set-config "TIMELINE_LIMIT_YEARLY=0"
        # snapper's default NUMBER_LIMIT=50 keeps ~all pacman pre/post
        # pairs for months. On a box with docker churn each snapshot pins
        # the image layers of its era — observed ~160G held by stale
        # snapshots. A dozen transactions of rollback depth is plenty.
        snapper -c root set-config "NUMBER_LIMIT=12"
        snapper -c root set-config "NUMBER_LIMIT_IMPORTANT=6"
        snapper -c root set-config "ALLOW_GROUPS=wheel"

        chmod 750 /.snapshots 2>/dev/null || true
        chgrp wheel /.snapshots 2>/dev/null || true
    fi

    # /var/lib/docker as its own subvolume — snapper snapshots of the
    # root subvolume then EXCLUDE container storage. Without this every
    # pacman pre/post snapshot pins the docker image layers of its era
    # and the disk silently fills. Only safe to create while empty
    # (i.e. before dockerd first starts).
    if [ ! -e /var/lib/docker ] || [ -z "$(ls -A /var/lib/docker 2>/dev/null)" ]; then
        rmdir /var/lib/docker 2>/dev/null || true
        btrfs subvolume create /var/lib/docker 2>/dev/null || mkdir -p /var/lib/docker
    fi

    # Disable CoW on dirs with lots of small random writes (databases,
    # container storage). chattr +C only takes effect on NEW files, so do
    # this BEFORE postgres initdb / dockerd populates them.
    for _dir in /var/lib/postgres /var/lib/valkey /var/lib/docker; do
        [ -d "$_dir" ] || mkdir -p "$_dir"
        chattr +C "$_dir" 2>/dev/null || true
    done

    systemctl enable --now snapper-timeline.timer 2>/dev/null || true
    systemctl enable --now snapper-cleanup.timer  2>/dev/null || true
    if command -v grub-mkconfig &>/dev/null; then
        systemctl enable --now grub-btrfsd 2>/dev/null || true
    fi
fi

# -----------------------------------------------------------------------------
# Zsh — Zinit + hand-rolled prompt (mirrors the Mac dotfiles; no framework)
# -----------------------------------------------------------------------------
echo -e "${GREEN}Configuring zsh...${NC}"

# Back up an existing .zshrc, but only if it differs from what we're about
# to install — re-runs shouldn't spam identical .ff-backup files.
if [[ -f "$REAL_HOME/.zshrc" ]] && ! cmp -s "$CONFIGS/zshrc" "$REAL_HOME/.zshrc"; then
    cp "$REAL_HOME/.zshrc" "$REAL_HOME/.zshrc.ff-backup.$(date +%Y%m%d_%H%M%S)" || true
fi

# Pre-clone Zinit so the first interactive shell doesn't pause to
# bootstrap it. The zshrc self-installs it anyway — this just front-loads
# the network fetch. Tolerate flakes; a GitHub blip must not abort setup.
sudo -u "$REAL_USER" bash <<EOF || FAILED_SERVICES+=("zinit")
set -e
export HOME="$REAL_HOME"
ZINIT_HOME="\${XDG_DATA_HOME:-\$HOME/.local/share}/zinit/zinit.git"
if [ ! -f "\$ZINIT_HOME/zinit.zsh" ]; then
    mkdir -p "\$(dirname "\$ZINIT_HOME")"
    git clone https://github.com/zdharma-continuum/zinit "\$ZINIT_HOME"
fi
EOF

# herdr — persistent agent sessions (replaces tmux). Official installer,
# user-level (~/.local/bin/herdr). Guarded + tolerated like bun/rustup.
if ! sudo -u "$REAL_USER" bash -c 'command -v herdr || [ -x "$HOME/.local/bin/herdr" ]' >/dev/null 2>&1; then
    sudo -u "$REAL_USER" bash -c 'curl -fsSL https://herdr.dev/install.sh | sh' \
        || FAILED_SERVICES+=("herdr")
fi

install -m 644 -o "$REAL_USER" -g "$REAL_USER" "$CONFIGS/zshrc"  "$REAL_HOME/.zshrc"
install -m 644 -o "$REAL_USER" -g "$REAL_USER" "$CONFIGS/zshenv" "$REAL_HOME/.zshenv"
install -d -o "$REAL_USER" -g "$REAL_USER" "$REAL_HOME/.config/zsh"
install -m 644 -o "$REAL_USER" -g "$REAL_USER" \
    "$CONFIGS/zsh-prompt.zsh" "$REAL_HOME/.config/zsh/prompt.zsh"

install -d -o "$REAL_USER" -g "$REAL_USER" "$REAL_HOME/.config/fastfetch"
install -m 644 -o "$REAL_USER" -g "$REAL_USER" \
    "$CONFIGS/fastfetch.jsonc" "$REAL_HOME/.config/fastfetch/config.jsonc"

# Drop a CLAUDE.md at $HOME so Claude Code (and the user) sees what's
# installed. Don't clobber an existing one.
if [ ! -e "$REAL_HOME/CLAUDE.md" ]; then
    install -m 644 -o "$REAL_USER" -g "$REAL_USER" \
        "$CONFIGS/server-CLAUDE.md" "$REAL_HOME/CLAUDE.md"
fi

# -----------------------------------------------------------------------------
# Docker
# -----------------------------------------------------------------------------
echo -e "${GREEN}Configuring Docker...${NC}"
# Give every container working public DNS. The Arch host runs
# systemd-resolved which points /etc/resolv.conf at 127.0.0.53 — that
# doesn't translate inside containers, leaving them unable to resolve
# api.anthropic.com etc. Set explicit upstream resolvers at the daemon
# level so every container inherits them.
install -d /etc/docker
cat > /etc/docker/daemon.json <<'EOF'
{
  "dns": ["1.1.1.1", "1.0.0.1", "8.8.8.8"]
}
EOF
systemctl enable docker
# pacman -Syu earlier may have upgraded the kernel; iptables modules in the
# running kernel won't match until reboot. Don't let that abort the bootstrap.
if ! systemctl start docker; then
    journalctl -xeu docker.service --no-pager -n 10 || true
    FAILED_SERVICES+=("docker")
fi
usermod -aG docker "$REAL_USER"
echo -e "${YELLOW}Note: log out and back in for docker group membership.${NC}"

# -----------------------------------------------------------------------------
# Firewall rules + fail2ban + SSH
# -----------------------------------------------------------------------------
echo -e "${GREEN}Configuring firewall rules + fail2ban...${NC}"
# Configure UFW rules now, but DO NOT enable yet. If pacman -Syu upgraded the
# kernel earlier in this run, the running kernel is missing iptables modules
# (xt_addrtype, conntrack, etc.). UFW would half-apply, leaving iptables in a
# fail-closed state that blocks outbound DNS — which then breaks every
# downstream AUR/curl install. We enable UFW at the very end of the script,
# AFTER all network-dependent installs are done.
ufw default deny incoming || true
ufw default allow outgoing || true

# Detect the actual sshd port instead of trusting `ufw allow ssh` (which
# only opens 22). Critical when running setup.sh over SSH on a remote box
# with a non-default port — wrong rule = locked out, need physical access.
SSH_PORTS=$(awk '/^[[:space:]]*Port[[:space:]]+[0-9]+/ {print $2}' /etc/ssh/sshd_config 2>/dev/null)
[ -z "$SSH_PORTS" ] && SSH_PORTS=22
for _port in $SSH_PORTS; do
    ufw allow "$_port/tcp" || true
    echo "  ufw: opened sshd port $_port/tcp"
done
ufw allow 80/tcp || true
ufw allow 443/tcp || true
# Kiosk wake push — LAN UDP nudge from the CV box (ff-kiosk-wake-listener).
# Worst a spoofed datagram can do is turn the display on.
ufw allow 8666/udp || true
# Containers must reach host-native services (the caddy container proxies
# add-site/pm2 apps via host.docker.internal). That traffic arrives on a
# docker bridge and traverses INPUT — Docker only manages FORWARD — so
# default-deny silently drops it. Docker's default address pools all live
# in 172.16.0.0/12.
ufw allow from 172.16.0.0/12 comment 'docker containers -> host services' || true

install -m 644 "$CONFIGS/fail2ban-jail.local" /etc/fail2ban/jail.local
# Match the jail to the real sshd port(s) detected above — the template's
# `port = ssh` only covers 22, which is wrong on a non-default-port box.
F2B_PORTS=$(printf '%s' "$SSH_PORTS" | tr -s ' \n' ',')
sed -i "s/^port = ssh$/port = $F2B_PORTS/" /etc/fail2ban/jail.local
systemctl enable fail2ban
if ! systemctl start fail2ban; then
    journalctl -xeu fail2ban.service --no-pager -n 10 || true
    FAILED_SERVICES+=("fail2ban")
fi

# SSH hardening — a lexically-first drop-in, not sed on the main config.
# Arch's sshd_config `Include`s sshd_config.d/*.conf at the TOP and sshd is
# first-value-wins, so editing the main file loses to any drop-in (notably
# cloud-init's 50-cloud-init.conf re-enabling password auth). 00- sorts
# ahead of everything, so these values actually take effect.
install -d /etc/ssh/sshd_config.d
grep -q '^Include /etc/ssh/sshd_config\.d/\*\.conf' /etc/ssh/sshd_config 2>/dev/null || \
    sed -i '1i Include /etc/ssh/sshd_config.d/*.conf' /etc/ssh/sshd_config
{
    echo "# farfield os SSH hardening — first-match-wins, keep this file 00-first."
    echo "PermitRootLogin no"
    echo "PubkeyAuthentication yes"
    # Only disable password auth if the user has authorized keys.
    if [ -s "$REAL_HOME/.ssh/authorized_keys" ]; then
        echo "PasswordAuthentication no"
    fi
} > /etc/ssh/sshd_config.d/00-farfield.conf
if [ -s "$REAL_HOME/.ssh/authorized_keys" ]; then
    echo "SSH password auth disabled (authorized_keys present)"
else
    echo -e "${YELLOW}No authorized_keys for $REAL_USER; leaving PasswordAuthentication unchanged.${NC}"
    echo -e "${YELLOW}Add your key with: ssh-copy-id $REAL_USER@<host>${NC}"
fi
# Never reload into a broken config — that's how you lose the box.
if ! sshd -t 2>/dev/null; then
    echo -e "${RED}sshd config validation failed — removing farfield os drop-in${NC}"
    rm -f /etc/ssh/sshd_config.d/00-farfield.conf
fi
# Reload — not restart — so the connection running this script doesn't get dropped.
# sshd re-reads its config on SIGHUP; no need to bounce the daemon.
systemctl reload sshd || systemctl restart sshd || true

# -----------------------------------------------------------------------------
# Databases
# -----------------------------------------------------------------------------
echo -e "${GREEN}Configuring PostgreSQL + Valkey...${NC}"
if [ ! -d "/var/lib/postgres/data" ] || [ -z "$(ls -A /var/lib/postgres/data 2>/dev/null)" ]; then
    # Use C.UTF-8 — always available regardless of glibc state, no
    # locale-archive dependency. (en_US.UTF-8 fails to start post-reboot
    # when pacman upgraded glibc earlier in this same script run, because
    # the locale-archive needs to be regenerated by the new glibc.)
    sudo -u postgres initdb -D /var/lib/postgres/data --locale=C.UTF-8 --encoding=UTF8
fi

systemctl enable postgresql
if ! systemctl start postgresql; then
    journalctl -xeu postgresql.service --no-pager -n 10 || true
    FAILED_SERVICES+=("postgresql")
fi

if systemctl is-active --quiet postgresql; then
    if ! sudo -u postgres psql -tAc "SELECT 1 FROM pg_roles WHERE rolname='$REAL_USER'" | grep -q 1; then
        sudo -u postgres createuser -s "$REAL_USER" || true
        sudo -u postgres createdb "$REAL_USER" || true
    fi
fi

systemctl enable valkey
if ! systemctl start valkey; then
    journalctl -xeu valkey.service --no-pager -n 10 || true
    FAILED_SERVICES+=("valkey")
fi

# tmpfiles rules (RAPL counters readable for ff-board power display).
install -m 644 "$CONFIGS/tmpfiles-farfield.conf" /etc/tmpfiles.d/farfield.conf
systemd-tmpfiles --create /etc/tmpfiles.d/farfield.conf 2>/dev/null || true

# RAPL perms, deterministically: at boot the tmpfiles pass races the
# intel_rapl module load, and when it loses ff-board's wattage silently
# disappears. A udev rule fires when the powercap device appears; the
# tmpfiles rule above remains as a fallback for the already-booted case.
install -m 644 "$CONFIGS/udev-rapl.rules" /etc/udev/rules.d/99-farfield-rapl.rules
udevadm control --reload-rules 2>/dev/null || true

# Cap the persistent journal — unbounded, it quietly eats gigabytes.
install -d /etc/systemd/journald.conf.d
install -m 644 "$CONFIGS/journald-farfield.conf" /etc/systemd/journald.conf.d/farfield.conf
systemctl restart systemd-journald 2>/dev/null || true

# Single-uplink box (often wifi): wait-online should be satisfied by ANY
# online interface, or an unplugged ethernet port fails the unit every boot.
install -d /etc/systemd/system/systemd-networkd-wait-online.service.d
cat > /etc/systemd/system/systemd-networkd-wait-online.service.d/ff-any.conf <<'EOF'
[Service]
ExecStart=
ExecStart=/usr/lib/systemd/systemd-networkd-wait-online --any
EOF

# Passwordless sudo for the user. The box is managed non-interactively —
# Claude Code over SSH, the kiosk's touch action buttons (update / reboot /
# prune run `sudo -n`), and ff-board's samplers (journalctl, smartctl,
# fail2ban-client) — none of which can answer a password prompt. The auth
# surface for "root-on-this-box" is already the user's SSH key (a compromise
# implies full host access), so this doesn't materially widen the threat
# model on a single-tenant server.
#
# This must happen BEFORE the yay build below: makepkg runs `sudo pacman -U`
# as $REAL_USER, whose sudo timestamp expired long ago behind pacman -Syu —
# without the grant, the "unattended" bootstrap hangs on a password prompt.
SUDOERS_FILE=/etc/sudoers.d/ff-${REAL_USER}-nopasswd
echo "$REAL_USER ALL=(ALL) NOPASSWD: ALL" > "$SUDOERS_FILE"
chmod 440 "$SUDOERS_FILE"
# Validate only the file we wrote — `visudo -c` checks ALL sudoers files,
# and an unrelated broken drop-in would make us delete our valid grant.
visudo -cq -f "$SUDOERS_FILE" || { echo -e "${RED}sudoers syntax error — removing $SUDOERS_FILE${NC}"; rm -f "$SUDOERS_FILE"; }

# -----------------------------------------------------------------------------
# yay (AUR helper)
# -----------------------------------------------------------------------------
install_yay() {
    sudo -u "$REAL_USER" bash <<'EOF'
set -e
cd /tmp
rm -rf yay
git clone https://aur.archlinux.org/yay.git
cd yay
makepkg -si --noconfirm
cd /tmp && rm -rf yay
EOF
}
if ! command -v yay &>/dev/null; then
    echo -e "${GREEN}Installing yay...${NC}"
    # AUR builds + git clone over the network can flake. One retry, then tolerate.
    if ! install_yay; then
        echo -e "${YELLOW}yay install failed; retrying once...${NC}"
        sleep 5
        install_yay || FAILED_SERVICES+=("yay")
    fi
fi

# The kiosk compositor is sway (installed via pacman with the display stack
# above). It's used instead of a minimal dwl-style WM because it exposes
# wl_touch — the rack touch panel's taps only reach the dashboard under a
# compositor that delivers touch to clients.

# -----------------------------------------------------------------------------
# Container stack (tailscale + caddy + cloudflared — see /srv/stack/README)
# -----------------------------------------------------------------------------
# The network-ingress layer runs as a docker-compose stack out of
# /srv/stack. Updating the stack is `git pull && docker compose up -d
# --build` — atomic, easy rollback, isolated from the host.
echo -e "${GREEN}Deploying container stack to /srv/stack...${NC}"
install -d -o "$REAL_USER" -g "$REAL_USER" /srv/stack
# The live Caddyfile accumulates add-site / add-public-site blocks at
# runtime — clobbering it with the repo boilerplate on a re-run would
# silently drop every configured site. Preserve it if it exists.
if [ -f /srv/stack/Caddyfile ]; then
    rsync -a --exclude Caddyfile "$REPO_ROOT/stack/." /srv/stack/
else
    cp -r "$REPO_ROOT/stack/." /srv/stack/
fi
chown -R "$REAL_USER:$REAL_USER" /srv/stack

# .env is sensitive (TS_AUTHKEY) — start from .env.example if not present.
if [ ! -f /srv/stack/.env ]; then
    cp /srv/stack/.env.example /srv/stack/.env
    chmod 600 /srv/stack/.env
    chown "$REAL_USER:$REAL_USER" /srv/stack/.env
fi

# Bind-mount target dirs (created with the right ownership before
# docker auto-creates them with root).
install -d -o "$REAL_USER" -g "$REAL_USER" \
    /srv/stack/data \
    /srv/stack/data/tailscale \
    /srv/stack/data/caddy \
    /srv/stack/data/caddy/data \
    /srv/stack/data/caddy/config

# ~/.gitconfig stub so git works out of the box. Edit to your identity.
if [ ! -f "$REAL_HOME/.gitconfig" ]; then
    cat > "$REAL_HOME/.gitconfig" <<EOF
# Edit user.name + user.email to your identity.
[user]
    name = $REAL_USER
    email = $REAL_USER@$(hostname)
[init]
    defaultBranch = main
EOF
    chown "$REAL_USER:$REAL_USER" "$REAL_HOME/.gitconfig"
fi

# systemd unit that runs `docker compose up -d --build` at boot.
install -m 644 "$CONFIGS/ff-stack.service" /etc/systemd/system/ff-stack.service

# Weekly prune of dangling images + old stopped containers — every
# `--build` strands the previous image as untagged layers, which
# otherwise accumulate unbounded.
install -m 644 "$CONFIGS/ff-docker-prune.service" /etc/systemd/system/ff-docker-prune.service
install -m 644 "$CONFIGS/ff-docker-prune.timer" /etc/systemd/system/ff-docker-prune.timer

# (The passwordless-sudo grant moved up before the yay build, which
# needs it — see the AUR helper section.)

systemctl daemon-reload
systemctl enable ff-stack.service
systemctl enable --now ff-docker-prune.timer

# -----------------------------------------------------------------------------
# Per-user runtime tooling
# -----------------------------------------------------------------------------
echo -e "${GREEN}Installing per-user tooling (npm, bun, python, ruby, rust, go)...${NC}"

# Whole heredoc is wrapped in `|| true` — every step has its own `|| true`
# inside, but the heredoc as a whole shouldn't be allowed to abort the
# bootstrap if (e.g.) the rustup curl-installer hits a network blip.
sudo -u "$REAL_USER" bash <<'EOF' || true
# Don't `set -e` — each command guards itself, and any single failure
# (npm registry hiccup, AUR mirror flake, rustup curl glitch) shouldn't
# stop the rest of the per-user tooling from being installed.
mkdir -p "$HOME/.npm-global"
npm config set prefix "$HOME/.npm-global"
export PATH="$HOME/.npm-global/bin:$PATH"
npm install -g yarn pnpm pm2 eslint prettier jest @anthropic-ai/claude-code || true

# Bun
curl -fsSL https://bun.sh/install | bash || true

# Python (uv)
# uv replaces pip / pipx / pipenv / poetry. uv tool installs land in ~/.local/bin.
uv tool install ruff || true
uv tool install pytest || true
uv tool install black || true

# Ruby
gem install bundler || true

# Rust (rustup)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y || true
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
command -v rustup &>/dev/null && rustup default stable || true

# Go
export GOPATH="$HOME/go"
mkdir -p "$GOPATH/bin"
go install github.com/go-delve/delve/cmd/dlv@latest || true
EOF

# -----------------------------------------------------------------------------
# Helper scripts
# -----------------------------------------------------------------------------
echo -e "${GREEN}Installing helper scripts...${NC}"
install -m 755 "$BIN/ff-info"             /usr/local/bin/ff-info
install -m 755 "$BIN/ff-update"           /usr/local/bin/ff-update
install -m 755 "$BIN/ff-help"             /usr/local/bin/ff-help
install -m 755 "$BIN/ff-dashboard"        /usr/local/bin/ff-dashboard
install -m 755 "$BIN/ff-services-status"  /usr/local/bin/ff-services-status
install -m 755 "$BIN/ff-docker-status"    /usr/local/bin/ff-docker-status
install -m 755 "$BIN/ff-status-board"     /usr/local/bin/ff-status-board
install -m 755 "$BIN/ff-metrics-board"    /usr/local/bin/ff-metrics-board
install -m 755 "$BIN/ff-kiosk-tiles"      /usr/local/bin/ff-kiosk-tiles
install -m 755 "$BIN/ff-kiosk-restart"    /usr/local/bin/ff-kiosk-restart
install -m 755 "$BIN/ff-kiosk-shot"       /usr/local/bin/ff-kiosk-shot
install -m 755 "$BIN/ff-kiosk-presence"   /usr/local/bin/ff-kiosk-presence
install -m 755 "$BIN/ff-kiosk-wake-listener" /usr/local/bin/ff-kiosk-wake-listener
install -m 755 "$BIN/ff-display"          /usr/local/bin/ff-display
install -m 755 "$BIN/ff-claude-stats"     /usr/local/bin/ff-claude-stats
install -m 755 "$BIN/ff-project-init"     /usr/local/bin/ff-project-init
install -m 755 "$BIN/ff-bootstrap"        /usr/local/bin/ff-bootstrap
install -m 755 "$BIN/ff-deploy"           /usr/local/bin/ff-deploy
install -m 755 "$BIN/ff-migrate"          /usr/local/bin/ff-migrate

# One-time migrations (migrations/*.sh, marker-tracked, idempotent) —
# converges upgraded boxes with fresh installs.
FF_REPO="$REPO_ROOT" FF_REAL_USER="$REAL_USER" bash "$BIN/ff-migrate" || \
    echo -e "${YELLOW}migrations incomplete — re-run ff-migrate after fixing${NC}"

# ff-board — the ratatui kiosk TUI (flicker-free fullscreen board).
# Built from source as $REAL_USER (cargo via rustup, installed above).
# Failure tolerated: ff-dashboard falls back to btop + shell boards.
if sudo -u "$REAL_USER" bash -c '. "$HOME/.cargo/env" 2>/dev/null; command -v cargo' &>/dev/null; then
    echo -e "${GREEN}Building ff-board (kiosk TUI)...${NC}"
    if sudo -u "$REAL_USER" bash -c ". \"\$HOME/.cargo/env\" 2>/dev/null; cargo build --release --manifest-path '$REPO_ROOT/board/Cargo.toml'"; then
        install -m 755 "$REPO_ROOT/board/target/release/ff-board" /usr/local/bin/ff-board
    else
        echo -e "${YELLOW}ff-board build failed — kiosk will use the shell boards${NC}"
    fi
else
    echo -e "${YELLOW}cargo not found — skipping ff-board build${NC}"
fi

# Default project roots. /srv/projects is owned by the user so
# `ff-project-init` doesn't need sudo; ~/projects is what ff-deploy,
# create-react-hono, and the cd aliases assume exists.
install -d -o "$REAL_USER" -g "$REAL_USER" /srv/projects
install -d -o "$REAL_USER" -g "$REAL_USER" "$REAL_HOME/projects"

# -----------------------------------------------------------------------------
# Kiosk dashboard (sway on tty1 when a display is attached)
# -----------------------------------------------------------------------------
echo -e "${GREEN}Configuring kiosk dashboard (auto-login + sway on tty1)...${NC}"
install -d -o "$REAL_USER" -g "$REAL_USER" "$REAL_HOME/.config/sway"
install -m 644 -o "$REAL_USER" -g "$REAL_USER" \
    "$CONFIGS/sway-config" "$REAL_HOME/.config/sway/config"
install -d -o "$REAL_USER" -g "$REAL_USER" "$REAL_HOME/.config/foot"
install -m 644 -o "$REAL_USER" -g "$REAL_USER" \
    "$CONFIGS/foot.ini" "$REAL_HOME/.config/foot/foot.ini"
install -d -o "$REAL_USER" -g "$REAL_USER" "$REAL_HOME/.config/btop" "$REAL_HOME/.config/btop/themes"
install -m 644 -o "$REAL_USER" -g "$REAL_USER" \
    "$CONFIGS/btop.conf" "$REAL_HOME/.config/btop/btop.conf"
install -m 644 -o "$REAL_USER" -g "$REAL_USER" \
    "$CONFIGS/btop-farfield.theme" "$REAL_HOME/.config/btop/themes/farfield.theme"
install -m 644 -o "$REAL_USER" -g "$REAL_USER" \
    "$CONFIGS/zprofile" "$REAL_HOME/.zprofile"

# Auto-login on tty1 only — other TTYs still prompt for a password.
install -d /etc/systemd/system/getty@tty1.service.d
sed "s|__USER__|$REAL_USER|g" "$CONFIGS/getty-autologin.conf" \
    > /etc/systemd/system/getty@tty1.service.d/autologin.conf
chmod 644 /etc/systemd/system/getty@tty1.service.d/autologin.conf
systemctl daemon-reload
systemctl enable getty@tty1.service &>/dev/null || true

# Default shell
chsh -s /usr/bin/zsh "$REAL_USER" || \
    echo -e "${YELLOW}Could not change shell; run: chsh -s /usr/bin/zsh${NC}"

# -----------------------------------------------------------------------------
# Enable UFW (last — see comment in firewall rules section above for why).
# -----------------------------------------------------------------------------
echo -e "${GREEN}Enabling firewall...${NC}"
# Always enable the systemd unit so ufw will start on boot regardless of
# whether the immediate `ufw enable` succeeds. If it fails now (kernel module
# mismatch from pacman -Syu), the unit will retry post-reboot when modules
# match — and at that point everything we configured will Just Work.
systemctl enable ufw &>/dev/null || true
if ! ufw --force enable; then
    echo -e "${YELLOW}UFW enable failed now (running kernel missing iptables modules).${NC}"
    echo -e "${YELLOW}Marked enabled in /etc/ufw/ufw.conf — will activate on next boot.${NC}"
    # ufw refused to flip ENABLED=yes because iptables-restore failed; do it
    # manually so /usr/lib/ufw/ufw-init brings it up cleanly post-reboot.
    sed -i 's/^ENABLED=.*/ENABLED=yes/' /etc/ufw/ufw.conf 2>/dev/null || true
    FAILED_SERVICES+=("ufw")
else
    systemctl start ufw || true
fi

# -----------------------------------------------------------------------------
# Status
# -----------------------------------------------------------------------------
echo
echo -e "${GREEN}=== Service status ===${NC}"
for svc in docker postgresql valkey fail2ban ufw ff-stack; do
    if systemctl is-active --quiet "$svc"; then
        echo "  [+] $svc"
    else
        echo "  [-] $svc"
    fi
done

echo
echo -e "${GREEN}=== Setup complete ===${NC}"
echo
echo
if [ ${#FAILED_SERVICES[@]} -gt 0 ]; then
    echo -e "${YELLOW}Some services did not start cleanly:${NC} ${FAILED_SERVICES[*]}"
    echo -e "${YELLOW}This is usually because pacman -Syu upgraded the kernel — reboot will resolve it.${NC}"
    echo
fi
echo "Next steps:"
echo "  1. sudo reboot"
echo "  2. ssh in, run: herdr"
echo "  3. add-site myapp 3000   # reverse proxy a service"
echo "  4. ff-help             # full reference"
echo
echo "After this reboot, run:"
echo
echo "  ff-bootstrap"
echo
echo "It walks tailscale auth → claude login → optional gh + cloudflared."
echo "Idempotent — re-run any time and it skips steps already done."
echo
echo "Day-to-day management is Claude Code over SSH: ssh in, run \`claude\`."
if [ "$ROOT_FS" = "btrfs" ]; then
    echo
    echo "Btrfs detected — Snapper is enabled. Useful commands:"
    echo "  snapper -c root list          # list snapshots"
    echo "  snapper -c root create        # manual snapshot"
    echo "  snapper -c root undochange N..M  # selectively roll back files"
    echo "  (snap-pac auto-snapshots before/after each pacman transaction)"
fi
