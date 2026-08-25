# Deploying

## Projects

Project checkouts live under `~/projects/<name>` (overridable with
`FF_PROJECTS`). `/srv/projects/` holds the checkouts the preview sites serve.

```bash
ff-deploy <project>
```

It git-pulls `--ff-only`, finds the compose file (repo root or `deploy/`, any
of compose's accepted spellings), runs `docker compose up -d --build`, and
prints the resulting container table. Named volumes survive, so data is
preserved.

`--ff-only` means a diverged local checkout **fails rather than merges**. If it
refuses, look at what is actually in the working tree before forcing anything —
the box is a deploy target, not a place to hold unpushed work.

## Migrations

`migrations/` holds run-once convergence scripts, tracked by marker files in
`/var/lib/farfield/migrations/`.

```bash
ff-migrate --status    # list pending without running
ff-migrate             # apply pending
```

A migration exists for work that `setup.sh` cannot do because it has to **undo**
something an older install did — a fresh box would never need it. The contract
in `migrations/_template.sh` is load-bearing:

1. **Idempotent.** Guard every action on the artifact it fixes actually
   existing, so a fresh install and a re-run are both no-ops.
2. **Exit non-zero if you cannot do the work.** `ff-migrate` marks a migration
   converged on exit 0, permanently. "I couldn't find it so I skipped it" must
   fail, not succeed quietly.
3. **Leave the box working.** If you take a service down mid-migration, `trap`
   to bring it back on failure.

`FF_REAL_USER` is provided and guaranteed non-root. `setup.sh` ends by calling
`ff-migrate`, so upgrades and fresh installs converge on the same state.

## Editing the OS itself

The installed helpers in `/usr/local/bin/ff-*` and the generated user files
(`~/.zshrc`, `~/.zshenv`, `~/.config/zsh/prompt.zsh`, `~/CLAUDE.md`) are
**copies**. Edit `bin/` or `configs/` in the repo and reinstall; anything
changed in place is lost on the next setup or migration. `ff-doctor` reports
installed-vs-repo drift, which is usually someone having edited the copy.

Helper convention worth preserving: scripts that **change** state use
`set -euo pipefail`; scripts that **render** a board use `set -uo pipefail`, so
one failing sub-command does not blank the whole display.

After editing `README.md` or `docs/*.md`, regenerate the doc site so it does not
drift, and commit the result:

```bash
go run ./scripts/build-os-docs . stack/homepage
```

## System updates

```bash
ff-update          # pacman -Syu + cache clean
ff-update --yes    # unattended; survives an SSH drop
```

On btrfs, snap-pac snapshots before and after every pacman transaction, so a
bad update is a reboot away from yesterday. See `recovery.md`.
