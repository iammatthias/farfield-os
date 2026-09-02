---
name: farfield-os
description: >
  REQUIRED for operating the farfield os homelab box. Use when touching
  /srv/stack, the Caddyfile, Cloudflare Tunnel, Tailscale, UFW or DOCKER-USER,
  the docker compose stack, snapper snapshots, or any ff-* helper. Triggers:
  ff-deploy, ff-doctor, ff-migrate, ff-update, ff-firewall, ff-board,
  ff-agent, ff-switchboard, omp, claude, the agent, switchboard, texting the box,
  add-site, add-public-site, remove-site, list-sites, test-caddy, Caddyfile,
  caddy, cloudflared, tunnel, tailscale, tailnet, snapper, snap-pac,
  grub-btrfs, /srv/stack, /srv/projects, ~/projects, kiosk, sway, herdr,
  publish a site, expose a port, deploy a project, the box is down.
  Excludes application code inside a deployed project — that belongs to the
  project's own repo and skills.
---

# farfield os

Operating the homelab: an Arch box running the farfield fleet in docker
compose behind Caddy and a Cloudflare Tunnel, managed over SSH by an agent.

This skill is for **running and changing the box**. It is not for writing the
applications that run on it.

## When this skill MUST be used

**Before** any of these, stop and read the matching guide below:

- Editing or reading `/srv/stack/*` — the Caddyfile, the compose file, `.env`
- Publishing, moving, or removing a site or hostname
- Restarting, rebuilding, or debugging the ingress stack
- Changing firewall policy, Tailscale, or anything about what is reachable
- Deploying a project, or running `ff-deploy` / `ff-migrate`
- Diagnosing "the box is down", "the site is 502", "I can't reach it"

**If you are about to run `docker compose` or edit a file under `/srv/stack`,
stop and read `stack.md` and `ingress.md` first.** Both directories contain
live state that looks like configuration and is not.

## Critical safety rules

**The live `/srv/stack/Caddyfile` is state, not config.** It accumulates
`add-site` blocks at runtime. The copy in this repo is boilerplate for a fresh
install. Never overwrite the live file with the repo's, and never hand-edit it
— use `add-site`, `add-public-site`, `remove-site`, then `test-caddy`.

**Never restart caddy alone.** `ff-cloudflared` runs with
`network_mode: service:caddy`, so it shares caddy's network namespace. Cycling
caddy by itself orphans the tunnel connector and the public sites go dark
without the container appearing unhealthy. Cycle the whole stack.

**Never edit `/usr/local/bin/ff-*` in place.** Those are installed copies. The
next `setup.sh` or `ff-migrate` overwrites them. Edit `bin/` or `configs/` in
this repo and reinstall.

**Secrets live in `/srv/stack/.env` (chmod 600) and are never committed.**
`CF_API_TOKEN` and `CLOUDFLARED_TOKEN` are there. Do not echo them into logs,
commit messages, or a chat.

```
READ-ONLY unless the guide says otherwise
/srv/stack/Caddyfile        live state — use the helpers
/srv/stack/.env             secrets, chmod 600
/var/lib/tailscale/         tailnet identity (host tailscaled)
/srv/stack/data/caddy/      certs and cache
/usr/local/bin/ff-*         installed copies — edit the repo instead
```

## Start here

`ff-doctor` is a read-only convergence check: locale, systemd units, tailnet,
stack health, installed-vs-repo drift, and pending migrations. **Run it first
when picking up work on the box**, and again before declaring something fixed.

`ff-help` prints the full command reference. `ff-info` is the system report.

## Hashing — use `@noble/hashes`, never hashlib and never by hand

Any hash the box computes — contract bytecode, content addresses, checksums —
goes through [`@noble/hashes`](https://github.com/paulmillr/noble-hashes) under
bun. It is audited, dependency-free, and covers keccak, sha2, sha3, blake and
the rest.

**Python's `hashlib` is a trap for Ethereum work.** `hashlib.sha3_256` is
FIPS-202 SHA3, which differs from Keccak-256 only in a padding byte — so it
returns a plausible, wrong 32-byte answer with no error. `cast` is not
installed here. Hand-rolling the permutation works but is unreviewed code on
the verification path, which is the wrong place for it.

    cd /tmp/hashwork && bun add @noble/hashes

    // NOTE: v2 requires the .js extension in the specifier.
    // "@noble/hashes/sha3" fails to resolve; "@noble/hashes/sha3.js" works.
    import { keccak_256 } from "@noble/hashes/sha3.js";
    import { bytesToHex, hexToBytes } from "@noble/hashes/utils.js";

    console.log("0x" + bytesToHex(keccak_256(hexToBytes(codeHex))));

**Pin known vectors before trusting any hash output**, whoever computed it:

    keccak256("")    c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470
    keccak256("abc") 4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45

A hash that matches an expectation is worthless if the hasher is wrong, and a
wrong hasher is silent. Verified against a live contract on 2026-09-02: noble
and an independent implementation agreed byte for byte.

## Topic guides

Read the one that matches the task before starting:

- [`ingress.md`](ingress.md) — Caddy, the tunnel, sites, hostnames, firewall
- [`stack.md`](stack.md) — the `/srv/stack` compose stack and its lifecycle
- [`deploy.md`](deploy.md) — deploying projects, `ff-deploy`, `ff-migrate`
- [`agents.md`](agents.md) — `ff-agent`, the switchboard host service, the persona
- [`recovery.md`](recovery.md) — snapshots, rollback, and what to check when

## Shape of the box

- **Ingress** — Caddy and cloudflared in compose at `/srv/stack`. Caddy owns
  the network namespace and publishes :80/:443 to the host; :8080 stays
  internal and is the tunnel's target. No ports open on the router.
- **Identity** — host `tailscaled` owns the box's single tailnet identity.
  Tailscale is deliberately *not* in the stack. SSH and Caddy's published
  ports land on its address.
- **Firewall** — UFW filters INPUT, which does not cover docker's published
  ports; those are DNAT'd and hit FORWARD. `ff-firewall` puts the real policy
  in `DOCKER-USER`. Public traffic never transits that chain — it arrives
  through cloudflared over loopback inside caddy's namespace.
- **Snapshots** — on btrfs roots, snapper plus snap-pac photograph the system
  before and after every pacman transaction; grub-btrfs boots into one.
- **Agents** — omp and Claude Code are both installed; `ff-agent` decides which
  one anything gets. `~/CLAUDE.md` carries always-on system context; this skill
  is the on-demand depth. See `agents.md`.
- **switchboard** — a systemd unit rather than a container, because it hands
  inbound iMessages to an agent. `ff-deploy farfield` does not cover it.
- **Sessions** — herdr provides persistent terminals that survive an SSH drop.
