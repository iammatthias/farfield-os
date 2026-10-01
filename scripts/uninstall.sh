#!/bin/bash
#
# farfield os - Uninstall
# Reverts the configuration installed by setup.sh.
# Does NOT remove pacman packages — keep or remove those manually.
#

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

if [[ $EUID -ne 0 ]]; then
   echo -e "${RED}Run as root: sudo ./uninstall.sh${NC}"
   exit 1
fi

REAL_USER="${SUDO_USER:-$(logname 2>/dev/null || echo "")}"
if [ -z "$REAL_USER" ] || [ "$REAL_USER" = "root" ]; then
    echo -e "${RED}Could not determine the target user. Run as: sudo ./uninstall.sh${NC}"
    exit 1
fi
REAL_HOME="/home/$REAL_USER"
TS=$(date +%Y%m%d_%H%M%S)

# --deep also reverts the per-user language tooling setup.sh installed
# (npm globals, bun, rustup, uv tools, gem bundler). Off by default: those
# are shared with whatever else the user builds on this box, and removing
# them is not part of "revert the farfield configuration".
DEEP=0
case "${1:-}" in
    --deep) DEEP=1 ;;
    --help | -h)
        echo "usage: sudo ./uninstall.sh [--deep]"
        echo "  reverts farfield os configuration, keeping timestamped backups."
        echo "  --deep  also removes per-user tooling (npm globals, bun, rustup, uv tools)"
        exit 0
        ;;
esac

# Restore a file from its setup-time .ff-orig snapshot. The current state
# is moved aside as .ff-backup.<ts> so the user can audit the diff.
restore_orig() {
    local f=$1
    if [ -f "$f.ff-orig" ]; then
        [ -f "$f" ] && mv "$f" "$f.ff-backup.$TS"
        mv "$f.ff-orig" "$f"
        echo "Restored $f from setup-time snapshot"
    fi
}

echo -e "${YELLOW}Reverting farfield os configuration for $REAL_USER...${NC}"
echo

# -----------------------------------------------------------------------------
# Stop / disable services
# -----------------------------------------------------------------------------
# Tear down the container stack first — this needs dockerd still running.
# Leaves /srv/stack/data in place — that's user state (caddy certs).
# Move it aside if you want a clean wipe.
if [ -f /srv/stack/docker-compose.yml ]; then
    docker compose -f /srv/stack/docker-compose.yml down 2>/dev/null || true
fi

# docker is disabled here too (setup.sh enabled it); the package remains —
# the epilogue below already suggests removing it manually.
# tailscaled: the host tailnet node. Disabling stops host ssh over the
# tailnet; node state in /var/lib/tailscale is left alone (user identity —
# `tailscale logout` + package removal wipe it if wanted).
for svc in ff-stack ff-docker-prune.timer ff-firewall fail2ban valkey postgresql ufw docker tailscaled; do
    systemctl is-active --quiet "$svc" && systemctl stop "$svc" || true
    systemctl is-enabled --quiet "$svc" 2>/dev/null && systemctl disable "$svc" || true
done

rm -f /etc/systemd/system/ff-stack.service
rm -f /etc/systemd/system/ff-docker-prune.service /etc/systemd/system/ff-docker-prune.timer
rm -f /etc/systemd/system/ff-firewall.service
rm -f /etc/tmpfiles.d/farfield.conf
rm -f /etc/udev/rules.d/99-farfield-rapl.rules
rm -f /etc/systemd/journald.conf.d/farfield.conf

# Re-read the rule/config files we just deleted — without these the
# removed udev rules and journald limits stay live until the next boot,
# so an uninstall that "succeeded" hasn't actually reverted anything yet.
udevadm control --reload-rules 2>/dev/null || true
systemctl restart systemd-journald 2>/dev/null || true

# Drop the DOCKER-USER rules ff-firewall installed; without this they
# outlive the uninstall until docker is restarted.
if command -v iptables >/dev/null 2>&1; then
    while read -r n; do
        iptables -D DOCKER-USER "$n" 2>/dev/null || true
    done < <(iptables -L DOCKER-USER -n --line-numbers 2>/dev/null \
             | awk '/ff-managed/ {print $1}' | sort -rn)
fi

# wait-online drop-in (setup.sh's --any override)
if [ -d /etc/systemd/system/systemd-networkd-wait-online.service.d ]; then
    rm -f /etc/systemd/system/systemd-networkd-wait-online.service.d/ff-any.conf
    rmdir --ignore-fail-on-non-empty /etc/systemd/system/systemd-networkd-wait-online.service.d
fi

# Docker daemon.json: restore the pre-farfield os original if we snapshotted one,
# otherwise remove the file farfield os wrote.
if [ -f /etc/docker/daemon.json.ff-orig ]; then
    restore_orig /etc/docker/daemon.json
else
    rm -f /etc/docker/daemon.json
fi

# Reset UFW to default deny-all-allow-all (before disabling) so reinstall is clean
if command -v ufw &>/dev/null; then
    ufw --force reset >/dev/null || true
fi

# -----------------------------------------------------------------------------
# System config files
# -----------------------------------------------------------------------------
echo -e "${YELLOW}Removing system config...${NC}"

[ -f /etc/fail2ban/jail.local ] && \
    mv /etc/fail2ban/jail.local "/etc/fail2ban/jail.local.ff-backup.$TS"


# tty1 auto-login drop-in
if [ -d /etc/systemd/system/getty@tty1.service.d ]; then
    rm -f /etc/systemd/system/getty@tty1.service.d/autologin.conf
    rmdir --ignore-fail-on-non-empty /etc/systemd/system/getty@tty1.service.d
fi

# grub-btrfsd + snapper timers — disable but leave snapper config +
# snapshots alone (those are user data; the user can
# `snapper -c root delete-config` manually).
systemctl is-enabled --quiet grub-btrfsd 2>/dev/null && \
    systemctl disable --now grub-btrfsd >/dev/null 2>&1 || true
for t in snapper-timeline.timer snapper-cleanup.timer; do
    systemctl is-enabled --quiet "$t" 2>/dev/null && \
        systemctl disable --now "$t" >/dev/null 2>&1 || true
done

# Drop the agent-mode passwordless-sudo grant. Other sudoers config left alone.
rm -f "/etc/sudoers.d/ff-${REAL_USER}-nopasswd"

# SSH hardening drop-in (newer installs) plus the legacy sed/snapshot
# paths for boxes configured before the drop-in existed.
rm -f /etc/ssh/sshd_config.d/00-farfield.conf
if [ -f /etc/ssh/sshd_config.ff-orig ]; then
    restore_orig /etc/ssh/sshd_config
elif [ -f /etc/ssh/sshd_config ]; then
    sed -i 's/^PermitRootLogin no$/#PermitRootLogin prohibit-password/' /etc/ssh/sshd_config
    sed -i 's/^PasswordAuthentication no$/#PasswordAuthentication yes/' /etc/ssh/sshd_config
fi
systemctl reload sshd 2>/dev/null || true

restore_orig /etc/locale.gen
restore_orig /etc/locale.conf
# Regenerate against the restored locale.gen — otherwise locale-archive
# keeps serving the locales setup.sh added, which is not "reverted".
locale-gen &>/dev/null || true

# Remove user from the docker group (added by setup.sh).
if getent group docker >/dev/null 2>&1 && id -nG "$REAL_USER" 2>/dev/null | grep -qw docker; then
    gpasswd -d "$REAL_USER" docker >/dev/null || true
    echo "Removed $REAL_USER from docker group"
fi

# Restore login shell. We don't know the user's original shell, so we default
# to bash if zsh is currently set. If they've since changed it, leave it alone.
if [ "$(getent passwd "$REAL_USER" | cut -d: -f7)" = "/usr/bin/zsh" ] && [ -x /bin/bash ]; then
    chsh -s /bin/bash "$REAL_USER" 2>/dev/null && echo "Login shell restored to /bin/bash"
fi

systemctl daemon-reload

# -----------------------------------------------------------------------------
# User-level config (with backups)
# -----------------------------------------------------------------------------
echo -e "${YELLOW}Removing user config from $REAL_HOME...${NC}"

backup_and_remove() {
    local f=$1
    if [ -e "$f" ]; then
        mv "$f" "${f}.ff-backup.$TS"
    fi
}

backup_and_remove "$REAL_HOME/.zshrc"
backup_and_remove "$REAL_HOME/.zshenv"
backup_and_remove "$REAL_HOME/.zprofile"
backup_and_remove "$REAL_HOME/.tmux.conf"  # legacy — pre-herdr installs
backup_and_remove "$REAL_HOME/.config/fastfetch/config.jsonc"
backup_and_remove "$REAL_HOME/.config/sway/config"
backup_and_remove "$REAL_HOME/.config/foot/foot.ini"
backup_and_remove "$REAL_HOME/.config/btop/btop.conf"
rm -f "$REAL_HOME/.config/btop/themes/farfield.theme"

# Only back up CLAUDE.md if it's the one farfield os installed (sentinel: first
# line is "# farfield os server" — or "# GNAR Server" from pre-rename
# installs). Hand-written ones are left alone.
if [ -f "$REAL_HOME/CLAUDE.md" ] && head -n1 "$REAL_HOME/CLAUDE.md" | grep -qE "^# (farfield os server|GNAR Server)$"; then
    mv "$REAL_HOME/CLAUDE.md" "$REAL_HOME/CLAUDE.md.ff-backup.$TS"
fi

# Zinit + prompt (and legacy Oh My Zsh from pre-refresh installs)
rm -f "$REAL_HOME/.config/zsh/prompt.zsh"
sudo -u "$REAL_USER" rm -rf "$REAL_HOME/.local/share/zinit" "$REAL_HOME/.oh-my-zsh" || true

# Claude Code native binary (auth/state in ~/.claude is left alone —
# that's user data).
rm -f "$REAL_HOME/.local/bin/claude"

# The .gitconfig stub setup.sh writes, but only if it's still the stub —
# a real identity means the user edited it, and it's theirs now.
if [ -f "$REAL_HOME/.gitconfig" ] && \
   head -n1 "$REAL_HOME/.gitconfig" | grep -q "^# Edit user.name"; then
    backup_and_remove "$REAL_HOME/.gitconfig"
fi

# Per-user language tooling (--deep only).
if [ "$DEEP" = 1 ]; then
    echo -e "${YELLOW}--deep: removing per-user language tooling...${NC}"
    sudo -u "$REAL_USER" bash <<'DEEPEOF' || true
set -uo pipefail
# rustup owns its whole toolchain dir; -y so it doesn't prompt.
command -v rustup >/dev/null 2>&1 && rustup self uninstall -y
# uv-installed CLI tools (ruff/pytest/black) live under ~/.local/share/uv.
command -v uv >/dev/null 2>&1 && uv tool uninstall --all
rm -rf "$HOME/.bun" "$HOME/.npm-global" "$HOME/go/bin/dlv"
DEEPEOF
    echo "Removed rustup, uv tools, bun, npm globals."
fi

# herdr — stop the background server, then remove the user-level install.
sudo -u "$REAL_USER" "$REAL_HOME/.local/bin/herdr" server stop 2>/dev/null || true
rm -f "$REAL_HOME/.local/bin/herdr"
sudo -u "$REAL_USER" rm -rf "$REAL_HOME/.config/herdr" "$REAL_HOME/.local/share/herdr" || true

# Helper scripts — everything setup.sh installs to /usr/local/bin.
rm -f /usr/local/bin/ff-info /usr/local/bin/ff-update /usr/local/bin/ff-help \
      /usr/local/bin/ff-dashboard /usr/local/bin/ff-services-status \
      /usr/local/bin/ff-docker-status /usr/local/bin/ff-status-board \
      /usr/local/bin/ff-metrics-board /usr/local/bin/ff-kiosk-tiles \
      /usr/local/bin/ff-kiosk-restart /usr/local/bin/ff-kiosk-shot \
      /usr/local/bin/ff-claude-stats /usr/local/bin/ff-project-init \
      /usr/local/bin/ff-bootstrap \
      /usr/local/bin/ff-deploy /usr/local/bin/ff-board \
      /usr/local/bin/ff-kiosk-presence /usr/local/bin/ff-display \
      /usr/local/bin/ff-kiosk-wake-listener /usr/local/bin/ff-migrate \
      /usr/local/bin/ff-doctor /usr/local/bin/ff-firewall

# Migration markers (and the legacy gnar-deploy compat symlink, if present)
rm -rf /var/lib/farfield
rm -f /usr/local/bin/gnar-deploy

echo
echo -e "${GREEN}farfield os configuration removed.${NC}"
echo
echo "Backups: *.ff-backup.$TS"
echo
echo "Packages remain installed. To remove the farfield os package set:"
echo "  sudo pacman -Rns zsh neovim docker docker-compose tailscale \\"
echo "    nodejs npm python uv ruby go jdk-openjdk maven gradle \\"
echo "    ghostty-terminfo eza bat fd fzf zoxide ripgrep jq yq \\"
echo "    fastfetch htop btop iotop nethogs lsof ncdu \\"
echo "    tree bc rsync rclone 7zip imagemagick httpie \\"
echo "    mosh speedtest-cli ufw fail2ban nmap tcpdump wireshark-cli \\"
echo "    postgresql valkey sqlite smartmontools pacman-contrib arch-audit \\"
echo "    sway foot grim ttf-ibm-plex ttf-jetbrains-mono-nerd \\"
echo "    noto-fonts noto-fonts-emoji"
echo
echo "(Mirrors the pac groups in setup.sh. Review before running — some of"
echo " these are ordinary tools you may want regardless of farfield os.)"
echo
echo "Caddy state is preserved at /srv/stack/data/, and the tailnet"
echo "identity at /var/lib/tailscale/ — wipe manually (tailscale logout;"
echo "rm -r) if you want a clean slate."
echo
echo "NOTE: /srv/stack/.env still holds your Cloudflare tokens. It is kept"
echo "so a reinstall doesn't need them re-entered — shred it yourself if"
echo "this box is being handed on:  sudo shred -u /srv/stack/.env"
