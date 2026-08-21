#!/bin/bash
# Migration: shell refresh (2026-08-13) — strip Oh My Zsh + Spaceship,
# replace tmux with herdr. The new zsh config (Zinit + hand-rolled
# prompt, installed by setup.sh) doesn't read any of this; remove the
# leftovers so they don't linger as cruft. Idempotent.
set -euo pipefail

# Fail rather than skip: every step below is in the user's home, so a run
# without a resolvable user does nothing — and ff-migrate would then mark
# it converged forever, leaving OMZ in place with no second chance.
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

# Oh My Zsh + its plugin clones (the new config uses Zinit under
# ~/.local/share/zinit instead).
[ -d "$REAL_HOME/.oh-my-zsh" ] && rm -rf "$REAL_HOME/.oh-my-zsh"

# Stale completion dumps from the OMZ era — the new config rebuilds its
# own cached dump on first shell.
rm -f "$REAL_HOME"/.zcompdump*

# tmux: config out, package out. Any running server is killed first —
# sessions don't survive the package's removal anyway, and herdr is the
# session runtime now.
if [ -f "$REAL_HOME/.tmux.conf" ]; then
    mv "$REAL_HOME/.tmux.conf" "$REAL_HOME/.tmux.conf.ff-backup"
fi
sudo -u "$REAL_USER" tmux kill-server 2>/dev/null || true
if pacman -Qi tmux &>/dev/null; then
    pacman -Rns --noconfirm tmux 2>/dev/null || \
        echo "tmux removal skipped (dependency?) — remove manually" >&2
fi

echo "shell refresh migration complete"
