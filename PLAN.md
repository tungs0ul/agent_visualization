# Agent Room — Implementation Plan

A native Rust desktop app (no webview) that shows Claude Code and its sub-agents as
animated characters working in a room, like a small game. It also lets the user answer
Claude Code permission prompts (allow / deny) from the app.

Naming: the product, plugin and binaries are called **agent-room** ("Agent Room").
Plugin names starting with `claude-` are reserved by Claude Code (`claude plugin
validate` errors), and a shared product should not look like an official Anthropic one.
The local repo folder is still `~/claude-room`; renaming it is optional.

This file is the single source of truth for the project. It is written so that any
engineer or LLM can pick up the work cold. **Update the "Status" and "Handoff log"
sections at the end of every work session.**

---

## 1. Goals

1. Show, live, everything the main Claude Code agent does: prompts received, tools used,
   what it is thinking/saying.
2. Show every sub-agent it spawns, what each one does, and an animation when a
   sub-agent reports its result back to the main agent.
3. Let the user answer permission prompts (for the main agent or any sub-agent) by
   clicking Allow or Deny in the app.
4. Look modern and polished: flat vector art, smooth animation, game-like feel.
   Not pixel art.
5. Cost almost nothing when idle (the machine runs 24/7; electricity matters).

### Non-goals (v1)

- Controlling Claude (sending prompts, stopping agents) from the app.
- "Always allow" rules that write to Claude Code settings.
- Windows/Linux support (macOS first; keep code portable where free).
- Remote/multi-machine viewing.

---

## 2. Decisions already made (do not re-litigate)

| Topic | Decision | Reason |
|---|---|---|
| Language | Rust | User request |
| UI tech | **Bevy 0.20.x** game engine, no webview | Game-like scene: sprites, tweening, particles, camera. GPUI (Zed's framework) was considered and rejected: great for app UI, weak for scene animation. |
| Panels / log UI | `bevy_egui` 0.43.x (supports Bevy ^0.20) | Cheap way to get scrollable log, buttons, text input |
| Vector rendering | `bevy_prototype_lyon` 0.18.x (supports Bevy ^0.20) | Tessellated vector shapes stay crisp at any window size. Characters are built from shapes in code. |
| Not now | `bevy_vello` (SVG + Lottie) | Latest 0.14.0 only supports Bevy ^0.19. Revisit if it gains 0.20 support; Lottie would allow designer-made animations. |
| Art style | Flat vector, soft gradients/shadows, rounded shapes | User: "nice and modern, not old pixel" |
| Window | Always-on-top small window in bottom-right corner; resizable; can maximize / go full screen | User request |
| Repo | Standalone repo at `~/claude-room` (not inside any other project) | User request |
| Event source | Claude Code **hooks** + tailing transcript `.jsonl` files | Only official, stable way to observe Claude Code |
| Hook delivery | Hooks ship inside a **Claude Code plugin** (`plugin/hooks/hooks.json`), never written into the user's `settings.json` | Plugin hooks merge with the user's own hooks; `/plugin uninstall` removes them cleanly; works for other people |
| Hook type | `command` hook running the bundled `room-hook` binary (exec form) | Exits silently when the app is closed. An `http` hook would show a "hook error" notice on every event while the app is down (connection failure = non-blocking error) |
| Name | `agent-room` | `claude-*` plugin names are reserved |
| Permission safety | App never auto-allows. No answer / app down / timeout ⇒ fall back to the normal terminal prompt | Safety |

Versions verified on 2026-11-17: rustc/cargo 1.97.1, Claude Code 2.1.323,
bevy 0.20.1, bevy_egui 0.43.1, bevy_prototype_lyon 0.18.0, bevy_vello 0.14.0 (Bevy 0.19).

**Bevy API caution:** Bevy changes its API between minor versions. Before writing Bevy
code, read the 0.20 examples (`~/.cargo/registry/src/*/bevy-0.20.*/examples` after the
first build, or https://github.com/bevyengine/bevy/tree/v0.20.1/examples) and the
migration guides. Do not write Bevy code from memory of older versions.

---

## 3. Architecture

```
┌──────────────┐  hook JSON on stdin   ┌───────────┐  NDJSON over      ┌───────────────────────────┐
│ Claude Code  │ ────────────────────▶ │ room-hook │ ─unix socket────▶ │ room-app                  │
│ (main + sub- │                       │ (tiny CLI)│                   │  ├ socket server (tokio   │
│  agents)     │ ◀──── stdout JSON ─── │           │ ◀── reply ─────── │  │  thread) ──channel──┐  │
└──────────────┘  (permission answer)  └───────────┘  (permissions)    │  ├ transcript tailer ──┤  │
       │                                                               │  └ Bevy ECS ◀─────────┘  │
       └── writes transcript .jsonl ─────────────────────────────────▶ │     (room scene + egui)  │
                                                                       └───────────────────────────┘
```

### 3.1 Cargo workspace layout

```
claude-room/
├── PLAN.md                 # this file
├── AGENTS.md               # short pointer for coding agents
├── CLAUDE.md               # imports AGENTS.md for Claude Code
├── Cargo.toml              # [workspace]
├── crates/
│   ├── room-proto/         # shared serde types: socket messages, parsed hook events
│   ├── room-hook/          # hook CLI binary. NO Bevy, NO tokio; std only + serde_json
│   └── room-app/           # Bevy app + socket server + transcript tailer
├── plugin/                 # the Claude Code plugin users install
│   ├── .claude-plugin/plugin.json
│   ├── hooks/hooks.json    # all hook events ⇒ ${CLAUDE_PLUGIN_ROOT}/libexec/room-hook
│   └── libexec/room-hook   # built binary (gitignored in dev; release artifact)
├── .claude-plugin/
│   └── marketplace.json    # (M7) lets users `/plugin marketplace add <owner>/<repo>`
├── assets/                 # fonts (e.g. Inter / a rounded sans), optional SVG icons
├── spike/                  # notes from the M1 spike (raw logs live in ~/.agent-room)
└── scripts/
    ├── build-plugin.sh     # builds room-hook (release) and copies it into plugin/libexec/
    └── replay.sh           # feeds a recorded events.jsonl into a running app
```

The binary goes in `libexec/`, **not** `bin/`: a plugin's `bin/` is added to the Bash
tool's PATH, and claude.ai/Cowork refuse to install plugins that have a top-level `bin/`.

Runtime state dir: `~/.agent-room/` (mode 0700) holding `room.sock`, `app.lock`,
`hook-errors.log`, `window.json`, and in dev the spike log `spike/events.jsonl`.

### 3.2 `room-hook` (the hook CLI)

Claude Code runs this on every hook event, so it **must start fast** (target < 10 ms)
and **must never break Claude Code**.

Behavior:

1. Read all of stdin (the hook JSON).
2. Connect to the socket `~/.agent-room/room.sock` with a short timeout (~50 ms).
   - If it can't connect (app not running): exit 0 with **no stdout**. Claude Code
     proceeds normally.
3. Send one NDJSON line: `{"v":1,"needs_reply":<bool>,"hook":<raw hook JSON>}`.
   `needs_reply` is true only for `PermissionRequest`.
4. If `needs_reply` is false: exit 0 right away.
5. If true: wait for one reply line, with a timeout (default 60 s, configurable with
   env `AGENT_ROOM_PERMISSION_TIMEOUT`).
   - Reply `{"decision":"allow"}`: print the allow JSON (see 3.4), exit 0.
   - Reply `{"decision":"deny","message":"..."}`: print the deny JSON, exit 0.
   - Timeout, socket closed, parse error, anything unexpected: exit 0 with **no
     stdout**, so the normal terminal prompt appears.
6. Never exit non-zero on our own errors (exit 1 shows a "hook error" notice, exit 2
   blocks some events). Log our own errors to `~/.agent-room/hook-errors.log`.

Mode for milestone 1: `room-hook --log` appends `{"ts_ms":<unix ms>,"hook":<raw stdin>}`
as one line to `~/.agent-room/spike/events.jsonl` (override with `--log <path>`) and
prints nothing.

### 3.3 Hook installation: a Claude Code plugin

Hooks are never written into anyone's `settings.json`. They ship in the plugin at
`plugin/hooks/hooks.json`, which Claude Code merges with the user's own hooks while the
plugin is enabled.

- Dev / spike: `claude --plugin-dir ~/claude-room/plugin` loads it for one session only.
- Users: `/plugin marketplace add <owner>/<repo>` then `/plugin install agent-room` (M7).
- Hooks use **exec form** (`command` + `args` array, no shell), with
  `"command": "${CLAUDE_PLUGIN_ROOT}/libexec/room-hook"`. `~` is not expanded in exec
  form, so room-hook finds `$HOME` itself.
- Validate with `claude plugin validate plugin`.
- Default command-hook timeout is 600 s (30 s for `UserPromptSubmit`). The
  `PermissionRequest` hook keeps the default, which is far above room-hook's own wait,
  so room-hook always falls back cleanly before Claude Code kills it.
- Other plugins' hooks run too (the author's machine has ECC GateGuard and caveman). A
  `PreToolUse` hook elsewhere can block a tool call after ours fired, so the app must
  handle a `PreToolUse` with no matching `PostToolUse`: `PostToolUseFailure` covers
  failures, otherwise clear it on the agent's next event or after ~30 s.

Events subscribed: `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`,
`PostToolUse`, `PostToolUseFailure`, `PermissionRequest`, `PermissionDenied`,
`SubagentStart`, `SubagentStop`, `Notification`, `Stop`, `StopFailure`.

### 3.4 Hook payloads

From the Claude Code hooks reference (read 2026-11-17, Claude Code 2.1.323). M1 checks
these against real logs; mark each item "seen" once confirmed.

Common fields on every event: `session_id`, `prompt_id`, `transcript_path`, `cwd`,
`scratchpad_dir`, `permission_mode`, `effort`, `hook_event_name`. **Inside a sub-agent,
events also carry `agent_id` and `agent_type`.** No `agent_id` means the main agent.
This is how tool calls are attributed to sub-agents.

- `UserPromptSubmit`: `prompt`.
- `PreToolUse`: `tool_name`, `tool_input`, `tool_use_id`.
- `PostToolUse`: `tool_name`, `tool_input`, `tool_response`, `tool_use_id`, optional
  `duration_ms`, `mcp_server` for MCP tools.
- `PostToolUseFailure`: `tool_name`, `tool_input`, `tool_use_id`, `error`, optional
  `is_interrupt`, `duration_ms`.
- `PermissionRequest`: `tool_name`, `tool_input` (**no** `tool_use_id`), `mcp_server` for
  MCP tools, optional `permission_suggestions`. Plus `agent_id`/`agent_type` when asked
  from a sub-agent.
- `PermissionDenied`: only fires when *auto mode* denies a call. `tool_name`,
  `tool_input`, `tool_use_id`, `reason`.
- `SubagentStart`: `agent_id` (e.g. `"agent-abc123"`), `agent_type`.
- `SubagentStop`: `agent_id`, `agent_type`, `agent_transcript_path` (sub-agent transcript,
  in a `subagents/` folder next to the main transcript), `last_assistant_message` (its
  final text, used for the report-back bubble), `stop_hook_active`.
  Note: since v2.1.271 a sub-agent may report via the `SubagentHandback` tool; then the
  report is that tool call's `tool_input.message` (seen in Pre/PostToolUse).
- `Notification`: `message`, optional `title`, `notification_type` (`permission_prompt`,
  `idle_prompt`, `agent_needs_input`, `agent_completed`, …).
- `Stop`: `last_assistant_message`, `stop_hook_active`, `background_tasks`,
  `session_crons`.
- `StopFailure`: `error`, optional `error_details`, `last_assistant_message` (the API
  error text).
- `SessionStart` / `SessionEnd`: common fields (check extra fields in the log).

Permission answer on stdout (exit 0):

```json
{ "hookSpecificOutput": { "hookEventName": "PermissionRequest",
    "decision": { "behavior": "allow" } } }
```

```json
{ "hookSpecificOutput": { "hookEventName": "PermissionRequest",
    "decision": { "behavior": "deny", "message": "Denied in Agent Room" } } }
```

Other decision fields exist: `updatedInput` and `updatedPermissions` (allow only), and
`interrupt` (deny only, stops Claude). v1 does not use them. Deny and ask rules are
still applied after an allow. No stdout ⇒ normal terminal prompt.

#### Seen in the first real run (2026-11-17, `claude -p --model haiku`, one Explore sub-agent)

Confirmed: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`,
`PermissionRequest`, `SubagentStart`, `SubagentStop`, `Stop`, `SessionEnd`.
Not yet seen: `PostToolUseFailure`, `PermissionDenied`, `Notification`, `StopFailure`,
an interactive permission answered by a human, parallel sub-agents.

- Sub-agent events carry `agent_id` + `agent_type`; main-agent events don't. ✔
- `agent_id` looks like `"a22f70bf2ae66dab9"` (no `agent-` prefix). The sub-agent
  transcript is `agent_transcript_path` on `SubagentStop`.
- `SessionStart` has only `session_id`, `transcript_path`, `cwd`, `hook_event_name`,
  `source` (`"startup"`). No `permission_mode`, `prompt_id`.
- `SessionEnd` has `reason` (`"other"` in `-p` mode).
- `PermissionRequest` fires in `-p` mode too, and from sub-agents (with `agent_id`).
  `permission_suggestions` is a list of `{type: "addRules", rules: [{toolName, ruleContent}], ...}`.
- `PostToolUse.tool_response` for `Agent` contains `status: "completed"` and the prompt.
- **Orphan `PreToolUse` is real:** 2 of 6 `PreToolUse` events got no `PostToolUse` and no
  `PostToolUseFailure` (one was blocked by another plugin's hook or by the permission
  flow). The app must time these out.
- **`SubagentStart` has no `tool_use_id`.** Linking a sub-agent to the main agent's
  `Agent` tool call (to show *which* delegation it came from) must go by order: the
  oldest unmatched `Agent` `PreToolUse` in that session. Check with parallel sub-agents.
- Sequence for one delegation: main `PreToolUse(Agent)` → `SubagentStart` → sub-agent
  `PreToolUse`/`PostToolUse`… → `SubagentStop(last_assistant_message)` → main
  `PostToolUse(Agent)`.
- room-hook cost: ~6 ms per call (100 calls in 0.57 s, including shell spawn).

#### Seen in the interactive run (2026-11-17, Opus 5.5, two parallel sub-agents)

Fixtures for every case below are in `crates/room-proto/tests/fixtures/` (redacted).

- **Sub-agents usually run in the background.** `PostToolUse(Agent)` fires ~5 ms after
  `PreToolUse(Agent)` with `tool_response = {isAsync: true, status: "async_launched",
  agentId, description, prompt, resolvedModel, outputFile, ...}`. So `PostToolUse(Agent)`
  is **not** completion. Synchronous runs (seen in `-p` mode) return
  `status: "completed"` instead.
- **`tool_response.agentId` links a delegation to its sub-agent exactly.** Use it
  instead of matching by order. (`SubagentStart` can arrive before or after that
  `PostToolUse`; buffer whichever comes first.)
- **Report-back path for background agents:** sub-agent calls the `SubagentHandback` tool
  (`tool_input.message` = the report) → `SubagentStop` **without**
  `last_assistant_message` → main agent gets a `UserPromptSubmit` whose `prompt` starts
  with `<agent-message from="<agent_id>">` and `[Subagent hand-back]`. Use the handback
  `message` for the report bubble, and treat that `UserPromptSubmit` as "main agent
  receives the report" (the catch animation), **not** as a user prompt.
- `SubagentStop` without a matching `SubagentStart`, `agent_type: ""` and a
  `last_assistant_message` was seen ~3 min after the last activity. It is an internal
  Claude Code helper agent (looks like a session recap). Ignore `SubagentStop`s whose
  `agent_id` was never started, or show them only in the log.
- **Manual permission deny fires nothing.** `PermissionRequest` → user picks No in the
  terminal → no `PostToolUse`, no `PostToolUseFailure`, no `PermissionDenied`
  (that event is auto-mode only). Allow in the terminal ⇒ the normal `PostToolUse`.
  So a pending permission that the app did not answer is resolved by: `PostToolUse` /
  `PostToolUseFailure` for the same tool+input ⇒ allowed; any later event from the same
  agent, or `Stop` ⇒ answered in the terminal (deny or interrupted).
- `PermissionRequest.permission_suggestions` can be `addDirectories` as well as
  `addRules`.
- `PostToolUseFailure` for a failing Bash: `error` = `"Exit code 1\n<stderr>"`,
  `is_interrupt: false`, `duration_ms`.
- `SessionStart` also has `model` (`"claude-opus-5-5"`) and `scratchpad_dir` in an
  interactive session.
- `Notification` (idle) did not fire within the test window; it likely needs a longer
  idle time. Not needed for v1 visuals; handle it if it arrives.
- Each `UserPromptSubmit` from the user starts a turn; `Stop` ends it. Several sub-agent
  handbacks can arrive within one turn.

### 3.5 Socket protocol (`room-proto`)

- Unix domain socket at `~/.agent-room/room.sock`. Directory mode 0700, socket 0600.
- App removes a stale socket file on start; only one app instance (lock file
  `~/.agent-room/app.lock`).
- Newline-delimited JSON, one request per connection.
- Request: `HookEnvelope { v: u32, needs_reply: bool, hook: serde_json::Value }`.
- Reply (only when `needs_reply`): `PermissionReply { decision: Allow | Deny, message: Option<String> }`.
- Keep the raw hook JSON in the envelope. Parse it into typed events inside the app,
  so schema changes in Claude Code only need app changes, not hook changes.

### 3.6 Transcript tailing

Hooks don't carry the agent's own text mid-turn ("Now I'll check the tests…"). For
richer bubbles, the app tails `transcript_path` (JSONL) from the hook payload, and the
sub-agent transcripts in the `subagents/` folder next to it
(`<session>/subagents/agent-<id>.jsonl`). The `Stop` and `SubagentStop` hooks carry the
final text as `last_assistant_message`, so the tailer is only for mid-turn text.

- Use the `notify` crate, or poll every 500 ms only while a session is active.
- Read only appended bytes (keep a file offset per transcript).
- Extract assistant text blocks. Shorten to one sentence for bubbles.
- Treat transcript content as display data only. Never execute or interpret it.

### 3.7 App domain model (Bevy ECS)

- `Session { id, cwd, transcript_path, started_at, last_event_at }`: a resource keyed by
  session id. Multiple concurrent sessions are possible; v1 shows the most recently active
  one and has a session picker.
- `Agent` component: `{ session_id, agent_id, kind: Main | Sub, agent_type, slot, state,
  history: Vec<LogLine> }`.
- `AgentState`: `Idle | Thinking | Reading | Writing | Terminal | Web | Delegating |
  WaitingPermission | Reporting | Done`.
- `PendingPermission { id, agent, tool_name, tool_input, reply: oneshot::Sender<PermissionReply>, created_at }`.
- `Bubble` component: text, created_at, ttl, pinned.

Tool to state mapping (starting point):

| Tool | State | Bubble text |
|---|---|---|
| Read, Grep, Glob, LSP | Reading | "Reading `file`", "Searching for `pattern`" |
| Edit, Write, NotebookEdit | Writing | "Editing `file`" |
| Bash | Terminal | the `description` field of `tool_input`, else first 60 chars of `command` |
| WebFetch, WebSearch | Web | "Searching the web: `query`" |
| Agent / Task | Delegating | "Asking `subagent_type`: `description`" |
| mcp__* | Terminal | server + tool name, humanized |
| other | Thinking | tool name |

Threading: the socket server runs on its own thread (tokio current-thread runtime or
plain std threads). It sends events into Bevy over a `crossbeam_channel`. A Bevy system
drains the channel each frame. Permission replies go back via a `oneshot` stored in
`PendingPermission`.

---

## 4. Visual design

### 4.1 Scene

- Top-down / slight 3/4 view of one room. Soft floor gradient, rug, plant, window with
  day/night tint matching local time (optional, polish).
- **Main Claude desk in the center.** Main Claude: warm orange-tan accent.
- **Sub-agent desks on a ring around the center.** Up to 8 visible slots; more go into a
  "+N" queue badge. Each agent type gets a stable accent color (hash of `agent_type`
  into a curated palette: Explore blue, Plan purple, reviewer orange, general green, …).
- A door at the bottom. A new sub-agent walks in through the door to its desk on
  `SubagentStart`.

### 4.2 Characters (built from lyon shapes in code)

- Parts as child entities: body (rounded capsule), head (circle), two eyes, two arms
  (rounded rects), laptop (rounded rect plus screen glow).
- Animations (simple tweens, no external animation crate needed unless helpful):
  - Idle: slow breathing scale on body, random blink.
  - Reading: head tilts, eyes move left-right.
  - Writing / Terminal: arms alternate fast (typing), laptop screen flickers.
  - Web: small globe icon spins above head.
  - Delegating: main Claude turns toward the new sub-agent's desk, small "wave".
  - WaitingPermission: red "!" badge bounces above head, desk light pulses red.
  - Done: sub-agent stands, walks to the door, fades out.
- Use easing (cubic in/out). Never snap.

### 4.3 Bubbles

- Rounded rect plus tail, white with soft shadow (dark mode: dark slate).
- Typewriter text reveal (~40 chars/s), max ~2 lines, ellipsis.
- Fade out after ~4 s unless it's the agent's latest action (latest stays, dimmed).

### 4.4 Report-back animation (`SubagentStop`)

1. Sub-agent stands up, holds up a paper.
2. Paper becomes a paper plane, flies along a quadratic bezier arc to the main desk,
   with a particle trail in the agent's accent color.
3. Pulse ring at main desk; main Claude bubble: "Got it from Explore".
4. Sub-agent walks out the door.

### 4.5 Permission card

- Appears as a card anchored above the requesting agent, plus in the side panel
  (egui).
- Shows: agent name and type, tool name, **full** command or file path or input (scrollable,
  monospace), cwd, countdown to fallback.
- Buttons: **Allow** (green) and **Deny** (red). Keys: `Y` and `N` when the card has focus.
- Several pending at once: stacked, oldest first.
- After timeout: card greys out, "answered in terminal".
- If the window is not visible when a request arrives: bounce the dock icon / request
  user attention (winit `request_user_attention`). Optional macOS notification later.

### 4.6 Side panel / ticker (egui)

- Wide window (> ~700 px): right-side panel with a scrolling event log, filter per agent,
  click an agent in the scene to filter.
- Narrow (corner) window: single-line ticker at the bottom with the latest event.

### 4.7 Fonts

Bundle one open-licensed font (e.g. Inter or Nunito) in `assets/fonts/`, plus a
monospace (JetBrains Mono) for commands. Record licenses in `assets/LICENSES.md`.

---

## 5. Window behavior

- Start: about 420×320 logical px, bottom-right corner of the primary monitor with a 16 px
  margin, `WindowLevel::AlwaysOnTop`, `resizable: true`, decorations on (revisit:
  borderless with custom drag area may look nicer).
- Hotkeys:
  - `F`: toggle `BorderlessFullscreen` ↔ previous windowed size and position.
  - `P`: toggle always-on-top.
  - Double-click empty room: toggle maximized.
  - `Esc`: leave full screen.
- Camera: orthographic, scaled so the whole room fits the window on resize
  (letterbox with the room background color).
- Remember last size, position and pin state in `~/.agent-room/window.json`
  (wrap reads in error handling; bad file ⇒ defaults).
- **macOS risk:** staying above *other apps' full-screen Spaces* needs NSWindow
  `collectionBehavior` (`canJoinAllSpaces | fullScreenAuxiliary`). winit may not expose
  it. If needed, set it with `objc2` / `objc2-app-kit` via the raw window handle. Test in
  milestone 3.

---

## 6. Power and process hygiene (hard requirements)

The machine runs 24/7; wasted CPU/GPU costs money.

- Use Bevy reactive rendering (`WinitSettings::desktop_app()` or the 0.20 equivalent):
  - Render continuously **only while** an animation, bubble fade or pending permission
    countdown is active.
  - Otherwise wait for input or a socket event (send a winit wake-up/redraw request from
    the socket thread when an event arrives).
- Target: idle app < 1 % CPU. Measure with `ps -o pcpu` and Activity Monitor (GPU column).
- Transcript tailer: no busy polling. File watching, or polling only while a session had
  events in the last 60 s.
- Single process. Closing the window exits everything. No helper daemons.
- **Every dev session:** after running the app, tests or replay scripts, check
  `pgrep -fl room-app` / `pgrep -fl room-hook` / `pgrep -fl -- --plugin-dir` /
  `ps -Ao pid,pcpu,etime,comm -r | head`
  and kill leftovers before finishing.

---

## 7. Security

- Socket and state dir permissions: 0700 dir, 0600 socket. Refuse to start if the dir
  is owned by another user.
- The app never sends `allow` without an explicit user click or key press on that specific
  card. No default button focus that a stray Enter could trigger. Require the card to have
  been visible for ≥ 300 ms before input is accepted (prevents accidental clicks when it
  pops up under the cursor).
- Show the full tool input in the card. Truncation hides dangerous commands.
- Hook payloads and transcripts may contain secrets. Keep the event log in memory only;
  spike recordings live in `~/.agent-room/spike/`, outside the repo; test fixtures are
  redacted by hand before they are committed.
- room-hook fails open to the *terminal prompt*, never to "allow".

---

## 8. Milestones

Each milestone ends with: builds clean (`cargo build`, `cargo clippy -- -D warnings`),
tests pass, no leftover processes, PLAN.md status updated.

### M1: Hook spike (verify the data)

1. Create the workspace and `room-hook` with `--log` mode only.
2. Create `plugin/` (manifest + `hooks/hooks.json`) and `scripts/build-plugin.sh`.
   `claude plugin validate plugin` passes.
3. Run sessions with `claude --plugin-dir ~/claude-room/plugin`. This touches no global
   config. Cover: plain tool use, sub-agents in parallel, a permission prompt (allow
   once, deny once), a failing Bash command.
4. Compare `~/.agent-room/spike/events.jsonl` with section 3.4; mark items "seen" and fix
   anything that differs. Copy a few redacted samples of each event into
   `crates/room-proto/tests/fixtures/` (M2 needs them).
5. Measure room-hook overhead (time per call) and note it.

Done when: section 3.4 is confirmed against real logs, with a fixture for every event
type.

### M2: Protocol and app skeleton

- `room-proto` types plus parser from raw hook JSON to typed `RoomEvent`, with unit
  tests against `spike/` samples (copy a redacted subset into `crates/room-proto/tests/fixtures/`
  so tests work without the gitignored log).
- `room-hook` real mode (socket + permission wait + fallback). Tests: app absent ⇒ no
  output, fast exit; timeout ⇒ no output; allow and deny ⇒ correct JSON.
- `room-app` with Bevy window (corner, always-on-top, resizable), socket server, event
  channel, egui log panel listing events as text. No scene art yet.
- `scripts/replay.sh` replays `spike/events.jsonl` with original timing (speed factor).

Done when: live Claude session events appear in the app's log panel.

### M3: Static room and window behavior

- Room background, desks, main Claude character (static), sub-agent spawn/despawn on
  slots.
- Hotkeys F/P/Esc, camera fit on resize, window state persistence.
- Verify the always-on-top behavior over other apps' full-screen Spaces on macOS.
- Reactive rendering in place; measure idle CPU.

### M4: Life (animations and bubbles)

- Character part rigs and all states from 4.2.
- Bubbles with typewriter and fade.
- Walk-in on spawn, walk-out on done.
- Report-back paper plane plus particles plus pulse.
- Transcript tailer for richer bubble text.

### M5: Permissions

- Permission cards (scene + panel), Allow/Deny, Y/N, countdown, stacking, 300 ms input
  guard, request user attention.
- End-to-end test with a real session: allow, deny, timeout fallback, app closed mid-wait.

### M6: Polish

- Session picker for concurrent sessions.
- Agent click ⇒ history filter.
- Optional: sounds (soft, toggle M), launch at login, macOS notification for pending
  permission, day/night tint.
- Revisit `bevy_vello` + Lottie if it supports the Bevy version in use.

### M7: Distribution (so other people can install it)

Goal: a stranger installs the app and the plugin without Rust, without editing any
config file, and can remove both cleanly.

- **Plugin**
  - Root `.claude-plugin/marketplace.json` lists the `agent-room` plugin at `./plugin`,
    so users run `/plugin marketplace add <owner>/<repo>` and `/plugin install agent-room`.
  - Ship a universal macOS `room-hook` (`lipo` of `aarch64-apple-darwin` +
    `x86_64-apple-darwin`) in `plugin/libexec/`. Either commit it on a release branch or
    have CI put it into the release that the marketplace entry points at. Decide in M7.
  - Set `version` in `plugin.json`; bump it every release (users stay pinned otherwise).
  - `claude plugin validate --strict plugin` in CI.
- **App**
  - `.app` bundle (e.g. `cargo-bundle` or a hand-made `Info.plist`), packaged as `.dmg`.
  - Code signing + notarization needs an Apple Developer account (99 USD/year).
    Without it, users must right-click ⇒ Open the first time; fine for friends, not for
    the public.
  - Optional Homebrew cask.
- **First run**
  - The app checks for recent hook traffic. If none, it shows a "Connect to Claude
    Code" screen with the two `/plugin` commands and a copy button.
  - The app never edits Claude Code settings itself, and never installs hooks
    silently. This app can answer permission prompts, so trust matters.
- **Docs**: README with screenshots/GIF, install, uninstall, privacy note (everything
  stays local, nothing is sent anywhere), and the permission fallback behavior.
- **Later**: Linux (Unix socket works; window-level differs per WM), Windows (named
  pipe instead of Unix socket).

---

## 9. Testing strategy

- `room-proto`: unit tests parsing every recorded payload fixture.
- `room-hook`: integration tests with a fake socket server (spawned in-test, killed at the
  end): no server, slow server, allow, deny, malformed reply.
- `room-app`: logic systems (state mapping, slot assignment, permission lifecycle) tested
  as plain functions or with a headless Bevy `App` (no window, `MinimalPlugins`).
- Visual checks: run app + `scripts/replay.sh`, take screenshots
  (`screencapture -l <windowid>` on macOS), then close the app.

---

## 10. Open questions

- Should several Claude sessions share one room (one main desk each) or have separate
  rooms/tabs? (Default v1: separate, picker.)
- Borderless window with custom rounded corners vs native decorations?
- Should denied permissions let the user type a reason that goes back to Claude as the
  deny `message`? (Nice; small egui text field.)

---

## 11. Status

- [x] Plan written (2026-11-17)
- [x] M1 Hook spike (2026-11-17)
- [x] M2 Protocol and app skeleton (2026-11-17). Pending: user clicks Allow/Deny once
      in a live session to confirm the full round trip (see handoff log).
- [ ] M3 Static room and window behavior
- [ ] M4 Life
- [ ] M5 Permissions
- [ ] M6 Polish
- [ ] M7 Distribution

## 12. Handoff log

Add one entry per work session: date, who/which model, what was done, what is next,
anything surprising.

- 2026-11-17, Claude Opus 5.5: Wrote this plan with the user. Repo initialized.
- 2026-11-17, Claude Opus 5.5: Renamed product to agent-room (`claude-*` plugin names
  are reserved). Moved hook delivery to a Claude Code plugin; added M7 Distribution.
  Built M1: cargo workspace, `crates/room-hook` (log mode, 3 unit tests, clippy clean),
  `plugin/` (manifest + hooks.json for 13 events, exec form), `scripts/build-plugin.sh`.
  `claude plugin validate plugin` passes (directory checks want a README and icon; M7).
  Ran one headless session with the plugin; findings are in 3.4 "Seen in the first real
  run". Nothing touches `~/.claude/settings.json`.
  **Next (finish M1):**
  1. `./scripts/build-plugin.sh` (plugin/libexec/ is gitignored, so build after clone).
  2. User runs an interactive `claude --plugin-dir ~/claude-room/plugin` session: two
     parallel sub-agents, one permission allowed, one denied, a failing Bash command,
     wait for an idle notification.
  3. Check the new events in `~/.agent-room/spike/events.jsonl`; update 3.4.
  4. Copy redacted samples of each event into `crates/room-proto/tests/fixtures/`.
  Then start M2 (room-proto crate, room-hook socket mode, Bevy skeleton).
- 2026-11-17, Claude Opus 5.5: User ran the interactive session. Analyzed it (section
  3.4 "Seen in the interactive run"): background sub-agents, `agentId` link,
  SubagentHandback report path, silent manual deny. Wrote 20 redacted fixtures to
  `crates/room-proto/tests/fixtures/`. M1 done. Next: M2.
- 2026-11-17, Claude Opus 5.5: M2 part 1.
  - `crates/room-proto`: `HookEnvelope`, `PermissionReply` (+ `hook_output()` = exact
    stdout JSON), `state_dir()` (honours `$AGENT_ROOM_DIR`), `event::RoomEvent::parse`
    (typed events incl. `ReportReceived`, `Handback`, `ToolFinished.launched` with
    `agentId`), `describe` (tool call ⇒ `Activity` + bubble label, char-safe truncate).
    21 tests incl. all 20 fixtures.
  - `room-hook` forward mode (no args): connects to `<state_dir>/room.sock`; app absent
    ⇒ silent exit; non-permission events don't wait; `PermissionRequest` waits up to
    `AGENT_ROOM_PERMISSION_TIMEOUT` (default 60 s) and prints the decision; timeout,
    closed socket or bad reply ⇒ no stdout (terminal prompt). 7 integration tests
    against a fake app socket. `--log` mode unchanged.
  - The plugin's hooks.json still uses `--log`. Switch to forward mode (drop `args`)
    once room-app can serve the socket, otherwise nothing listens.
  **Next (finish M2):** create `crates/room-app` with Bevy 0.20 + bevy_egui 0.43:
  read the Bevy 0.20 examples first (window settings, `WinitSettings` reactive mode,
  waking the event loop from another thread). Socket server thread: create
  `state_dir` 0700, remove stale socket, bind, chmod 0600, lock file; per connection
  read one `HookEnvelope` line, send `(RoomEvent, Option<reply sender>)` over a
  crossbeam channel, wake the app. Bevy: corner window (always on top, resizable),
  egui log panel listing parsed events. Then `scripts/replay.sh` and the hooks.json
  switch.
- 2026-11-17, Claude Opus 5.5: M2 done.
  - `crates/room-app` (Bevy 0.20.1, bevy_egui 0.43.1 / egui 0.36):
    - `server.rs`: state dir 0700, `app.lock` via `File::try_lock` (second instance
      exits with "Agent Room is already running"), stale socket removed, socket 0600,
      accept thread + one short thread per connection, `Incoming { RoomEvent,
      Option<ReplyHandle> }` over crossbeam, wakes Bevy through
      `EventLoopProxyWrapper` + `WinitUserEvent::WakeUp`.
    - **macOS gotcha:** `set_read_timeout` on an accepted Unix stream returns EINVAL
      once the peer has closed (every fire-and-forget hook). Ignore the error and read
      anyway; treating it as fatal silently dropped most events.
    - `log.rs`: `RoomLog` (capped lines, pending permissions, current session) and
      `summary()` per event. Lines are mirrored to `debug!`; run with
      `RUST_LOG=room_app=debug` to see them in a terminal.
    - `ui.rs`: egui 0.36 panels need a root `Ui` (`egui::Ui::new(ctx, id, UiBuilder::new()
      .layer_id(LayerId::background()).max_rect(ctx.viewport_rect()))`), then
      `egui::Panel::top(..).show(&mut root, ..)` / `egui::CentralPanel::default().show(&mut root, ..)`.
      Permission cards: full input, cwd, Allow/Deny (disabled for the first 300 ms),
      countdown; expiry note "answered in the terminal".
    - `main.rs`: corner window 420×320 logical, always on top, resizable;
      `WinitSettings::desktop_app()` while idle, 250 ms reactive ticks only while a
      countdown shows. Placement uses the `PrimaryMonitor` entity once it exists.
  - `Cargo.toml`: dev profile opt-level 1, dependencies opt-level 3 (Bevy is too slow
    unoptimized).
  - `scripts/replay.py <events.jsonl> [--speed N] [--session PREFIX]` pipes a recorded
    log through `target/debug/room-hook`.
  - Plugin hooks now run room-hook in forward mode (`"args": []`). With the app closed
    they exit silently, so it is safe to keep the plugin loaded.
  - Verified: replaying the interactive session (46 events) shows every event in the
    log in order, both permission requests fall back after the timeout, idle CPU 0.0 %.
    `screencapture` fails from this agent ("could not create image from display": no
    Screen Recording permission), so the UI was checked through the debug log only.
  **Next:**
  1. User check (needs a human click): `cargo run -p room-app` in one terminal,
     `claude --plugin-dir ~/claude-room/plugin` in another, ask for something that
     needs permission, click Allow once and Deny once in the app. Look at the window
     placement and layout.
  2. Start M3 (room scene with lyon shapes: desks, main Claude, sub-agent slots;
     hotkeys F/P/Esc; window.json; agent tracking keyed by `agent_id`, using
     `ToolFinished.launched.agent_id` to link delegations, ignore `SubagentStopped` for
     agents never started).
  Small known gaps: "Got the report from helper a7f0" doesn't know the agent type yet
  (M3 agent tracking fixes it); no Y/N keys yet (M5).
