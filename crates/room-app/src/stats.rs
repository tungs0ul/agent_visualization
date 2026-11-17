//! The app's own cost (PLAN.md M11): CPU, memory and frames per second in a small
//! overlay, toggled with I or the "Stats" button. Nothing is measured while it is
//! hidden, and while shown it refreshes once a second.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// How often the numbers refresh while the overlay is shown.
pub const REFRESH: Duration = Duration::from_secs(1);

#[derive(Resource, Default)]
pub struct Stats {
    pub shown: bool,
    /// Percent of one core, as Activity Monitor shows it.
    pub cpu: f32,
    pub memory_mb: f32,
    /// Frames drawn in the last second (0 while idle: the app only redraws on change).
    pub fps: f32,
    system: Option<System>,
    frames: u32,
    since: Option<Instant>,
}

impl Stats {
    pub fn toggle(&mut self) {
        self.shown = !self.shown;
        if !self.shown {
            // Free sysinfo's process table while hidden.
            self.system = None;
            self.since = None;
        }
    }

    pub fn line(&self) -> String {
        if self.since.is_none() || self.memory_mb == 0.0 {
            return "Measuring…".to_owned();
        }
        format!("CPU {:.1} %  ·  RAM {:.0} MB  ·  {:.0} fps", self.cpu, self.memory_mb, self.fps)
    }
}

/// Counts frames and, once a second, reads this process's CPU and memory.
pub fn measure(mut stats: ResMut<Stats>) {
    if !stats.shown {
        return;
    }
    stats.frames += 1;
    let now = Instant::now();
    let Some(since) = stats.since else {
        stats.since = Some(now);
        stats.frames = 0;
        refresh(&mut stats);
        return;
    };
    let elapsed = now - since;
    if elapsed < REFRESH {
        return;
    }
    stats.fps = stats.frames as f32 / elapsed.as_secs_f32();
    stats.frames = 0;
    stats.since = Some(now);
    refresh(&mut stats);
}

fn refresh(stats: &mut Stats) {
    let pid = Pid::from_u32(std::process::id());
    let system = stats.system.get_or_insert_with(System::new);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );
    if let Some(process) = system.process(pid) {
        stats.cpu = process.cpu_usage();
        stats.memory_mb = process.memory() as f32 / 1_048_576.0;
    }
}
