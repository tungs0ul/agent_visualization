//! Unix socket server: receives hook events from `room-hook` (PLAN.md 3.5).
//!
//! One background thread accepts connections; each connection gets a short-lived thread
//! that reads one `HookEnvelope` line, parses it and hands it to Bevy over a channel.
//! For permission requests the connection stays open inside a [`ReplyHandle`] until the
//! user answers.

use std::fs::{self, File, Permissions};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender};
use room_proto::event::RoomEvent;
use room_proto::{ChannelHello, ChannelInfo, ChannelMessage, HookEnvelope, LOCK_NAME, PermissionReply, SOCKET_NAME};

const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Wakes the Bevy event loop, which sleeps while nothing happens (reactive mode).
pub type Waker = Arc<OnceLock<Box<dyn Fn() + Send + Sync>>>;

/// One event from Claude Code, plus the open connection when it waits for an answer.
pub struct Incoming {
    pub event: RoomEvent,
    pub reply: Option<ReplyHandle>,
}

/// The waiting `room-hook` process for one permission request. Dropping it without
/// answering closes the connection, and Claude Code falls back to its terminal prompt.
pub struct ReplyHandle(UnixStream);

impl ReplyHandle {
    /// True once the waiting `room-hook` is gone: Claude Code stopped waiting (the user
    /// pressed Esc, the turn was interrupted) or the hook timed out.
    pub fn is_closed(&self) -> bool {
        peer_closed(&self.0)
    }

    pub fn send(mut self, reply: &PermissionReply) {
        let Ok(mut line) = serde_json::to_string(reply) else { return };
        line.push('\n');
        // The hook may have timed out already; then there is nobody left to tell.
        if let Err(err) = self.0.write_all(line.as_bytes()) {
            warn!("permission reply not delivered: {err}");
        }
    }
}

/// True once the other end hung up. The hook and the channel never send anything after
/// their first line, so reading consumes nothing that matters: 0 bytes means closed.
fn peer_closed(stream: &UnixStream) -> bool {
    if stream.set_nonblocking(true).is_err() {
        return false;
    }
    let mut probe = [0u8; 1];
    let closed = match (&*stream).read(&mut probe) {
        Ok(0) => true,
        Ok(_) => false,
        Err(err) => err.kind() != ErrorKind::WouldBlock,
    };
    let _ = stream.set_nonblocking(false);
    closed
}

/// A terminal session's channel (`room-hook --channel`, PLAN.md M8b): what the user
/// sends here reaches that session as if typed in its terminal.
pub struct Channel {
    pub info: ChannelInfo,
    stream: UnixStream,
}

impl Channel {
    pub fn is_closed(&self) -> bool {
        peer_closed(&self.stream)
    }

    /// False when the session is gone.
    pub fn send(&mut self, text: &str) -> bool {
        let Ok(mut line) = serde_json::to_string(&ChannelMessage { text: text.to_owned() }) else { return false };
        line.push('\n');
        self.stream.write_all(line.as_bytes()).is_ok()
    }
}

#[derive(Clone)]
struct Senders {
    events: Sender<Incoming>,
    channels: Sender<Channel>,
}

#[derive(Resource)]
pub struct Server {
    pub events: Receiver<Incoming>,
    pub channels: Receiver<Channel>,
    pub waker: Waker,
    /// Held for the life of the app: a second instance can't take the lock.
    _lock: File,
}

impl Server {
    /// Takes the single-instance lock and starts listening. Fails if another Agent Room
    /// is already running or the state dir is unusable.
    pub fn start() -> Result<Self, String> {
        let dir = room_proto::state_dir().ok_or("HOME is not set")?;
        prepare_dir(&dir)?;

        let lock = File::create(dir.join(LOCK_NAME)).map_err(|e| format!("lock file: {e}"))?;
        if lock.try_lock().is_err() {
            return Err("Agent Room is already running".into());
        }

        let socket = dir.join(SOCKET_NAME);
        // Safe while we hold the lock: no other instance can be using it.
        if socket.exists() {
            fs::remove_file(&socket).map_err(|e| format!("remove stale {}: {e}", socket.display()))?;
        }
        let listener = UnixListener::bind(&socket).map_err(|e| format!("bind {}: {e}", socket.display()))?;
        fs::set_permissions(&socket, Permissions::from_mode(0o600)).map_err(|e| format!("chmod socket: {e}"))?;

        let (sender, events) = crossbeam_channel::unbounded();
        let (channel_sender, channels) = crossbeam_channel::unbounded();
        let senders = Senders { events: sender, channels: channel_sender };
        let waker: Waker = Arc::default();
        let thread_waker = waker.clone();
        thread::Builder::new()
            .name("room-socket".into())
            .spawn(move || accept_loop(listener, senders, thread_waker))
            .map_err(|e| format!("spawn socket thread: {e}"))?;
        info!("listening on {}", socket.display());
        Ok(Self { events, channels, waker, _lock: lock })
    }
}

/// Creates the state dir as 0700, or tightens an existing one.
fn prepare_dir(dir: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| format!("create {}: {e}", dir.display()))?;
    fs::set_permissions(dir, Permissions::from_mode(0o700)).map_err(|e| format!("chmod {}: {e}", dir.display()))
}

fn accept_loop(listener: UnixListener, senders: Senders, waker: Waker) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let sender = senders.clone();
                let waker = waker.clone();
                thread::spawn(move || handle(stream, &sender, &waker));
            }
            Err(err) => warn!("socket accept failed: {err}"),
        }
    }
}

fn handle(stream: UnixStream, senders: &Senders, waker: &Waker) {
    // macOS answers EINVAL once the peer has closed, which every fire-and-forget hook
    // does right after writing. Its line is still readable, so carry on without a timeout.
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if let Err(err) = reader.read_line(&mut line) {
        warn!("socket read failed: {err}");
        return;
    }
    // A bare connect without data: someone checking that the app is up.
    if line.trim().is_empty() {
        return;
    }
    if let Ok(ChannelHello { channel }) = serde_json::from_str::<ChannelHello>(&line) {
        // The channel stays connected for the session's life: no read timeout.
    let _ = reader.get_ref().set_read_timeout(None);
        if senders.channels.send(Channel { info: channel, stream: reader.into_inner() }).is_ok()
            && let Some(wake) = waker.get()
        {
            wake();
        }
        return;
    }
    let envelope: HookEnvelope = match serde_json::from_str(&line) {
        Ok(envelope) => envelope,
        Err(err) => return warn!("bad envelope: {err}"),
    };
    let event = match RoomEvent::parse(&envelope.hook) {
        Ok(event) => event,
        Err(err) => return warn!("bad hook event: {err}"),
    };
    let reply = envelope.needs_reply.then(|| ReplyHandle(reader.into_inner()));
    if senders.events.send(Incoming { event, reply }).is_ok()
        && let Some(wake) = waker.get()
    {
        wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_handle_notices_when_the_hook_goes_away() {
        let (app_side, hook_side) = UnixStream::pair().unwrap();
        let handle = ReplyHandle(app_side);
        assert!(!handle.is_closed());
        assert!(!handle.is_closed(), "probing must not change the answer");
        drop(hook_side);
        assert!(handle.is_closed());
    }

    #[test]
    fn reply_reaches_the_hook() {
        let (app_side, hook_side) = UnixStream::pair().unwrap();
        ReplyHandle(app_side).send(&PermissionReply { decision: room_proto::Decision::Allow, message: None, answers: None });
        let mut line = String::new();
        BufReader::new(hook_side).read_line(&mut line).unwrap();
        assert_eq!(line, "{\"decision\":\"allow\"}\n");
    }

    #[test]
    fn channel_hello_becomes_a_channel_the_app_can_type_into() {
        let (events, _) = crossbeam_channel::unbounded();
        let (channels, arrived) = crossbeam_channel::unbounded();
        let (app_side, mut hook_side) = UnixStream::pair().unwrap();
        hook_side.write_all(b"{\"channel\":{\"claude_pid\":42,\"session_id\":\"s1\"}}\n").unwrap();
        handle(app_side, &Senders { events, channels }, &Waker::default());
        let mut channel = arrived.try_recv().unwrap();
        assert_eq!(channel.info.claude_pid, 42);
        assert!(!channel.is_closed());
        assert!(channel.send("hello"));
        let mut line = String::new();
        BufReader::new(&hook_side).read_line(&mut line).unwrap();
        assert_eq!(line, "{\"text\":\"hello\"}\n");
        drop(hook_side);
        assert!(channel.is_closed());
    }
}
