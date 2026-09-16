//! The board's state: one `App` the sampler threads write and the render
//! pass reads, plus the per-subject series and snapshots hanging off it.
//!
//! Everything here is behind one mutex. `lock_app` is the only way in, so a
//! panicked sampler thread cannot poison the display.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Mutex,
    time::Instant,
};



// State
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct Series {
    pub(crate) cpu: VecDeque<f64>, // percent
    pub(crate) mem: VecDeque<f64>, // MiB
    pub(crate) cpu_cur: f64,
    pub(crate) mem_cur: f64,
    pub(crate) net_rate: Option<f64>, // bytes/sec; None = unknown or shared netns
    pub(crate) flag: Option<String>,  // "restarting" / "unhealthy"
    pub(crate) prev: Option<Prev>,
}

/// A Caddy vhost plus its live probe result. `ok: None` = not probed
/// yet; `code: 0` = TCP/TLS-connect check only (preview sites).
#[derive(Clone)]
pub(crate) struct Site {
    pub(crate) host: String,
    pub(crate) kind: String,
    pub(crate) ok: Option<bool>,
    pub(crate) code: u16,
    pub(crate) ms: u64,
}

/// 5-minute traffic for one Caddy host: request total, error buckets, and
/// a request-rate sparkline across the window. The shape makes a bot scan
/// or a sudden spike visible where a lone "N/5m" can't.
#[derive(Default, Clone)]
pub(crate) struct Traffic {
    pub(crate) reqs: u32,
    pub(crate) e404: u32,
    pub(crate) e4xx: u32, // 4xx that isn't 404 — the actionable ones
    pub(crate) e5xx: u32,
    pub(crate) spark: Vec<f64>, // per-bin request counts, oldest→newest
}

/// Host alerts with actionable detail: which units, which crashed procs,
/// and a sample of the actual error messages — not bare counts.
#[derive(Default)]
pub(crate) struct Alerts {
    pub(crate) failed_units: Vec<String>, // unit names
    pub(crate) crashes: Vec<String>,      // "process ×N" per coredumped process this hour
    pub(crate) journal_errs: usize,
    pub(crate) err_sample: Vec<String>, // a couple of recent error messages, clipped
    pub(crate) banned_ips: usize,
    pub(crate) ssh_fails: usize,
}

pub(crate) struct Prev {
    pub(crate) at: Instant,
    pub(crate) cpu_total: u64,
    pub(crate) sys_total: u64,
    pub(crate) net_total: Option<u64>,
}

#[derive(Default, Clone)]
pub(crate) struct Host {
    pub(crate) cpu: VecDeque<f64>, // total %
    pub(crate) cpu_cur: f64,
    pub(crate) cores: Vec<f64>,                // per-core %, latest sample
    pub(crate) cores_hist: Vec<VecDeque<f64>>, // per-core history (tile mode)
    pub(crate) temp_c: Option<f64>,
    pub(crate) temp_hist: VecDeque<f64>, // package temp trend (°C)
    pub(crate) mem_used: VecDeque<f64>, // MiB
    pub(crate) mem_cur: f64,
    pub(crate) mem_total: f64,
    pub(crate) mem_avail: f64,
    pub(crate) mem_cache: f64,
    pub(crate) swap_used: f64,
    pub(crate) swap_total: f64,
    pub(crate) iface: String,
    pub(crate) wifi: VecDeque<f64>, // link quality %, only when iface is wireless
    pub(crate) wifi_dbm: f64,
    pub(crate) rx: VecDeque<f64>, // bytes/sec
    pub(crate) tx: VecDeque<f64>,
    pub(crate) rx_cur: f64,
    pub(crate) tx_cur: f64,
    pub(crate) rx_total: u64, // bytes since boot
    pub(crate) tx_total: u64,
    pub(crate) tcp_inuse: u64,
    pub(crate) tcp_tw: u64,
    pub(crate) tcp_hist: VecDeque<f64>, // established+timewait sockets trend
    pub(crate) io_r: VecDeque<f64>, // bytes/sec
    pub(crate) io_w: VecDeque<f64>,
    pub(crate) io_r_cur: f64,
    pub(crate) io_w_cur: f64,
    pub(crate) io_r_total: u64, // bytes since boot
    pub(crate) io_w_total: u64,
    pub(crate) uptime: u64,
    pub(crate) hostname: String,
    pub(crate) load: f64,
    pub(crate) load_hist: VecDeque<f64>,
    pub(crate) ncpu: usize,
    pub(crate) watts: Option<f64>,       // CPU package draw via RAPL (None if unreadable)
    pub(crate) watts_hist: VecDeque<f64>, // package draw trend (W)
    pub(crate) prev_cpu: Option<(Vec<(u64, u64)>, (u64, u64))>, // per-core + total (busy, total)
    pub(crate) prev_net: Option<(Instant, u64, u64)>,
    pub(crate) prev_io: Option<(Instant, u64, u64)>,
    pub(crate) prev_energy: Option<(Instant, u64)>,
}

#[derive(Default)]
pub(crate) struct App {
    pub(crate) host: Host,
    pub(crate) containers: BTreeMap<String, Series>,
    pub(crate) containers_total: usize,
    pub(crate) containers_running: usize, // cheap list-count (no per-container stats)
    pub(crate) docker_ok: bool,           // dockerd answered — 0/0 means zero, not "down"
    pub(crate) services: Vec<(String, String)>,
    pub(crate) sites: Vec<Site>,
    pub(crate) procs_cpu: Vec<String>,
    pub(crate) procs_mem: Vec<String>,
    pub(crate) disk_pct: Option<u8>, // None = df failed; never render that as 0%
    pub(crate) disk_detail: String,
    pub(crate) images: usize,
    pub(crate) prune_next: String,
    pub(crate) updates: Option<usize>,
    pub(crate) sec_updates: Option<usize>, // packages with a known CVE fix available (arch-audit); None = unknown
    pub(crate) claude_runs: usize,  // live claude processes on the host
    pub(crate) claude_24h: usize,   // session transcripts touched in 24h (~/.claude)
    pub(crate) claude_total: usize, // total session transcripts
    pub(crate) claude_authed: bool, // ~/.claude.json exists — the CLI has been signed in
    pub(crate) claude_projects: Vec<(String, usize, u64)>, // (name, transcripts, last-touch epoch), newest first
    pub(crate) claude_daily: VecDeque<f64>, // transcripts last-touched per day, 14 days, oldest first
    pub(crate) traffic: HashMap<String, Traffic>, // per-host 5-minute traffic + sparkline
    pub(crate) failed_units: Vec<String>,
    pub(crate) journal_errs: usize,
    pub(crate) crashes: Vec<String>,
    pub(crate) err_sample: Vec<String>,
    pub(crate) banned_ips: usize,
    pub(crate) ssh_fails: usize,
    pub(crate) reboot_pending: Option<String>,
    pub(crate) last_update_days: Option<u64>,
    pub(crate) ts_ip: String,
    pub(crate) ts_online: usize,
    pub(crate) ts_total: usize,
    pub(crate) nvme_wear: Option<u64>,
    pub(crate) nvme_temp: Option<u64>,
    pub(crate) snapshots: usize,
    pub(crate) docker_err: Option<String>,
    pub(crate) host_err: Option<String>,   // host sampler restarted after a panic
    pub(crate) status_err: Option<String>, // status sampler restarted after a panic
    pub(crate) clock: String,
    pub(crate) render_secs: f64, // seconds since the process started — drives list cycling
}

/// Lock the shared state, recovering from a poisoned mutex — a sampler
/// panic (caught and restarted in its loop) may have poisoned it mid-
/// write; every field is re-sampled shortly, so the last view is fine.
pub(crate) fn lock_app(app: &Mutex<App>) -> std::sync::MutexGuard<'_, App> {
    app.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// Docker Engine API over the unix socket
// ---------------------------------------------------------------------------
