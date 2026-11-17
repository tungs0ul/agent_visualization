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
│   └── libexec/room-hook   # sh shim: execs the installed room-hook, silent if missing
├── .claude-plugin/
│   └── marketplace.json    # (M7) lets users `/plugin marketplace add <owner>/<repo>`
├── assets/                 # fonts (e.g. Inter / a rounded sans), optional SVG icons
├── spike/                  # notes from the M1 spike (raw logs live in ~/.agent-room)
└── scripts/
    ├── install-dev.sh      # cargo install this checkout's agent-room + room-hook
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

### 4.4 Report-back animation (`SubagentHandback`, else `SubagentStop`)

Decided with the user: the sub-agent **walks the report over and hands it to Claude**
(no flying paper plane).

1. Sub-agent says "Here's my report!", steps out sideways from behind its desk and
   walks to a spot beside the front of Claude's desk, holding a paper sheet (white,
   lines in its accent color) in its raised right hand.
2. On arrival the sheet passes in a small arc into Claude's hand (0.45 s); a ring in
   the agent's color pulses there. Claude's bubble: "Got it from Explore" (from the
   `<agent-message>` prompt event).
3. Once it stands still and has stopped (`SubagentStop`), it lingers 0.7 s and walks
   out the door; its desk disappears when it gets there.
4. An agent that stops without a hand-back still walks its last message over.

### 4.4b Office zones: agents go where the work is (M4b)

Requested by the user after seeing an "AI agent office" demo (a pixel-art office where
each agent walks to a room matching its task, with a status chip over its head and a
roster list beside the map). We take the **idea only**: our own flat vector art, our own
characters, no copied layout or names.

- The single room becomes a small office floor plan (design space ~1400×900) with
  zones, each a spot the activity from `describe()` maps to:

  | Zone | Activity | Props |
  |---|---|---|
  | Lead desk (center) | main Claude's home | big desk, two monitors |
  | Desks (one per sub-agent) | `Writing`, `Thinking`, home seat | desk, laptop |
  | Library | `Reading` (Read, Grep, Glob, LSP) | bookshelves, reading chairs |
  | Server corner | `Terminal` (Bash, MCP tools) | racks with blinking LEDs |
  | Window bay | `Web` (WebFetch, WebSearch) | big window, globe |
  | Approval desk | `WaitingPermission` | desk with a red lamp that pulses |
  | Meeting table | `Delegating` (main briefs a new agent), `Reporting` | round table |
  | Lounge | idle / done, before leaving | sofa, coffee machine |
  | Door | spawn and leave | |

- Each zone has a few standing spots; an agent takes a free spot, else waits nearby.
- **Movement:** agents walk (ease in/out, ~250 px/s) along waypoints through doorways
  (a small hand-placed graph, no general pathfinding). A walking agent shows where it's
  heading: a faint dotted line to the target, and the chip says "→ Library".
- **No flicker:** tool calls can last 3 ms. An agent stays in a zone at least ~2 s; newer
  activities replace the queued one, so it only ever walks to the latest.
- **Status chip** over each head (activity word + accent color), the speech bubble from
  4.3 above it for the detail ("Reading PLAN.md").
- Main Claude mostly stays at the lead desk; it walks to the approval desk while a
  permission waits, and to the meeting table when it delegates.
- **Roster list** in the side panel (wide layout): one row per agent with name, type,
  status dot, current action, and where it's heading. Clicking a row highlights that
  agent in the scene and filters the log (replaces the M6 "agent click ⇒ filter" item).
- **Small window problem:** a whole floor plan is too small at 420×320. In the narrow
  layout the camera zooms to the zones where something is happening (smoothly), and
  zooms out to the whole office when idle. Decide in M4b after trying it.

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
  `pgrep -fl agent-room` / `pgrep -fl room-hook` / `pgrep -fl -- --plugin-dir` /
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

**Order decided with the user (2026-11-17): function first, visuals later.** After M4:
1. Agent model refactor (pure, tested state; scene and panels only read it).
2. M5 Permissions.
3. From M6: session picker, roster list (plain list for now).
4. M7 Distribution (cargo install + plugin marketplace).
5. M4b office zones and other visual polish.

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

### M4b: Office zones (see 4.4b)

- Office floor plan with zones and props; replaces the single room.
- Agent movement along the waypoint graph, zone spots, minimum dwell time, "heading
  to" chip and dotted line.
- Activity ⇒ zone mapping, main Claude's trips to the approval desk / meeting table.
- Roster list in the side panel; click to highlight and filter.
- Narrow-window camera that follows activity.
- Check with replays at 420×320 and full screen (screenshot mode).

### M4c: Mini mode (requested 2026-11-17)

For watching while working in the terminal or editor: a widget-sized view that hides
as little as possible, like a macOS widget / Control Center panel.

- Toggle with a key (M) and a small button; remembered in `window.json`.
- Small borderless window (~300×190 logical) in the top-right corner, always on top.
- Background: frosted translucent material like macOS widgets (NSVisualEffectView
  behind a transparent Bevy window: `Window { transparent: true, decorations: false,
  composite_alpha_mode: PostMultiplied }` + `ClearColor(Color::NONE)`, then add the
  effect view via `objc2-app-kit` on the window's NSView). Fallback if the blur can't
  be done: a rounded semi-opaque panel.
- Only Claude and the agents (no floor, zones or desks), drawn smaller; speech bubbles
  shortened; no side panel or ticker. Pending permissions still show as a compact card
  with Allow/Deny (Y/N).
- Draggable by its background (no title bar).

### M4d: Rooms per repository (requested 2026-11-17)

When an agent works in another repository than the session's, a room for that
repository appears and the agent walks there.

- Detect the repo per tool call: from file paths in `tool_input` (`file_path`,
  `notebook_path`, `path`) and Bash commands with `cd <dir>`/absolute paths; walk up to
  the nearest `.git` and read the branch from `.git/HEAD` (in the app, cached; no git
  process per event). The session's own repo is the main room.
- Each repo room: its own floor area with a sign "repo-name · branch", desks, and the
  same zones as needed (library, servers…), so an agent "fetching" or "editing" there
  uses that room's props. Rooms appear when first used and fade after a while unused.
- The roster list shows each agent's room. The office camera fits all rooms in use.
- Decide the layout: rooms side by side with a corridor, or separate "floors"/tabs.

### M4e: Waiting at the Approval desk, with the card there (requested 2026-11-17)

- A sub-agent that needs permission walks to the Approval desk (already) and **stays
  there** until answered; Claude asking shows the same card at his desk.
- The permission card is drawn in the scene next to the waiting agent and is clickable
  (Allow / Deny / reason), in addition to the side-panel card. Both answer the same
  request; whichever is used first wins.

### M4f: Camera controls (requested 2026-11-17)

- Zoom with scroll / pinch, pan by dragging, double-click to reset to the automatic view.
- **Pin an agent:** click an agent in the scene or its roster row to make the camera
  follow it; click again (or Esc) to unpin. The pinned agent gets a highlight ring.
- Manual camera input pauses the automatic follow until reset.

### M8a status (2026-11-17, end of session: IN PROGRESS)

- Done: `chat.rs` — `ChatSession` spawns `claude -p --input-format stream-json
  --output-format stream-json --verbose` in a chosen folder (+ `--plugin-dir
  $AGENT_ROOM_PLUGIN_DIR`, + `$AGENT_ROOM_CLAUDE_ARGS`), reader thread → channel → wakes
  the app; `send()` writes `{"type":"user","message":{"role":"user","content":…}}`;
  `assistant` text blocks → transcript, `result` → not busy + session id. Drop kills the
  child; also, when the app dies, the child sees stdin close and exits (verified).
  `Chats` resource; "Chat" button in the side panel; `ui::chat_window` (folder field +
  Start, transcript with markdown, input with Enter-to-send, Stop).
  Dev: `AGENT_ROOM_DEV_CHAT="<folder>|<message>"` starts a session and sends a message.
- Verified: a session started from the app runs, its hooks reach the room (log shows the
  prompt and "Done: - 2+2 = 4…").
- **Bug to fix first:** in that run neither the chat window nor the final-answer pop-up
  was visible in the screenshot (both should be). Suspect `chat_window`: the
  `ScrollArea::max_height(ui.available_height() - 70.0)` inside a new `egui::Window` may
  be negative/zero on the first frame, or the window is placed off-screen; check with a
  fixed `min_size`/`fixed_size` and `.anchor(...)`, and check whether drawing it before
  the final-answer pop-up hides that pop-up. Reproduce with the dev env var above plus
  `AGENT_ROOM_CLAUDE_ARGS="--model haiku"`, `AGENT_ROOM_PLUGIN_DIR=$PWD/plugin`,
  `AGENT_ROOM_HOOK=$PWD/target/debug/room-hook`, screenshot mode.
- Requested, not started: **M11** show the app's own CPU, RAM and FPS (e.g. a small
  toggleable overlay; `sysinfo` for the process, Bevy `FrameTimeDiagnosticsPlugin`).
  Rendering is already capped: ~30 fps while animating (`FRAME` 33 ms), 0 when idle.

### M8 design update (2026-11-17, user asked: "can we start a new session in our app instead of opening a terminal?")

Two parts; do **8a first**, it is simpler and fully supported:

- **8a: Sessions started from the app (no terminal).** A "New session" button (folder
  picker for the working directory, default: last used) starts Claude Code as a child
  process: `claude -p --input-format stream-json --output-format stream-json --verbose
  --plugin-dir <plugin>` (or rely on the installed plugin) in that directory. The app
  writes user messages as stream-json lines to its stdin and reads its stdout:
  assistant text and tool use for a chat transcript panel, `result` events for turn
  end. Hook events still flow through the plugin, so the room, permissions (the
  `PermissionRequest` hook fires in `-p` mode, seen in M1), questions and the final
  answer pop-up work unchanged. Chat panel: transcript (markdown via egui_commonmark) +
  input box (Enter sends, Shift+Enter newline) + Stop button (kill/interrupt the child).
  Read the current headless docs first (`code.claude.com/docs/en/headless.md`,
  stream-json input format, `--resume <session_id>` to continue a session later).
  Process hygiene: children are killed when the app exits (AGENTS.md rule).
- **8b: Talking to a session already open in a terminal.** Needs a plugin **channel**
  (MCP server declared in plugin.json `channels`) that injects messages into that
  session; see the original M8 notes below. Research availability first.

### M8: Chat with Claude from the app (later; requested 2026-11-17)

A text input in the app (side panel and mini mode) to send messages to the running
Claude Code session instead of typing in the terminal.

- Hooks can't inject input. The supported way is a plugin **channel**: the plugin
  declares an MCP server bound to a channel (`channels` in plugin.json, see the plugin
  manifest reference "Channels" and the channels reference "package as a plugin"); the
  MCP server pushes messages into the session. The app would talk to that MCP server
  over the same `~/.agent-room` socket dir.
- Check first: channel availability/flags in the installed Claude Code version, how a
  message is shown in the session, and how to target one session when several run.
- Alternative if channels don't fit: start/resume sessions headless
  (`claude -p --resume <session_id>`), which only suits sessions not open in a terminal.
- Never send anything without the user pressing Send.

### M9: Auto-accept mode and centre pop-ups (requested 2026-11-17)

- A toggle (button + key) that, while on, **allows permission requests automatically**.
  The user's explicit choice; off by default, never remembered across app starts, shown
  clearly (an amber "Auto-accept ON" badge in the room and the mini view). Log every
  auto-allowed request. Consider an exclude list (`rm -rf`, `git push`) that still asks.
- Questions Claude asks with options (`AskUserQuestion`) appear as a **pop-up panel in
  the middle** of the room to pick or type an answer. Research first whether a
  `PermissionRequest`/`PreToolUse` hook for `AskUserQuestion` can return the answer
  (e.g. `updatedInput`), or whether this needs the M8 channel.
- When Claude finishes a turn (`Stop.last_assistant_message`), show the answer in a
  centre pop-up (dismissable; compact in mini mode).

### M10: Context and usage limits (requested 2026-11-17)

Show the session's context use, the 5-hour limit and the weekly limit (side panel
header; compact bar in mini mode). Hook payloads don't carry it. Candidates: the
transcript's usage fields (context tokens); Claude Code's status-line command input
(it receives model, context and rate-limit info; a plugin status line could forward
it to the app socket); CLI output. Pick the supported, cheap one.

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
- **App: build from source, no `.app` bundle, no code signing** (decided 2026-11-17:
  an Apple Developer account costs 99 USD/year, not worth it for now).
  - `cargo install --git https://github.com/<owner>/<repo> room-app` puts `room-app` in
    `~/.cargo/bin`. Binaries compiled on the user's own machine carry no quarantine
    flag, so Gatekeeper doesn't block them and no notarization is needed.
  - Homebrew: a tap with a **formula** (not a cask) that builds from source with
    `depends_on "rust" => :build` and `cargo install`, e.g.
    `brew install <owner>/tap/agent-room`. Same reason: built locally ⇒ no signing.
  - Name the installed binary `agent-room` (`[[bin]] name = "agent-room"` in
    room-app's Cargo.toml) so the command is memorable.
  - Downsides to state in the README: needs Rust (or Homebrew pulls it in), first build
    takes a few minutes (Bevy), no Dock icon or Spotlight entry. "Launch at login" can be
    a `launchd` plist that `agent-room --install-login-item` writes (optional, later).
  - The plugin keeps shipping its own prebuilt `room-hook` (tiny, std-only). Check in M7
    that a marketplace install (git clone) doesn't set the quarantine attribute on it; if
    it does, the plugin instead runs a `room-hook` installed next to `agent-room`.
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
- [x] M2 Protocol and app skeleton (2026-11-17). Live round trip confirmed by the user:
      Allow in the app ⇒ command ran; Deny ⇒ Claude got "The user denied this in Agent
      Room."; no terminal prompt for either.
- [x] M3 Static room and window behavior (2026-11-17). Not yet checked by a human:
      F/P/Esc keys, window.json restore, always-on-top over other apps' full-screen
      Spaces.
- [x] M4 Life (2026-11-17)
- [x] M4b Office zones (2026-11-17)
- [x] M4c Mini mode, code (2026-11-17); frosted glass still to be confirmed by the user on screen
- [ ] M4d Rooms per repository (part 1 done 2026-11-17: repo + branch detection, shown in roster/title; part 2: separate rooms in the scene)
- [x] M4e Waiting at the Approval desk with the card in the scene (2026-11-17)
- [x] M4f Camera controls: zoom, pan (2026-11-17; pinning dropped by the user)
- [ ] M8 Chat with Claude from the app
- [x] M9 Auto-accept mode and centre pop-ups (2026-11-17)
- [ ] M10 Context, 5-hour and weekly limits in the app
- [x] M5 Permissions (2026-11-17; user still to try Y/N and a deny reason live)
- [ ] M6 Polish
- [x] M7 Distribution, code side (2026-11-17); publishing left to the user

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
      `RUST_LOG=agent_room=debug` to see them in a terminal.
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
- 2026-11-17, Claude Opus 5.5: M3 done. User decided: no `.app` bundle or code signing;
  ship via `cargo install` / Homebrew formula (M7 updated).
  - `scene.rs`: room drawn with bevy_prototype_lyon 0.18 (`ShapeBuilder::with(&shape)
    .fill(c).stroke((c, w)).build()`), design space 900×640 kept in view by
    `ScalingMode::AutoMin`. Floor, wall with windows, door, rug, plants. Characters are
    parent entities with child shapes (shadow, body, head, eyes, desk, laptop, name
    `Text2d`): main Claude at the center ×1.4, sub-agents on 8 seats. `Roster` keys
    seats by `agent_id`, follows one session (switches on a new `SessionStart` /
    `UserPromptSubmit` from another session), seats any sub-agent on its first event
    (so starting mid-session works), keeps `known` id ⇒ type for report labels, ignores
    `SubagentStop` from never-seen agents, removes desks 1.5 s after `SubagentStop`.
    Hook events reach scene systems as the `HookEvent` message.
  - `window.rs`: `initial_window()` from `~/.agent-room/window.json`
    (`{x, y, width, height, on_top}`, physical position / logical size), corner
    placement only on first start, geometry saved 1 s after moving/resizing, keys
    F (full screen), Esc (leave it), P (always on top). **Screenshot mode:**
    `AGENT_ROOM_SCREENSHOT_DIR=<dir>` saves a PNG every 2 s via Bevy's own
    `Screenshot::primary_window()` (no Screen Recording permission needed) and redraws
    continuously; this is how agents check visuals.
  - `ui.rs`: separate egui camera (`PrimaryEguiContext`, `RenderLayers::none()`,
    order 1, alpha blend), room camera viewport clipped to the space the panels leave.
    ≥ 700 logical px wide: right side panel (session, permission cards, log); narrower:
    permission cards on top, one-line ticker at the bottom. Light theme; green Allow,
    red Deny.
  - Visual check: replays at 420×320 and 1000×620 in screenshot mode look right.
  **Next:** user tries F/P/Esc, moves/resizes the window and restarts the app (should
  reopen in the same place), and checks always-on-top over a full-screen app. Then M4
  (animation: idle breathing/blink, typing/reading states from `describe()` activity,
  speech bubbles, walk-in/out, paper-plane report, bundled font per 4.7).
- 2026-11-17, Claude Opus 5.5: Fixed a crash when the user shrinks the window: egui lays
  out with last frame's size, so the room camera's viewport stuck out of the smaller
  window, wgpu rejected `set_scissor_rect` and Bevy quit (the "CommandQueue has
  un-applied commands" warnings are just fallout). `fit_camera` now clamps with
  `Viewport::clamp_to_size`. Added `AGENT_ROOM_SCREENSHOT_RESIZE=<w>x<h>` (resizes after
  the 2nd shot) and verified shrink 1000→400 and grow 420→1100 run with no errors.
  Added section 4.4b and milestone M4b "Office zones" at the user's request.
- 2026-11-17, Claude Opus 5.5: M4 done.
  - `anim.rs`: `Pose` per character (Idle, Thinking, Reading, Typing, Web, Delegating,
    Reporting, Waiting) set from hook events in `scene::react()`; parts (`Part` +
    `Rest` + `RigOf`) animate on top of their rest transform: breathing, blinking,
    head tilt per pose, typing arms, raised arm, bouncing red "!" badge while waiting,
    walk bob. Speech bubbles: typewriter, sized to the text, dim after 4 s. `Walk` in
    two legs (around desks), `Done` lingers then walks out, `HandOver` sheet + `Pulse`.
    `Animating` drives redraw pacing: ~30 fps only while something moves or a
    character isn't idle, otherwise the app sleeps.
  - `scene.rs`: desk and character are separate entities; sub-agents walk in from the
    door to their seat; report hand-over per 4.4. Font Nunito embedded
    (`include_bytes!`, `assets/fonts`, OFL in `assets/LICENSES.md`).
  - **Bevy 0.20 gotcha:** 2D draw items are retained and their sort key (`mesh_z`) is
    computed once, when first queued. Changing an entity's z later does NOT re-sort
    it. So never change z after spawn (characters keep z 0 while walking), and keep
    layers whole units apart (see the comment at the top of `scene.rs`).
  - Screenshot mode got `AGENT_ROOM_SCREENSHOT_EVERY_MS` (catch animations).
  - Verified with replays: walk-in, seated behind desk, typing/reading, permission
    badge, walk-over hand-over with pulse, walk-out.
  **Next:** user watches a live session. Then M4b (office zones) or M5 (permission
  polish: Y/N keys, card above the agent, attention request).
- 2026-11-17, Claude Opus 5.5: Step 1 of the agreed order done: **agent model
  refactor**. `crates/room-app/src/model.rs` holds all agent/session logic as plain
  data: `Model::apply(&RoomEvent) -> Vec<Change>` (ShowSession, Joined, Pose, Say,
  Deliver, Leave), sessions with their agents (`AgentInfo`: type, status, pose, task,
  doing, report), which session is shown, internal-helper detection. 7 unit tests.
  `scene.rs` only maps `Change`s to entities (`Roster` = who is drawn by which entity;
  `Pending` applies the first pose/words to characters spawned in the same frame).
  The log and UI read the model (`model.shown()`, `agent_type()`), so the roster list
  and office zones later only need drawing code. Visual replay unchanged.
  **Next:** M5 Permissions.
- 2026-11-17, Claude Opus 5.5: M5 done (function part).
  - Y / N answer the oldest card once its 300 ms guard has passed (`log::keyboard_target`);
    keys are ignored while egui has keyboard focus (typing a reason). Buttons show the
    key on the card the keys apply to.
  - Optional deny reason per card; sent as "The user denied this: <reason>".
  - `ReplyHandle::is_closed()` (non-blocking read: 0 bytes = hook gone). Cards whose hook
    disappeared (Claude Code stopped waiting: Esc / interrupt) are removed as
    "cancelled"; timed-out ones as "answered in the terminal" (`settle_permissions`).
  - Hook timing tests warm the binary up first: macOS scans a freshly built binary on
    first launch, which broke the 500 ms limits.
  - Verified end to end: request through room-hook ⇒ card with reason field and key
    hints; killing the hook ⇒ "Bash: cancelled".
  - Deferred to the visual phase: card anchored above the asking agent in the scene;
    "request attention" (needs the winit window via `bevy::winit::WINIT_WINDOWS`, a
    main-thread `thread_local`, so a system with `NonSendMarker`).
  **Next:** step 3: session picker + roster list.
- 2026-11-17, Claude Opus 5.5: Step 3 done (session picker + roster list).
  - Model: `sessions()` (most recent first, by an event counter), `show(id)` (returns
    ShowSession + replay changes), `SessionState::{name(), is_busy(), ended,
    main_doing}`, `Pose::word()`. Test added.
  - Side panel (wide layout): session drop-down when there are 2+ sessions
    ("smart_haus · working"), roster list (painted color dot, name, status word,
    current action / task / report; working agents first, last 4 finished ones
    faded), then permission cards, then the log.
  - Log lines and permission cards carry their session; the log and the ticker show
    only the viewed session. Log lines are single `LayoutJob` labels that break
    anywhere (long paths used to push the panel wider). Don't use `ui.indent` in the
    panel: it draws a guide line.
  - egui's default font has no "●"; draw dots with the painter.
  **Next:** step 4: M7 distribution.
- 2026-11-17, Claude Opus 5.5: Step 4 (M7 distribution), code side done.
  - **Changed from the original M7 plan:** no binaries in git. `cargo install --git …
    room-app room-hook` (or the Homebrew formula) installs `agent-room` and `room-hook`
    into `~/.cargo/bin` / Homebrew's bin. The plugin ships `plugin/libexec/room-hook`, a
    POSIX sh shim that execs the first of `$AGENT_ROOM_HOOK`, `~/.cargo/bin/room-hook`,
    `/opt/homebrew/bin/room-hook`, `/usr/local/bin/room-hook`, and otherwise drains stdin
    and exits 0 (so a plugin without the app installed is silent). Overhead ~6 ms per
    event. No universal binary, no quarantine question, no release artifacts.
  - `room-app` builds a binary named `agent-room` (`[[bin]]`).
  - Release profile: thin LTO + strip for all; `room-hook` additionally `opt-level = "z"`,
    one codegen unit.
  - `.claude-plugin/marketplace.json` (marketplace and plugin both named `agent-room`;
    users: `/plugin marketplace add <owner>/agent-room`, `/plugin install
    agent-room@agent-room`). `claude plugin validate .` and `… plugin` pass.
  - `README.md` (install, use, privacy, uninstall, dev), `LICENSE` (MIT, tungs0ul),
    `packaging/homebrew/agent-room.rb` (build-from-source formula template; needs the
    GitHub owner, a release tag and its sha256).
  - `scripts/build-plugin.sh` replaced by `scripts/install-dev.sh`.
  **Left for the user (outward-facing, not done by agents):** create the GitHub repo and
  push, replace `<owner>` in README and the formula, tag a release, optionally publish
  the tap. Then test the real install path on a clean machine/user.
- 2026-11-17, Claude Opus 5.5: README gained "Install from a clone" + troubleshooting.
  Step 5 started: **M4b office zones done.**
  - Office 1400×900: team area (Claude's desk at (0,60), 8 seats), zones Library
    (top-left, shelves), Servers (top-right, racks), Window bay (left, window + globe),
    Approval (right, desk + red lamp), lounge by the door. `scene::ZONES`, `SPOTS`.
  - `Pose::Terminal` added (Bash/MCP), separate from `Typing` (edits).
  - `route_agents`: sub-agents (with `Home` = seat) go to `place_for(pose)` once they
    have been `MIN_DWELL` (2 s) where they are; `Station` records place + arrival time;
    mid-walk changes wait for arrival, then the latest pose wins. Reporting is left to
    the model's `Deliver`. Claude stays at his desk (deliveries need him there).
  - Walks draw a dotted path (consumed as the agent walks) and a "to Library" chip
    under the feet. (Nunito has no "→" glyph; check glyphs before using symbols.)
  - `follow_action`: windows narrower than 640 logical px zoom smoothly onto the agents;
    wider ones show the whole office.
  - The binary is `agent-room`, so log targets are `agent_room` (use
    `RUST_LOG=agent_room=debug`; model changes at `trace`).
  - `replay.py` waits for the app's socket (a freshly built binary can take seconds to
    start on macOS; earlier replays lost their first events). The server ignores empty
    connections.
  - A permission whose hook closes within the last second of the countdown is logged
    as "answered in the terminal", not "cancelled".
  - New requests recorded as milestones, not built: M4c mini mode, M4d rooms per repo,
    M4e approval desk with in-scene card, M4f camera zoom/pan/pin, M8 chat.
  **Next:** user picks the next milestone (suggested: M4c mini mode, since the app is
  meant to sit in a corner while working).
- 2026-11-17, Claude Opus 5.5: **M4c mini mode** (code done, needs a human look).
  - `crates/room-app/src/mini.rs`: `ViewMode { mini, applied, full }` (resource),
    `ToggleMini` message (buttons), systems `toggle` (key M), `drag` (left-drag on empty
    space calls `Window::start_drag_move`), `apply_mode` (main thread via
    `NonSendMarker`), `hide_new_desks`.
  - Entering mini: remembers the full geometry, window 320×200 logical at the top-right
    under the menu bar, no decorations, always on top, `ClearColor(Color::NONE)`,
    `Furniture` + `Desk` entities hidden, glass on. Leaving restores all of it.
  - The window is now always created `transparent: true` with
    `CompositeAlphaMode::PostMultiplied`; the full view paints `BACKDROP` over it.
  - Glass: `window-vibrancy` 0.8.1 (macOS-only dependency). `apply_liquid_glass`
    (macOS 26, `NSGlassEffectViewStyle::Regular`, radius 18), falling back to
    `apply_vibrancy(Popover)`. The effect view is inserted below the view's other
    content; wgpu's CAMetalLayer is a sublayer of the same view, so Bevy should draw on
    top. **Unverified:** Bevy screenshots can't show the composited glass (they read
    Bevy's own frame, where the background is just transparent). If the glass covers
    the room, try `LiquidGlassOptions::content_view` or moving the effect view.
  - `window.json` gains `"mini": bool`; while mini, geometry isn't saved (the full
    window's is kept) and P is ignored.
  - UI: mini shows only permission cards (top) and a "Full" button bottom-right; the
    full views have a "Mini" button (side panel header / ticker).
  - Verified by screenshot: 320×200, transparent background, only characters drawn,
    camera follows agents, no warnings.

### Handover (2026-11-17, end of this session)

State: all work committed on master in `~/claude-room`; 46 tests pass; clippy clean;
plugin + marketplace validate. Not published anywhere (no GitHub repo yet).

How to run: see README "Install from a clone". Dev loop: `cargo run -p room-app` +
`AGENT_ROOM_HOOK=$PWD/target/debug/room-hook claude --plugin-dir ./plugin`.
Visual checks without a human: screenshot mode + `scripts/replay.py` with
`AGENT_ROOM_DIR` under `$TMPDIR` (see AGENTS.md). Scenario files used this session
(two-session spike log, synthetic office walk-through) live in the user's
`~/.agent-room/spike/events.jsonl`; the synthetic `office.jsonl` generator is in the
handoff entry for M4b (recreate it: SessionStart, prompt, two Agent launches, sub-agents
reading → web, Bash → permission → edit, hand-backs, Stop; ~3 s apart).

Open items, in order:
1. **User checks mini mode on screen** (press M or "Mini"): frosted glass visible behind
   the characters? drag works? Fix per the note above if not.
2. User tries Y/N and deny-with-reason live (M5 leftovers).
3. Next milestones, user to choose: M4e (wait at Approval desk + in-scene card),
   M4f (camera zoom/pan/pin agent), M4d (rooms per repo), M8 (chat via plugin channel).
4. Publishing (user only): GitHub repo, replace `<owner>` in README and
   `packaging/homebrew/agent-room.rb`, tag a release.

Later in the session (2026-11-17):
- Fixed: the room stuck to the first session it saw, hiding the active session's events
  (only permission cards showed). It now switches when the shown session has had no
  events for `QUIET_EVENTS` (5) events from other sessions, or ended (test added).
- Fixed: switching mini mode made one frame mix old/new window sizes; wgpu rejected it
  and Bevy quit. A `RenderErrorHandler` now ignores such errors (logged, frame skipped).
  `AGENT_ROOM_SCREENSHOT_TOGGLE_MINI=1` toggles mini after the 2nd shot and back after
  the 4th; verified 2200→640→2200 px with the app still running.
- New milestones M9 (auto-accept + centre pop-ups) and M10 (context and limits).
- Not pushed: `git push` is blocked for agents by the auto-mode classifier; the user
  runs `git push -u origin main`. Branch `main`; remote
  `git@github.com:tungs0ul/agent_visualization.git`; the user's SSH key works.
- If the app shows nothing: the plugin's shim needs an installed `room-hook`
  (`scripts/install-dev.sh` or `AGENT_ROOM_HOOK=...`), and Claude sessions must be
  started after the plugin is enabled.

- **M9 done:** `AutoAccept` (key A / "Auto-accept" button, amber when on, off at every
  start) allows permission requests in `receive_events` except `log::ALWAYS_ASK`
  (rm -rf, git push, git reset --hard, sudo, mkfs, dd if=, git clean -f); each is logged
  "Auto-allowed …". `AskUserQuestion`: the hook now also waits on its `PreToolUse`
  (`room_proto::needs_reply`), `PermissionReply.answers` carries question ⇒ answer, and
  `hook_output(hook)` returns `permissionDecision: allow` + `updatedInput` (questions
  echoed + `answers`). Centre pop-up "<agent> asks" (options, multi-select, free text,
  Send, "Answer in terminal" = drop the reply so the terminal asks). Final answer of
  the shown session pops up centre on `Stop` until closed / next prompt. Verified by
  screenshots; the question hook prints nothing on timeout.
- Fixed: starting in mini mode now creates the window at mini size (macOS ignored the
  early resize, leaving Bevy drawing 640×400 into a 1536×924 window).
- Fixed: the side panel header overflowed (drop-down 296 px + Mini button), pushing the
  panel wider and leaving its old edge as a line through the text.

- **M4e done:** `PendingPermission.who` records who asks; `ui::InRoom` (SystemParam)
  maps that character to window coordinates (`Camera::world_to_viewport`), and the
  oldest card of the shown session floats next to the character (`egui::Area`, popup
  frame: "May I? <tool>", command, Allow/Deny with the same 300 ms guard). Answers from
  either card go through one path. Sub-agents already walk to and stay at the Approval
  desk while `Pose::Waiting`.
- **M4f done (without pinning, dropped by the user: agents come and go too fast):**
  `camera.rs` — pinch or ⌘/Ctrl+scroll zooms, scroll/two-finger or left-drag pans (drag
  only in the full view; in mini it moves the window), double-click resets to automatic
  framing. `CameraControl.manual` holds the chosen view; `scene::follow_action` uses it
  while set. Gestures can't be scripted here: the user should try them.
- **M4d part 1 done:** `repo.rs` — `RepoCache::for_tool` finds the repo of a tool call
  (file_path/notebook_path/path; Bash `cd X` and absolute/`~/` paths), walking up to
  `.git` (file or dir, worktrees supported) and reading the branch from `HEAD` (detached ⇒
  short hash), cached per directory. `Model::set_repo` stores the session's home repo
  (from cwd) and each agent's latest repo (`AgentInfo.repo`, `main_repo`). Side panel
  title shows "repo · branch"; roster rows add "· in other-repo · branch" when an agent
  works outside the home repo. Verified with real repos (claude-room · main, smart_haus ·
  master).
- Fixed (user report): speech bubbles faded to 60 % after 4 s and were small, so the
  text was unreadable; now font 17, box 32 high, dim only to 90 %, darker `TEXT`.
- Fixed (user report): mini mode showed a blurred blob. `window-vibrancy` inserts its
  effect view *inside* Bevy's NSView, above the CAMetalLayer, so it blurred the room
  itself. Replaced by our own `NSVisualEffectView` (Popover material, behind-window
  blending) added to the view's **superview**, below Bevy's view (`mini::set_glass`,
  objc2 0.6 / objc2-app-kit 0.3). Toggle test passes; the look must be checked on screen
  (no rounded corners yet).
- Final answer pop-up (user request): rendered as markdown with `egui_commonmark` 0.25
  (egui 0.36): headings, lists, inline code, highlighted code blocks with a copy button,
  quotes. Covers 86 % of the window (`FINAL_ANSWER_SHARE`), body text 18 (13 in mini),
  scrolls. egui has no bold face, so `**bold**` shows as normal weight.
- **M4d part 2 (compact) done:** a zone "Other repos" (bottom-left, `Place::Away`)
  with two desks; sub-agents whose latest tool call is in another repository than the
  session's go there instead of the home zones (except Approval). Its sign
  (`AwaySign`, bottom edge) lists the repos in use, e.g. "Other repos: smart_haus ·
  master". Separate rooms per repository (several rooms, own zones) remain an option.
- Agents reading hold an open book (`Part::Book`, accent-colored pages, slow tilt).
- Fixed: session drop-down grew with long paths (ComboBox fits its text) → `.truncate()`
  and `ui.set_max_width` in the side panel; the stray line is gone.
- Fixed: in the full view the title bar showed the desktop (the window is created
  transparent). `mini::set_glass` now always installs a backdrop behind Bevy's view:
  `WindowBackground`/within-window in full view, `Popover`/behind-window in mini.
- **Next:** M8 (chat via plugin channel), then M10 (context/limits) last.
- (older note) **Next:** M4d part 2 — draw a room per repo in use: a second floor area beside the
  office (sign "repo · branch", a few desks, library/servers corners), agents whose
  `repo` differs from the home repo walk there instead of the home zones; rooms appear
  on first use, fade when unused; the wide camera fits all rooms. Then M8 (chat), and
  **M10 last** (the user doesn't need context/limits yet). The user should reinstall (`scripts/install-dev.sh`) or
  `cargo run -p room-app` to get M9/M4e.

Known gaps: macOS attention bounce not done; roster click-to-highlight not done
(part of M4f); `agent-room` release binary is 96 MB (Bevy default features; could trim).
