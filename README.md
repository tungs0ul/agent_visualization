# Agent Room

A small always-on-top window that shows Claude Code at work as a little animated room.
Claude sits at the center desk; every sub-agent it starts walks in, sits at its own desk,
says what it is doing, walks its report over to Claude and leaves. When Claude Code
asks for permission, you can answer right there with **Allow** / **Deny** (or **Y** / **N**).

It is a native Rust app (Bevy), not a web view. Everything stays on your machine.

## Requirements

- macOS (Linux/Windows later)
- Claude Code 2.1.300 or newer
- Rust (to build), or Homebrew

## Install

1. Install the app and its hook:

   ```sh
   cargo install --locked --git https://github.com/<owner>/agent-room room-app room-hook
   ```

   The first build takes a few minutes. It installs `agent-room` and `room-hook` into
   `~/.cargo/bin`. (Homebrew: see `packaging/homebrew/agent-room.rb`.)

2. Add the Claude Code plugin, inside Claude Code:

   ```
   /plugin marketplace add <owner>/agent-room
   /plugin install agent-room@agent-room
   ```

   The plugin only forwards Claude Code's events to the app. It never edits your
   settings, and it does nothing while the app isn't running.

3. Start the app: `agent-room`. Then use Claude Code as usual.

## Install from a clone of this repository

Use this before the repository is published, or to run your own changes.

1. **Get the code and build both programs** (first build takes about 6 minutes):

   ```sh
   git clone <repository url> agent-room   # or use the folder you already have
   cd agent-room
   scripts/install-dev.sh                   # = cargo install --path crates/room-hook
                                            #   + cargo install --path crates/room-app
   ```

   Check: `which agent-room room-hook` prints two paths in `~/.cargo/bin`. If not,
   add `~/.cargo/bin` to your `PATH`.

2. **Add the plugin to Claude Code.** Either permanently, inside Claude Code:

   ```
   /plugin marketplace add /full/path/to/agent-room
   /plugin install agent-room@agent-room
   ```

   or for a single session, without installing anything:

   ```sh
   claude --plugin-dir /full/path/to/agent-room/plugin
   ```

   Restart Claude Code sessions that were already open; plugins load at session start.

3. **Start the app:** run `agent-room` in a terminal (leave it running).

4. **Use Claude Code** in another terminal. The room shows "Hello!" when the session
   starts, and the log panel fills up as Claude works.

After pulling new changes, run `scripts/install-dev.sh` again and restart `agent-room`.

### Nothing shows up?

- Is the app running? Only one can run at a time; a second one prints
  `Agent Room is already running`.
- Is the hook installed? `ls ~/.cargo/bin/room-hook`. The plugin is silent without it.
- Is the plugin active? `/plugin` in Claude Code lists `agent-room` as enabled. Sessions
  started before installing it don't have it.
- Errors from the hook land in `~/.agent-room/hook-errors.log`.

## Using it

- The window starts in the bottom-right corner, always on top. Move and resize it
  freely; it remembers where it was.
- **F** full screen, **Esc** leave full screen, **P** toggle always-on-top.
- When it is at least 700 px wide, a side panel shows the session (pick another one when
  several Claude sessions run), who is in the room and what they are doing, permission
  cards, and the event log. When small, a one-line ticker shows the latest event.
- **Permissions:** a card shows the full command. Allow / Deny, or press Y / N for the
  oldest card. You can type a reason before denying; Claude gets it. If you don't answer
  within 60 seconds (`AGENT_ROOM_PERMISSION_TIMEOUT`), Claude Code asks in the terminal
  as usual. Agent Room never answers on its own.

## Privacy

Hook events (commands, file paths, prompts) go from Claude Code to the app over a Unix
socket in `~/.agent-room` that only your user can open. Nothing is sent anywhere or
written to disk, except the window position in `~/.agent-room/window.json`.

## Uninstall

```
/plugin uninstall agent-room@agent-room
/plugin marketplace remove agent-room
```

```sh
cargo uninstall room-app room-hook
rm -rf ~/.agent-room
```

## Development

See `PLAN.md` for the design, the milestones and a handoff log, and `AGENTS.md` for
rules when working on it with a coding agent.

```sh
cargo run -p room-app                              # the app
claude --plugin-dir ./plugin                       # Claude Code with the plugin, for one session
AGENT_ROOM_HOOK=$PWD/target/debug/room-hook claude --plugin-dir ./plugin   # with your dev hook
scripts/install-dev.sh                             # install this checkout into ~/.cargo/bin
scripts/replay.py ~/.agent-room/spike/events.jsonl # replay recorded events into the app
```

Fonts: Nunito (SIL Open Font License), see `assets/LICENSES.md`.

## License

MIT, see `LICENSE`.
