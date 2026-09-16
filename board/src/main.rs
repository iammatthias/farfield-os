// farfield os — ff-board: the full-screen kiosk dashboard.
//
// A single ratatui TUI owning the whole display — host monitoring up
// top, container charts and stack status below. Replaces the previous
// btop + clear-and-reprint shell-board tmux split, which flickered:
// `clear` blanked each pane, docker-stats took ~2s to answer, then
// output painted progressively. Ratatui double-buffers and writes only
// cell diffs, so updates are seamless.
//
//   ┌ CPU 12% · 54°C ──────────────┐┌ MEM 2.6G/27G ────────────────┐
//   │ ▂▃▂▁▂▆▂▁… (history graph)    ││ ▆▆▆▆▆▆▆… (history graph)     │
//   │ cores ▁▃▂▁▅▁▁▂▁▁▁▁▁▁▁▁       ││ swap 137M/4.0G               │
//   └──────────────────────────────┘└──────────────────────────────┘
//   ┌ NET enp3s0 ──────────────────┐┌ DISK / 19% ──────────────────┐
//   │ ↓ 1.2M/s ▁▂▃…  ↑ 300K/s ▁▁▂… ││ ▓▓░░░░ 175G/931G · io r/w    │
//   └──────────────────────────────┘└──────────────────────────────┘
//   ┌ CONTAINERS ──────────────────┐┌ STATUS ──────────────────────┐
//   │ name CPU ▁▂▁ 0.4% MEM ▆▆ NET ││ services · sites · top procs │
//   │ …                            ││ alerts · security · claude   │
//   └──────────────────────────────┘└──────────────────────────────┘
//
// Host metrics come straight from /proc + /sys (no subprocesses):
// /proc/stat (total + per-core CPU), /proc/meminfo, /proc/net/dev
// (default-route interface only, so bridge/veth traffic isn't double
// counted), /proc/diskstats (whole-disk sectors), hwmon temperatures.
// Container metrics read the Docker Engine API off the unix socket —
// one-shot /stats per container every 2s; CPU% is computed from
// consecutive samples the same way the CLI does. Containers sharing
// another service's netns (network_mode: service:…) report no network
// stats; their NET column shows "-" — the netns owner carries the
// aggregate. Slow-moving status (systemd units, Caddy sites, top
// processes, alerts, security posture) refreshes every 30/60s.
//
// Keys: q / Esc / Ctrl-C to quit.

use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use ratatui::{
    crossterm::{
        event::{
            self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
            KeyModifiers, MouseEventKind,
        },
        execute,
    },
    prelude::*,
};

mod app;
mod facts;
mod loops;
mod model;
mod panels;
mod render;
mod sample;
mod theme;

use crate::app::*;
use crate::loops::*;
use crate::model::*;
use crate::render::*;
use crate::theme::*;
fn main() -> std::io::Result<()> {
    let mode = match std::env::args().nth(1).as_deref() {
        None | Some("full") => Mode::Full,
        Some("cpu") => Mode::Cpu,
        Some("mem") => Mode::Mem,
        Some("host") => Mode::Host,
        Some("claude") => Mode::Claude,
        Some("net") => Mode::Net,
        Some("disk") => Mode::Disk,
        Some("containers") => Mode::Containers,
        Some("status") => Mode::Status,
        Some(other) => {
            eprintln!("ff-board: unknown panel '{other}'");
            eprintln!("usage: ff-board [full|host|cpu|mem|claude|net|disk|containers|status]");
            std::process::exit(2);
        }
    };

    let app = Arc::new(Mutex::new(App::default()));

    // Each panel only runs the samplers it displays — six sway tiles
    // shouldn't mean six docker pollers.
    if matches!(mode, Mode::Full | Mode::Cpu | Mode::Mem | Mode::Host | Mode::Net | Mode::Disk) {
        let a = app.clone();
        thread::spawn(move || host_loop(a));
    }
    // Per-container stats are expensive for dockerd (a cgroup sweep per
    // container per 2s) — only the panels that RENDER per-container rows
    // run the stats loop. The OPS tile's running-count comes from the
    // cheap list endpoint in status_loop.
    if matches!(mode, Mode::Full | Mode::Containers) {
        let a = app.clone();
        thread::spawn(move || docker_loop(a));
    }
    {
        // Every panel shows something from the status loop now (procs,
        // sites, services, disk) — the heavy hourly samplers stay gated
        // to the panels that render them.
        let a = app.clone();
        thread::spawn(move || status_loop(a, mode));
    }
    if matches!(mode, Mode::Full | Mode::Cpu | Mode::Host) {
        let a = app.clone();
        thread::spawn(move || clock_loop(a));
    }

    let mut terminal = ratatui::init();
    // ratatui::init()'s panic hook restores the terminal but not the mouse
    // capture enabled below, and it must not fire at all for a sampler
    // thread's panic — those are caught and restarted (see *_loop), and
    // tearing the terminal down from a surviving process would leave the
    // main loop drawing onto a restored screen. Chain a hook that acts
    // only for the main thread, shutting mouse reporting off first.
    // Pointer present (rack touch panel)? Read once, up here: it gates
    // both the terminal's mouse-reporting mode and the tap handling.
    let touch_enabled = std::env::var("FF_TOUCH").as_deref() == Ok("1");

    let main_thread = thread::current().id();
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if thread::current().id() == main_thread {
            let _ = execute!(std::io::stdout(), DisableMouseCapture);
            prev_hook(info);
        }
    }));
    // Restore on EVERY exit path — the `?` returns below would otherwise
    // skip DisableMouseCapture + ratatui::restore() and leave mouse
    // reporting spewing escapes into the caller's herdr/ssh session.
    struct TermGuard;
    impl Drop for TermGuard {
        fn drop(&mut self) {
            let _ = execute!(std::io::stdout(), DisableMouseCapture);
            ratatui::restore();
        }
    }
    let _restore = TermGuard;
    // Touch/mouse: each kiosk tile is its own foot window, so a tap on a
    // tile reaches that one process. Capturing the press lets it advance
    // its own paginated view (see `taps` below). Needs the compositor to
    // deliver touch as pointer events.
    //
    // Only on the touch panel: mouse reporting makes the terminal swallow
    // drag-to-select, so leaving it on would cost every ssh/herdr viewer
    // the ability to copy text off the board for no benefit.
    if touch_enabled {
        execute!(std::io::stdout(), EnableMouseCapture).ok();
    }
    // Kiosk tiles respawn inside one long-lived foot alt-screen (the
    // `while :; do ff-board …` loop), so the buffer still holds the
    // previous run's cells. ratatui assumes a blank screen and only
    // diffs against it, leaving stale glyphs wherever the new frame is
    // shorter. A one-time clear realigns its model with the screen.
    terminal.clear()?;
    let start = Instant::now();
    // A tap (or a fallback key) advances every cycled list in this tile by
    // one page. Modeled as added time so the page index jumps forward by
    // exactly one and the gentle auto-cycle simply continues from there.
    let mut taps: u64 = 0;
    // A tap fullscreens a tile; the fullscreen view carries the back button
    // and (on OPS) the action buttons. Enabled when a pointer is present.
    let mut touch = Touch { enabled: touch_enabled, ..Touch::default() };
    let mut redraw = true;
    let mut quit = false;
    let mut last_draw = Instant::now();
    loop {
        if redraw {
            let mut st = lock_app(&app);
            st.render_secs = start.elapsed().as_secs_f64() + taps as f64 * PAGE_SECS;
            terminal.draw(|f| ui(f, &st, mode, &mut touch))?;
            last_draw = Instant::now();
        }
        // An armed button disarms itself if not confirmed within a few seconds.
        if let Some((_, at)) = touch.armed {
            if at.elapsed() > Duration::from_secs(4) {
                touch.armed = None;
            }
        }
        // The 500ms timeout is the data-refresh tick. When events arrive,
        // drain the whole backlog before the next draw — any-motion mouse
        // reporting (mode 1003) floods Moved events, and a full redraw per
        // event burns CPU on frames where nothing the app reacts to
        // changed. Only reacted-to events mark the frame dirty.
        redraw = true;
        if event::poll(Duration::from_millis(500))? {
            redraw = false;
            loop {
                match event::read()? {
                    Event::Mouse(m) if matches!(m.kind, MouseEventKind::Down(_)) && touch.enabled => {
                        // Display powered off (ff-display off)? This tap only
                        // wakes it — swallowed so it can't act on something
                        // the user couldn't see.
                        if !wake_tap_swallowed() {
                            let pos = Position { x: m.column, y: m.row };
                            // `back` is set by render only when this tile is fullscreen.
                            if touch.back.is_some() {
                                if touch.back.is_some_and(|r| r.contains(pos)) {
                                    wm_toggle_fullscreen(); // back to the grid
                                    touch.armed = None;
                                } else if let Some(action) =
                                    touch.buttons.iter().find(|b| b.rect.contains(pos)).map(|b| b.action)
                                {
                                    // Two-tap confirm on the destructive actions.
                                    if action.confirm() && !matches!(touch.armed, Some((a, _)) if a == action) {
                                        touch.armed = Some((action, Instant::now()));
                                    } else {
                                        action.spawn();
                                        touch.armed = None;
                                    }
                                }
                            } else {
                                wm_toggle_fullscreen(); // grid tap zooms this tile
                            }
                            redraw = true;
                        }
                    }
                    Event::Mouse(_) => {} // Moved / Up / drag — nothing rendered changes
                    Event::Key(k) if k.kind == KeyEventKind::Press => {
                        // Advance keys double as a keyboard fallback for the tap.
                        if matches!(
                            k.code,
                            KeyCode::Char(' ') | KeyCode::Char('n') | KeyCode::Right | KeyCode::Down
                        ) {
                            taps += 1;
                            redraw = true;
                        } else if matches!(k.code, KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc)
                            || (k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL))
                        {
                            quit = true;
                        }
                    }
                    Event::Resize(..) => redraw = true,
                    _ => {}
                }
                if quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
            if quit {
                break;
            }
            // Under continuous motion poll never times out — keep the
            // normal data-refresh cadence anyway.
            if last_draw.elapsed() >= Duration::from_millis(500) {
                redraw = true;
            }
        }
    }
    Ok(())
}
