# Agent Room

A small always-on-top window that shows Claude Code at work as a 3D space-station HQ.
Claude is a robot at the main desk; it and every sub-agent it starts walk to the
station for what they are doing (reading files, research, running commands, writing),
and sub-agents bring their reports back to Claude. A mini mode follows Claude up close
beside your terminal. When Claude Code
asks for permission, you can answer right there with **Allow** / **Deny** (or **Y** / **N**).

It is a native Rust app (Bevy), not a web view. Everything stays on your machine.

## Requirements

- macOS (Linux/Windows later)
- Claude Code 2.1.300 or newer
- Rust (to build), or Homebrew

## Install

1. Install the app and its hook:

   ```sh
   cargo install --locked --git https://github.com/tungs0ul/agent_visualization room-app room-hook
   ```

   The first build takes a few minutes. It installs `agent-room` and `room-hook` into
   `~/.cargo/bin`. (Homebrew: see `packaging/homebrew/agent-room.rb`.)

2. Add the Claude Code plugin, inside Claude Code:

   ```
   /plugin marketplace add tungs0ul/agent_visualization
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
- The stations are the controls. Click the **main desk** (or its sign, or press **S**)
  for settings: full screen, always on top, auto-accept, sound, stats, mini view.
  Click the **bookshelf** (**L**) for the history log, and the **research console**
  (**C**) to chat with Claude. The **writing desk** (**D**) shows what changed in the
  session's folder (git diff), the **processing lab** (**R**) its recent commands with
  their output, the **teleporter** (**O**) starts a new session in a folder, and the
  **wall screen** (**T**) shows today's numbers. Esc or a click on the floor closes it.
- **Chat:** answers stream in as Claude writes, each tool step shows with ✔ or ✖,
  **Stop** ends the turn (not the session), and you can switch the model and the
  permission mode. Resumed sessions show their earlier conversation; several sessions
  run side by side as tabs. Type **@** to pick a file from the session's folder (Tab or
  Enter takes the first match). Drop files on the window: images go with your next
  message, other files are mentioned by path.
- **Plans:** when Claude finishes a plan in plan mode, the plan opens on a board in the
  middle. Approve it (accept edits, or ask for each edit), or keep planning and type
  what should change. Plans are never approved automatically.
- **Why it stopped:** a turn that ends on an API error says why (usage limit, signed
  out, API overloaded, ...). The Claude card shows the session's model and follows
  `/model`; the context line says when Claude Code is compacting.
- When Claude needs you and the window is behind other apps, macOS shows a
  notification (Settings, "Notifications"; macOS may ask to allow notifications for
  Script Editor the first time). A robot working in a linked git worktree says so on
  its label (⎇ branch).
- The top-right cards switch between sessions and show who is working; click a robot's
  name for its details.
- **Permissions:** a card shows the full command. Allow / Deny, or press Y / N for the
  oldest card. You can type a reason before denying; Claude gets it. If you don't answer
  within 60 seconds (`AGENT_ROOM_PERMISSION_TIMEOUT`), Claude Code asks in the terminal
  as usual. Agent Room never answers on its own.
- **Usage limits** (claude.ai subscriptions): the top-right card shows how much of the
  5-hour and the weekly limit you have used. Sessions you start from the app's Chat
  report them on their own. For terminal sessions, Claude Code gives them only to a
  status line command, so add this line to your status line script, after it reads its
  input into `$input` (it prints nothing and does not change your status line):

  ```bash
  printf '%s' "$input" | room-hook --status >/dev/null 2>&1 &
  ```

  Agent Room never edits your Claude Code settings or scripts itself.

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

3D models: made by the Blender scripts in `tools/blender/`, see `assets/LICENSES.md`.

## License

MIT, see `LICENSE`.
