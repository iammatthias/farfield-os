//! The background threads. Each owns one cadence — host metrics every 2s,
//! containers every 2s, slow status every 30s, the clock on the minute — and
//! each takes the mutex only to write its results, never while sampling.

use std::{
    collections::{HashMap, VecDeque},
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};


use crate::app::*;
use crate::facts::*;
use crate::model::*;
use crate::sample::*;

// Container + status sampling
// ---------------------------------------------------------------------------

pub(crate) fn host_loop(app: Arc<Mutex<App>>) {
    // The authoritative Host lives here — sample into it unlocked, then
    // lock only long enough to publish a clone, so the render loop never
    // waits out a full /proc + hwmon sweep.
    let mut host = Host::default();
    loop {
        let started = Instant::now();
        // A panicking sampler must not silently kill the thread — the tile
        // would keep drawing frozen stats. Catch it, surface it, retry.
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sample_host(&mut host);
        }))
        .is_err();
        if panicked {
            lock_app(&app).host_err = Some("host sampler restarted (panic)".into());
        } else {
            let snap = host.clone();
            let mut a = lock_app(&app);
            a.host = snap;
            a.host_err = None;
        }
        thread::sleep(if panicked {
            STATS_EVERY // back off before retrying
        } else {
            // Floored — an overrun cycle must not become a busy loop.
            STATS_EVERY.saturating_sub(started.elapsed()).max(STATS_EVERY / 4)
        });
    }
}

pub(crate) fn docker_loop(app: Arc<Mutex<App>>) {
    loop {
        let started = Instant::now();
        // Same guard as host_loop — a panic surfaces in the panel and the
        // loop restarts instead of freezing the charts.
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match sample_containers(&app) {
                Ok(()) => lock_app(&app).docker_err = None,
                Err(e) => lock_app(&app).docker_err = Some(e),
            }
        }))
        .is_err();
        if panicked {
            lock_app(&app).docker_err = Some("sampler restarted (panic)".into());
        }
        thread::sleep(if panicked {
            STATS_EVERY // back off before retrying
        } else {
            // Floored — an overrun cycle must not become a busy loop.
            STATS_EVERY.saturating_sub(started.elapsed()).max(STATS_EVERY / 4)
        });
    }
}

pub(crate) fn sample_containers(app: &Arc<Mutex<App>>) -> Result<(), String> {
    let list = docker_get("/containers/json")?;
    let list = list.as_array().ok_or("unexpected /containers/json shape")?;
    let mut seen = Vec::with_capacity(list.len());

    for c in list {
        let id = c["Id"].as_str().unwrap_or_default();
        let name = c["Names"][0]
            .as_str()
            .unwrap_or("?")
            .trim_start_matches('/')
            .to_string();
        let state = c["State"].as_str().unwrap_or("");
        let status = c["Status"].as_str().unwrap_or("");
        let flag = if state == "restarting" {
            Some("restarting".to_string())
        } else if status.contains("unhealthy") {
            Some("unhealthy".to_string())
        } else {
            None
        };
        let stats = match docker_get(&format!("/containers/{id}/stats?stream=false&one-shot=true")) {
            Ok(v) => v,
            Err(_) => continue, // container racing us to exit
        };

        let cpu_total = stats["cpu_stats"]["cpu_usage"]["total_usage"].as_u64().unwrap_or(0);
        let sys_total = stats["cpu_stats"]["system_cpu_usage"].as_u64().unwrap_or(0);
        let online = stats["cpu_stats"]["online_cpus"].as_u64().unwrap_or(0).max(1) as f64;
        let usage = stats["memory_stats"]["usage"].as_u64().unwrap_or(0);
        // The CLI subtracts page cache; the key differs across cgroup v1/v2.
        let cache = stats["memory_stats"]["stats"]["inactive_file"]
            .as_u64()
            .or_else(|| stats["memory_stats"]["stats"]["total_inactive_file"].as_u64())
            .unwrap_or(0);
        let mem_mib = usage.saturating_sub(cache) as f64 / 1048576.0;
        let net_total = stats["networks"].as_object().map(|nets| {
            nets.values()
                .map(|n| n["rx_bytes"].as_u64().unwrap_or(0) + n["tx_bytes"].as_u64().unwrap_or(0))
                .sum::<u64>()
        });

        let now = Instant::now();
        let mut a = lock_app(app);
        let s = a.containers.entry(name.clone()).or_default();
        if let Some(p) = &s.prev {
            let dsys = sys_total.saturating_sub(p.sys_total);
            if dsys > 0 {
                let dcpu = cpu_total.saturating_sub(p.cpu_total);
                s.cpu_cur = dcpu as f64 / dsys as f64 * online * 100.0;
            }
            push(&mut s.cpu, s.cpu_cur);
            let dt = now.duration_since(p.at).as_secs_f64();
            s.net_rate = match (net_total, p.net_total) {
                (Some(cur), Some(prev)) if dt > 0.0 => Some(cur.saturating_sub(prev) as f64 / dt),
                _ => None,
            };
        } else {
            // First sight has no delta to compute — push a placeholder so
            // the cpu and mem sparkline columns stay aligned forever after.
            push(&mut s.cpu, 0.0);
        }
        s.mem_cur = mem_mib;
        push(&mut s.mem, mem_mib);
        s.flag = flag;
        s.prev = Some(Prev { at: now, cpu_total, sys_total, net_total });
        drop(a);
        seen.push(name);
    }

    lock_app(app).containers.retain(|k, _| seen.contains(k));
    Ok(())
}

pub(crate) fn status_loop(app: Arc<Mutex<App>>, mode: Mode) {
    // Which hourly samples this panel actually renders. Each kiosk tile is
    // its own process, so an ungated sampler runs once per tile — six
    // concurrent `checkupdates` racing the temp sync DB is what produced a
    // false "up to date". Gate each heavy sampler to the panel that shows it.
    // updates/security/last-update render in the OPS (Status) tile's
    // SECURITY section, so only it runs checkupdates — keeping the one
    // network sync off the critical path and out of a multi-tile race.
    let shows_updates = matches!(mode, Mode::Full | Mode::Status);
    let shows_disk = matches!(mode, Mode::Full | Mode::Disk);
    let shows_claude = matches!(mode, Mode::Full | Mode::Status | Mode::Claude);
    // The 30s samples get the same treatment — sites/traffic/tailscale
    // render only in the NET tile, services/alerts/docker-ops only in OPS,
    // and the top-procs tables only in CPU/MEM. `Full` renders (nearly)
    // all of it, so it keeps sampling everything.
    let shows_sites = matches!(mode, Mode::Full | Mode::Net);
    let shows_ops = matches!(mode, Mode::Full | Mode::Status);
    let shows_procs_cpu = matches!(mode, Mode::Full | Mode::Cpu | Mode::Host);
    let shows_procs_mem = matches!(mode, Mode::Full | Mode::Mem | Mode::Host);
    let mut tick: u64 = 0;
    let mut log_offset: u64 = 0;
    let mut traffic_window: VecDeque<(f64, String, u16)> = VecDeque::new();
    loop {
        let started = Instant::now();

        // Same guard as host_loop — a panic anywhere in a cycle surfaces
        // in the panel and the loop restarts instead of freezing silently.
        // (The closure body keeps the loop's indentation on purpose.)
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // ---- one full sampling cycle ----
        let services: Vec<(String, String)> = if shows_ops {
            SERVICES
                .iter()
                .map(|s| {
                    let state = Command::new("systemctl")
                        .args(["is-active", s])
                        .output()
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| "unknown".into());
                    (s.to_string(), state)
                })
                .collect()
        } else {
            Vec::new()
        };

        let mut sites = Vec::new();
        if shows_sites {
            sites = caddy_sites();
            probe_sites(&mut sites);
        }
        let (disk_pct, disk_detail) = if shows_disk { disk_usage() } else { (None, String::new()) };
        let procs_cpu = if shows_procs_cpu { top_procs("-pcpu", 14) } else { Vec::new() };
        let procs_mem = if shows_procs_mem { top_procs("-pmem", 14) } else { Vec::new() };
        let prune_next = if shows_ops { prune_timer_next() } else { String::new() };
        let images = if shows_ops || shows_disk {
            docker_get("/images/json")
                .ok()
                .and_then(|v| v.as_array().map(|a| a.len()))
                .unwrap_or(0)
        } else {
            0
        };
        // `None` = dockerd didn't answer. Kept distinct from Some(0) so
        // the OPS tile can't paint a healthy green "0/0 running" at the
        // exact moment the daemon is down.
        let containers_total = if shows_ops {
            docker_get("/containers/json?all=1").ok().and_then(|v| v.as_array().map(|a| a.len()))
        } else {
            None
        };
        // Running count via the cheap list endpoint — the OPS tile shows
        // only this number, so it doesn't need the per-container stats
        // loop (which costs dockerd a cgroup sweep per container per 2s).
        let containers_running = if shows_ops {
            docker_get("/containers/json").ok().and_then(|v| v.as_array().map(|a| a.len()))
        } else {
            None
        };
        let docker_ok = !shows_ops || containers_total.is_some();

        let traffic = if shows_sites {
            sample_traffic(&mut log_offset, &mut traffic_window)
        } else {
            HashMap::new()
        };
        let alerts = if shows_ops { sample_alerts() } else { Alerts::default() };
        let tailscale = if shows_sites { sample_tailscale() } else { None };
        let reboot_pending = if shows_ops { reboot_pending() } else { None };

        // Slow-moving / heavier samples — hourly. checkupdates does a
        // network sync, smartctl wakes the drive, pacman.log is big.
        //
        // Deliberately offset to tick 1, not 0: at boot the panel should
        // paint everything fast within one cycle rather than hold a blank
        // tile behind a network sync. The hourly cadence is unchanged.
        let hourly = if tick % 120 == 1 {
            Some((
                if shows_updates { pending_updates() } else { None },
                if shows_updates { security_updates() } else { None },
                if shows_disk { nvme_health() } else { (None, None) },
                if shows_disk { snapshot_count() } else { 0 },
                if shows_updates { last_update_days() } else { None },
                if shows_claude { claude_usage() } else { (0, 0) },
            ))
        } else {
            None
        };

        // Live claude processes on the host — cheap, every cycle for the
        // panels that show it.
        let claude = if shows_claude { Some(claude_runs()) } else { None };
        // The dedicated CLAUDE tile gets the full activity scan (native
        // metadata walk of ~/.claude/projects — no subprocesses).
        let claude_detail = if mode == Mode::Claude { Some(claude_scan()) } else { None };

        {
            let mut a = lock_app(&app);
            a.status_err = None;
            a.services = services;
            a.sites = sites;
            a.procs_cpu = procs_cpu;
            a.procs_mem = procs_mem;
            a.disk_pct = disk_pct;
            a.disk_detail = disk_detail;
            a.images = images;
            a.containers_total = containers_total.unwrap_or(0);
            a.containers_running = containers_running.unwrap_or(0);
            a.docker_ok = docker_ok;
            a.prune_next = prune_next;
            a.traffic = traffic;
            a.failed_units = alerts.failed_units;
            a.journal_errs = alerts.journal_errs;
            a.crashes = alerts.crashes;
            a.err_sample = alerts.err_sample;
            a.banned_ips = alerts.banned_ips;
            a.ssh_fails = alerts.ssh_fails;
            a.reboot_pending = reboot_pending;
            if let Some((ip, online, total)) = tailscale {
                a.ts_ip = ip;
                a.ts_online = online;
                a.ts_total = total;
            }
            if let Some((updates, sec_updates, (wear, temp), snaps, upd_days, (c24, ctotal))) = hourly {
                a.updates = updates;
                a.sec_updates = sec_updates;
                a.nvme_wear = wear;
                a.nvme_temp = temp;
                a.snapshots = snaps;
                a.last_update_days = upd_days;
                a.claude_24h = c24;
                a.claude_total = ctotal;
            }
            if let Some(r) = claude {
                a.claude_runs = r;
            }
            if let Some((projects, daily, authed, recent, total)) = claude_detail {
                a.claude_projects = projects;
                a.claude_daily = daily;
                a.claude_authed = authed;
                a.claude_24h = recent;
                a.claude_total = total;
            }
        }
        // ---- end of the guarded cycle ----
        }))
        .is_err();
        if panicked {
            lock_app(&app).status_err = Some("status sampler restarted (panic)".into());
        }
        tick += 1;
        // Floored — serial probe timeouts during an outage can overrun the
        // period, and a zero sleep would busy-loop the samplers. The full
        // period after a panic doubles as the retry backoff.
        thread::sleep(if panicked {
            STATUS_EVERY
        } else {
            STATUS_EVERY.saturating_sub(started.elapsed()).max(STATUS_EVERY / 4)
        });
    }
}

/// Wall-clock for the header — cheap thread, minute resolution.
///
/// Sleeps to the next minute boundary rather than polling every 10s:
/// same one `date` fork doing six times less work, and the displayed
/// minute now turns over when the minute actually does.
pub(crate) fn clock_loop(app: Arc<Mutex<App>>) {
    loop {
        let out = Command::new("date")
            .arg("+%a %b %d  %H:%M")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        lock_app(&app).clock = out;

        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() % 60)
            .unwrap_or(0);
        // +1s of slack so we land just past the rollover, never just shy
        // of it (which would show the old minute for a full extra cycle).
        thread::sleep(Duration::from_secs(61 - secs));
    }
}

