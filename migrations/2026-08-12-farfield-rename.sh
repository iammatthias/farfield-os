#!/bin/bash
# Migration: gnar → farfield os rename (2026-08-12).
#
# Renames every on-box artifact the old bootstrap installed under gnar-*
# names. Idempotent: every step is guarded on the old artifact existing.
# Runs as root (via ff-migrate). Deliberately does NOT touch the
# /srv/stack compose project (container renames are a coordinated,
# stack-cycling change — see stack/README.md).
set -euo pipefail

# Fail rather than skip the user half: a run that converges only the root
# half still gets marked done, so the user's dotfiles would keep their
# gnar-era state permanently.
REAL_USER=${FF_REAL_USER:-${SUDO_USER:-$(logname 2>/dev/null || echo "")}}
if [ -z "$REAL_USER" ] || [ "$REAL_USER" = "root" ]; then
    echo "cannot determine target user (set FF_REAL_USER)" >&2
    exit 1
fi
REAL_HOME=$(getent passwd "$REAL_USER" | cut -d: -f6)
if [ -z "$REAL_HOME" ] || [ ! -d "$REAL_HOME" ]; then
    echo "no home directory for $REAL_USER" >&2
    exit 1
fi

# --- old helper binaries (new ones are installed by setup.sh) ---------------
for f in /usr/local/bin/gnar-*; do
    [ -e "$f" ] || continue
    case "$f" in
        # ff-deploy compat: external automation still calls gnar-deploy.
        # Replaced with a symlink below, not deleted.
        */gnar-deploy) continue ;;
    esac
    rm -f "$f"
done
if [ -x /usr/local/bin/ff-deploy ]; then
    ln -sf /usr/local/bin/ff-deploy /usr/local/bin/gnar-deploy
fi

# --- systemd units ----------------------------------------------------------
if [ -f /etc/systemd/system/gnar-stack.service ]; then
    # Don't stop it — that would take the ingress stack down. Just move
    # the boot enablement to the new unit (installed by setup.sh).
    systemctl disable gnar-stack.service 2>/dev/null || true
    rm -f /etc/systemd/system/gnar-stack.service
    systemctl daemon-reload
    systemctl enable ff-stack.service 2>/dev/null || true
fi
if [ -f /etc/systemd/system/gnar-docker-prune.timer ]; then
    systemctl disable --now gnar-docker-prune.timer 2>/dev/null || true
    rm -f /etc/systemd/system/gnar-docker-prune.timer /etc/systemd/system/gnar-docker-prune.service
    systemctl daemon-reload
    systemctl enable --now ff-docker-prune.timer 2>/dev/null || true
fi
systemctl reset-failed gnar-sysupgrade.service 2>/dev/null || true

# --- /etc drop-ins ----------------------------------------------------------
if [ -f /etc/systemd/journald.conf.d/gnar.conf ] && [ -f /etc/systemd/journald.conf.d/farfield.conf ]; then
    rm -f /etc/systemd/journald.conf.d/gnar.conf
fi
if [ -f /etc/tmpfiles.d/gnar.conf ] && [ -f /etc/tmpfiles.d/farfield.conf ]; then
    rm -f /etc/tmpfiles.d/gnar.conf
fi
if [ -f /etc/udev/rules.d/99-gnar-rapl.rules ] && [ -f /etc/udev/rules.d/99-farfield-rapl.rules ]; then
    rm -f /etc/udev/rules.d/99-gnar-rapl.rules
    udevadm control --reload-rules 2>/dev/null || true
fi

# sshd drop-in: setup.sh writes 00-farfield.conf; drop the old file only
# once the new one validates so a failed run can't leave sshd unhardened.
if [ -f /etc/ssh/sshd_config.d/00-gnar.conf ] && [ -f /etc/ssh/sshd_config.d/00-farfield.conf ]; then
    rm -f /etc/ssh/sshd_config.d/00-gnar.conf
    if sshd -t 2>/dev/null; then
        systemctl reload sshd 2>/dev/null || true
    else
        echo "sshd config invalid after rename — investigate before reloading" >&2
        exit 1
    fi
fi

# --- sudoers grant ----------------------------------------------------------
old_sudoers="/etc/sudoers.d/gnar-${REAL_USER}-nopasswd"
new_sudoers="/etc/sudoers.d/ff-${REAL_USER}-nopasswd"
if [ -f "$old_sudoers" ] && [ -f "$new_sudoers" ] && visudo -cq -f "$new_sudoers"; then
    rm -f "$old_sudoers"
fi

# --- setup-time snapshots (uninstall.sh now looks for .ff-orig) -------------
for f in /etc/locale.gen /etc/locale.conf /etc/ssh/sshd_config /etc/docker/daemon.json; do
    [ -f "$f.gnar-orig" ] && [ ! -f "$f.ff-orig" ] && mv "$f.gnar-orig" "$f.ff-orig"
done

# --- agent context: replace the machine-installed ~/CLAUDE.md ---------------
# setup.sh only installs it when absent; on a renamed box the old GNAR
# copy must be replaced so agents get current names. Sentinel-guarded —
# a hand-written CLAUDE.md is left alone.
if [ -f "$REAL_HOME/CLAUDE.md" ] && \
   head -n1 "$REAL_HOME/CLAUDE.md" | grep -q "^# GNAR Server$"; then
    src=""
    for cand in "$REAL_HOME/farfield-os/configs/server-CLAUDE.md" "$REAL_HOME/gnar/configs/server-CLAUDE.md"; do
        [ -f "$cand" ] && src=$cand && break
    done
    if [ -n "$src" ]; then
        install -m 644 -o "$REAL_USER" -g "$REAL_USER" "$src" "$REAL_HOME/CLAUDE.md"
    fi
fi

# --- pre-rename residue -----------------------------------------------------
[ -d "$REAL_HOME/.config/mango" ] && rm -rf "$REAL_HOME/.config/mango"
[ -f "$REAL_HOME/.z" ] && rm -f "$REAL_HOME/.z"   # zsh-z datafile; zoxide owns jumping now

echo "farfield rename migration complete"
