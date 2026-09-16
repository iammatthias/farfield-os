//! Modes, the action buttons, touch hit-boxes, and the window-manager
//! plumbing that puts the board full-screen on the kiosk.
//!
//! `wake_tap_swallowed` is the subtle one: the first tap on a sleeping
//! display wakes it and must not also press whatever was under the finger.

use std::{
    fs,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use ratatui::prelude::*;

use crate::theme::*;

pub(crate) const DOCKER_SOCK: &str = "/var/run/docker.sock";
pub(crate) const KEEP: usize = 240; // samples per series (~8 min at 2s cadence)
pub(crate) const STATS_EVERY: Duration = Duration::from_secs(2);
pub(crate) const STATUS_EVERY: Duration = Duration::from_secs(30);
pub(crate) const SERVICES: [&str; 6] = ["docker", "postgresql", "valkey", "fail2ban", "ufw", "ff-stack"];
pub(crate) const STACK: &str = "/srv/stack";
pub(crate) const TICKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// What this process renders. `Full` is the whole composite board (the
/// herdr / ssh view); the rest are single panels — one per sway tile,
/// so the compositor does the layout and each tile only runs the
/// samplers it needs.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Full,
    Cpu,
    Mem,
    Host, // cpu + mem stacked in one tile
    Net,
    Disk,
    Containers,
    Status,
    Claude, // Claude Code activity on the box
}

// ---------------------------------------------------------------------------
// Touch action buttons (rack touch LCD; enabled by FF_TOUCH)
// ---------------------------------------------------------------------------

/// An on-screen control the OPS tile exposes when a touchscreen is present.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Update,
    RestartKiosk,
    RestartStack,
    Prune,
    Reboot,
}

/// Rendered order. Reboot last — most destructive.
pub(crate) const BUTTONS: [Action; 5] = [
    Action::Update,
    Action::RestartKiosk,
    Action::RestartStack,
    Action::Prune,
    Action::Reboot,
];

impl Action {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Action::Update => "Update",
            Action::RestartKiosk => "Kiosk ↻",
            Action::RestartStack => "Stack ↻",
            Action::Prune => "Prune",
            Action::Reboot => "Reboot",
        }
    }
    /// Destructive/disruptive actions require a second confirming tap.
    pub(crate) fn confirm(self) -> bool {
        !matches!(self, Action::RestartKiosk)
    }
    pub(crate) fn color(self) -> Color {
        match self {
            Action::Reboot => C_ALARM,
            Action::Update | Action::Prune | Action::RestartStack => C_SUN,
            _ => C_SIGNAL,
        }
    }
    pub(crate) fn cmd(self) -> &'static str {
        match self {
            // ff-update --yes runs the upgrade as a detached systemd unit.
            Action::Update => "ff-update --yes",
            Action::RestartKiosk => "ff-kiosk-restart",
            Action::RestartStack => "sudo -n systemctl restart ff-stack.service",
            Action::Prune => "sudo -n systemctl start ff-docker-prune.service",
            Action::Reboot => "sudo -n systemctl reboot",
        }
    }
    /// Fire and forget — the user has NOPASSWD sudo, so these just run.
    pub(crate) fn spawn(self) {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(self.cmd());
        spawn_reaped(cmd);
    }
}

/// Spawn a fire-and-forget command AND reap it from a detached thread —
/// an un-`wait()`ed child stays a zombie forever in a process that runs
/// for months.
pub(crate) fn spawn_reaped(mut cmd: Command) {
    if let Ok(mut child) = cmd.spawn() {
        thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// A rendered button's hit box + what it does. Populated each frame by the
/// OPS tile's render so the event loop can map a tap to an action.
pub(crate) struct Btn {
    pub(crate) rect: Rect,
    pub(crate) action: Action,
}

/// Touch state carried across frames. A tap fullscreens a tile (via sway
/// IPC); the roomy fullscreen view is where the OPS action buttons + a back
/// button live. `buttons`/`back` are this frame's hit boxes.
#[derive(Default)]
pub(crate) struct Touch {
    pub(crate) enabled: bool,           // a pointer/touchscreen is present (FF_TOUCH)
    pub(crate) armed: Option<(Action, Instant)>,
    pub(crate) buttons: Vec<Btn>,       // OPS action buttons (fullscreen only)
    pub(crate) back: Option<Rect>,      // back button — Some iff this frame is fullscreen
}

/// A kiosk tile wider than this (cols) is fullscreen, not a grid slot. A
/// grid tile is ~1/3 the screen: ≈42 cols at 1280/size-10, ≈77 at
/// 2560/size-14 — both under this; fullscreen (≈128 / ≈232) is over it.
pub(crate) const FS_MIN_COLS: u16 = 90;

/// Toggle the focused tile between fullscreen and its grid slot via the
/// compositor's IPC. A tap focuses the tile first, so this acts on the
/// tapped one.
pub(crate) fn wm_toggle_fullscreen() {
    // Fire-and-forget: this runs on the input thread, and waiting on the
    // compositor's reply would stall the next tap behind it.
    let mut cmd = Command::new("swaymsg");
    cmd.args(["fullscreen", "toggle"]);
    spawn_reaped(cmd);
}

/// If the display was powered off (ff-display off), a tap should ONLY
/// wake it: power the output on, mark the shared state on, and report
/// the tap as swallowed — it must not fullscreen a tile or press a
/// button the user couldn't see. Fail-open: any error here means "not
/// asleep" and the tap proceeds normally.
pub(crate) fn wake_tap_swallowed() -> bool {
    let Ok(rt) = std::env::var("XDG_RUNTIME_DIR") else { return false };
    let dir = format!("{rt}/ff-display");
    let asleep = fs::read_to_string(format!("{dir}/state"))
        .map(|s| s.trim() == "off")
        .unwrap_or(false);
    if asleep {
        let mut on = Command::new("swaymsg");
        on.args(["output", "*", "power", "on"]);
        spawn_reaped(on);
        let _ = fs::write(format!("{dir}/state"), b"on");
        let mut log = Command::new("logger");
        log.args(["-t", "ff-display", "display on — tap"]);
        spawn_reaped(log);
    }
    asleep
}

// ---------------------------------------------------------------------------
