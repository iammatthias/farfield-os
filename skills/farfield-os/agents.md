# Agents on the box

The box runs coding agents, and one of them answers text messages. Two things
follow from that: which agent runs is a setting, and the thing that texts is a
service with a lifecycle.

## ff-agent — one name for "the agent"

Nothing on this box should invoke `omp` or `claude` by name. They ask `ff-agent`,
which reads the chosen agent from a file and knows each one's spelling of "run
one prompt and exit".

```bash
ff-agent                       # launch interactively (alias: a)
ff-agent --prompt "..."        # one prompt, print the reply, exit
ff-agent --prompt "..." --session-dir DIR   # ...continuing that conversation
ff-agent --prompt "..." --file photo.jpg    # ...with an image (repeatable)
ff-agent which | list | set <omp|claude>
```

The default lives in `~/.config/farfield/defaults/agent` — one word. Switching
harnesses is `ff-agent set claude` and nothing else changes, because switchboard
and any cron job go through the same indirection.

**The output contract is load-bearing.** `--prompt` puts the reply on **stdout
and nothing else**; progress and diagnostics go to stderr. Callers redirect
stderr and get a clean answer. Nothing streams. Preserve that when adding an
agent to the case statement — a harness that prints progress to stdout will
send that progress to somebody's phone.

`ff-agent` finds its own binaries (`~/.local/bin`, `~/.bun/bin`) and sources
`/etc/farfield/agent.env` itself, so a systemd unit needs no `PATH=` line and no
`EnvironmentFile` for credentials. Verified from a bare `env -i`.

## Credentials

`/etc/farfield/agent.env`, mode 600, owned by the real user. It holds the model
provider key (`OPENROUTER_API_KEY`). `ff-doctor` checks that the file exists,
has a non-empty key, and is not group- or world-readable.

`claude` is separate: it uses subscription OAuth in the user's home, not this
file. That is why the switchboard unit runs as the real user rather than as a
service account — a service account cannot see that session.

## switchboard — the thing that texts

`switchboard.farfield.systems`, port 8802, **a systemd unit and not a
container**. It is the one farfield app that is not in the compose stack,
because it hands messages to an agent: exec'ing a binary and reading the user's
agent config is not something a distroless image with one data volume can do.

```bash
ff-switchboard deploy    # build from ~/projects/farfield, install, restart
ff-switchboard status    # unit + the service's own /status
ff-switchboard logs [n]
```

**`ff-deploy farfield` does NOT cover it.** That rebuilds the compose stack, and
switchboard is not in it. Deploying farfield without also running
`ff-switchboard deploy` leaves the old binary running against new siblings.

Two things that will bite:

- It binds the **docker0 gateway**, not loopback, because caddy is a container
  and reaches the host through `host.docker.internal`. A service on 127.0.0.1 is
  invisible to ingress while looking perfectly healthy locally.
- It needs a **shared group** over `~/projects/farfield/data`. The containers
  write there as uid 65532, and SQLite creates `-wal`/`-shm` files beside its
  database, so the unit needs write access to the directory rather than just to
  its own file. The `farfield-data` group (gid 65532) is how both get it, and
  systemd rejects a bare gid — it must be the name.

## How a message is handled

A leading `/` is a command: deterministic, in-process, answered in the same
webhook cycle, and it never reaches an agent. Anything else is conversation,
handed to `ff-agent` as a background turn.

The turn does not run inside the webhook — Photon's delivery is acknowledged at
once, because holding it open for a model turn invites a timeout and a retry,
and a retried instruction is the instruction done twice. The sender hears at
most two messages: one "on it" if the turn runs long, then the answer.

`/jobs`, `/job <id>` and `/cancel <id>` expose turns that are running somewhere
the thread cannot see. A turn interrupted by a restart is marked failed at boot
rather than leaving somebody waiting.

**If the agent is unreachable, slash commands still work.** Keep it that way.

## The persona

Embedded in the switchboard binary and written to
`<state>/switchboard-agent/workspace/AGENTS.md` (and `CLAUDE.md`) at every boot,
so the voice cannot drift from the code that speaks it. Edit it in
`apps/switchboard/agent/` in the farfield repo — editing the file on disk is
pointless, it is overwritten on restart.

The workspace is deliberately **not** the farfield checkout. An agent whose
working directory is the source tree treats every question as an invitation to
edit it.
