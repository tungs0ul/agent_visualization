//! Pause, resume and stop a session from the app (PLAN.md M25).
//!
//! The app writes `~/.agent-room/hold/<session_id>`, and `room-hook` reads it before
//! each tool call. A paused session's hook waits there, so the tool does not run until
//! the app removes the file. A stopped session's hook ends the turn. No file means the
//! session runs. Files make it work for terminal sessions too, and cost one failed open
//! per tool call when nothing is held.

use std::path::PathBuf;

/// What the app asks of a session's next tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// Wait before the tool runs, until resumed or stopped.
    Pause,
    /// End the turn before the tool runs. The session stays.
    Stop,
}

/// Shown to Claude and in the terminal when the turn is stopped.
pub const STOP_REASON: &str = "Stopped from Agent Room";

impl Hold {
    fn word(self) -> &'static str {
        match self {
            Hold::Pause => "pause",
            Hold::Stop => "stop",
        }
    }

    fn parse(word: &str) -> Option<Self> {
        match word.trim() {
            "pause" => Some(Hold::Pause),
            "stop" => Some(Hold::Stop),
            _ => None,
        }
    }
}

/// `~/.agent-room/hold`.
pub fn dir() -> Option<PathBuf> {
    crate::state_dir().map(|dir| dir.join("hold"))
}

/// The session's hold file. `None` for an id that could leave the folder.
fn path(session: &str) -> Option<PathBuf> {
    let safe = !session.is_empty() && session.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if safe { dir().map(|dir| dir.join(session)) } else { None }
}

/// The session's hold, if the app set one.
pub fn read(session: &str) -> Option<Hold> {
    Hold::parse(&std::fs::read_to_string(path(session)?).ok()?)
}

/// Sets or clears (`None`) the session's hold.
pub fn write(session: &str, hold: Option<Hold>) -> std::io::Result<()> {
    let Some(path) = path(session) else { return Ok(()) };
    match hold {
        Some(hold) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, hold.word())
        }
        None => match std::fs::remove_file(path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        },
    }
}

/// Clears every hold: the app does this when it starts and quits, so no session waits
/// on an app that is gone.
pub fn clear_all() {
    if let Some(dir) = dir() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_refuses_paths_outside_the_folder() {
        let dir = std::env::temp_dir().join(format!("hold-test-{}", std::process::id()));
        // SAFETY: the only test in this crate that touches AGENT_ROOM_DIR.
        unsafe { std::env::set_var("AGENT_ROOM_DIR", &dir) };
        assert_eq!(read("s-1"), None);
        write("s-1", Some(Hold::Pause)).unwrap();
        assert_eq!(read("s-1"), Some(Hold::Pause));
        write("s-1", Some(Hold::Stop)).unwrap();
        assert_eq!(read("s-1"), Some(Hold::Stop));
        write("s-1", None).unwrap();
        write("s-1", None).unwrap();
        assert_eq!(read("s-1"), None);
        write("../escape", Some(Hold::Pause)).unwrap();
        assert!(!dir.join("escape").exists() && !dir.parent().unwrap().join("escape").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
