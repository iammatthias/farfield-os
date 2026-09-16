//! Slow-moving facts about the box: site probes, pending updates, alerts,
//! tailscale, snapshots, Claude usage, Caddy vhosts, disk headroom.
//!
//! These cost forks and network calls, so they refresh on the 30s/60s cycle
//! rather than with the metrics.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs,
    io::{Read, Seek, SeekFrom, Write},
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use crate::app::*;
use crate::model::*;
use crate::sample::*;

/// Live-probe each Caddy vhost, each through the path it is actually
/// served over. caddy publishes :80/:443 on the host, so private and
/// preview sites are probed via loopback — that exercises the publish
/// hop a tailnet client uses. :8080 (public-hostname routing) is
/// deliberately NOT published, so those are probed via caddy's bridge
/// IP, the only route in from the host.
///
/// Probes run concurrently: they're 3s-timeout network calls, and one
/// dead site must not push the rest past the caller's 30s cycle.
///
/// Degrades in halves — if the container IP lookup fails, private and
/// preview sites are still probed (only the public ones go unknown).
pub(crate) fn probe_sites(sites: &mut [Site]) {
    let caddy_ip = docker_get("/containers/ff-caddy/json").ok().and_then(|v| {
        let ns = &v["NetworkSettings"];
        ns["IPAddress"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(String::from)
            .or_else(|| {
                ns["Networks"]
                    .as_object()
                    .and_then(|o| o.values().next())
                    .and_then(|n| n["IPAddress"].as_str())
                    .filter(|s| !s.is_empty())
                    .map(String::from)
            })
    });

    let handles: Vec<_> = sites
        .iter()
        .map(|site| {
            let host = site.host.clone();
            let kind = site.kind.clone();
            let pub_ip = caddy_ip.clone();
            thread::spawn(move || match kind.as_str() {
                "private" => Some(http_probe("127.0.0.1", 80, &host)),
                "public" => pub_ip.map(|ip| http_probe(&ip, 8080, &host)),
                _ => Some(tcp_probe("127.0.0.1", 443).map(|ms| (0u16, ms))),
            })
        })
        .collect();

    for (site, h) in sites.iter_mut().zip(handles) {
        // A panicked probe thread reads as "couldn't tell", never as ok.
        match h.join().unwrap_or(None) {
            // Probed, got an answer.
            Some(Some((code, ms))) => {
                site.code = code;
                site.ms = ms;
                site.ok = Some(code == 0 || (200..400).contains(&code));
            }
            // Probed, no answer — the site is down.
            Some(None) => site.ok = Some(false),
            // Not probed (no route to :8080) — leave it unknown.
            None => site.ok = None,
        }
    }
}

pub(crate) fn http_probe(ip: &str, port: u16, host: &str) -> Option<(u16, u64)> {
    use std::net::{SocketAddr, TcpStream};
    let addr: SocketAddr = format!("{ip}:{port}").parse().ok()?;
    let start = Instant::now();
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok();
    s.set_write_timeout(Some(Duration::from_secs(3))).ok();
    write!(
        s,
        "GET / HTTP/1.1\r\nHost: {host}\r\nUser-Agent: ff-board\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut buf = [0u8; 32];
    let n = s.read(&mut buf).ok()?;
    let code = String::from_utf8_lossy(&buf[..n]).split_whitespace().nth(1)?.parse().ok()?;
    Some((code, start.elapsed().as_millis() as u64))
}

pub(crate) fn tcp_probe(ip: &str, port: u16) -> Option<u64> {
    use std::net::{SocketAddr, TcpStream};
    let addr: SocketAddr = format!("{ip}:{port}").parse().ok()?;
    let start = Instant::now();
    TcpStream::connect_timeout(&addr, Duration::from_secs(3)).ok()?;
    Some(start.elapsed().as_millis() as u64)
}

/// Pending pacman updates (pacman-contrib's checkupdates; syncs to a
/// temp DB, never touches the real one). On a rolling release this is
/// routinely dozens — informational, not an alarm; the security count
/// below is the signal that actually warrants attention.
///
/// `None` means "couldn't tell", not "zero": checkupdates exits 0 with a
/// list, 2 for none-pending, and 1 on failure (network, or a temp-db
/// lock when several callers sync at once). Treating a failed run as 0
/// would paint a false "up to date", so only 0/2 yield a count.
pub(crate) fn pending_updates() -> Option<usize> {
    // Timed: checkupdates syncs over the network, and a wedged mirror
    // would otherwise stall the whole status cycle indefinitely.
    let out = Command::new("timeout").args(["30", "checkupdates"]).output().ok()?;
    match out.status.code() {
        Some(0) | Some(2) => Some(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count(),
        ),
        _ => None,
    }
}

/// Installed packages with a known security advisory whose fix is
/// already available to install (Arch Security Tracker, via arch-audit
/// `-u`). `None` when arch-audit isn't installed OR its run failed
/// (network, advisory-DB fetch) — absence of signal, not a clean bill
/// of health, so the UI shows nothing rather than a false green "0".
/// This is what separates "stay current for security" from the much
/// noisier "stay current for everything".
pub(crate) fn security_updates() -> Option<usize> {
    // Timed for the same reason as pending_updates: this fetches the
    // advisory DB over the network. A timeout exits 124 → None.
    let out = Command::new("timeout").args(["30", "arch-audit", "-uq"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count(),
    )
}

/// Count live Claude Code processes on the host — the box is managed by
/// Claude Code over SSH, so a running session shows up here. Match the
/// npm-global install path to avoid false-matching shell aliases.
pub(crate) fn claude_runs() -> usize {
    Command::new("pgrep")
        .args(["-fc", r"(^|/)claude( |$)|@anthropic-ai/claude-code"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

pub(crate) fn top_procs(sort: &str, n: usize) -> Vec<String> {
    Command::new("ps")
        .args(["-eo", "pcpu,pmem,comm", &format!("--sort={sort}"), "--no-headers"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .take(n)
                .map(|l| l.trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) const ACCESS_LOG: &str = "/srv/stack/data/caddy/data/access/access.log";

pub(crate) const TRAFFIC_BINS: usize = 8; // sparkline columns across the 5-min window

/// Incrementally tail caddy's shared JSON access log and aggregate a
/// 5-minute window of per-host request counts, 404 / other-4xx / 5xx
/// tallies, and a request-rate sparkline. Only complete lines are
/// consumed; rotation resets the offset.
pub(crate) fn sample_traffic(
    offset: &mut u64,
    window: &mut VecDeque<(f64, String, u16)>,
) -> HashMap<String, Traffic> {
    // Age and bucket by the log's own `ts` (unix secs), not read time —
    // otherwise a respawn dumps the whole backlog tail into "now", spiking
    // every count and sparkline for the first five minutes.
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    if let Ok(mut f) = fs::File::open(ACCESS_LOG) {
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < *offset {
            *offset = 0; // rotated
        }
        // First pass: start near the tail, not from history.
        if *offset == 0 && len > 524_288 {
            *offset = len - 524_288;
        }
        if len > *offset && f.seek(SeekFrom::Start(*offset)).is_ok() {
            let mut buf = Vec::with_capacity((len - *offset) as usize);
            if (&mut f).take(len - *offset).read_to_end(&mut buf).is_ok() {
                // Consume only up to the last complete line.
                if let Some(last_nl) = buf.iter().rposition(|b| *b == b'\n') {
                    for l in String::from_utf8_lossy(&buf[..=last_nl]).lines() {
                        let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
                        // The board's own site probes carry this UA —
                        // self-inflicted requests must not pollute the counts.
                        if v["request"]["headers"]["User-Agent"][0].as_str() == Some("ff-board") {
                            continue;
                        }
                        let host = v["request"]["host"]
                            .as_str()
                            .unwrap_or("")
                            .split(':')
                            .next()
                            .unwrap_or("")
                            .to_string();
                        if host.is_empty() {
                            continue;
                        }
                        let ts = v["ts"].as_f64().unwrap_or(now);
                        let status = v["status"].as_u64().unwrap_or(0) as u16;
                        window.push_back((ts, host, status));
                    }
                    *offset += last_nl as u64 + 1;
                }
            }
        }
    }
    while window.front().is_some_and(|(t, ..)| now - t > 300.0) {
        window.pop_front();
    }
    // Hard cap — a request flood must not grow the window unbounded.
    if window.len() > 20_000 {
        window.drain(..window.len() - 20_000);
    }
    // 404 is bucketed apart from the rest of the 4xx range: on a public
    // host it's almost entirely bot/scanner/missing-asset noise, whereas
    // 400/401/403/429 usually mean something a human should look at.
    let mut map: HashMap<String, Traffic> = HashMap::new();
    for (t, host, status) in window.iter() {
        let e = map.entry(host.clone()).or_default();
        if e.spark.is_empty() {
            e.spark = vec![0.0; TRAFFIC_BINS];
        }
        e.reqs += 1;
        match *status {
            404 => e.e404 += 1,
            400..=499 => e.e4xx += 1,
            s if s >= 500 => e.e5xx += 1,
            _ => {}
        }
        // Oldest (≈300s ago) → bin 0, newest → last bin.
        let age = (now - t).clamp(0.0, 300.0);
        let bin = (((300.0 - age) / 300.0) * (TRAFFIC_BINS as f64 - 1.0)).round() as usize;
        e.spark[bin.min(TRAFFIC_BINS - 1)] += 1.0;
    }
    map
}

pub(crate) fn sudo_lines(args: &[&str]) -> Vec<String> {
    Command::new("sudo")
        .arg("-n")
        .args(args)
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim_end().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Host alerts, with enough detail to act on — not just counts. All via
/// passwordless sudo (farfield os grants it by design). Coredumps log entire
/// stack traces at priority err, which would read as hundreds of "errors"
/// per crash — so crashes are their own signal (grouped by process) and
/// the trace spam is excluded from the error count + sample.
pub(crate) fn sample_alerts() -> Alerts {
    let failed_units: Vec<String> = sudo_lines(&["systemctl", "--failed", "--plain", "--no-legend"])
        .iter()
        .filter_map(|l| l.split_whitespace().next().map(String::from))
        .collect();
    let p3 = sudo_lines(&["journalctl", "-p", "3", "--since", "-1 hour", "-q", "--no-pager", "-o", "cat", "-n", "1000"]);
    // Group coredumps by the "(process)" field so it reads "mango ×3".
    let mut crash_counts: BTreeMap<String, usize> = BTreeMap::new();
    for l in p3.iter().filter(|l| l.contains(" dumped core")) {
        let proc = l
            .split('(')
            .nth(1)
            .and_then(|s| s.split(')').next())
            .filter(|s| !s.is_empty())
            .unwrap_or("?");
        *crash_counts.entry(proc.to_string()).or_default() += 1;
    }
    let crashes: Vec<String> = crash_counts
        .into_iter()
        .map(|(p, n)| if n > 1 { format!("{p} ×{n}") } else { p })
        .collect();
    // Exclude every line of a systemd-coredump dump — crashes are their own
    // signal, so the trace, module list, and the trailing "ELF object binary
    // architecture: …" epilogue must not read as separate "journal errors".
    let err_lines: Vec<&String> = p3
        .iter()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with('#')
                || t.starts_with("Module ")
                || t.starts_with("Found module")
                || t.starts_with("Stack trace of")
                || t.starts_with("ELF object")
                || t.starts_with("Stored in")
                || t.contains(" dumped core"))
        })
        .collect();
    let journal_errs = err_lines.len();
    // The two most recent distinct messages, clipped — what's actually wrong.
    let mut err_sample: Vec<String> = Vec::new();
    for l in err_lines.iter().rev() {
        let s: String = l.trim().chars().take(64).collect();
        if !s.is_empty() && !err_sample.contains(&s) {
            err_sample.push(s);
        }
        if err_sample.len() >= 2 {
            break;
        }
    }
    let banned_ips = sudo_lines(&["fail2ban-client", "status", "sshd"])
        .iter()
        .find_map(|l| {
            l.contains("Currently banned:")
                .then(|| l.split_whitespace().last()?.parse().ok())
                .flatten()
        })
        .unwrap_or(0);
    let ssh_fails = sudo_lines(&["journalctl", "-u", "sshd", "--since", "-1 hour", "-q", "--no-pager", "-o", "cat", "-n", "500"])
        .iter()
        .filter(|l| l.contains("Failed password") || l.contains("Invalid user"))
        .count();
    Alerts { failed_units, crashes, journal_errs, err_sample, banned_ips, ssh_fails }
}

/// (tailnet IP, peers online, peers total) from host tailscaled — the
/// box's tailnet identity lives on the metal, not in a container.
pub(crate) fn sample_tailscale() -> Option<(String, usize, usize)> {
    let out = Command::new("timeout")
        .args(["10", "tailscale", "status", "--json"])
        .output()
        .ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    let ip = v["Self"]["TailscaleIPs"][0].as_str().unwrap_or("").to_string();
    let (online, total) = v["Peer"]
        .as_object()
        .map(|p| {
            (
                p.values().filter(|x| x["Online"].as_bool().unwrap_or(false)).count(),
                p.len(),
            )
        })
        .unwrap_or((0, 0));
    Some((ip, online, total))
}

/// Split a version string into its numeric runs, for ordering — a
/// lexicographic max would rank 6.9.7 above 6.10.1.
pub(crate) fn version_key(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter_map(|p| p.parse().ok())
        .collect()
}

/// The running kernel's modules vanish from /usr/lib/modules when
/// pacman installs a new kernel — the classic Arch "reboot pending"
/// signal. Returns the newest installed version when they differ.
pub(crate) fn reboot_pending() -> Option<String> {
    let running = fs::read_to_string("/proc/sys/kernel/osrelease").ok()?.trim().to_string();
    let dirs: Vec<String> = fs::read_dir("/usr/lib/modules")
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .collect();
    if dirs.iter().any(|d| *d == running) {
        None
    } else {
        dirs.into_iter().max_by_key(|d| version_key(d))
    }
}

/// (NVMe wear %, NVMe temperature °C) via smartctl.
pub(crate) fn nvme_health() -> (Option<u64>, Option<u64>) {
    Command::new("sudo")
        .args(["-n", "smartctl", "-j", "-A", "/dev/nvme0"])
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok())
        .map(|v| {
            let log = &v["nvme_smart_health_information_log"];
            (log["percentage_used"].as_u64(), log["temperature"].as_u64())
        })
        .unwrap_or((None, None))
}

pub(crate) fn snapshot_count() -> usize {
    sudo_lines(&["ls", "/.snapshots"])
        .iter()
        .filter(|l| l.chars().all(|c| c.is_ascii_digit()))
        .count()
}

pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Days since the last `pacman -Syu` per /var/log/pacman.log
/// (timestamps are UTC, e.g. [2026-06-11T00:08:11+0000]). Reads only a
/// tail chunk — the log grows for years, and the last upgrade is at the
/// end of it.
pub(crate) fn last_update_days() -> Option<u64> {
    let mut f = fs::File::open("/var/log/pacman.log").ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(65_536))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let s = String::from_utf8_lossy(&buf);
    let line = s.lines().rev().find(|l| l.contains("starting full system upgrade"))?;
    let ts = line.split(']').next()?.trim_start_matches('[');
    let (date, time) = ts.split_once('T')?;
    let mut dp = date.split('-');
    let (y, m, d): (i64, i64, i64) = (
        dp.next()?.parse().ok()?,
        dp.next()?.parse().ok()?,
        dp.next()?.parse().ok()?,
    );
    let mut tp = time.get(..8)?.split(':');
    let (hh, mm, ss): (i64, i64, i64) = (
        tp.next()?.parse().ok()?,
        tp.next()?.parse().ok()?,
        tp.next()?.parse().ok()?,
    );
    let then = days_from_civil(y, m, d) * 86400 + hh * 3600 + mm * 60 + ss;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(((now - then).max(0) / 86400) as u64)
}

/// (claude transcripts touched in 24h, total transcripts) — Claude Code
/// activity on the host, from the user's own ~/.claude state.
pub(crate) fn claude_usage() -> (usize, usize) {
    let base = std::env::var("HOME").map(|h| format!("{h}/.claude/projects")).unwrap_or_default();
    let lines = |args: &[&str]| {
        Command::new("find")
            .args(args)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
            .unwrap_or(0)
    };
    let recent = lines(&[&base, "-name", "*.jsonl", "-mtime", "-1"]);
    let total = lines(&[&base, "-name", "*.jsonl"]);
    (recent, total)
}

/// Full Claude Code activity scan for the CLAUDE tile: per-project
/// transcript counts + last-touch times, a 14-day daily-activity strip,
/// whether the CLI has ever been signed in, and the 24h/total counts.
/// Pure fs::metadata walk of ~/.claude/projects — mtimes only, no
/// transcript parsing, no subprocesses.
pub(crate) fn claude_scan() -> (Vec<(String, usize, u64)>, VecDeque<f64>, bool, usize, usize) {
    let home = std::env::var("HOME").unwrap_or_default();
    // Signed-in means credentials, not config — merely running `claude
    // --version` creates ~/.claude.json without any login.
    let authed = std::path::Path::new(&format!("{home}/.claude/.credentials.json")).exists()
        || std::fs::read_to_string(format!("{home}/.claude.json"))
            .map(|s| s.contains("\"oauthAccount\""))
            .unwrap_or(false);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut projects: Vec<(String, usize, u64)> = Vec::new();
    let mut daily = [0u64; 14];
    let mut recent = 0usize;
    let mut total = 0usize;
    if let Ok(dirs) = std::fs::read_dir(format!("{home}/.claude/projects")) {
        for dir in dirs.flatten() {
            if !dir.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let raw = dir.file_name().to_string_lossy().to_string();
            // Project dirs are slugged absolute paths ("-home-iam-farfield-os");
            // strip the home prefix so the box's own checkouts read clean.
            let name = raw
                .strip_prefix(&format!("-{}-", home.trim_start_matches('/').replace('/', "-")))
                .unwrap_or(raw.trim_start_matches('-'))
                .to_string();
            let mut count = 0usize;
            let mut last = 0u64;
            if let Ok(files) = std::fs::read_dir(dir.path()) {
                for f in files.flatten() {
                    if f.path().extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }
                    count += 1;
                    let mtime = f
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    last = last.max(mtime);
                    let age = now.saturating_sub(mtime);
                    if age < 86_400 {
                        recent += 1;
                    }
                    let days = (age / 86_400) as usize;
                    if days < 14 {
                        daily[13 - days] += 1;
                    }
                }
            }
            total += count;
            if count > 0 {
                projects.push((name, count, last));
            }
        }
    }
    projects.sort_by(|a, b| b.2.cmp(&a.2));
    projects.truncate(8);
    (projects, daily.iter().map(|v| *v as f64).collect(), authed, recent, total)
}

/// "3m" / "5h" / "2d" since an epoch, or "—" for never.
pub(crate) fn ago(epoch: u64) -> String {
    if epoch == 0 {
        return "—".into();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let s = now.saturating_sub(epoch);
    if s < 3600 {
        format!("{}m", (s / 60).max(1))
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

/// "Mon 2026-06-15" from the prune timer's next elapse, or "".
pub(crate) fn prune_timer_next() -> String {
    Command::new("systemctl")
        .args(["show", "ff-docker-prune.timer", "-p", "NextElapseUSecRealtime", "--value"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

pub(crate) fn caddy_sites() -> Vec<Site> {
    let hostish = |h: &str| !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    let site = |host: &str, kind: &str| Site {
        host: host.to_string(),
        kind: kind.to_string(),
        ok: None,
        code: 0,
        ms: 0,
    };
    let mut out = Vec::new();
    if let Ok(s) = fs::read_to_string(format!("{STACK}/Caddyfile")) {
        for l in s.lines() {
            let l = l.trim_end();
            if let Some(h) = l.strip_suffix(":80 {") {
                if h.ends_with(".local") && hostish(h) {
                    out.push(site(h, "private"));
                }
            } else if let Some(h) = l.strip_prefix("http://").and_then(|r| r.strip_suffix(":8080 {")) {
                if hostish(h) {
                    out.push(site(h, "public"));
                }
            }
        }
    }
    let apex = fs::read_to_string(format!("{STACK}/.env"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("PREVIEW_APEX=").map(|v| v.trim_matches(['"', '\'', ' ']).to_string()))
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "previews.example.com".into());
    if let Ok(dir) = fs::read_dir(format!("{STACK}/preview-handles")) {
        let mut previews: Vec<String> = dir
            .flatten()
            .filter_map(|e| {
                e.file_name()
                    .to_str()?
                    .strip_suffix(".caddy")
                    .map(|stem| format!("{stem}.{apex}"))
            })
            .collect();
        previews.sort();
        out.extend(previews.into_iter().map(|h| site(&h, "preview")));
    }
    out
}

/// Root filesystem usage. `None` when `df` failed or its output didn't
/// parse — rendering that as 0% would paint an empty disk, which is the
/// most reassuring possible lie about a full one.
pub(crate) fn disk_usage() -> (Option<u8>, String) {
    Command::new("df")
        .args(["-h", "/"])
        .output()
        .ok()
        .and_then(|o| {
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            let f: Vec<&str> = out.lines().nth(1)?.split_whitespace().collect();
            let pct = f.get(4)?.trim_end_matches('%').parse().ok()?;
            Some((Some(pct), format!("{}/{}", f.get(2)?, f.get(1)?)))
        })
        .unwrap_or((None, "?".into()))
}

// ---------------------------------------------------------------------------
