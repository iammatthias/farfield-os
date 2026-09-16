//! Reading the machine: the Docker Engine API over its unix socket, and the
//! host metrics that come straight from /proc and /sys with no subprocess.
//!
//! Sampling is deliberately allocation-light and fork-free — this runs every
//! two seconds, forever, on a box whose job is to run other things.

use std::{
    collections::VecDeque,
    fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::app::*;
use crate::model::*;


pub(crate) fn docker_get(path: &str) -> Result<Value, String> {
    let mut s = UnixStream::connect(DOCKER_SOCK).map_err(|e| format!("docker.sock: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok();
    s.set_write_timeout(Some(Duration::from_secs(5))).ok();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: docker\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let pos = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("malformed HTTP response")?;
    let headers = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
    let body = &buf[pos + 4..];
    let body = if headers.contains("transfer-encoding: chunked") {
        dechunk(body)?
    } else {
        body.to_vec()
    };
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

pub(crate) fn dechunk(data: &[u8]) -> Result<Vec<u8>, String> {
    // All indexing via .get() — dockerd restarting mid-response can
    // truncate the stream anywhere (even right after a chunk's bytes),
    // and that must surface as Err, never a panic.
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let rest = data.get(i..).ok_or("truncated chunk stream")?;
        let nl = rest
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("bad chunk header")?;
        let hex: String = String::from_utf8_lossy(&rest[..nl])
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        let size = usize::from_str_radix(&hex, 16).map_err(|e| e.to_string())?;
        if size == 0 {
            return Ok(out);
        }
        let start = nl + 2;
        let end = start.checked_add(size).ok_or("bad chunk size")?;
        out.extend_from_slice(rest.get(start..end).ok_or("short chunk")?);
        i += end + 2;
    }
}

// ---------------------------------------------------------------------------
// Host sampling (/proc + /sys, no subprocesses)
// ---------------------------------------------------------------------------

pub(crate) fn push(series: &mut VecDeque<f64>, v: f64) {
    series.push_back(v);
    while series.len() > KEEP {
        series.pop_front();
    }
}

/// (busy, total) jiffies per core, plus the aggregate "cpu " line.
pub(crate) fn read_cpu_stat() -> Option<(Vec<(u64, u64)>, (u64, u64))> {
    let s = fs::read_to_string("/proc/stat").ok()?;
    let mut cores = Vec::new();
    let mut agg = None;
    for l in s.lines() {
        if !l.starts_with("cpu") {
            break;
        }
        let f: Vec<u64> = l.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect();
        if f.len() < 5 {
            continue;
        }
        let idle = f[3] + f.get(4).copied().unwrap_or(0); // idle + iowait
        let total: u64 = f.iter().sum();
        let entry = (total - idle, total);
        if l.starts_with("cpu ") {
            agg = Some(entry);
        } else {
            cores.push(entry);
        }
    }
    Some((cores, agg?))
}

/// (used, total, available, buff+cache, swap_used, swap_total) in MiB.
pub(crate) fn meminfo() -> Option<(f64, f64, f64, f64, f64, f64)> {
    let s = fs::read_to_string("/proc/meminfo").ok()?;
    let get = |k: &str| {
        s.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|kb| kb / 1024.0) // MiB
    };
    let total = get("MemTotal:")?;
    let avail = get("MemAvailable:")?;
    let cache = get("Buffers:").unwrap_or(0.0) + get("Cached:").unwrap_or(0.0);
    let st = get("SwapTotal:").unwrap_or(0.0);
    let sf = get("SwapFree:").unwrap_or(0.0);
    Some((total - avail, total, avail, cache, st - sf, st))
}

/// Interface holding the default route — bridge/veth traffic would
/// double-count container bytes already shown per container.
pub(crate) fn default_iface() -> Option<String> {
    let s = fs::read_to_string("/proc/net/route").ok()?;
    s.lines().skip(1).find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.get(1) == Some(&"00000000") {
            f.first().map(|v| v.to_string())
        } else {
            None
        }
    })
}

pub(crate) fn iface_bytes(iface: &str) -> Option<(u64, u64)> {
    let s = fs::read_to_string("/proc/net/dev").ok()?;
    s.lines().find_map(|l| {
        let l = l.trim();
        let rest = l.strip_prefix(&format!("{iface}:"))?;
        let f: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        Some((*f.first()?, *f.get(8)?))
    })
}

/// Whole-disk read/written bytes summed across physical disks.
pub(crate) fn disk_io_bytes() -> Option<(u64, u64)> {
    let s = fs::read_to_string("/proc/diskstats").ok()?;
    let mut r = 0u64;
    let mut w = 0u64;
    let mut any = false;
    for l in s.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        let Some(name) = f.get(2) else { continue };
        // sd whole disks are "sd" + letters (sda … sdaa); partitions
        // append digits. nvme whole disks have no 'p'.
        let whole_disk = (name.starts_with("nvme") && !name.contains('p'))
            || (name.starts_with("sd")
                && name.len() > 2
                && name.chars().skip(2).all(|c| c.is_ascii_alphabetic()));
        if !whole_disk {
            continue;
        }
        // A malformed line skips, not aborts — one bad row shouldn't
        // blank the whole io graph.
        let (Some(rs), Some(ws)) = (f.get(5), f.get(9)) else { continue };
        let (Ok(rv), Ok(wv)) = (rs.parse::<u64>(), ws.parse::<u64>()) else { continue };
        r += rv * 512;
        w += wv * 512;
        any = true;
    }
    any.then_some((r, w))
}

pub(crate) fn cpu_temp() -> Option<f64> {
    // Prefer the CPU's own driver. Taking the hottest sensor in
    // /sys/class/hwmon indiscriminately reports whatever chip runs
    // warmest — often the NVMe drive — under a "CPU" label.
    const CPU_DRIVERS: [&str; 4] = ["coretemp", "k10temp", "zenpower", "cpu_thermal"];
    let mut cpu_best: Option<f64> = None;
    let mut any_best: Option<f64> = None;

    for hw in fs::read_dir("/sys/class/hwmon").ok()?.flatten() {
        let is_cpu = fs::read_to_string(hw.path().join("name"))
            .map(|n| CPU_DRIVERS.contains(&n.trim()))
            .unwrap_or(false);
        if let Ok(entries) = fs::read_dir(hw.path()) {
            for e in entries.flatten() {
                let n = e.file_name();
                let n = n.to_string_lossy().to_string();
                if n.starts_with("temp") && n.ends_with("_input") {
                    if let Some(c) = fs::read_to_string(e.path())
                        .ok()
                        .and_then(|v| v.trim().parse::<f64>().ok())
                        .map(|v| v / 1000.0)
                        .filter(|v| (1.0..150.0).contains(v))
                    {
                        any_best = Some(any_best.map_or(c, |b: f64| b.max(c)));
                        if is_cpu {
                            cpu_best = Some(cpu_best.map_or(c, |b: f64| b.max(c)));
                        }
                    }
                }
            }
        }
    }
    // Fall back to the old behavior on boards with no recognized CPU
    // driver — a number from the wrong chip still beats no number.
    cpu_best.or(any_best)
}

pub(crate) fn sample_host(h: &mut Host) {
    if let Some((cores, agg)) = read_cpu_stat() {
        if let Some((pcores, pagg)) = &h.prev_cpu {
            let pct = |cur: (u64, u64), prev: (u64, u64)| {
                let dt = cur.1.saturating_sub(prev.1);
                if dt == 0 {
                    0.0
                } else {
                    cur.0.saturating_sub(prev.0) as f64 / dt as f64 * 100.0
                }
            };
            h.cpu_cur = pct(agg, *pagg);
            push(&mut h.cpu, h.cpu_cur);
            h.cores = cores
                .iter()
                .zip(pcores.iter())
                .map(|(c, p)| pct(*c, *p))
                .collect();
            if h.cores_hist.len() != h.cores.len() {
                h.cores_hist = vec![VecDeque::new(); h.cores.len()];
            }
            for (i, p) in h.cores.iter().enumerate() {
                push(&mut h.cores_hist[i], *p);
            }
        }
        h.ncpu = cores.len().max(1);
        h.prev_cpu = Some((cores, agg));
    }
    if let Some((used, total, avail, cache, sused, stotal)) = meminfo() {
        h.mem_cur = used;
        h.mem_total = total;
        h.mem_avail = avail;
        h.mem_cache = cache;
        h.swap_used = sused;
        h.swap_total = stotal;
        push(&mut h.mem_used, used);
    }
    let now = Instant::now();
    if h.iface.is_empty() {
        h.iface = default_iface().unwrap_or_else(|| "eth0".into());
    }
    if let Some((rx, tx)) = iface_bytes(&h.iface) {
        if let Some((at, prx, ptx)) = h.prev_net {
            let dt = now.duration_since(at).as_secs_f64();
            if dt > 0.0 {
                h.rx_cur = rx.saturating_sub(prx) as f64 / dt;
                h.tx_cur = tx.saturating_sub(ptx) as f64 / dt;
                push(&mut h.rx, h.rx_cur);
                push(&mut h.tx, h.tx_cur);
            }
        }
        h.rx_total = rx;
        h.tx_total = tx;
        h.prev_net = Some((now, rx, tx));
    }
    if let Ok(s) = fs::read_to_string("/proc/net/sockstat") {
        if let Some(l) = s.lines().find(|l| l.starts_with("TCP:")) {
            let f: Vec<&str> = l.split_whitespace().collect();
            h.tcp_inuse = f.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
            h.tcp_tw = f.get(6).and_then(|v| v.parse().ok()).unwrap_or(0);
            push(&mut h.tcp_hist, (h.tcp_inuse + h.tcp_tw) as f64);
        }
    }
    // Wireless link quality — this box's uplink is wifi, so the radio
    // is load-bearing telemetry. /proc/net/wireless: link is x/70.
    if h.iface.starts_with("wl") {
        if let Ok(s) = fs::read_to_string("/proc/net/wireless") {
            let prefix = format!("{}:", h.iface);
            if let Some(l) = s.lines().find(|l| l.trim_start().starts_with(&prefix)) {
                let f: Vec<&str> = l.split_whitespace().collect();
                let q = f.get(2).and_then(|v| v.trim_end_matches('.').parse::<f64>().ok()).unwrap_or(0.0);
                h.wifi_dbm = f.get(3).and_then(|v| v.trim_end_matches('.').parse::<f64>().ok()).unwrap_or(0.0);
                push(&mut h.wifi, (q / 70.0 * 100.0).clamp(0.0, 100.0));
            }
        }
    }
    if let Some((r, w)) = disk_io_bytes() {
        if let Some((at, pr, pw)) = h.prev_io {
            let dt = now.duration_since(at).as_secs_f64();
            if dt > 0.0 {
                h.io_r_cur = r.saturating_sub(pr) as f64 / dt;
                h.io_w_cur = w.saturating_sub(pw) as f64 / dt;
                push(&mut h.io_r, h.io_r_cur);
                push(&mut h.io_w, h.io_w_cur);
            }
        }
        h.io_r_total = r;
        h.io_w_total = w;
        h.prev_io = Some((now, r, w));
    }
    if let Ok(s) = fs::read_to_string("/proc/uptime") {
        h.uptime = s.split('.').next().and_then(|v| v.parse().ok()).unwrap_or(0);
    }
    if let Ok(s) = fs::read_to_string("/proc/loadavg") {
        h.load = s.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        push(&mut h.load_hist, h.load);
    }
    if h.hostname.is_empty() {
        h.hostname = fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).unwrap_or_default();
    }
    h.temp_c = cpu_temp();
    if let Some(t) = h.temp_c {
        push(&mut h.temp_hist, t);
    }
    // CPU package power via RAPL. Root-only by default; farfield os ships a
    // tmpfiles.d rule opening it to 0444 — degrade silently without it.
    if let Some(e) = fs::read_to_string("/sys/class/powercap/intel-rapl:0/energy_uj")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        if let Some((at, pe)) = h.prev_energy {
            let dt = now.duration_since(at).as_secs_f64();
            if dt > 0.0 && e > pe {
                // skip wrap-around samples (e < pe)
                let w = (e - pe) as f64 / 1e6 / dt;
                h.watts = Some(w);
                push(&mut h.watts_hist, w);
            }
        }
        h.prev_energy = Some((now, e));
    }
}

// ---------------------------------------------------------------------------
