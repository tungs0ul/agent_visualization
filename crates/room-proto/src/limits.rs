//! The account's usage limits: how much of the 5-hour and the 7-day window is used
//! (PLAN.md M10). Hook payloads don't carry them. Two places do:
//!
//! - the JSON Claude Code passes to a status line command (`rate_limits`), which
//!   `room-hook --status` forwards for terminal sessions;
//! - the `rate_limit_event` lines of `claude -p --output-format stream-json`, read by
//!   the app for the sessions it runs itself.
//!
//! Both appear only for claude.ai subscribers, after the session's first answer.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One window: percent used (0 to 100) and when it resets (Unix seconds).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub used: f32,
    pub resets_at: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
}

/// The line `room-hook --status` sends on the socket (instead of a `HookEnvelope`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LimitsReport {
    pub limits: Limits,
}

impl Limits {
    /// From a status line's input: `rate_limits.five_hour.used_percentage` (0 to 100).
    pub fn from_status_line(input: &Value) -> Option<Self> {
        let window = |name: &str| {
            let window = &input["rate_limits"][name];
            Some(Window { used: window["used_percentage"].as_f64()? as f32, resets_at: window["resets_at"].as_u64()? })
        };
        Self { five_hour: window("five_hour"), seven_day: window("seven_day") }.known()
    }

    /// From a stream-json `rate_limit_event`: `unifiedWindows.five_hour.utilization`
    /// (0 to 1).
    pub fn from_stream(event: &Value) -> Option<Self> {
        if event["type"] != "rate_limit_event" {
            return None;
        }
        let window = |name: &str| {
            let window = &event["rate_limit_info"]["unifiedWindows"][name];
            Some(Window { used: window["utilization"].as_f64()? as f32 * 100.0, resets_at: window["resetsAt"].as_u64()? })
        };
        Self { five_hour: window("five_hour"), seven_day: window("seven_day") }.known()
    }

    fn known(self) -> Option<Self> {
        (self.five_hour.is_some() || self.seven_day.is_some()).then_some(self)
    }

    /// Takes the newer report's windows, keeping ours where it has none.
    pub fn merge(&mut self, newer: Limits) {
        self.five_hour = newer.five_hour.or(self.five_hour);
        self.seven_day = newer.seven_day.or(self.seven_day);
    }

    /// Drops windows that have reset since they were reported (Unix seconds `now`).
    pub fn current(self, now: u64) -> Self {
        let live = |window: Option<Window>| window.filter(|window| window.resets_at > now);
        Self { five_hour: live(self.five_hour), seven_day: live(self.seven_day) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_both_sources_the_same_way() {
        let status = json!({ "rate_limits": {
            "five_hour": { "used_percentage": 23.0, "resets_at": 1000 },
            "seven_day": { "used_percentage": 70.0, "resets_at": 2000 } } });
        let stream = json!({ "type": "rate_limit_event", "rate_limit_info": { "unifiedWindows": {
            "five_hour": { "utilization": 0.23, "resetsAt": 1000, "status": "allowed" },
            "seven_day": { "utilization": 0.7, "resetsAt": 2000, "status": "allowed" } } } });
        let a = Limits::from_status_line(&status).unwrap();
        let b = Limits::from_stream(&stream).unwrap();
        assert_eq!(a.five_hour.unwrap().resets_at, b.five_hour.unwrap().resets_at);
        assert!((a.seven_day.unwrap().used - b.seven_day.unwrap().used).abs() < 0.01);
    }

    #[test]
    fn no_limits_without_a_subscription_or_after_a_reset() {
        assert_eq!(Limits::from_status_line(&json!({ "model": {} })), None);
        assert_eq!(Limits::from_stream(&json!({ "type": "assistant" })), None);
        let limits = Limits { five_hour: Some(Window { used: 5.0, resets_at: 100 }), seven_day: None };
        assert_eq!(limits.current(100).five_hour, None);
        assert_eq!(limits.current(99).five_hour, limits.five_hour);
    }
}
