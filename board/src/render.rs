//! The render pass: the full-screen layout and the per-subject panels that
//! draw CPU, memory, network, disk, containers and status into it.

use std::collections::VecDeque;

use ratatui::{
    prelude::*,
    widgets::Paragraph,
};

use crate::app::*;
use crate::facts::*;
use crate::model::*;
use crate::panels::*;
use crate::theme::*;


pub(crate) fn ui(frame: &mut Frame, app: &App, mode: Mode, touch: &mut Touch) {
    let area = frame.area();
    touch.buttons.clear();
    touch.back = None;
    if mode == Mode::Full {
        ui_full(frame, app);
        sampler_err_line(frame, app);
        return;
    }
    // A tap zooms this tile to fullscreen (detected by its size). Then a top
    // strip carries the back button, the panel renders below at full size
    // (its rich layout), and the OPS tile gets the action button row.
    let fullscreen = touch.enabled && area.width >= FS_MIN_COLS;
    let body = if fullscreen {
        let [bar, rest] = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(area);
        render_back(frame, bar, touch);
        rest
    } else {
        area
    };
    // Kiosk tiles: sway draws the frame, so render borderless.
    match mode {
        Mode::Cpu => render_cpu(frame, body, app, false),
        Mode::Mem => render_mem(frame, body, app, false),
        Mode::Host => render_host(frame, body, app, false),
        Mode::Claude => render_claude(frame, body, app, false),
        Mode::Net => render_net(frame, body, app, false),
        Mode::Disk => render_disk(frame, body, app, false),
        Mode::Containers => render_containers(frame, body, app, false),
        Mode::Status if fullscreen => {
            let [content, btns] = Layout::vertical([Constraint::Min(1), Constraint::Length(5)]).areas(body);
            render_status(frame, content, app, true, false);
            render_buttons(frame, btns, touch);
        }
        Mode::Status => render_status(frame, body, app, true, false),
        Mode::Full => {}
    }
    sampler_err_line(frame, app);
}

/// The whole composite board — what herdr/ssh sessions see. The kiosk
/// instead runs six single-panel processes tiled by sway.
pub(crate) fn ui_full(frame: &mut Frame, app: &App) {
    let h = &app.host;
    // Proportional heights: big history graphs up top (~24% of however
    // tall the display is), a fixed net/disk band, the rest to the
    // container + status grids. Degrades cleanly on small terminals.
    let [header, hosttop, hostnet, lower] =
        Layout::vertical([Constraint::Length(1), Constraint::Percentage(24), Constraint::Length(8), Constraint::Min(8)])
            .areas(frame.area());

    let loadc = heat(h.load / h.ncpu.max(1) as f64);
    let title = Line::from(vec![
        Span::styled(" ⊙ ", Style::new().fg(C_SIGNAL)),
        Span::styled("farfield ", accent(C_FG)),
        Span::styled(format!("· {} · up {} · ", h.hostname, human_uptime(h.uptime)), dim()),
        Span::styled(format!("load {:.2}", h.load), Style::new().fg(loadc)),
        Span::styled(format!("/{}", h.ncpu), dim()),
    ]);
    let clock = Line::from(Span::styled(format!("{} ", app.clock), accent(C_MIST))).right_aligned();
    frame.render_widget(Paragraph::new(title), header);
    frame.render_widget(Paragraph::new(clock), header);

    // Full board has no compositor — keep the boxes as panel separators.
    let [cpu_a, mem_a] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(hosttop);
    render_cpu(frame, cpu_a, app, true);
    render_mem(frame, mem_a, app, true);

    let [net_a, disk_a] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(hostnet);
    render_net(frame, net_a, app, true);
    render_disk(frame, disk_a, app, true);

    let [cont_a, stat_a] = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).areas(lower);
    render_containers(frame, cont_a, app, true);
    render_status(frame, stat_a, app, false, true);
}

/// CPU + MEM stacked in one tile — the combined host panel.
pub(crate) fn render_host(frame: &mut Frame, area: Rect, app: &App, bordered: bool) {
    let [cpu_a, mem_a] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    render_cpu(frame, cpu_a, app, bordered);
    render_mem(frame, mem_a, app, bordered);
}

/// Claude Code activity: live sessions, a 14-day activity strip, and the
/// most recently touched projects. The box is managed by an agent over
/// SSH, so this tile answers "who's been working here, and when".
pub(crate) fn render_claude(frame: &mut Frame, area: Rect, app: &App, bordered: bool) {
    let live = app.claude_runs > 0;
    let mut title = vec![
        Span::styled(" CLAUDE ", accent(C_OXIDE)),
        Span::styled(
            if live { "● ".to_string() } else { "○ ".to_string() },
            Style::new().fg(if live { C_SIGNAL } else { C_DIM }),
        ),
        Span::styled(
            if live {
                format!("{} session{} live", app.claude_runs, if app.claude_runs == 1 { "" } else { "s" })
            } else {
                "idle".to_string()
            },
            if live { Style::new().fg(C_FG) } else { dim() },
        ),
    ];
    if !app.claude_authed {
        title.push(Span::styled(" · not signed in", Style::new().fg(C_SUN)));
    }
    let inner = panel(frame, area, bordered, title, None);
    let w = inner.width as usize;

    let mut lines: Vec<Line> = vec![Line::default()];
    if !app.claude_authed && app.claude_total == 0 {
        // Never used on this box — say so plainly, and say what to do.
        lines.push(Line::from(Span::styled("   ⊙", Style::new().fg(C_SIGNAL))));
        lines.push(Line::default());
        lines.push(Line::styled("   claude code has never signed in here", dim()));
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("   ssh in and run ", dim()),
            Span::styled("claude", Style::new().fg(C_FG)),
            Span::styled(" (or ff-bootstrap)", dim()),
        ]));
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    lines.push(Line::from(vec![
        Span::styled(format!("{} active 24h", app.claude_24h), Style::new().fg(C_MIST)),
        Span::styled(format!(" · {} transcripts", app.claude_total), dim()),
    ]));
    let peak = app.claude_daily.iter().cloned().fold(1.0_f64, f64::max);
    lines.push(Line::from(vec![
        Span::styled("14d ", dim()),
        Span::styled(
            spark_abs(&app.claude_daily, w.saturating_sub(4).min(28), peak),
            Style::new().fg(C_OXIDE),
        ),
    ]));
    lines.push(Line::default());
    lines.push(Line::styled("PROJECTS", accent(C_BLUE)));
    if app.claude_projects.is_empty() {
        lines.push(Line::styled("  no transcripts yet", dim()));
    }
    let namew = w.saturating_sub(14).clamp(10, 34);
    for (name, count, last) in &app.claude_projects {
        // Tail-truncate on CHAR boundaries — project slugs come from
        // real paths, and a byte slice through a multi-byte char panics
        // the render thread (which takes the tile down with it).
        let shown: String = if name.chars().count() > namew {
            let keep = namew - 1;
            let start = name.chars().count() - keep;
            format!("…{}", name.chars().skip(start).collect::<String>())
        } else {
            name.clone()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {shown:<namew$}"), Style::new().fg(C_MIST)),
            Span::styled(format!(" ×{count:<3}"), dim()),
            Span::styled(format!(" {:>3}", ago(*last)), dim()),
        ]));
        if lines.len() as u16 >= inner.height {
            break;
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(crate) fn render_cpu(frame: &mut Frame, cpu_a: Rect, app: &App, bordered: bool) {
    let h = &app.host;
    // Sensors ride the header as Tufte word-graphics: trend sparkline then
    // the current reading. Flat = steady, which is its own glanceable signal.
    let mut title = vec![
        Span::styled(" CPU ", accent(C_MIST)),
        Span::styled(format!("{:.1}% ", h.cpu_cur), Style::new().fg(heat(h.cpu_cur / 100.0))),
    ];
    if let Some(t) = h.temp_c {
        title.push(Span::styled("· ", dim()));
        title.push(Span::styled(spark_abs(&h.temp_hist, 5, 95.0), Style::new().fg(heat((t / 90.0).clamp(0.0, 1.0)))));
        title.push(Span::styled(format!(" {t:.0}°C "), dim()));
    }
    if let Some(w) = h.watts {
        title.push(Span::styled("· ", dim()));
        title.push(Span::styled(spark_abs(&h.watts_hist, 5, 25.0), Style::new().fg(C_SUN)));
        title.push(Span::styled(format!(" {w:.0}W "), dim()));
    }
    // Tile mode: the kiosk has no global header, so clock + uptime ride
    // this tile's header line.
    let right = (cpu_a.height >= 22 && !app.clock.is_empty()).then(|| {
        Line::from(Span::styled(
            format!(" {} · up {} ", app.clock, human_uptime(h.uptime)),
            dim(),
        ))
    });
    let cpu_inner = panel(frame, cpu_a, bordered, title, right);
    if cpu_inner.height >= 20 && !h.cores_hist.is_empty() {
        // Tile mode: graph + per-core sparkline grid + load + top procs.
        let ncores = h.cores_hist.len();
        let half = ncores.div_ceil(2);
        // The per-core grid is the rich CPU view; the total-cpu history is a
        // compact strip above it, and the top-procs table fills the rest.
        let [graph_a, cores_a, load_a, procs_a] = Layout::vertical([
            Constraint::Length(6),
            Constraint::Length(half as u16 + 1),
            Constraint::Length(1),
            Constraint::Min(4),
        ])
        .areas(cpu_inner);
        frame.render_widget(
            // Fixed 0–100% axis, sqrt-compressed: 4% renders at 20% height
            // (visible texture), 25% at half, 100% full. Color stays linear.
            Paragraph::new(graph_lines(&h.cpu, graph_a.width as usize, graph_a.height as usize, |v| (v / 100.0).sqrt(), |v| heat(v / 100.0))),
            graph_a,
        );
        let csw = ((cores_a.width as usize).saturating_sub(2 * 9 + 3) / 2).clamp(8, 32);
        let seg = |i: usize| -> Vec<Span<'static>> {
            let cur = h.cores.get(i).copied().unwrap_or(0.0);
            let t = (cur / 100.0).clamp(0.0, 1.0);
            vec![
                Span::styled(format!("c{i:<2} "), dim()),
                Span::styled(spark(&h.cores_hist[i], csw, 5.0, 1.0), Style::new().fg(heat(t))),
                Span::styled(format!(" {cur:3.0}%"), Style::new().fg(heat(t))),
            ]
        };
        let mut core_lines = vec![Line::default()];
        for r in 0..half {
            let mut spans = seg(r);
            if r + half < ncores {
                spans.push(Span::raw("   "));
                spans.extend(seg(r + half));
            }
            core_lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(core_lines), cores_a);
        let loadc = heat(h.load / h.ncpu.max(1) as f64);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("load ", dim()),
                Span::styled(spark(&h.load_hist, 24, h.ncpu as f64, 1.0), Style::new().fg(loadc)),
                Span::styled(format!(" {:.2}", h.load), Style::new().fg(loadc)),
                Span::styled(format!("/{}", h.ncpu), dim()),
            ])),
            load_a,
        );
        frame.render_widget(
            Paragraph::new(proc_lines("TOP CPU", &app.procs_cpu, (procs_a.height as usize).saturating_sub(3), false)),
            procs_a,
        );
    } else if cpu_inner.height > 1 {
        let graph = Rect { height: cpu_inner.height - 1, ..cpu_inner };
        frame.render_widget(
            Paragraph::new(graph_lines(&h.cpu, graph.width as usize, graph.height as usize, |v| (v / 100.0).sqrt(), |v| heat(v / 100.0))),
            graph,
        );
        let mut core_spans = vec![Span::styled("cores ", dim())];
        for p in &h.cores {
            let t = (p / 100.0).clamp(0.0, 1.0);
            core_spans.push(Span::styled(TICKS[(t * 7.0).round() as usize].to_string(), Style::new().fg(heat(t))));
        }
        let coreline = Rect { y: cpu_inner.y + cpu_inner.height - 1, height: 1, ..cpu_inner };
        frame.render_widget(Paragraph::new(Line::from(core_spans)), coreline);
    }
}

pub(crate) fn render_mem(frame: &mut Frame, mem_a: Rect, app: &App, bordered: bool) {
    let h = &app.host;
    let mem_t = if h.mem_total > 0.0 { h.mem_cur / h.mem_total } else { 0.0 };
    let title = vec![
        Span::styled(" MEM ", accent(C_OXIDE)),
        Span::styled(
            format!("{} / {} ", human_mem(h.mem_cur).trim(), human_mem(h.mem_total).trim()),
            Style::new().fg(C_FG),
        ),
        Span::styled(format!("· {:.0}% ", mem_t * 100.0), dim()),
    ];
    let mem_inner = panel(frame, mem_a, bordered, title, None);
    if mem_inner.height >= 20 {
        // Memory barely moves, so a tall area graph can only void (absolute
        // scale) or paint a solid block (fill scale). Use a one-line trend
        // sparkline and give the panel to the gauges + top-memory table,
        // which carry the real detail.
        let total = h.mem_total.max(1.0);
        let [spark_a, brk_a, procs_a] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(6),
            Constraint::Min(4),
        ])
        .areas(mem_inner);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("trend ", dim()),
                Span::styled(
                    spark(&h.mem_used, (spark_a.width as usize).saturating_sub(6), total * 0.2, 1.1),
                    Style::new().fg(C_BLUE),
                ),
            ])),
            spark_a,
        );
        let w = brk_a.width as usize;
        frame.render_widget(
            Paragraph::new(vec![
                Line::default(),
                gauge_line("used", h.mem_cur / total, human_mem(h.mem_cur).trim().to_string(), C_OXIDE, w),
                gauge_line("avail", h.mem_avail / total, human_mem(h.mem_avail).trim().to_string(), C_GREEN, w),
                gauge_line("cache", h.mem_cache / total, human_mem(h.mem_cache).trim().to_string(), C_BLUE, w),
                gauge_line(
                    "swap",
                    if h.swap_total > 0.0 { h.swap_used / h.swap_total } else { 0.0 },
                    format!("{} / {}", human_mem(h.swap_used).trim(), human_mem(h.swap_total).trim()),
                    C_SUN,
                    w,
                ),
            ]),
            brk_a,
        );
        frame.render_widget(
            Paragraph::new(proc_lines("TOP MEM", &app.procs_mem, (procs_a.height as usize).saturating_sub(3), true)),
            procs_a,
        );
    } else if mem_inner.height > 1 {
        let graph = Rect { height: mem_inner.height - 1, ..mem_inner };
        let total = h.mem_total.max(1.0);
        frame.render_widget(
            Paragraph::new(graph_lines(
                &h.mem_used,
                graph.width as usize,
                graph.height as usize,
                move |v| (v / total).sqrt(),
                move |v| lerp(BLUE_RGB, OXIDE_RGB, v / total),
            )),
            graph,
        );
        let swap_style = if h.swap_total > 0.0 && h.swap_used / h.swap_total > 0.5 {
            Style::new().fg(C_SUN)
        } else {
            dim()
        };
        let swapline = Rect { y: mem_inner.y + mem_inner.height - 1, height: 1, ..mem_inner };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("swap {} / {}", human_mem(h.swap_used).trim(), human_mem(h.swap_total).trim()),
                swap_style,
            ))),
            swapline,
        );
    }
}

pub(crate) fn render_net(frame: &mut Frame, net_a: Rect, app: &App, bordered: bool) {
    let h = &app.host;
    let peak = |v: &VecDeque<f64>| v.iter().copied().fold(0.0f64, f64::max);

    let net_inner = panel(
        frame,
        net_a,
        bordered,
        vec![
            Span::styled(" NET ", accent(C_SUN)),
            Span::styled(format!("{} ", h.iface), dim()),
        ],
        None,
    );
    // Tile mode gets a totals/sockets footer and the probed site list
    // (ingress health is network business) under the graphs.
    let mut sites_a = None;
    let (body, footer) = if net_inner.height >= 22 && !app.sites.is_empty() {
        // Fixed graphs band, then the rest goes to the site list — which
        // cycles (see paged()) rather than stretching the tile to fit all.
        let [b, f, s] =
            Layout::vertical([Constraint::Length(9), Constraint::Length(3), Constraint::Min(5)]).areas(net_inner);
        sites_a = Some(s);
        (b, Some(f))
    } else if net_inner.height >= 14 {
        let [b, f] = Layout::vertical([Constraint::Min(4), Constraint::Length(3)]).areas(net_inner);
        (b, Some(f))
    } else {
        (net_inner, None)
    };
    let [rx_a, tx_a] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    for (area, arrow, hue, cur, series) in [
        (rx_a, '↓', MIST_RGB, h.rx_cur, &h.rx),
        (tx_a, '↑', OXIDE_RGB, h.tx_cur, &h.tx),
    ] {
        let [label_a, graph_a] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{arrow} "), accent(lerp(hue, hue, 0.0))),
                Span::styled(human_rate(Some(cur)), Style::new().fg(C_FG)),
                Span::styled(format!("   peak {}", human_rate(Some(peak(series)))), dim()),
            ])),
            label_a,
        );
        // Fixed log axis (1 KB/s → 1 GbE): a given bar height always means
        // the same rate, so idle chatter reads low regardless of history.
        let tint_hue = tint(hue);
        frame.render_widget(
            Paragraph::new(graph_lines(
                series,
                graph_a.width as usize,
                graph_a.height as usize,
                |v| log_t(v, NET_FLOOR, NET_CEIL),
                move |v| tint_hue(log_t(v, NET_FLOOR, NET_CEIL)),
            )),
            graph_a,
        );
    }
    if let Some(f) = footer {
        let mut lines = vec![
            Line::default(),
            Line::from(vec![
                Span::styled("Σ ", dim()),
                Span::styled(format!("↓ {}", human_size(h.rx_total as f64)), Style::new().fg(C_MIST)),
                Span::styled(" · ", dim()),
                Span::styled(format!("↑ {}", human_size(h.tx_total as f64)), Style::new().fg(C_OXIDE)),
                Span::styled("      tcp ", dim()),
                Span::styled(spark_abs(&h.tcp_hist, 10, 128.0), Style::new().fg(C_BLUE)),
                Span::styled(format!(" {} estab · {} tw", h.tcp_inuse, h.tcp_tw), dim()),
            ]),
        ];
        // Radio + tailnet line: link quality colored good→bad (inverted
        // heat — high quality is green), peer reachability beside it.
        let mut link = Vec::new();
        if let Some(q) = h.wifi.back().copied() {
            let qc = heat(1.0 - q / 100.0);
            link.push(Span::styled("wifi ", dim()));
            link.push(Span::styled(spark(&h.wifi, 16, 100.0, 1.0), Style::new().fg(qc)));
            link.push(Span::styled(format!(" {q:.0}%"), Style::new().fg(qc)));
            link.push(Span::styled(format!(" · {:.0}dBm", h.wifi_dbm), dim()));
        }
        if app.ts_total > 0 {
            link.push(Span::styled("      TS ", dim()));
            link.push(Span::styled(
                "● ".to_string(),
                Style::new().fg(if app.ts_online > 0 { C_GREEN } else { C_SUN }),
            ));
            link.push(Span::styled(
                format!("{}/{} peers", app.ts_online, app.ts_total),
                Style::new().fg(C_FG),
            ));
            link.push(Span::styled(format!(" · {}", app.ts_ip), dim()));
        }
        if !link.is_empty() {
            lines.push(Line::from(link));
        }
        frame.render_widget(Paragraph::new(lines), f);
    }
    if let Some(sa) = sites_a {
        let down = app.sites.iter().filter(|s| s.ok == Some(false)).count();
        let mut header = vec![
            Span::styled("SITES ".to_string(), accent(C_BLUE)),
            Span::styled(format!("{}", app.sites.len()), dim()),
        ];
        if down > 0 {
            header.push(Span::styled(format!(" · {down} down"), Style::new().fg(C_ALARM)));
        }
        let mut lines = vec![Line::default(), Line::from(header), Line::default()];
        // Three header lines above; cycle the site rows through what's left.
        lines.extend(paged(site_rows(app), (sa.height as usize).saturating_sub(3), 0, app.render_secs));
        frame.render_widget(Paragraph::new(lines), sa);
    }
}

pub(crate) fn render_disk(frame: &mut Frame, disk_a: Rect, app: &App, bordered: bool) {
    let h = &app.host;
    let peak = |v: &VecDeque<f64>| v.iter().copied().fold(0.0f64, f64::max);

    let nvme = match (app.nvme_wear, app.nvme_temp) {
        (Some(w), Some(t)) => format!("· wear {w}% · {t}°C "),
        (Some(w), None) => format!("· wear {w}% "),
        _ => String::new(),
    };
    let disk_inner = panel(
        frame,
        disk_a,
        bordered,
        vec![
            Span::styled(" DISK ", accent(C_BLUE)),
            Span::styled(
                format!(
                    "/ {} · {} · {} images {}",
                    app.disk_pct.map_or("?%".into(), |p| format!("{p}%")),
                    app.disk_detail,
                    app.images,
                    nvme
                ),
                dim(),
            ),
        ],
        None,
    );
    // Tile mode gets a lifetime-IO footer under the graphs.
    let (body, footer) = if disk_inner.height >= 14 {
        let [b, f] = Layout::vertical([Constraint::Min(4), Constraint::Length(2)]).areas(disk_inner);
        (b, Some(f))
    } else {
        (disk_inner, None)
    };
    if let Some(f) = footer {
        frame.render_widget(
            Paragraph::new(vec![
                Line::default(),
                Line::from(vec![
                    Span::styled("Σ ", dim()),
                    Span::styled(format!("read {}", human_size(h.io_r_total as f64)), Style::new().fg(C_MIST)),
                    Span::styled(" · ", dim()),
                    Span::styled(format!("written {}", human_size(h.io_w_total as f64)), Style::new().fg(C_OXIDE)),
                    Span::styled(
                        if app.snapshots > 0 {
                            format!("      since boot · {} btrfs snapshots", app.snapshots)
                        } else {
                            "      since boot".to_string()
                        },
                        dim(),
                    ),
                ]),
            ]),
            f,
        );
    }
    let [gauge_a, iolabel_a, iograph_a] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(body);
    let barw = gauge_a.width as usize;
    // Unknown usage draws an empty trough, not a full-looking bar.
    let filled = (app.disk_pct.unwrap_or(0) as usize * barw / 100).min(barw);
    let mut gauge_spans = Vec::with_capacity(barw);
    for i in 0..barw {
        if i < filled {
            gauge_spans.push(Span::styled("▓", Style::new().fg(heat(i as f64 / barw as f64))));
        } else {
            gauge_spans.push(Span::styled("░", Style::new().fg(C_BORDER)));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(gauge_spans)), gauge_a);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("io  read ", dim()),
            Span::styled(human_rate(Some(h.io_r_cur)), Style::new().fg(C_MIST)),
            Span::styled(format!("  peak {}", human_rate(Some(peak(&h.io_r)))), dim()),
            Span::styled("      write ", dim()),
            Span::styled(human_rate(Some(h.io_w_cur)), Style::new().fg(C_OXIDE)),
            Span::styled(format!("  peak {}", human_rate(Some(peak(&h.io_w)))), dim()),
        ])),
        iolabel_a,
    );
    let [ior_a, iow_a] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(iograph_a);
    // Fixed log axis (10 KB/s → ~2 GB/s NVMe): journal/container-log
    // trickle reads as low texture, real transfers climb the panel.
    let tint_r = tint(MIST_RGB);
    let tint_w = tint(OXIDE_RGB);
    frame.render_widget(
        Paragraph::new(graph_lines(
            &h.io_r,
            ior_a.width as usize,
            ior_a.height as usize,
            |v| log_t(v, IO_FLOOR, IO_CEIL),
            move |v| tint_r(log_t(v, IO_FLOOR, IO_CEIL)),
        )),
        ior_a,
    );
    frame.render_widget(
        Paragraph::new(graph_lines(
            &h.io_w,
            iow_a.width as usize,
            iow_a.height as usize,
            |v| log_t(v, IO_FLOOR, IO_CEIL),
            move |v| tint_w(log_t(v, IO_FLOOR, IO_CEIL)),
        )),
        iow_a,
    );
}

pub(crate) fn render_containers(frame: &mut Frame, cont_a: Rect, app: &App, bordered: bool) {
    let total_mem: f64 = app.containers.values().map(|s| s.mem_cur).sum();
    let flagged = app.containers.values().filter(|s| s.flag.is_some()).count();
    let mut title = vec![
        Span::styled(" CONTAINERS ", accent(C_MIST)),
        Span::styled(format!("{} ", app.containers.len()), Style::new().fg(C_FG)),
        Span::styled(format!("· mem {} ", human_mem(total_mem).trim()), dim()),
    ];
    if flagged > 0 {
        title.push(Span::styled(format!("· {flagged} unhealthy "), Style::new().fg(C_ALARM)));
    }
    let cont_inner = panel(frame, cont_a, bordered, title, None);
    // The list cycles (containers_panel) with breathing room at the bottom;
    // host-services + docker-ops moved to the OPS tile, which had the room.
    frame.render_widget(
        Paragraph::new(containers_panel(app, cont_inner.width as usize, cont_inner.height as usize)),
        cont_inner,
    );
}

pub(crate) fn render_status(frame: &mut Frame, stat_a: Rect, app: &App, tile: bool, bordered: bool) {
    // On the kiosk this tile is the OPS surface — services/sites/docker
    // live in the NET and CONTAINERS tiles there. Action buttons show only
    // in the fullscreen view (see ui()), never crammed into the grid tile.
    let (title, content) = if tile {
        (" OPS ", ops_panel(app))
    } else {
        (" STATUS ", status_panel(app))
    };
    let stat_inner = panel(frame, stat_a, bordered, vec![Span::styled(title, accent(C_OXIDE))], None);
    // Page rather than clip. This panel ends with the SECURITY section, so
    // silently dropping the overflow hides exactly the lines worth seeing.
    let content = paged(content, stat_inner.height as usize, 0, app.render_secs);
    frame.render_widget(Paragraph::new(content), stat_inner);
}
