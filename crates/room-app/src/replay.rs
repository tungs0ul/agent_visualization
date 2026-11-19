//! Mission replay (PLAN.md M28, the user's brief): every hook event the app receives
//! is recorded per session in `~/.agent-room/missions/<session>.jsonl` (the format
//! `scripts/replay.py` reads). Replaying one parks the live model, feeds the recorded
//! events into a fresh one at the chosen speed, and the bridge rebuilds itself from
//! them: the same rules as live, no canned animation. Live events keep going into the
//! parked model meanwhile; Exit brings it back.

use std::io::{BufRead, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::prelude::*;
use room_proto::event::RoomEvent;

use crate::model::Model;

/// Idle stretches longer than this play as this long (a session can sit for hours).
const MAX_GAP_MS: u64 = 4000;
/// Recordings older than this are deleted at start.
const KEEP_DAYS: u64 = 30;
/// A recording stops growing at this size.
const MAX_BYTES: u64 = 50 * 1024 * 1024;
pub const SPEEDS: [f32; 4] = [1.0, 2.0, 5.0, 10.0];

fn dir() -> Option<PathBuf> {
    room_proto::state_dir().map(|dir| dir.join("missions"))
}

/// The file a session's events go to; ids are hex and dashes, anything else is dropped.
fn file_for(session: &str) -> Option<PathBuf> {
    let safe: String = session.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    (!safe.is_empty()).then(|| dir().map(|dir| dir.join(format!("{safe}.jsonl")))).flatten()
}

/// Appends one hook event to its session's recording (called on the socket thread).
pub fn record(session: &str, hook: &serde_json::Value) {
    let Some(path) = file_for(session) else { return };
    if path.metadata().is_ok_and(|meta| meta.len() > MAX_BYTES) {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let ts_ms = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64);
    let line = serde_json::json!({ "ts_ms": ts_ms, "hook": hook });
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{line}");
    }
}

/// Deletes recordings not touched for `KEEP_DAYS`.
pub fn prune() {
    let Some(dir) = dir() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let limit = Duration::from_secs(KEEP_DAYS * 24 * 3600);
    for entry in entries.flatten() {
        let old = entry.metadata().and_then(|meta| meta.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age > limit);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// A recorded session, for the list.
pub struct Mission {
    pub path: PathBuf,
    pub name: String,
    pub modified: SystemTime,
    pub bytes: u64,
}

/// Recorded sessions, newest first.
pub fn missions() -> Vec<Mission> {
    let Some(dir) = dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut list: Vec<Mission> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            let path = entry.path();
            Some(Mission { name: name_of(&path), path, modified: meta.modified().ok()?, bytes: meta.len() })
        })
        .collect();
    list.sort_by_key(|mission| std::cmp::Reverse(mission.modified));
    list
}

/// The project folder of a recording (its first event with a `cwd`), else the id.
fn name_of(path: &Path) -> String {
    let id = path.file_stem().map(|stem| stem.to_string_lossy().chars().take(8).collect()).unwrap_or_default();
    let Ok(file) = std::fs::File::open(path) else { return id };
    std::io::BufReader::new(file)
        .lines()
        .take(50)
        .map_while(Result::ok)
        .find_map(|line| {
            let value: serde_json::Value = serde_json::from_str(&line).ok()?;
            let cwd = value["hook"]["cwd"].as_str()?.to_owned();
            cwd.rsplit('/').find(|part| !part.is_empty()).map(str::to_owned)
        })
        .unwrap_or(id)
}

/// Reads a recording into (time from its start in ms, event), long gaps shortened.
pub fn load(path: &Path) -> std::io::Result<Vec<(u64, RoomEvent)>> {
    let file = std::fs::File::open(path)?;
    let mut events = Vec::new();
    let (mut last, mut clock) = (None::<u64>, 0u64);
    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        let Ok(event) = RoomEvent::parse(&value["hook"]) else { continue };
        let ts = value["ts_ms"].as_u64().unwrap_or(0);
        if let Some(last) = last {
            clock += ts.saturating_sub(last).min(MAX_GAP_MS);
        }
        last = Some(ts);
        events.push((clock, event));
    }
    Ok(events)
}

/// What the user asked the replay to do; `tick` does it (it owns the model).
pub enum Request {
    Start(PathBuf),
    Seek(u64),
    Stop,
}

#[derive(Resource)]
pub struct Replay {
    pub active: Option<Playback>,
    pub request: Option<Request>,
    /// Why the last start failed, to show in the console.
    pub error: Option<String>,
}

impl Default for Replay {
    /// `AGENT_ROOM_REPLAY=<recording>` starts replaying it at launch (screenshot tests
    /// can't click).
    fn default() -> Self {
        let request = std::env::var_os("AGENT_ROOM_REPLAY").map(|path| Request::Start(PathBuf::from(path)));
        Self { active: None, request, error: None }
    }
}

pub struct Playback {
    pub name: String,
    events: Vec<(u64, RoomEvent)>,
    next: usize,
    /// Where playback stands, ms from the recording's start.
    pub clock: u64,
    pub playing: bool,
    pub speed: f32,
    /// The live model, parked while the replay owns the room.
    pub live: Model,
}

impl Playback {
    pub fn length(&self) -> u64 {
        self.events.last().map_or(0, |(at, _)| *at)
    }

    pub fn played(&self) -> usize {
        self.next
    }

    pub fn total(&self) -> usize {
        self.events.len()
    }

    /// Applies every event up to the clock.
    fn catch_up(&mut self, model: &mut Model) {
        while let Some((at, event)) = self.events.get(self.next) {
            if *at > self.clock {
                break;
            }
            model.apply(event);
            self.next += 1;
        }
    }
}

/// Plays the replay and carries out the console's requests. While a replay runs, the
/// room's `Model` is the replay's; `main::receive_events` sends live events to the
/// parked one (`Replay::live_model`).
pub fn tick(time: Res<Time>, mut replay: ResMut<Replay>, mut model: ResMut<Model>, mut view: ResMut<crate::view3d::View3d>) {
    match replay.request.take() {
        Some(Request::Start(path)) => match load(&path) {
            Ok(events) if !events.is_empty() => {
                let live = match replay.active.take() {
                    Some(old) => old.live,
                    None => std::mem::take(&mut *model),
                };
                *model = Model::default();
                replay.error = None;
                replay.active =
                    Some(Playback { name: name_of(&path), events, next: 0, clock: 0, playing: true, speed: SPEEDS[1], live });
            }
            Ok(_) => replay.error = Some("That recording is empty.".to_owned()),
            Err(err) => replay.error = Some(format!("Couldn't read it: {err}")),
        },
        Some(Request::Seek(to)) => {
            if let Some(play) = replay.active.as_mut() {
                if to < play.clock {
                    *model = Model::default();
                    play.next = 0;
                }
                play.clock = to.min(play.length());
                play.catch_up(&mut model);
            }
        }
        Some(Request::Stop) => {
            if let Some(play) = replay.active.take() {
                *model = play.live;
            }
        }
        None => {}
    }
    let Some(play) = replay.active.as_mut() else { return };
    if !play.playing {
        return;
    }
    view.moving = true;
    play.clock += (time.delta_secs() * play.speed * 1000.0) as u64;
    play.catch_up(&mut model);
    if play.next >= play.events.len() {
        play.playing = false;
    }
}

impl Replay {
    /// The model live events go to: the parked one during a replay.
    pub fn live_model<'a>(&'a mut self, room: &'a mut Model) -> &'a mut Model {
        match self.active.as_mut() {
            Some(play) => &mut play.live,
            None => room,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_recording_plays_back_with_long_gaps_shortened() {
        let dir = std::env::temp_dir().join(format!("replay-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        let lines = [
            json!({ "ts_ms": 1_000, "hook": { "session_id": "s", "cwd": "/src/app", "hook_event_name": "UserPromptSubmit", "prompt": "go" } }),
            json!({ "ts_ms": 2_500, "hook": { "session_id": "s", "hook_event_name": "PreToolUse", "tool_name": "Read", "tool_input": { "file_path": "/src/app/a.rs" }, "tool_use_id": "t" } }),
            json!({ "ts_ms": 3_600_000, "hook": { "session_id": "s", "hook_event_name": "Stop" } }),
        ];
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(&path, format!("{text}not json\n")).unwrap();
        let events = load(&path).unwrap();
        assert_eq!(events.iter().map(|(at, _)| *at).collect::<Vec<_>>(), vec![0, 1_500, 1_500 + MAX_GAP_MS]);
        assert_eq!(name_of(&path), "app");

        // Played to 2 s: the prompt and the Read, not the Stop.
        let mut model = Model::default();
        let mut play =
            Playback { name: "app".into(), events, next: 0, clock: 2_000, playing: true, speed: 1.0, live: Model::default() };
        play.catch_up(&mut model);
        assert_eq!(play.played(), 2);
        let session = model.shown().unwrap();
        assert_eq!(session.main_work.calls.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
