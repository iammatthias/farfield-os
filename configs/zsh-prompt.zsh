# ============================================================================
#  prompt.zsh — hand-rolled, zsh-native prompt (no external prompt binary)
#  ⊙ host directory git-branch/status ➜   (right side: cmd duration ≥ 2s, clock)
#  Mirrors the Mac dotfiles prompt; the ⊙ glyph + dim hostname are the server
#  deviation, so an ssh session is unmistakably homelab at a glance.
#  Colors: Teal #5f8787, Peach #fbcb97, Orange #e78a53, Grey #888888
#  Everything here is plain zsh — override any of it from ~/.zshrc.local,
#  which is sourced after this file.
# ============================================================================

setopt PROMPT_SUBST
zmodload zsh/datetime
autoload -Uz vcs_info add-zsh-hook

# --- git segment ------------------------------------------------------------
zstyle ':vcs_info:*' enable git
zstyle ':vcs_info:git:*' check-for-changes true
zstyle ':vcs_info:git:*' unstagedstr '*'
zstyle ':vcs_info:git:*' stagedstr '+'
zstyle ':vcs_info:git:*' formats       ' %F{#888888}%B %b%%b%f%F{#e78a53}%B%m%u%c%%b%f'
zstyle ':vcs_info:git:*' actionformats ' %F{#888888}%B %b(%a)%%b%f%F{#e78a53}%B%m%u%c%%b%f'
zstyle ':vcs_info:git+set-message:*' hooks git-escape git-untracked

# Branch/action names end up prompt-expanded (PROMPT_SUBST) — neutralize any
# '%' they contain so a branch like "x%F{red}y" can't inject escapes
+vi-git-escape() {
    hook_com[branch]=${hook_com[branch]//\%/%%}
    hook_com[action]=${hook_com[action]//\%/%%}
}

# vcs_info doesn't track untracked files natively — add '?' when present
+vi-git-untracked() {
    if [[ -n $(git ls-files --others --exclude-standard 2>/dev/null | head -1) ]]; then
        hook_com[misc]+='?'
    fi
}

# --- hooks: blank line, command duration, read-only dir ----------------------
typeset -g _prompt_cmd_start= _prompt_duration= _prompt_ro= _prompt_ran_once=

_prompt_preexec() { _prompt_cmd_start=$EPOCHREALTIME }

_prompt_precmd() {
    # blank line between prompts (but not before the first)
    if [[ -n $_prompt_ran_once ]]; then print; else _prompt_ran_once=1; fi

    _prompt_duration=
    if [[ -n $_prompt_cmd_start ]]; then
        local -F raw=$(( EPOCHREALTIME - _prompt_cmd_start ))
        local -i s=$(( raw + 0.5 ))  # rounded for display; gate on the raw time
        if (( raw >= 2 )); then
            if (( s >= 60 )); then
                _prompt_duration="%F{#e78a53}%Btook $((s / 60))m $((s % 60))s%b%f"
            else
                _prompt_duration="%F{#e78a53}%Btook ${s}s%b%f"
            fi
        fi
        _prompt_cmd_start=
    fi

    _prompt_ro=
    [[ -w $PWD ]] || _prompt_ro=' 🔒'

    vcs_info
}

add-zsh-hook preexec _prompt_preexec
add-zsh-hook precmd  _prompt_precmd

# --- assembly ----------------------------------------------------------------
# ⊙ signal glyph + dim host, then directory (last 3 components, '…/' when
# truncated), git, status-colored arrow.
PROMPT='%F{#e78a53}%B⊙ %b%f%F{#888888}%m%f %F{#5f8787}%B%(4~|…/%3~|%~)%b%f${_prompt_ro}${vcs_info_msg_0_} %(?.%F{#fbcb97}.%F{#e78a53})%B➜%b%f '
RPROMPT='${_prompt_duration:+${_prompt_duration} }%F{#888888}%*%f'

# The clock shows when a command *ran*, not when the prompt was drawn: redraw
# the prompt as the line is accepted, so idle time doesn't leave stale times
# in scrollback
_prompt_accept_line() {
    zle reset-prompt
    zle .accept-line
}
zle -N accept-line _prompt_accept_line
