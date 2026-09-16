//! The palette from docs/BRAND.md and the drawing primitives built on it:
//! sparklines, gauges, panels, and the human-readable formatters.
//!
//! Horizon orange is the one rare accent — it marks the thing that needs a
//! person, never ordinary data.

use std::{
    collections::VecDeque,
};

use ratatui::{
    prelude::*,
    widgets::{Block, Paragraph},
};

use crate::app::*;
use crate::model::*;

// Rendering
// ---------------------------------------------------------------------------

// farfield truecolor palette — the product half of docs/BRAND.md (see
// farfield's lib/theme/theme.css): Paper ink on a Deep Space field, warm
// earth tones for data, Horizon orange reserved as the rare signal. foot
// (the kiosk terminal) speaks 24-bit color; on lesser terminals crossterm
// degrades to nearest-match.
pub(crate) const C_BG: Color = Color::Rgb(0x0e, 0x22, 0x2d); // Deep Space
pub(crate) const C_FG: Color = Color::Rgb(0xf3, 0xe5, 0xd1); // Paper
pub(crate) const C_DIM: Color = Color::Rgb(0x6a, 0x86, 0x99); // Signal Mist
pub(crate) const C_BORDER: Color = Color::Rgb(0x2e, 0x3d, 0x44); // Paper 14% over Deep Space
pub(crate) const C_BG_ALT: Color = Color::Rgb(0x13, 0x2f, 0x3d); // panel surface
pub(crate) const C_MIST: Color = Color::Rgb(0x9b, 0xab, 0xb3); // cool data — observation
pub(crate) const C_BLUE: Color = Color::Rgb(0x5b, 0x8a, 0xb8); // Atmosphere, lifted for dark ground
pub(crate) const C_OXIDE: Color = Color::Rgb(0xad, 0x8a, 0x72); // warm data — Oxide, lifted
pub(crate) const C_GREEN: Color = Color::Rgb(0x87, 0xa5, 0x83); // terrestrial green — ok
pub(crate) const C_SUN: Color = Color::Rgb(0xd1, 0xaa, 0x83); // Distant Sun — warn
pub(crate) const C_ALARM: Color = Color::Rgb(0xf2, 0x90, 0x7d); // alarm — never emphasis
pub(crate) const C_SIGNAL: Color = Color::Rgb(0xe5, 0x9f, 0x67); // Horizon — the one rare accent

pub(crate) const GREEN_RGB: (u8, u8, u8) = (0x87, 0xa5, 0x83);
pub(crate) const SUN_RGB: (u8, u8, u8) = (0xd1, 0xaa, 0x83);
pub(crate) const ALARM_RGB: (u8, u8, u8) = (0xf2, 0x90, 0x7d);
pub(crate) const BLUE_RGB: (u8, u8, u8) = (0x5b, 0x8a, 0xb8);
pub(crate) const OXIDE_RGB: (u8, u8, u8) = (0xad, 0x8a, 0x72);
pub(crate) const MIST_RGB: (u8, u8, u8) = (0x9b, 0xab, 0xb3);

pub(crate) fn lerp(a: (u8, u8, u8), b: (u8, u8, u8), t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    let c = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    Color::Rgb(c(a.0, b.0), c(a.1, b.1), c(a.2, b.2))
}

/// terrestrial green → distant sun → alarm as t goes 0 → 1. The signature
/// gradient for anything that means "how loaded is this".
pub(crate) fn heat(t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        lerp(GREEN_RGB, SUN_RGB, t * 2.0)
    } else {
        lerp(SUN_RGB, ALARM_RGB, (t - 0.5) * 2.0)
    }
}

/// Intensity tint of a hue: muted deep-space slate at t=0, full color at
/// t=1. Used for throughput graphs, where "hot" means "busy" not "bad".
pub(crate) fn tint(hue: (u8, u8, u8)) -> impl Fn(f64) -> Color {
    move |t: f64| lerp((0x1f, 0x3b, 0x49), hue, 0.35 + 0.65 * t.clamp(0.0, 1.0))
}

pub(crate) fn dim() -> Style {
    Style::new().fg(C_DIM)
}

pub(crate) fn accent(c: Color) -> Style {
    Style::new().fg(c).add_modifier(Modifier::BOLD)
}

pub(crate) fn section(title: Vec<Span<'static>>) -> Block<'static> {
    Block::bordered()
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(Style::new().fg(C_BORDER))
        .title(Line::from(title))
}

/// Panel chrome, returning the content Rect. On the kiosk, sway already
/// draws a 2px border + gap around every tile, so a second ratatui box in
/// the same color is pure redundant ink (and costs two rows + two cols) —
/// there we render only a bold header row and hand back the rest. The
/// ssh/herdr `full` board has no compositor, so it keeps the box as its
/// only separation between panels. `right` is an optional right-aligned
/// header (clock, etc.).
pub(crate) fn panel(
    frame: &mut Frame,
    area: Rect,
    bordered: bool,
    title: Vec<Span<'static>>,
    right: Option<Line<'static>>,
) -> Rect {
    if bordered {
        let mut block = section(title);
        if let Some(r) = right {
            block = block.title(r.right_aligned());
        }
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    } else {
        let hdr = Rect { height: 1, ..area };
        frame.render_widget(Paragraph::new(Line::from(title)), hdr);
        if let Some(r) = right {
            frame.render_widget(Paragraph::new(r).right_aligned(), hdr);
        }
        Rect { y: area.y + 1, height: area.height.saturating_sub(1), ..area }
    }
}

/// Single-row sparkline string — the per-container mini charts.
pub(crate) fn spark(vals: &VecDeque<f64>, width: usize, floor: f64, head: f64) -> String {
    let take = vals.len().min(width);
    let slice: Vec<f64> = vals.iter().skip(vals.len() - take).copied().collect();
    let mut max = floor;
    for v in &slice {
        if *v > max {
            max = *v;
        }
    }
    max = (max * head).max(f64::MIN_POSITIVE);
    let mut s = " ".repeat(width - take);
    for v in slice {
        let idx = ((v / max * 7.0).round() as usize).min(7);
        s.push(TICKS[idx]);
    }
    s
}

/// Sparkline scaled to a FIXED absolute ceiling, not the data's own max.
/// For level metrics (temp, watts, sockets) this is what makes a steady
/// reading render as a calm low line instead of an alarming full-height
/// block — auto-scaling a flat signal always pins it to the top.
pub(crate) fn spark_abs(vals: &VecDeque<f64>, width: usize, max: f64) -> String {
    let take = vals.len().min(width);
    let max = max.max(f64::MIN_POSITIVE);
    let mut s = " ".repeat(width - take);
    for v in vals.iter().skip(vals.len() - take) {
        s.push(TICKS[((v / max * 7.0).round() as usize).min(7)]);
    }
    s
}

/// Sparkline from a fixed slice (already bucketed), scaled to its own max
/// with `floor` as the minimum ceiling. Width == slice length.
pub(crate) fn spark_vec(vals: &[f64], floor: f64, head: f64) -> String {
    let mut max = floor;
    for v in vals {
        if *v > max {
            max = *v;
        }
    }
    max = (max * head).max(f64::MIN_POSITIVE);
    vals.iter()
        .map(|v| TICKS[((v / max * 7.0).round() as usize).min(7)])
        .collect()
}

/// Multi-row graph with per-column color, drawn in eighth-block resolution
/// across `rows` lines. `scale` maps a RAW sample to a 0..1 fill fraction
/// and `color` maps the same raw sample to its color.
///
/// Scales are FIXED, not peak-relative. Peak-relative height made an idle
/// box look busy — a 100 KB/s journal blip filled the whole panel because
/// nothing bigger had happened lately, and the same bar height meant a
/// different magnitude every few minutes. With a fixed axis a given height
/// always means the same thing; compression curves (sqrt for percentages,
/// log for throughput) keep small activity visible as low texture without
/// inflating it. Exact numbers live in the panel headers.
pub(crate) fn graph_lines(
    vals: &VecDeque<f64>,
    width: usize,
    rows: usize,
    scale: impl Fn(f64) -> f64,
    color: impl Fn(f64) -> Color,
) -> Vec<Line<'static>> {
    let take = vals.len().min(width);
    let slice: Vec<f64> = vals.iter().skip(vals.len() - take).copied().collect();
    let pad = width - take;
    let mut lines = Vec::with_capacity(rows);
    for r in 0..rows {
        let mut spans = Vec::with_capacity(take + 1);
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        for v in &slice {
            let th = scale(*v).clamp(0.0, 1.0);
            let eighths = (th * (rows * 8) as f64).round() as usize;
            let filled = eighths.saturating_sub((rows - 1 - r) * 8).min(8);
            if filled == 0 {
                spans.push(Span::raw(" "));
            } else {
                spans.push(Span::styled(TICKS[filled - 1].to_string(), Style::new().fg(color(*v))));
            }
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// 0..1 position of throughput `v` (bytes/s) on a fixed log axis from
/// `floor` to `ceil`. Throughput spans five orders of magnitude, so a
/// linear axis is useless at both ends; log keeps 100 KB/s visible (~⅓
/// height on the net axis) while 100 MB/s still reads clearly bigger.
pub(crate) fn log_t(v: f64, floor: f64, ceil: f64) -> f64 {
    if v <= floor {
        0.0
    } else {
        ((v / floor).ln() / (ceil / floor).ln()).clamp(0.0, 1.0)
    }
}

/// Fixed throughput axes. NET tops out at 1 GbE line rate; DISK at a
/// realistic NVMe sequential-write ceiling. Floors are where the bar
/// starts registering — below that is noise.
pub(crate) const NET_FLOOR: f64 = 1024.0; // 1 KB/s
pub(crate) const NET_CEIL: f64 = 125.0 * 1_000_000.0; // 1 GbE ≈ 125 MB/s
pub(crate) const IO_FLOOR: f64 = 10.0 * 1024.0; // 10 KB/s
pub(crate) const IO_CEIL: f64 = 2.0 * 1_073_741_824.0; // ~2 GB/s NVMe

pub(crate) const PAGE_SECS: f64 = 5.0; // seconds each page of a cycled list is shown

/// Cycle a list of rendered lines through pages, advancing every PAGE_SECS
/// so a long list is shown in full over time rather than crammed. Pages are
/// sized to fit `rows`, or to `cap` items when `cap > 0 and < rows` — that
/// forces a calmer page even when more would fit. Appends a dim "⟳ n/m"
/// indicator while paging; returns the list unchanged when it all fits.
pub(crate) fn paged(lines: Vec<Line<'static>>, rows: usize, cap: usize, secs: f64) -> Vec<Line<'static>> {
    if rows <= 1 {
        return lines;
    }
    let mut per = rows - 1; // reserve a row for the indicator
    if cap > 0 {
        per = per.min(cap);
    }
    if lines.len() <= per || (cap == 0 && lines.len() <= rows) {
        return lines;
    }
    let pages = lines.len().div_ceil(per);
    let page = ((secs / PAGE_SECS) as usize) % pages;
    let start = page * per;
    let end = (start + per).min(lines.len());
    let mut out: Vec<Line<'static>> = lines[start..end].to_vec();
    out.push(Line::from(Span::styled(format!("⟳ {}/{}", page + 1, pages), dim())));
    out
}

pub(crate) fn human_mem(mib: f64) -> String {
    if mib < 1024.0 {
        format!("{mib:5.0}M")
    } else {
        format!("{:5.1}G", mib / 1024.0)
    }
}

pub(crate) fn human_rate(rate: Option<f64>) -> String {
    match rate {
        None => "    -  ".into(),
        Some(b) if b < 1024.0 => format!("{b:4.0}B/s"),
        Some(b) if b < 1048576.0 => format!("{:4.1}K/s", b / 1024.0),
        Some(b) if b < 1073741824.0 => format!("{:4.1}M/s", b / 1048576.0),
        Some(b) => format!("{:4.1}G/s", b / 1073741824.0),
    }
}

pub(crate) fn human_size(bytes: f64) -> String {
    if bytes < 1048576.0 {
        format!("{:.0}K", bytes / 1024.0)
    } else if bytes < 1073741824.0 {
        format!("{:.1}M", bytes / 1048576.0)
    } else if bytes < 1099511627776.0 {
        format!("{:.1}G", bytes / 1073741824.0)
    } else {
        format!("{:.2}T", bytes / 1099511627776.0)
    }
}

/// "label ▓▓▓░░░ value" breakdown row (tile-mode MEM panel).
pub(crate) fn gauge_line(label: &'static str, frac: f64, value: String, color: Color, width: usize) -> Line<'static> {
    let frac = if frac.is_finite() { frac.clamp(0.0, 1.0) } else { 0.0 };
    let barw = width.saturating_sub(24).clamp(8, 40);
    let filled = (frac * barw as f64).round() as usize;
    Line::from(vec![
        Span::styled(format!("{label:<7}"), dim()),
        Span::styled("▓".repeat(filled), Style::new().fg(color)),
        Span::styled("░".repeat(barw - filled), Style::new().fg(C_BORDER)),
        Span::raw(format!(" {value}")),
    ])
}

/// "TOP CPU/MEM" process table (tile-mode CPU + MEM panels). `by_mem`
/// picks which column gets the heat color.
pub(crate) fn proc_lines(title: &'static str, procs: &[String], n: usize, by_mem: bool) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::default(),
        Line::styled(title, accent(C_BLUE)),
        Line::styled(format!("{:>5} {:>5}  {}", "CPU%", "MEM%", "COMMAND"), dim()),
    ];
    for p in procs.iter().take(n) {
        let f: Vec<&str> = p.split_whitespace().collect();
        if f.len() < 3 {
            continue;
        }
        let cpu: f64 = f[0].parse().unwrap_or(0.0);
        let mem: f64 = f[1].parse().unwrap_or(0.0);
        let (cpu_style, mem_style) = if by_mem {
            (dim(), Style::new().fg(heat(mem / 25.0)))
        } else {
            (Style::new().fg(heat(cpu / 50.0)), dim())
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{:>5}", f[0]), cpu_style),
            Span::styled(format!(" {:>5}", f[1]), mem_style),
            Span::styled(format!("  {}", f[2..].join(" ")), Style::new().fg(C_FG)),
        ]));
    }
    lines
}

pub(crate) fn human_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = secs % 86400 / 3600;
    let m = secs % 3600 / 60;
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

pub(crate) fn dot_color(state: &str) -> Color {
    match state {
        "active" | "running" | "ok" | "up" => C_GREEN,
        "inactive" | "failed" | "exited" | "dead" | "down" => C_ALARM,
        _ => C_SUN,
    }
}

/// A restarted-after-panic sampler surfaces here — one red line at the
/// bottom of the frame, instead of the tile silently freezing. (docker's
/// equivalent already renders inside the CONTAINERS panel.)
pub(crate) fn sampler_err_line(frame: &mut Frame, app: &App) {
    let Some(e) = app.host_err.as_ref().or(app.status_err.as_ref()) else { return };
    let area = frame.area();
    if area.height == 0 {
        return;
    }
    let line = Rect { y: area.y + area.height - 1, height: 1, ..area };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(format!(" {e} "), Style::new().fg(C_ALARM)))).right_aligned(),
        line,
    );
}
