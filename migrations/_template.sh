#!/bin/bash
# TEMPLATE — copy to migrations/YYYY-MM-DD-what-it-does.sh and edit.
# ff-migrate skips this file by name; it is never executed.
#
# One-line statement of what state this converges, and why it can't just
# live in setup.sh (usually: it has to UNDO something an older install
# did, which a fresh install would never have).
#
# Runs as root via ff-migrate, once per box, tracked by a marker file in
# /var/lib/farfield/migrations/.
set -euo pipefail

# ---------------------------------------------------------------------------
# Contract
# ---------------------------------------------------------------------------
# 1. IDEMPOTENT. Guard every action on the artifact it fixes actually
#    existing, so a fresh install and a re-run are both no-ops.
# 2. EXIT NONZERO IF YOU CAN'T DO THE WORK. ff-migrate marks a migration
#    converged on exit 0, permanently. "I couldn't find the user's home,
#    so I skipped the user half" must fail, not succeed quietly.
#    (Exiting 0 for "there is genuinely nothing to do here" is correct —
#    that's the guard in #1, not a skip.)
# 3. LEAVE THE BOX WORKING. If you take a service down mid-migration,
#    trap to bring it back up on failure.
# ---------------------------------------------------------------------------

# The user whose home this migration touches. ff-migrate guarantees this
# is set and non-root before invoking us.
REAL_USER=${FF_REAL_USER:?ff-migrate must provide FF_REAL_USER}
REAL_HOME="/home/$REAL_USER"
REPO=$(cd "$(dirname "$0")/.." && pwd)

# Guard: nothing to converge on a box that never had the old state.
if [ ! -e /path/to/the/thing/this/fixes ]; then
    echo "example-migration: nothing to migrate"
    exit 0
fi

echo "example-migration: converging <the thing>"

# ...the work. Example of the rollback discipline from contract #3:
#
#   cd /srv/stack
#   trap 'echo "FAILED — restoring stack" >&2; docker compose up -d' ERR
#   docker compose down
#   ...risky step...
#   docker compose up -d
#   trap - ERR

# Example of the user half. Anything writing into $REAL_HOME must be
# owned by the user, not root — a root-owned dotfile is a silent breakage
# that surfaces days later.
install -m 644 -o "$REAL_USER" -g "$REAL_USER" \
    "$REPO/configs/example" "$REAL_HOME/.config/example"

echo "example-migration: done"
