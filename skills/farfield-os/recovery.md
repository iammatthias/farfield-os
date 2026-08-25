# When something is wrong

## Always start here

```bash
ff-doctor
```

Read-only. Checks locale, systemd units, the tailnet, stack health,
board-vs-stack assumptions, installed-vs-repo drift, and pending migrations.
Run it when picking up work, and again before calling something fixed —
it asserts the measurement before trusting the verdict.

## Triage by symptom

**A public site is down, the private one is fine.** Almost always the tunnel.
Either cloudflared was orphaned by a caddy-only restart (see `stack.md` — cycle
the whole stack), or the hostname has no public-hostname route in the
Cloudflare Zero Trust dashboard (see `ingress.md`).

**A private `<name>.local` site is down.** Check `list-sites` for the block,
then `test-caddy`, then whether the target port is actually listening on the
host. Remember the proxy target is `host.docker.internal`, not `localhost`.

**Nothing is reachable from off-box, everything works locally.** Check
`ff-firewall` and the `DOCKER-USER` chain, not UFW — UFW does not filter
docker's published ports.

**A container is unhealthy or missing.** `ff-docker-status`, then the
container's own logs. Bring the stack up rather than restarting one service.

**The box is unreachable entirely.** Tailscale identity lives on the host at
`/var/lib/tailscale/`. If it is intact, the box is probably down rather than
misconfigured.

## Snapshots

On btrfs roots, snapper plus snap-pac photograph the system before and after
every pacman transaction, and grub-btrfs offers boot-into-snapshot from GRUB.
That covers a bad system update. It does **not** cover application data:
`/var/lib/{postgres,valkey,docker}` are `chattr +C` (no CoW) and the fleet's
own data has its own backup path.

Rolling back the system does not roll back `/srv/stack/Caddyfile` state or
container volumes — think about those separately before rebooting into an old
snapshot.

## What is not backed up here

The farfield fleet's databases and blobs have their own backup service. This
box's own state — the Caddyfile, `/srv/stack/.env`, the tailnet identity — is
not in it. Reprovisioning those is a manual step, so read before deleting.
