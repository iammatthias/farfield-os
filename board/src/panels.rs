//! Panel contents built as lines rather than drawn directly — container rows,
//! service and site tables, alerts, ops — plus the tappable buttons.
//!
//! Split from the drawing code because these are pure functions of `App`:
//! what to say, with where to put it left to render.rs.


use ratatui::{
    prelude::*,
    widgets::{Block, Paragraph},
};

use crate::app::*;
use crate::facts::*;
use crate::model::*;
use crate::theme::*;

/// A rounded, tappable control. Bordered + labelled in the action's accent
/// colour on the dark background (matches the tiles); an armed button fills
/// solid so the "tap again" state is unmistakable. Records the hit box.
pub(crate) fn button(frame: &mut Frame, rect: Rect, action: Action, armed: bool, touch: &mut Touch) {
    let color = if armed { C_SUN } else { action.color() };
    let label = if armed { format!("{}?", action.label()) } else { action.label().to_string() };
    let mut block = Block::bordered()
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(Style::new().fg(color));
    if armed {
        block = block.style(Style::new().bg(color));
    }
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let fg = if armed { C_BG } else { color };
    let pad = (inner.height.saturating_sub(1) / 2) as usize;
    let mut lines: Vec<Line> = vec![Line::default(); pad];
    lines.push(Line::from(Span::styled(label, Style::new().fg(fg).add_modifier(Modifier::BOLD))).centered());
    frame.render_widget(Paragraph::new(lines), inner);
    touch.buttons.push(Btn { rect, action });
}

/// The action button row across the bottom of the fullscreen OPS view.
pub(crate) fn render_buttons(frame: &mut Frame, area: Rect, touch: &mut Touch) {
    touch.buttons.clear();
    let n = BUTTONS.len() as u16;
    let gap = 2u16;
    if area.width < n * 8 || area.height < 3 {
        return;
    }
    let bw = (area.width - gap * (n - 1)) / n;
    for (i, &action) in BUTTONS.iter().enumerate() {
        let rect = Rect { x: area.x + i as u16 * (bw + gap), y: area.y, width: bw, height: area.height };
        let armed = matches!(touch.armed, Some((a, _)) if a == action);
        button(frame, rect, action, armed, touch);
    }
}

/// The back button in the top-left of any fullscreen tile.
pub(crate) fn render_back(frame: &mut Frame, area: Rect, touch: &mut Touch) {
    let rect = Rect { width: 12u16.min(area.width), ..area };
    let block = Block::bordered()
        .border_set(ratatui::symbols::border::ROUNDED)
        .border_style(Style::new().fg(C_DIM));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("← back", accent(C_FG))).centered()),
        inner,
    );
    touch.back = Some(rect);
}

pub(crate) fn containers_panel(app: &App, width: usize, height: usize) -> Vec<Line<'static>> {
    let namew = 26usize;
    let sw = ((width.saturating_sub(namew + 2 + 7 + 3 + 7 + 3 + 8)) / 2).clamp(8, 56);
    let rowlen = namew + 2 + sw + 7 + 3 + sw + 7 + 3 + 8;
    let memblue = C_BLUE;

    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{:<w$}", "NAME", w = namew + 2), dim()),
            Span::styled(format!("{:<w$}", "CPU", w = sw + 1 + 6 + 3), dim()),
            Span::styled(format!("{:<w$}", "MEM", w = sw + 1 + 6 + 3), dim()),
            Span::styled("NET".to_string(), dim()),
        ]),
        Line::default(),
    ];

    if let Some(err) = &app.docker_err {
        lines.push(Line::styled(format!("docker unavailable: {err}"), Style::new().fg(C_ALARM)));
        return lines;
    }

    let mut rows: Vec<Line<'static>> = Vec::new();
    for (i, (name, s)) in app.containers.iter().enumerate() {
        let t = (s.cpu_cur / 100.0).clamp(0.0, 1.0);
        let name_color = if s.flag.is_some() {
            C_ALARM // restarting / unhealthy
        } else if name.starts_with("ff-") || name.starts_with("gnar") {
            C_MIST
        } else {
            C_BLUE
        };
        let shown: String = name.chars().take(namew).collect();
        let net_style = match s.net_rate {
            Some(r) if r >= 1024.0 => Style::new().fg(C_MIST),
            _ => dim(),
        };
        let mut spans = vec![
            Span::styled(format!("{shown:<namew$}  "), Style::new().fg(name_color)),
            Span::styled(spark(&s.cpu, sw, 5.0, 1.0), Style::new().fg(heat(t))),
            Span::styled(format!(" {:5.1}%", s.cpu_cur), Style::new().fg(heat(t))),
            Span::raw("   "),
            Span::styled(spark(&s.mem, sw, 1.0, 1.25), Style::new().fg(memblue)),
            Span::styled(format!(" {}", human_mem(s.mem_cur)), Style::new().fg(C_FG)),
            Span::raw("   "),
            Span::styled(human_rate(s.net_rate), net_style),
        ];
        // Pad to the panel edge so the zebra stripe runs full width.
        spans.push(Span::raw(" ".repeat(width.saturating_sub(rowlen))));
        let mut line = Line::from(spans);
        if i % 2 == 1 {
            line = line.style(Style::new().bg(C_BG_ALT));
        }
        rows.push(line);
    }
    if rows.is_empty() {
        lines.push(Line::styled("(no running containers)", dim()));
    } else {
        // Cycle ONLY when the list overflows the tile (small screens) — on a
        // big tile all 20 fit, so showing fewer would just leave a void.
        lines.extend(paged(rows, height.saturating_sub(2), 0, app.render_secs));
    }
    lines
}

/// Host services, two per row — six units in three lines.
pub(crate) fn service_rows(app: &App) -> Vec<Line<'static>> {
    app.services
        .chunks(2)
        .map(|pair| {
            let mut spans = Vec::new();
            for (name, state) in pair {
                spans.push(Span::styled("● ".to_string(), Style::new().fg(dot_color(state))));
                spans.push(Span::styled(format!("{name:<14}"), Style::new().fg(C_FG)));
                spans.push(Span::styled(format!("{state:<12}"), dim()));
            }
            Line::from(spans)
        })
        .collect()
}

/// Probed site rows: dot = live health, then host, probe result,
/// 5-minute traffic (from the caddy access log), kind.
pub(crate) fn site_rows(app: &App) -> Vec<Line<'static>> {
    app.sites
        .iter()
        .map(|site| {
            let (dotc, info) = match site.ok {
                Some(true) if site.code == 0 => (C_GREEN, format!("tls {}ms", site.ms)),
                Some(true) => (C_GREEN, format!("{} {}ms", site.code, site.ms)),
                Some(false) if site.code > 0 => (C_ALARM, format!("{} {}ms", site.code, site.ms)),
                Some(false) => (C_ALARM, "down".to_string()),
                None => (C_SUN, String::new()),
            };
            let info_style = if matches!(site.ok, Some(false)) { Style::new().fg(C_ALARM) } else { dim() };
            let kind_color = match site.kind.as_str() {
                "private" => C_MIST,
                "public" => C_OXIDE,
                _ => C_SUN,
            };
            // Truncate long hostnames — format width pads but never cuts.
            let mut host: String = site.host.chars().take(24).collect();
            if site.host.chars().count() > 24 {
                host.pop();
                host.push('…');
            }
            let mut spans = vec![
                Span::styled("● ".to_string(), Style::new().fg(dotc)),
                Span::styled(format!("{host:<25}"), Style::new().fg(C_FG)),
                Span::styled(format!("{info:<10}"), info_style),
            ];
            match app.traffic.get(&site.host) {
                Some(t) if t.reqs > 0 => {
                    let sp = if t.spark.is_empty() {
                        " ".repeat(TRAFFIC_BINS)
                    } else {
                        spark_vec(&t.spark, 1.0, 1.1)
                    };
                    spans.push(Span::styled(sp, dim()));
                    spans.push(Span::styled(format!(" {:>4}/5m ", t.reqs), Style::new().fg(C_MIST)));
                    // Priority: server errors > actionable 4xx > 404 noise.
                    // 404s are dimmed so they read as background hum, not a fault.
                    if t.e5xx > 0 {
                        spans.push(Span::styled(format!("{}×5xx ", t.e5xx), Style::new().fg(C_ALARM)));
                    } else if t.e4xx > 0 {
                        spans.push(Span::styled(format!("{}×4xx ", t.e4xx), Style::new().fg(C_SUN)));
                    } else if t.e404 > 0 {
                        spans.push(Span::styled(format!("{}×404 ", t.e404), dim()));
                    } else {
                        spans.push(Span::raw("      "));
                    }
                }
                _ => {
                    spans.push(Span::raw(" ".repeat(TRAFFIC_BINS)));
                    spans.push(Span::styled("    -/5m      ", dim()));
                }
            }
            spans.push(Span::styled(site.kind.clone(), Style::new().fg(kind_color)));
            Line::from(spans)
        })
        .collect()
}

/// One-line docker ops summary: running/total, images, next prune,
/// pending updates.
pub(crate) fn docker_line(app: &App) -> Line<'static> {
    if !app.docker_ok {
        return Line::from(vec![
            Span::styled("● ".to_string(), Style::new().fg(C_ALARM)),
            Span::styled("dockerd unreachable".to_string(), Style::new().fg(C_ALARM)),
        ]);
    }
    let mut spans = vec![
        Span::styled("● ".to_string(), Style::new().fg(C_GREEN)),
        Span::styled(
            format!("{}/{} containers running", app.containers_running, app.containers_total),
            Style::new().fg(C_FG),
        ),
        Span::styled(format!("   {} images", app.images), dim()),
    ];
    if !app.prune_next.is_empty() {
        spans.push(Span::styled(format!("   prune {}", app.prune_next), dim()));
    }
    // Updates / security / reboot live in the OPS tile's SECURITY section,
    // which renders right below this line — no need to repeat them here.
    Line::from(spans)
}

/// One-line Claude Code summary: live host processes + session stats.
/// The box is managed by Claude Code over SSH, so this is the "is anyone
/// working on the box right now" signal.
pub(crate) fn claude_summary_line(app: &App) -> Line<'static> {
    let live = app.claude_runs > 0;
    let mut spans = vec![
        Span::styled("CLAUDE".to_string(), accent(C_OXIDE)),
        Span::raw("   "),
        Span::styled(
            if live { "● " } else { "○ " }.to_string(),
            Style::new().fg(if live { C_MIST } else { C_DIM }),
        ),
        Span::styled(
            if live {
                format!("{} session{} live", app.claude_runs, if app.claude_runs == 1 { "" } else { "s" })
            } else {
                "idle".to_string()
            },
            Style::new().fg(if live { C_MIST } else { C_DIM }),
        ),
    ];
    if app.claude_total > 0 {
        spans.push(Span::styled(
            format!("   {} active 24h · {} transcripts", app.claude_24h, app.claude_total),
            dim(),
        ));
    }
    Line::from(spans)
}

/// Full-board STATUS panel: services, probed sites, top procs, Claude
/// summary. (The kiosk splits this across tiles.)
pub(crate) fn status_panel(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled("HOST SERVICES", accent(C_BLUE)), Line::default()];
    lines.extend(service_rows(app));
    lines.push(Line::default());
    lines.push(Line::styled("CADDY SITES", accent(C_BLUE)));
    lines.push(Line::default());
    if app.sites.is_empty() {
        lines.push(Line::styled("(no sites)", dim()));
    }
    lines.extend(site_rows(app));
    if !app.procs_cpu.is_empty() {
        lines.extend(proc_lines("TOP PROCESSES", &app.procs_cpu, 10, false));
    }
    lines.push(Line::default());
    lines.push(claude_summary_line(app));
    lines
}

/// The prominent ALERTS block — a red title bar plus one descriptive line
/// per fault (which units, which crashed processes, sample error text).
/// Empty when the host is clean, so it only appears when it matters.
pub(crate) fn alert_lines(app: &App) -> Vec<Line<'static>> {
    if app.failed_units.is_empty() && app.crashes.is_empty() && app.journal_errs == 0 {
        return Vec::new();
    }
    let red_bold = Style::new().fg(C_ALARM).add_modifier(Modifier::BOLD);
    let mut out = vec![
        Line::default(),
        Line::from(Span::styled(
            "  ⚠  ALERTS  ",
            Style::new().bg(C_ALARM).fg(C_BG).add_modifier(Modifier::BOLD),
        )),
    ];
    let bullet = || Span::styled("● ".to_string(), Style::new().fg(C_ALARM));
    if !app.failed_units.is_empty() {
        out.push(Line::from(vec![
            bullet(),
            Span::styled(
                format!("{} unit{} failed", app.failed_units.len(), if app.failed_units.len() == 1 { "" } else { "s" }),
                red_bold,
            ),
            Span::styled(format!(" — {}", app.failed_units.join(", ")), Style::new().fg(C_FG)),
        ]));
    }
    if !app.crashes.is_empty() {
        out.push(Line::from(vec![
            bullet(),
            Span::styled("crashes/h".to_string(), red_bold),
            Span::styled(format!(" — {}", app.crashes.join(", ")), Style::new().fg(C_FG)),
        ]));
    }
    if app.journal_errs > 0 {
        out.push(Line::from(vec![
            bullet(),
            Span::styled(
                format!("{} journal error{}/h", app.journal_errs, if app.journal_errs == 1 { "" } else { "s" }),
                red_bold,
            ),
        ]));
        for s in &app.err_sample {
            out.push(Line::from(Span::styled(format!("    {s}"), dim())));
        }
    }
    out
}

/// The kiosk OPS tile: host alerts, Claude Code activity, services, and
/// the box's security posture — one dense ops surface.
pub(crate) fn ops_panel(app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![claude_summary_line(app)];
    // Alerts ride up top, right under the claude line, so a fault is the
    // first thing the eye lands on.
    lines.extend(alert_lines(app));
    // SERVICES + DOCKER — host units and the container-ops summary, moved
    // here from the (now calmer, cycling) CONTAINERS tile.
    lines.push(Line::default());
    lines.push(Line::styled("SERVICES", accent(C_BLUE)));
    lines.extend(service_rows(app));
    lines.push(docker_line(app));
    // SECURITY — posture at a glance. Updates stay dim (routine on a
    // rolling release); CVE-fixing updates, a pending reboot, and live
    // intrusion counters carry color.
    lines.push(Line::default());
    lines.push(Line::styled("SECURITY", accent(C_BLUE)));
    let mut sec = Vec::new();
    if let Some(n) = app.updates {
        sec.push(Span::styled(format!("{n} updates"), dim()));
    }
    match app.sec_updates {
        Some(s) if s > 0 => {
            sec.push(Span::styled("   ● ".to_string(), Style::new().fg(C_ALARM)));
            sec.push(Span::styled(format!("{s} security"), Style::new().fg(C_ALARM)));
        }
        Some(_) => sec.push(Span::styled("   ✓ no known CVEs".to_string(), Style::new().fg(C_GREEN))),
        None => {}
    }
    if let Some(d) = app.last_update_days {
        sec.push(Span::styled(format!("   · updated {d}d ago"), dim()));
    }
    if !sec.is_empty() {
        lines.push(Line::from(sec));
    }
    let mut intr = Vec::new();
    if let Some(v) = &app.reboot_pending {
        intr.push(Span::styled(format!("● reboot pending ({v})   "), Style::new().fg(C_SUN)));
    }
    intr.push(Span::styled(
        format!("{} banned", app.banned_ips),
        Style::new().fg(if app.banned_ips > 0 { C_SUN } else { C_DIM }),
    ));
    intr.push(Span::styled(" · ", dim()));
    intr.push(Span::styled(
        format!("{} ssh fails/h", app.ssh_fails),
        Style::new().fg(if app.ssh_fails > 0 { C_SUN } else { C_DIM }),
    ));
    lines.push(Line::from(intr));
    lines
}

// ---------------------------------------------------------------------------

