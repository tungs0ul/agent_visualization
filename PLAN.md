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

Done so far (2026-11-19): egui's built-in Ubuntu Light / Hack stay the main fonts; Noto
Sans Light and Noto Sans Mono are bundled (`assets/fonts/`, `src/fonts.rs`) as
fallbacks for the letters they lack (Vietnamese ộ, ữ, ẫ, …). Licences in
`assets/LICENSES.md`.

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

**Part 2 decided 2026-11-19 (user): a badge, no rooms.** Separate rooms would undo the
2026-11-18 "no wings, one shared HQ" decision (V2.md section 8). Instead an agent whose
latest tool call works in another checkout than its session's (`Repo::away_from`)
shows "↪ repo · branch" in its 3D label, and Claude's row in the sessions card says
the same. Linked worktrees keep their own "⎇" badge.

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
- Fixed: chat window collapsed (no height on its first frame) → `min_size` + floor on the
  transcript height; the final-answer pop-up is skipped while the chat window is open
  (it already shows the answer). Verified with a Haiku session.
- Fixed: the backdrop view covered the title-bar buttons → inserted at the bottom of the
  superview's stack (`relativeTo: None`). Check on screen.
- (resolved) Bug: in that run neither the chat window nor the final-answer pop-up
  was visible in the screenshot (both should be). Suspect `chat_window`: the
  `ScrollArea::max_height(ui.available_height() - 70.0)` inside a new `egui::Window` may
  be negative/zero on the first frame, or the window is placed off-screen; check with a
  fixed `min_size`/`fixed_size` and `.anchor(...)`, and check whether drawing it before
  the final-answer pop-up hides that pop-up. Reproduce with the dev env var above plus
  `AGENT_ROOM_CLAUDE_ARGS="--model haiku"`, `AGENT_ROOM_PLUGIN_DIR=$PWD/plugin`,
  `AGENT_ROOM_HOOK=$PWD/target/debug/room-hook`, screenshot mode.
- Fixed (user report: "the answer shows a very short time, sometimes not at all"):
  other plugins' Stop hooks (e.g. ECC's session summary) run `claude -p`; with
  agent-room enabled each one is a new session whose SessionStart/UserPromptSubmit
  took the room over and cleared the final answer, or kept the room on the helper so
  the next answer was skipped. `room-hook` now walks up to the `claude` process on
  SessionStart/UserPromptSubmit and adds `"agent_room_headless": true` when it runs
  with `-p`/`--print` (`room_proto::HEADLESS_FIELD`; sessions the app starts set
  `AGENT_ROOM_CHAT=1` and count as interactive). `SessionState.headless` sessions never
  take the room from an interactive one; the final answer remembers its session and
  only a prompt in that session closes it. Verified with a real `claude -p` (flag set),
  this interactive session (not set), and a replay (answer stays, helper summary
  hidden).
- Fixed (user report: error on pressing M): while the window resizes, wgpu rejects a
  frame or two (depth vs colour texture size); idle, the app then stopped drawing with
  the bad frame on screen. `mini::settle_redraw` keeps redrawing 600 ms after a switch.
  The ERROR log lines on the switch itself remain (harmless). Needs a check on screen.
- Fixed (user report: bubble text too small when zoomed out): `follow_action` stores
  room units per screen pixel in `scene::Zoom`; `anim::keep_text_readable` scales
  bubbles and walk chips by it (1×–4×), growing bubbles upward from the tail tip so
  they never cover the head. Zooming in still enlarges them. Verified by screenshots
  (whole office at 1100×650: bubbles at normal on-screen size).
- Added (user request): "Last answer" button (side panel header, ticker, mini view)
  reopens Claude's last final answer after Close or after the next prompt closed it.
  `FinalAnswer.open` replaces clearing the answer. Verified by screenshot.
- Added (user request): click Claude in the room (or "Chat") opens the chat for the
  shown session. Without a session of its own, the window offers: "Continue this
  session" when the shown session is closed (`claude -p --resume <id>`, same id,
  history kept: verified with a real Haiku session), "Resume" for earlier sessions of
  the folder (read from `~/.claude/projects/<slug>/*.jsonl`, first prompt as title;
  other tools' `sdk-cli` helpers hidden unless listed in
  `~/.agent-room/chat-sessions.txt`, where the app records its own chat sessions), and
  "New session". A shown session that is live in a terminal only gets a note: sending
  there needs the M8b channel (user's choice: research the channel, no fork).
  `camera::RoomClick` carries single clicks in room coordinates.
  Dev: `AGENT_ROOM_DEV_CHAT="<folder>|"` (empty message) only opens the window.
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

### M8b research (2026-11-17, Claude Code 2.1.326)

Source: code.claude.com/docs/en/channels-reference.md.
- A channel is an MCP server (stdio JSON-RPC) declaring
  `capabilities.experimental["claude/channel"] = {}`. It pushes
  `notifications/claude/channel` `{content, meta}`; Claude sees
  `<channel source="agent-room" …>text</channel>`, the terminal shows
  `← agent-room: text`, and Claude acts on it. No acknowledgement; messages queue
  while Claude is busy and arrive together on the next turn.
- Two-way: a `reply` MCP tool. Optional permission relay
  (`claude/channel/permission`): Claude Code sends
  `notifications/claude/channel/permission_request {request_id, tool_name,
  description, input_preview}`, the server answers
  `notifications/claude/channel/permission {request_id, behavior}`.
- **Big limitation:** custom channels are not on the allowlist (research preview).
  Each session must be started with
  `claude --dangerously-load-development-channels plugin:agent-room@agent-room`
  (interactive only, a warning dialog every start). With `-p` the flag is ignored.
- Session identity: children of Claude Code get `CLAUDE_PID` and
  `CLAUDE_CODE_SESSION_ID` (seen in Bash; check that the MCP server gets them too,
  and what happens after `/resume` or `/clear`). Hooks run under the same `claude`
  process, so the app can match a channel to a session by the claude pid.
- Design: `room-hook --channel` (no Node needed) speaks MCP over stdio, connects to
  `~/.agent-room/room.sock` with a `ChannelHello {claude_pid, session_id}` and keeps
  the connection open; the app sends `{text}` lines, the server emits the channel
  notification. Declared in `plugin/.mcp.json` (or plugin.json `mcpServers`) plus the
  `channels` manifest entry. The chat window's Send then targets a live terminal
  session when its channel is connected; otherwise it shows how to start Claude with
  the flag. Never send without the user pressing Send.
- Decision needed from the user before building: accept starting sessions with the
  development flag (and its dialog) until the channel is allowlisted. **Accepted
  2026-11-17 ("it is okay").**

**M8b built (2026-11-17):**
- `plugin/.mcp.json` starts `room-hook --channel` (`crates/room-hook/src/channel.rs`):
  answers `initialize` (capability `experimental["claude/channel"]`, instructions) and
  `ping`. Only when its `claude` parent was started with
  `--dangerously-load-development-channels`/`--channels` `plugin:agent-room@…` does it
  connect to the app (`ChannelHello {claude_pid, session_id}`, reconnecting every 3 s
  while the app is closed); otherwise it stays away, so the app never offers to send
  to a session that would drop the message.
- MCP servers get `CLAUDE_CODE_SESSION_ID` (verified); `CLAUDE_PID` is inherited and
  not reliable, so the pid comes from walking up the process tree, as in the hook.
  `room-hook` adds `agent_room_claude_pid` to SessionStart/UserPromptSubmit, and the
  app matches a session to its channel by pid first (survives `/clear`), session id
  second (`chat::Chats::channel_index`).
- App: `server::Channel`; the chat window for a live terminal session shows an input
  when a channel is connected, else the command to start Claude with (Copy button).
- Verified end to end with a real interactive `claude --plugin-dir plugin
  --dangerously-load-development-channels plugin:agent-room@inline` (Haiku) driven in a
  pty against a fake app: the hello arrived with the same pid as the hooks, the sent
  text reached Claude and it answered. Not yet through the real app's chat window.
- The installed plugin is a cached copy (`~/.claude/plugins/cache/agent-room/…/0.1.0`),
  so the plugin version went to 0.2.0 and `install-dev.sh` updates it.
- Not built: permission relay over the channel (hooks already do it).

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

### M12: Hide/show the side panel (requested 2026-11-17)

A button (and key, e.g. L) to hide the side panel with the old messages and show it
again, so the room gets the whole window. Remember the choice in `window.json`. The
ticker/mini views are unaffected.

### M13: Permission rules (requested 2026-11-17, done 2026-11-17)

User: "read, edit is allow, doing anything in tmp is allow, git push need to ask".
- Answered: the rules are what **Auto-accept** does ("the permissions i mentioned is
  when the auto accept permissions"); with Auto-accept off every request still asks.
- Built (`log::always_asks`): Auto-accept already allowed Read/Edit and ordinary
  commands; the risky commands in `ALWAYS_ASK` (`rm -rf`, `git reset --hard`, …) are
  now allowed too when they only touch a temporary folder (`/tmp`, `/private/tmp`,
  `$TMPDIR` and its `/private` path): one simple command (no `;`, `&`, `|`, redirects,
  `$`, `~`, `..`, …) whose every argument is a path strictly inside one, or relative
  while the session's cwd is inside one. `git push` and `sudo` always ask.
- If the user meant something stricter (Auto-accept allows *only* read/edit/tmp and
  asks for every other command), that is a small change in `always_asks`.
- Read/Glob/Grep normally don't ask at all; Edit/Write/NotebookEdit do.
- "In tmp": only when every path in the tool call is under `/tmp`, `/private/tmp`,
  `$TMPDIR`. Bash must be parsed carefully (`cd /tmp && rm -rf ~` must still ask);
  allow only simple commands whose path arguments are all inside tmp.
- `git push` already asks (`log::ALWAYS_ASK`).

### M14: Themes, light and dark mode (requested 2026-11-17, done 2026-11-17)

Built (`theme.rs`): Auto / Light / Dark / Spaceship, T or the header button,
kept in `~/.agent-room/theme.txt`. The room is still spawned in Light colours;
`theme::recolor` maps every Light colour in `PALETTE` to the current theme (new
shapes and texts as they appear, everything on a switch), alpha kept. White is
deliberately not in the table (bubbles, paper). Spaceship adds stars
(`theme::stars`). egui panels use `theme::visuals`. Not themed yet: the 3D view.

Original notes:

User: "light mode, dark mode, theme switching, and add another theme which is
futuristic in a space ship".
- A `Theme` resource with the colours now spread over `scene.rs`/`ui.rs` constants
  (backdrop, floor, zones, furniture, bubbles, egui visuals). Light and Dark follow the
  macOS appearance by default, with a manual switch; remembered in `ui.json`.
- "Spaceship" theme: dark hull, metal floor plates, glowing consoles instead of
  desks, a window with stars; same layout and zones so nothing else changes.
- 2D first (the current lyon shapes); a theme only changes colours and props.

### M15: 3D room (requested 2026-11-17, first version 2026-11-17)

Built (`view3d.rs`), 3 key or the 2D/3D button top left of the room:
- The 2D room stays the source of truth. Each 2D character and desk gets a 3D
  stand-in that copies its position (100 room units = 1 m); poses map to animations
  (Idle, Work, Walk, Wave). The 2D camera is switched off while 3D is on.
- Base after the Kenney Space Station Kit sample (user's choice): tiles of 1.5 m,
  raised floor (top at y = 0), back and side walls with windows/banners, pipes,
  scaffolding, props per zone, rails, stairs at the door, rocks outside.
- Claude: KayKit character (`assets/models/kaykit`), tinted orange. Sub-agents: a
  random KayKit Adventurer (user's request). Adventurer files have no animations;
  the shared `Rig_Medium_*.glb` clips play once `prepare_avatar` adds an
  `AnimationPlayer` on `Rig_Medium` and tags each bone with
  `AnimationTargetId::from_names(path)` + `AnimatedBy`.
- Kenney Space Kit models are exported with their root at (2, 0, 1.5);
  `scene_child` moves them back.
- Assets load from the checkout (`AssetPlugin.file_path` = `CARGO_MANIFEST_DIR/../../assets`);
  a binary installed elsewhere without the checkout had no 3D models. Done in M21:
  they are compiled into the binary.
- Cost: the first switch to 3D compiles shaders (several seconds); in 3D the app
  redraws at 30 fps all the time (animations). 2D stays cheap.
- `AGENT_ROOM_VIEW3D=1` starts in 3D (screenshot tests).
- Next: speech bubbles in 3D (now egui labels), theme-aware lighting, walking
  around props, characters sitting on chairs.

Original notes:

User: "does bevy support 3d? and can we do 3d? and support switching between 2d and 3d".
- Bevy has a full 3D renderer (PBR materials, lights, shadows, glTF scenes and
  skeletal animation), so yes. The room logic (model, routing, zones, poses) is
  independent of drawing; a 3D view would be a second set of draw systems over the
  same entities, with a perspective camera, switched by a key.
- Costs to weigh: GPU/power use (PLAN.md 6: the app must stay cheap while idle, and
  3D with shadows redraws more), and animated characters need rigged models.
- Free models (CC0, glTF): Kenney Space Station Kit / Space Kit and furniture kits
  (kenney.nl); Quaternius Modular Sci-Fi Megakit, Sci-Fi Essentials Kit, Universal
  Base Characters + Universal Animation Library (quaternius.com); KayKit Space Base
  Bits and Furniture Bits (kaylousberg.itch.io). Check each pack's page before use.

### M17: Agentverse, a 3D autonomous workspace (requested 2026-11-17, in progress)

The user's direction (with a reference image: a neon-lit space HQ, cute white robots
with dark visors and glowing blue eyes): the 3D world becomes the main view, one look
only (no theme switching for now; "just make this one good"). Movement shows the real
execution trace, not busy-looking animation. Stays in Rust/Bevy (section 2); the
React Three Fiber suggestion in the user's notes does not apply.

Decisions (user, 2026-11-17): 2D room stays, switchable with the 3D button; mini mode
always uses 2D (cheap). Robots are modelled, rigged and animated by a Blender Python
script kept in the repo (`tools/blender/robot.py` → `assets/models/robot/robot.glb`).

One shared workspace, five locations (agents move freely between them):

| Location | What happens | Tools / events |
|---|---|---|
| Main desk (centre, round platform with rings) | main agent thinks, plans, coordinates | prompt, thinking, `Agent` delegation |
| Bookshelf | retrieve and read files | Read, Grep, Glob, LS, WebFetch, WebSearch |
| Processing lab (glass cylinders) | run scripts and programs | Bash, MCP tools |
| Writing desk (holo screens) | create or edit files | Edit, Write, NotebookEdit |
| Handover station (round pad) | sub-agent returns results to the main agent | sub-agent report |

Events → animation: tool start ⇒ walk to its location and work there; sub-agent
start ⇒ a new robot appears and the main agent hands it a glowing data capsule;
sub-agent done ⇒ it walks to the handover station, a capsule travels to the main
agent; turn end ⇒ back to the main desk. Permission waits show at the robot.

UI (egui, dark glass style): top bar (title, agent count, active count, elapsed),
right "Activity flow" (the log, restyled), bottom execution timeline (Reading →
Research → Processing → Writing with done/running/pending), click a robot for its task,
tools and history. Glowing floor paths between locations; bloom on emissive parts.

Phases: (1) robot model + animations in Blender; (2) the workspace: layout, neon
materials, bloom, space backdrop; (3) event-driven movement in 3D (own routing from
the model's tool events, not copying 2D positions); (4) capsule hand-over and glowing
paths; (5) UI panels; (6) power: 30 fps focused / 10 unfocused, 0 when idle; check
the bloom cost.

### M16: Custom themes from the user's own models (requested 2026-11-17, not started)

User: "allow user to do custom theme, like if they want they can create anime theme,
any game, film theme when they provide the 3d model for the agent and background".
- A theme folder (`~/.agent-room/themes/<name>/theme.toml` + files): colours, a
  background image or glTF room, a model or sprite for Claude and for sub-agents,
  optional per-pose animations. Loaded at start and when the folder changes; a bad
  theme falls back to the default with a note in the log.
- The app ships only its own and CC0 themes; what users load is up to them.
- Depends on M14 (theme switching) and, for 3D models, M15.

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

### Backlog from the 2026-11-18 feature research (not started)

Ranked by a research pass (code.claude.com docs for hooks, statusline, agent-view,
Agent SDK and file checkpointing; `claude --help` 2.1.327; Tarsa; Agent Flow; the
Claude Code desktop redesign). Nothing here is built yet; the user picks.

1. **Session list from `claude agents --json`** (S). Lists every live session with
   `pid`, `status` (busy/waiting/idle), `name` (e.g. "tung-f4"), even those started
   before the app; would replace pid guessing in `end_dead_sessions` and end ghost
   sessions. Checked live; one call took 0.21 s and 145 MB, so poll at start, on focus
   and about every 60 s. The research also reports `waitingFor` values no hook gives
   (`sandbox request`, `dialog open`); not seen in the one live check.
2. **Undo a chat turn's file edits** (M). Chats start with
   `CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING=true` and `--replay-user-messages`; an
   "Undo this turn" button runs `claude -p --resume <id> --rewind-files <uuid>`. App
   chats only; Bash and sub-agent edits are not undone. In-stream control request name
   unverified.
3. **Stuck and loop alert** (S): same tool and input three times, three failures in a
   row, or busy with no event for N minutes; robot shows "?", notification fires.
4. **Pause or stop one sub-agent** (S): hold file keyed on `agent_id` (M25 "Not done").
   Check live first whether a sub-agent's `continue: false` ends only that sub-agent.
5. **"Ready for review" shelf** (S-M): a turn ending with a non-empty diff badges the
   writing desk until the diff is opened.
6. **Next-prompt chips** (S): chats with `--prompt-suggestions`.
7. **Sub-agent reasoning in bubbles** (S, chats): `--forward-subagent-text`.
8. **What fills the context** (S-M, chats): SDK `getContextUsage` on the wall screen.
9. **Run in background from the teleporter** (M): `claude --bg --name`, back via item 1.
10. **Branch a chat** (S): `--resume <id> --fork-session` into a new tab.
11. **Hot files on the bookshelf** (S): Read/Edit counts per file, click to open.

Left out: dollar cost (V2 phase 6 dropped it as a guess), Gantt/replay (V2 phase 5
trimmed), remote control (non-goal).

### Backlog from the 2026-11-19 feature research (not started)

A web research pass (code.claude.com docs for hooks, agent teams, cross-session
messaging and monitoring; HN threads; Conductor, Crystal, AgentsRoom, claude-code-kanban).
None of these is in the backlog above. The user picks.

1. **Same-file clash alert** (S): two sessions edit the same file within N minutes;
   warn and draw a red line between the robots. `PreToolUse` Edit/Write `file_path`.
2. **Limit forecast** (S): "5h window runs out at about 15:40 at this pace", from the
   rise in `rate_limit_event` / status-line `rate_limits` (M10 data).
3. **Claimed vs real** (S): the answer says "tests pass" but the last test command
   failed; flag the turn. `Stop.last_assistant_message` plus Bash outcomes. The
   matching rule is a guess.
4. **Messages between sessions** (S): a capsule flies between robots on `SendMessage`
   (on by default since 2.1.224). `tool_input` field names unverified.
5. **Retry after an auto-mode denial** (S): card with the classifier's reason and Retry;
   `PermissionDenied` returns `hookSpecificOutput.retry: true`.
6. **Agent teams in the HQ** (M): teammates as robots, inbox messages as capsules, idle
   teammates in a lounge. `~/.claude/teams/<team>/config.json`, `inboxes/`,
   `TeammateIdle`. Experimental flag only.
7. **Local telemetry receiver, opt-in** (M): listen on `localhost:4318` (OTLP http/json)
   for measured cost, lines changed, commits, PRs, API errors.
   `CLAUDE_CODE_ENABLE_TELEMETRY=1`, set by the user.
8. **Rules shelf** (S): which CLAUDE.md and `.claude/rules` files loaded and why.
   `InstructionsLoaded` (`load_reason`, `globs`, `trigger_file_path`).
9. **"Tell me when done" bell** (S): done 2026-11-19. A 🔔 on Claude's details window
   and each row of the sessions card; when Claude next stops, fails or the session
   ends, a macOS notification and the Purr sound (even with sound off), once.
10. **Read the answer aloud** (S): `say` the final answer, per session toggle.
11. **Commit or open a PR from the writing desk** (M): git and `gh`. Moves the app from
    watching to doing, so the user decides.

### M30: Visual overhaul to the target concept art (requested 2026-11-19, phase 0 done)

The user's "Agent Room: Visual Overhaul Brief" with a target image; summary, target,
metrics tool and baseline in `docs/agent-room-visual-overhaul/`. Nine phases, a report
and the user's review after each. Phase 0 (audit) is done; see the handoff log.
Decisions needed from the user before phase 1 (asked 2026-11-19): HUD stays egui
(restyled) or moves to bevy_ui; the brief's always-on ambient motion and TAA against
the always-on power budget (V2.md); the missing annotated image, crops and palette.

### M28: Mission control from the "Starship Mission Control 2.0" brief (requested 2026-11-19, done 2026-11-19)

The user answered the triage below with "just do everything": all of "Take", and
both "Ask" items (replay, bottom console). "Leave out" stays out. Built: `work.rs`
(per-agent record), desk halos (`view3d::desk_lights`), nameplates, robot details,
sub-agent tokens (`agent_transcript_path`), `palette.rs`, the bottom activity console
(`ui::activity_console`, tabs Live feed, Tool calls, Files, Errors, Metrics; the
Errors tab is the errors filter), `report.rs`, mission replay (`replay.rs`, Replay
console on H). Not built: the brief's second image (a chart wall on the window).

The user pasted a long brief (web-style mission control: desks per task, bottom
activity console with tabs, inspection panel, toolbar, replay, approval station,
event schema, responsive layouts) and asked what to take from it. Triage against what
exists and what Claude Code's hooks and transcripts can show; the user picks.

Take (real data, fits the bridge):
1. **Desk states** (S): each crew desk's ring and screens show its robot's state:
   dim when free, cyan working, amber waiting, green done (until it leaves), red
   failed. Today all eight desks glow the same whether used or not.
2. **Desk nameplate** (S): over an occupied desk, the agent type, its task (the
   `Agent` description), current tool and tool count. No % progress: Claude Code
   gives none (V2.md section 3).
3. **Report vs verified** (S-M, overlaps backlog item 3 "Claimed vs real"): label an
   agent's final text as its report, and show beside it what the hooks saw (tools run,
   failures, files written), flagging a report that claims success after a failure.
4. **Artifacts console** (S-M): files each agent created or edited (`Write`/`Edit`/
   `NotebookEdit` paths), newest first, click to open; per agent in the robot details.
5. **Errors filter** (S): failed tools and turns only, in the History console.
6. **Robot details, deeper** (M): each tool call with input and result (collapsible),
   files touched, tokens of a sub-agent from its own transcript.
7. **Mission report export** (S-M): a Markdown summary of a turn or session: prompt,
   agents and tasks, tools, files, failures, tokens, duration.
8. **Panel palette** (S): the brief's colours for the egui panels (text #F2F7FF on
   #101D36, secondary #91A4C4, cyan #42D9FF, status green/amber/red), one place in
   `ui::visuals`.

Ask the user (earlier decisions say no):
- **Mission replay**: the brief's top feature. V2.md phase 5 (replay / time machine)
  was trimmed by the user on 2026-11-18. Feasible: record each session's hook events
  to a file and play them through a separate `Model`; the Director is pure, so the
  room rebuilds from events, not canned animation.
- **Bottom activity console**: M19 replaced the side panel with consoles on purpose;
  a docked console brings a panel back. A one-line ticker of the latest event is the
  small version.

Leave out: tablet and mobile layouts (desktop app), WebSocket/reconnect (local socket
already), estimated cost (a guess; measured cost only via the telemetry backlog
item), task dependencies and % progress (no data), agent memory (no data), showing
reasoning (never), desk types per category (M27 made desks identical; the hologram
shows the tool).

### M27: Starship bridge instead of the station HQ (requested 2026-11-19, in progress)

User: "if the agent is working at their desk should we redesign our background? like a
cockpit of a starship, where the command in the front where we can find the settings,
and below it multiple desks so when the subagent need it it can go there". Follows M26:
robots now stay at their desk, so five stations to walk between no longer fit.

- Command deck: Claude's chair and console; Settings, Chat and Launch open there; the
  plan board above it; a viewscreen with space and the wall-screen stats.
- Crew deck: identical crew desks. A sub-agent beams in and works at a free desk; the
  desk's screen shows the kind of work (book, terminal, pen).
- History, Commands and Changes become clickable wall panels; robots don't walk there.
- Teleporter pad stays. Walks stay as in M26: task, report, needing the user.
- The ship flies while agents work (user, same day: "when the agents are working we
  can see that the starship is moving forward (background going behind). and when
  idle it just stop there"): the stars stream past the windows while any session is
  busy and stand still when all are idle. It costs nothing extra: the view already
  redraws while agents work, and still stars let it doze.
- A generic sci-fi bridge, not a copy of any film's.

Steps, each checked by screenshots of a replayed session:
1. Grey-box prototypes of two layouts behind `AGENT_ROOM_LAYOUT` (`front`: command at
   the back by the viewscreen, crew pit in two rows in front of it; `ring`: command
   chair in the centre, crew desks on an arc around it), with the moving stars. The
   user picks one.
2. Blender models (bridge shell, command deck, crew desk, viewscreen), wall panels for
   the consoles, desk assignment for sub-agents.
3. Remove the old stations and their models; measure power with `scripts/measure.sh`
   against the V2.md budget.

### M26: Robots work at their desk, walk only when it matters (requested 2026-11-19, done 2026-11-19)

User asked whether robots should stay at their desk or move between stations. Research:
motion in the periphery draws the eye and distracts (Bartram, Ware, Calvert,
"Moticons", IJHCS 2003); Pixel Agents keeps characters at desks with a pose per tool;
Office Claws and OpenClawfice seat working agents and move them for status. Walking on
every tool call also trailed Claude (2.5 s dwell, 1.8 m/s) and cost frames. The user
picked: (1) work at a home desk, the activity hologram shows the tool kind; (2) walk
only for what deserves a look. This replaces the M17 rule "tool start: walk to its
location".

- Claude always works at its seat at the main desk (`director::location_for`).
- A sub-agent's home is the station of its first tool call (`Avatar::home`); it uses
  every later tool there.
- Walks: teleporter arrive and leave; a sub-agent to the main desk for its task, its
  report, and when it needs the user (it waves there).
- Not built (option 3 of the proposal): walk to a station when one kind of work lasts
  over about 10 s.

### M25: Pause, resume and stop a session (requested 2026-11-18, done 2026-11-18)

User: "there is no way to stop or pause an agent, maybe we want to have feature that we
can stop, pause or resume?" Only chats started from the app could stop (stream-json
`interrupt`); terminal sessions had no control, and nothing could pause.

- One mechanism for every session, terminal or chat: the app writes
  `~/.agent-room/hold/<session_id>` with `pause` or `stop` (`room_proto::hold`), and
  `room-hook` reads it on each `PreToolUse` before forwarding the event. No file means
  run; it costs one failed open per tool call.
- Pause: the hook waits (polls every 250 ms) until the file goes, so the tool does not
  run. Sub-agents share the session id, so they wait too. Claude's text between tools
  still streams: the pause takes effect at the next tool call. `PreToolUse` gets
  `"timeout": 86400` in `hooks.json` (Claude Code's default of 600 s would let the tool
  run after 10 minutes). Plugin 0.4.0.
- Stop: the hook prints `continue: false` **and** `permissionDecision: "deny"`. Checked
  live with Haiku: `continue: false` alone ends the turn only after the tool ran, and no
  `Stop` hook follows either way, so `room-hook` sends the app a `Stop` event itself
  ("Stopped from Agent Room"). `UserPromptSubmit` clears a stop, so a stop set while
  idle never ends a later turn.
- Fail open: a hold only counts while the app's socket answers (checked at once, then
  every 2 s while paused), and the app clears the folder at start. A quit or crashed
  app never leaves Claude waiting.
- UI: Claude's card at the top shows "paused" and Pause / Resume / Stop; the session
  list shows ⏸ for paused (idle is now ·). The chat console keeps its own Stop
  (immediate `interrupt`).
- Not done: pausing a single sub-agent (the hook has `agent_id`; the file would key on it).

### M21-M24: from the research on similar apps and new hooks (requested 2026-11-18, done 2026-11-18)

User: "Do 1 2 3 4" (the top four from the research: Claude Code 2.1.327 hooks reference,
claude-view, Claude Code Agent Monitor, Pixel Agents, pixtuoid).

**M21 Models compiled into the app.** `cargo install` builds in a folder it deletes, so
an installed app had no 3D models (M15 "to do"). `crates/room-app/build.rs` lists
`assets/` (glb/gltf/bin/png/ktx2) into `$OUT_DIR/embedded_assets.rs`; `assets.rs`
serves them from memory as Bevy's default asset source (`MemoryAssetReader`), so load
paths are unchanged. About 4 MB in the binary. Checked: a copy of the binary run from
the scratchpad, no checkout nearby, draws the whole HQ.

**M22 Plan board.** Checked live (2.1.327, interactive, plan mode): `ExitPlanMode`
fires `PreToolUse` and then `PermissionRequest` with `tool_input.plan` (markdown) and
`planFilePath`, `permission_suggestions` empty. The PermissionRequest becomes
`log::PendingPlan` (never auto-allowed), shown as a centre board: the plan as
markdown, a feedback line, "Approve, accept edits" / "Approve, ask for each edit" /
"Keep planning" / "Answer in terminal". Approve answers `allow` with the input echoed
as `updatedInput` (the hooks reference requires it for ExitPlanMode) and
`updatedPermissions: [{type: setMode, mode, destination: session}]`
(`PermissionReply.mode`); Keep planning denies with the feedback as the message. The
hook waits 5 minutes for plans (`room_proto::reply_timeout`). Verified end to end
with a fake app socket: after approval the session's `permission_mode` was
`acceptEdits` and Claude's Write ran without a prompt. Not checked: the board on
screen (the screen was locked), plan mode in the app's own `-p` chat (Claude Code
offers ExitPlanMode in `-p` only with a permission host; unknown whether the
PermissionRequest hook counts).

**M23 Newer hook events.** StopFailure keeps its `error` type: log, bubble and toast
say why in words (`log::failure_text`). The plugin (0.3.0) forwards `PostCompact` and
`PostModelSwitch`: the context tank empties when compaction is done (the card says
"Compacting the context..." meanwhile; an old plugin without PostCompact empties it
at the next prompt or turn end), the Claude card shows the model and a toast follows
`/model` (payload checked live: `from_model`, `to_model`, `requested_model`,
`source`). Toasts for Notification types `elicitation_dialog`,
`elicitation_url_dialog`, `agent_needs_input`, `agent_completed`,
`quota_auto_resume_*`, `auth_storage_failure`; `permission_prompt` and `idle_prompt`
stay quiet (a card exists; idle follows every turn). Reinstall the plugin
(`scripts/install-dev.sh`) for the new events.

**M24 Chat composer.** `compose.rs`: typing `@` suggests files from the session's
folder (`git ls-files --cached --others --exclude-standard`, once per folder, in a
thread; names starting with the query first); Tab, Enter or a click completes it.
Files dropped on the window open the chat: images (PNG, JPEG, GIF, WebP, at most
3.7 MB) are sent as base64 image blocks before the text, other files are added as
`@path`. Image blocks checked live with `claude -p` stream-json (Claude named the
colour). Not checked on screen: the suggestion list, chips, the drop hint.

Left from the research for later: Cmd+K search over past transcripts (bookshelf),
open a changed file in the editor, recently closed sessions, source badges, idle
life (lounge, pet), `PostToolBatch` bursts, `TaskCreated`/`TaskCompleted` board,
`MessageDisplay` live text (measure its process-per-batch cost first), Elicitation
answers from the app.

### M20: More consoles, the chat as a terminal stand-in, notifications, worktrees (requested 2026-11-18, done 2026-11-18)

User: "do all" (the seven suggestions after M19).
- Writing desk (D): `diff.rs` runs `git diff [HEAD] --numstat`, the patch and
  `ls-files --others` in a thread when the console opens or the shown folder
  changes; files with +/- counts, click one for its coloured patch (400 lines max).
- Processing lab (R): `activity.rs` keeps 30 Bash commands per session with outcome,
  duration and the end of their output. room-proto's `ToolFinished` now carries
  `output` (PostToolUse `tool_response.stdout` + `stderr`, last 4000 bytes).
- Teleporter (O): pick a folder and start an app session; it opens as a chat tab.
- Wall screen (T): today's prompts, turns, tool calls by tool, tokens, limits and a
  24-hour heatmap; in memory, reset at local midnight (`date +%z` read once).
- Chat (M18 done): `--include-partial-messages` streaming, tool steps (✔/✖ from
  tool_result), Stop = `control_request` `interrupt`, model and permission-mode
  pickers (`set_model`, `set_permission_mode`), history from the transcript on resume,
  tabs (`Chats.sessions` + `active`). Control requests checked live on 2.1.327: each
  answers `control_response` `success`.
- Notifications: when a permission or question waits and the window isn't focused,
  `osascript` posts a macOS notification (Settings switch, `ui.json` `notify`).
- Worktrees: `Repo.worktree` from a `.git` file pointing into `.git/worktrees/<name>`;
  labels add "⎇ branch".
- The final-answer pop-up waits while a console is open.
- Measured (release, unfocused): idle 8.6 %, dozing 0.0 %, active 21.7 % (M19: 8.7 /
  0.0 / 22.7). Nothing new runs while its console is closed.
- Verified by screenshots (each console; a live haiku chat with a tool step) and unit
  tests. Not verified by hand: clicks, Stop mid-turn, the pickers in the UI, the
  notification banner, a worktree label on screen.

### M19: Station consoles instead of the side panel (requested 2026-11-18, done 2026-11-18)

User: "Instead of the logs panel on the side to control everything ... click on main
desk we can have a hologram like to change the settings (auto accept, mute,
fullscreen mini ...), click on bookshelf we can read the history logs, click on
research we can chat with claude ... the performance must stay."

- `console.rs`: `Console { Settings, History, Chat }`, each bound to a station
  (main desk, bookshelf, research); `OpenConsole(Option<Console>)` is the only "open"
  state (the chat's own `open` flag is gone).
- Opens by clicking the station (a click that didn't drag, within reach of its
  projected spot; since 2026-11-18 a mesh ray cast on the station's model instead, and
  hits on its platform or floor ring don't count, because the user kept opening
  consoles by mistake there: `Location::platform_top`, `view3d::StationPick`), its sign (which now says "⚙ Settings" etc.), or S / L / C. Esc, x,
  or a click on the floor closes it.
- The console is a cyan hologram standing over its station, kept inside the window.
- Removed: the side panel, its Hide/Show (L, `ui.json` `hidden`), the narrow-window
  ticker, the roster and session picker in the panel (the HUD cards already have
  them). Permission cards still show on top and next to whoever asks.
- `AGENT_ROOM_CONSOLE=settings|history|chat` opens one for screenshot tests.
- Measured (release, unfocused, `scripts/measure.sh`): idle 8.7 %, dozing 0.0 %,
  active 22.7 %, against 11.3-11.6 / 0.1 / 25.5-27.7 before. Within run-to-run
  noise (3-5 points) or slightly better: no regression.

### M10: Context and usage limits (requested 2026-11-17)

Show the session's context use, the 5-hour limit and the weekly limit (side panel
header; compact bar in mini mode). Hook payloads don't carry it. Candidates: the
transcript's usage fields (context tokens); Claude Code's status-line command input
(it receives model, context and rate-limit info; a plugin status line could forward
it to the app socket); CLI output. Pick the supported, cheap one.

**Done 2026-11-18.** Context use comes from the transcript (V2 phase 6, the tank). The
limits come from two documented places, verified on Claude Code 2.1.327:
- `claude -p --output-format stream-json` prints a `rate_limit_event` with
  `rate_limit_info.unifiedWindows.{five_hour,seven_day}.{utilization (0-1), resetsAt}`
  after each answer. The app's own chat sessions read it.
- A status line command gets `rate_limits.{five_hour,seven_day}.{used_percentage,
  resets_at}`. Plugins can't ship a status line, and the app must not edit the user's
  settings, so `room-hook --status` is a one-line opt-in for the user's own status line
  script (README "Usage limits"). It prints nothing.
Both are claude.ai-subscriber only. Shown in the top-right card ("5h 23 % · week 74 %",
amber from 70 %, red from 90 %, reset times on hover); a window drops once it resets.

### M18: Use the app instead of the terminal (requested 2026-11-18, research done)

User: "the chat still does not feel good. research on if i can chat with claude in this
app without terminal at all, so i can use this app instead of terminal with claude?"

Yes. The app's chat already runs `claude -p --input-format stream-json
--output-format stream-json`, which is the same process and protocol the official Agent
SDKs (Python, TypeScript) drive; they add nothing a Rust client can't do. What makes it
feel poor today is what the app ignores, not a limit of the protocol:

| Today | Available (docs, checked live where noted) |
|---|---|
| Whole answer appears at the end | `--include-partial-messages`: `stream_event` text deltas, token by token |
| Tool use invisible in the chat | `assistant` tool_use and `user` tool_result blocks; sub-agent messages carry `parent_tool_use_id` |
| No stop button | `control_request` `interrupt` (init `capabilities` lists `interrupt_receipt_v1`, checked live) |
| No model or mode switch | `set_model`, `set_permission_mode` control requests; `/model sonnet`, `/compact`, skills work as prompts |
| Resume shows no history | read the session's transcript `.jsonl` |
| Permissions only via the plugin hook | still fine (PermissionRequest hook); the SDK's `can_use_tool` control request is the plugin-free route, wire format to verify |
| One chat at a time | one child process per chat; each is a session and a robot already |

Not available in `-p`: terminal-only UI commands (`/login`, interactive pickers,
`/config` screens), vim mode. Login must exist already (the terminal's login is shared).

Alternatives considered:
- Embed a real terminal (PTY + terminal emulator widget) running interactive `claude`:
  full parity at once, but it is still a terminal inside the window, and streaming text
  redraws constantly (power). Keep as a fallback idea only.
- Agent SDK sidecar (Node or Python): same protocol, extra runtime, no gain.

Plan (smallest first, each verifiable by screenshot and a real session):
1. Streaming text and a Stop button.
2. Tool steps inline (one dim line each, "Read main.rs", expandable result).
3. History on resume; several chats as tabs.
4. Model and permission-mode pickers; slash command hints.
5. Optional: `can_use_tool` permissions so the app works without the plugin.

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
- [x] M4d Rooms per repository (part 1 2026-11-17: repo + branch detection; part 2 2026-11-19: a "↪ repo · branch" badge instead of rooms, user's choice; not yet seen on screen)
- [x] M4e Waiting at the Approval desk with the card in the scene (2026-11-17)
- [x] M4f Camera controls: zoom, pan (2026-11-17; pinning dropped by the user)
- [x] M8 Chat with Claude from the app (8a: new/continue/resume sessions; 8b: send to
      a terminal session through the plugin channel, 2026-11-17; needs the user's try)
- [x] M9 Auto-accept mode and centre pop-ups (2026-11-17)
- [x] M10 Context, 5-hour and weekly limits in the app (2026-11-18; terminal sessions need the status line opt-in)
- [x] M19 Station consoles instead of the side panel (2026-11-18)
- [x] M18 Use the app instead of the terminal (2026-11-18, in M20)
- [x] M20 More consoles, chat upgrade, notifications, worktrees (2026-11-18)
- [x] M12 Hide/show the side panel (2026-11-17; needs a look on screen)
- [x] M11 Own CPU/RAM/FPS overlay (2026-11-17; removed 2026-11-18: it stayed on "Measuring…" and the user chose removal)
- [x] M13 Permission rules as part of Auto-accept (2026-11-17)
- [x] M14 Themes: light, dark, spaceship (2026-11-17)
- [x] M15 3D station view, first version (2026-11-17; see M15 "Next")
- [ ] M16 Custom themes from the user's models (paused: one look for now, see M17)
- [ ] M17 Agentverse 3D workspace (robot, Blender stations and environment, movement, hand-over at the main desk, activity holograms, light trails, HUD with session switcher, robot details; 2026-11-17/18). The v2 roadmap from the user's Agentverse brief is in V2.md.
- [x] M5 Permissions (2026-11-17; user still to try Y/N and a deny reason live)
- [ ] M6 Polish
- [x] M21 3D models compiled into the binary (2026-11-18)
- [x] M22 Plan board (2026-11-18; board not yet seen on screen)
- [x] M23 StopFailure reasons, PostCompact, PostModelSwitch, notices (2026-11-18; plugin 0.3.0)
- [x] M24 Chat @ mentions and dropped images (2026-11-18; not yet seen on screen)
- [x] M7 Distribution, code side (2026-11-17); publishing left to the user
- [x] M28 Mission control: desk states, nameplates, details, activity console, report, replay (2026-11-19; console tabs and replay not yet seen on screen)
- [ ] M27 Starship bridge (2026-11-19: front bridge with eight crew desks, free desks for sub-agents, warp stars; power still to measure)
- [x] M26 Robots work at their desk, walk only when it matters (2026-11-19)
- [x] M25 Pause, resume and stop any session (2026-11-18; plugin 0.4.0; buttons not yet seen on screen)

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
  `AGENT_ROOM_SCREENSHOT_TOGGLE_MINI=1` (or `=stay`) toggles mini after the 2nd shot and back after
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

- 2026-11-17, Claude Opus 5.5 (later session): fixed the final answer closing or not
  showing (headless `claude -p` helper sessions from other plugins took the room; see
  "M8a status") and the blank window after pressing M (`mini::settle_redraw`). New
  request recorded as **M12** (hide/show side panel), not built. Then: speech bubbles
  stay readable at any zoom (`anim::keep_text_readable`); "Last answer" button.
  Permission-rules request recorded as **M13** with an open question. 55 tests pass,
  clippy clean. **Next:** user reinstalls (`scripts/install-dev.sh`, both binaries changed) and
  checks both fixes on screen; then M12, M8a leftovers, M11.
- 2026-11-17, Claude Opus 5.5 (same session, later): click Claude to open the chat;
  continue a closed session / resume earlier ones (`--resume`, verified with Haiku);
  M8b channel researched (see "M8b research"): works only with
  `--dangerously-load-development-channels`, so the user must OK that first.
  **M12 done:** "Hide" in the panel header / "Show log" top-right / key L, kept in
  `~/.agent-room/ui.json` (`ui::PanelPrefs`); with the panel hidden, permission cards
  move to a top panel. **M11 done:** `stats.rs`, "Stats" button / key I, bottom-left
  overlay "CPU · RAM · fps" from sysinfo (only measured while shown, refresh 1 s; the
  app then redraws once a second). Header buttons wrap (`horizontal_wrapped`).
  Not checked on screen: the Mac's screen was locked, and Bevy draws black frames
  then (screenshots all black, also for the unchanged build). Commits pushed.
  **Next:** user checks M11/M12/chat on screen; answers M13 (rules only with
  Auto-accept, or an opt-in setting) and M8b (accept the development flag?).
- 2026-11-17, Claude Opus 5.5 (same session, after the user's answers): **M13 done**
  (Auto-accept also allows risky commands confined to a temporary folder; `git push`,
  `sudo` always ask). **M8b done**: `room-hook --channel` MCP channel server +
  `plugin/.mcp.json`; the chat window types into a live terminal session started with
  the development-channels flag; verified with a real interactive session in a pty
  (Haiku), not yet through the app's own window. Fixes from the user's reports:
  dragging/scrolling the room follows the hand at once (no easing while manual) and
  is clamped where it is stored, so dragging past the edge no longer piles up;
  scrolling over the log panel no longer pans the room (pointer must be over the room
  viewport); in mini mode Claude's answer no longer pops up over the whole widget
  ("Last answer" opens it). New requests recorded: **M14** themes (light/dark/
  spaceship), **M15** 3D (researched, free CC0 model sources listed), **M16** custom
  themes. Plugin version 0.2.0. **Next:** user runs `scripts/install-dev.sh`
  (binaries + plugin update), restarts Claude Code with the flag, tries sending from
  the chat window; checks camera drag, panel scrolling and mini mode.
- 2026-11-17, Claude Opus 5.5 (same session, evening): user switched direction to the
  "Agentverse" 3D workspace (M17). Built: Blender robot (`tools/blender/robot.py`, a
  subagent wrote it), `world3d.rs` workspace with five locations and bloom, robots walk
  to the location for their pose, 3D is the default and also in mini mode (camera
  follows Claude), one dark look (theme switching removed). Perf fix: bubble text was
  re-shaped every frame; 10 fps while unfocused. Two earlier subagents (chibi 2D,
  bubble overflow) were stopped unfinished because their separate worktree builds
  heated the Mac; their worktrees are still in `.claude/worktrees` (12.8 GB), waiting
  for the user's OK to delete. **Next:** M17 phases 4-6; bubble overflow fix in 2D.
- 2026-11-18, Claude Opus 5.5 (unattended night run, then the user's Agentverse brief):
  bubble overflow fixed (real text layout, wrapping); HUD over the 3D view (title,
  main agent card, counts, stage timeline, robot details on label click, list of
  live sessions to switch rooms); robots walk round the main platform; unused models
  removed. Measured the release build: 3D idles at about 19 % CPU (it never sleeps),
  212 MB. Wrote **V2.md**: the user's brief turned into phases 0-6 with a power
  budget, an honest data map (no invented progress), the engine decision (stay on
  Bevy, not React Three Fiber, for power) and open questions. **Next:** V2 phase 0
  (3D sleeps when idle) and `scripts/measure.sh`.
- 2026-11-18, Claude Opus 5.5 (V2 phase 0): the user answered V2's questions (idle
  animation at a few fps, every session in one HQ, sound on with mute/off) and asked
  to remove 2D, add a teleporter (brief and reference image recorded in V2.md
  phase 3) and keep Claude where it is when a new message arrives (fixed). Built
  `scripts/measure.sh` and `AGENT_ROOM_FPS_LOG`. 3D now plays the idle clip at 4 fps
  for 30 s, then dozes: idle 21.7 % to 0.3 % after 30 s; three task threads took
  active from 35 % to 22 %. Active unfocused is still over the 10 % budget; the next
  lever is deleting 2D (phase 1). A subagent is making the teleporter model
  (`tools/blender/teleporter.py`). **Next:** phase 1 (Director, delete 2D).
- 2026-11-18, Claude Opus 5.5 (V2 phase 1): 2D deleted on the user's request ("we can
  remove 2d completely"). `director.rs` turns the model into robot roles and places
  (tested with a recorded turn); `view3d` plays them, keyed by agent and session; the
  3D camera owns the viewport the panels leave free; panels use the director's
  colours and one dark style. Removed scene/anim/camera/theme, lyon and the Nunito
  font. Measured: active 22.7 % to 14.8 %, idle 9 % to 5.3 %, memory about 270 MB.
  Screenshots of the full view checked; mini mode not re-checked (screen was locked).
  **Next:** check mini mode; phase 2 (stations) or the active-power levers in V2.md.
- 2026-11-18, Claude Opus 5.5 (V2 phases 2, 3 teleporter, 3b): every live session's
  Claude and sub-agents in one HQ (`director::cast_all`, seats by first appearance);
  teleporter placed with arrive/leave and a departure queue; Research station
  (subagent-built `research.glb`) replaces the file-reading desk, bookshelf reads
  files; HUD stage strip removed. Checked with a two-session replay (contact sheet).
  Measured A/B: no power regression; measurement noise about 4 points. **Next:**
  active power (HUD/labels only on change, audio thread), beam particles, the stray
  ring by the Research station, mini mode re-check.
- 2026-11-18, Claude Opus 5.5: activity holograms now float in front of each working
  robot, brighter (emissive x2.2), with what it does typed out on them (40 chars/s);
  head labels then only name the robot. Teleporter: beam forms and thins (width
  follows the beaming), 36 sparks spiral in or out; the overhead emitter ring was
  removed from `teleporter.py` (it was the "stray ring" by the Research station: 3 m
  up, the camera projected it there). Measured: dozing 0.3 %, active 18-26 % (noise).
  **Next:** active power, mini mode re-check.
- 2026-11-18, Claude Opus 5.5 (power, the user's "widget, not the main app"): `Pace`
  table (moving / working / idle / dozing), 2.5 s dwell at stations, measure.sh at
  real speed. Working about 27 % to 15-17 %. Bloom, MSAA, shadows, pixel density,
  pipelining and thread count measured as no effect (details in V2.md phase 0).
  Labels show the real text again; activity screens glow with abstract writing
  lines. Mini mode re-checked: follows Claude, screens and labels fine. **Next:**
  phase 3 rest (task capsules, connection lines, error ring).
- 2026-11-18, Claude Opus 5.5: answer window bounded (wide tables scroll; title-bar
  close, Esc); window no longer transparent and the AppKit glass view removed (it
  pushed the title over the traffic lights); Bevy audio plugin off. V2 phase 3 done
  (lines, task and report capsules, failure ring; thinking in place). target/prof
  deleted. **Next:** phase 4 (event timeline, notifications, minimap, sound).
- 2026-11-18, Claude Opus 5.5 (V2 phase 4): notification toasts and sounds
  (`notify.rs`, `afplay`, Sound on/off saved in ui.json; measure.sh and tests run
  with sound off); minimap removed on the user's request; the side log stays the
  timeline. **Next:** phase 5 (camera: orbit, follow; replay) or stop here.
- 2026-11-18, Claude Opus 5.5: V2 wrapped up for a widget. Pan and zoom in the full
  view (drag, scroll or pinch, double-click resets; not tried with a real mouse
  yet). Tokens per session in the counts card (`usage.rs`, read once per turn).
  "1 agent" grammar. V2 phases 0-6 done or trimmed; open: working CPU 15-18 % vs
  the 10 % budget (4 fps working would halve it, the user's call).
- 2026-11-18, Claude Opus 5.5 (ideas from the research on similar apps, the user's
  pick 1-6): who needs you (beacon, badge, window title); double-click a robot or
  Open terminal to bring its terminal to the front; Claude's TodoWrite plan on a
  board (the user's sessions never used TodoWrite so far); context tank with
  PreCompact drain (plugin forwards PreCompact now: reinstall the plugin);
  celebrations for commits and passing tests, trophy shelf of finished turns;
  day/night sky by local time. Measured: dozing 0.0 %, active 21 %. **Next:** the
  user tries pan/zoom/tilt, sounds and terminal jump with a real session.
- 2026-11-18, Claude Opus 5.5: the user found it laggy; focused now always 30 fps,
  unfocused walking 10 fps, working/idle 4 fps. Station point lights removed (floor
  pools). Measured unfocused: dozing 0.1 %, working 25-28 % (was about 21 % with
  6 fps walking). Focused cost not measured (a script can't focus the window);
  estimate 50-70 % of a core while something animates at 30 fps, dozing after 30 s.
- 2026-11-18, Claude Opus 5.5: M10 done (see M10 "Done"): `room_proto::limits`,
  `room-hook --status`, the server's `LimitsReport` line, the chat's `rate_limit_event`,
  the top-right card. Checked by screenshot with a fake status line input. Researched
  M18 (chat as a terminal replacement); not started, waiting for the user's go.
- 2026-11-18, Claude Opus 5.5: M19 done (station consoles; side panel removed).
  Checked by screenshot with each console open; clicks on stations and signs still
  need the user's hands. Power unchanged or slightly lower.
- 2026-11-18, Claude Opus 5.5: M20 done in three commits (consoles; chat; notifications
  and worktrees). A subagent started the consoles but was stopped before editing
  anything: the user's new global rule says one waiting subagent should be the main
  session's own work instead.
- 2026-11-18, Claude Opus 5.5: researched what is left and new features (hooks
  reference for 2.1.327, similar apps); the user picked four, built as M21-M24 in four
  commits. Live checks against real Claude Code: plan approval switches the mode,
  PostModelSwitch payload, image blocks in stream-json. The screen was asleep, then
  locked, so no screenshot of the plan board, the chat suggestions or the drop hint:
  **Next:** look at those on screen, reinstall the plugin (0.3.0) for PostCompact and
  PostModelSwitch. Test plan files the live probes wrote to `~/.claude/plans` were
  deleted.
- 2026-11-18, Claude Opus 5.5: station clicks are exact now. The user opened consoles
  by mistake when clicking the floor ring or platform under a station, because a click
  anywhere within 90 px of a point above the station counted. Workstations now use
  Bevy's `MeshRayCast` against the station's own model (`world3d::StationBody` on the
  model root) and ignore hits below `Location::platform_top` (main desk tiers 0.38 m,
  other floor discs 0.08 m). The teleporter and wall screen keep the old reach.
  Clippy clean, tests pass; **not yet clicked on screen** (a script can't click the
  user's window): the user tries it after a rebuild.
- 2026-11-18, Claude Opus 5.5: the stats overlay (M11, Settings "Stats", I key) is gone,
  with `stats.rs` and the `sysinfo` dependency: it always said "Measuring…" (likely
  because the dozing render loop never presented the updated frame), and the user
  chose removal over a fix. `scripts/measure.sh` still measures the app's cost. The
  chat now takes slash commands and skills like the terminal: `/` at the start of the
  input suggests names from Claude Code's `init` event (`slash_commands`, about 530
  here), cached in `~/.agent-room/commands.json` for chats whose session hasn't
  started; Tab, Enter or a click completes, and a name typed in full sends on Enter.
  Checked live: `/caveman:caveman-help` sent as stream-json text ran the skill under
  `claude -p`. Not yet seen on screen.
- 2026-11-18, Claude Opus 5.5: `/` showed no commands in a resumed chat, because
  `init` (with `slash_commands`) only comes with the first turn. Each chat now sends an
  `initialize` control request at start; its answer lists `commands` right away (checked
  live). Cmd+V or Ctrl+V in the chat input now attaches clipboard images (Finder-copied
  image files, or picture data encoded as PNG and halved until under the 3.75 MB
  limit) via `arboard` + `png`; egui still pastes the text. Not yet tried on screen.
  Up/Down now move the highlighted `/` or `@` suggestion; Tab or Enter takes it.
- 2026-11-18, Claude Opus 5.5: Ended sessions stayed in the HQ (a "tung" robot and
  roster row long after it ended). `SessionEnd` is registered and handled; it just never
  arrives when the `claude` process is killed: a closed terminal window, or a chat tab
  (`ChatSession` drops with SIGKILL, and its default folder is `$HOME`, hence "tung").
  Now `end_dead_sessions` (main.rs) checks every 5 s, on frames drawn anyway, whether
  each live session's `claude_pid` still exists (`kill(pid, 0)`, ESRCH = gone) and ends
  it (`Model::end_dead`); chat tabs report their child's pid (`Model::set_pid`), since
  room-hook doesn't tag `AGENT_ROOM_CHAT` sessions. Sessions with no known pid (app
  started mid-session, no prompt since) stay until removed by hand: each roster row has
  a ✖ and Claude's details window a "Remove" button (`Model::forget`); both only forget
  the session in the app, and its next event brings it back. While dozing the check
  waits for the next wake-up. Seen on screen: a dead-pid session pruned, ✖ and Remove
  drawn; the clicks themselves are covered by unit tests only.
- 2026-11-18, Claude Opus 5.5: M25 pause, resume and stop for every session, through
  hold files read by `room-hook` on `PreToolUse` (see M25). Checked live with Haiku
  sessions, a fake app socket and the dev hook: pause held the tool until the file went
  (8 s), stop while paused denied the tool (its `touch` never ran) and sent `Stop`, and a
  stale pause with no app ran the tool at once. Clippy clean, tests pass. **Not seen on
  screen**: the Mac was locked, so screenshots were black. The user rebuilds, updates
  the plugin to 0.4.0 (`scripts/install-dev.sh`) and tries the buttons on Claude's card.
- 2026-11-18, Claude Opus 5.5: a chat message Claude hasn't taken yet shows dimmed
  (`Entry::Queued`, grey italic in a darker bubble), as the terminal shows a queued
  message. Chats now run with `--replay-user-messages`: Claude Code echoes each message
  (`isReplay: true`) when it reads it, at once when idle and mid-turn right after the
  running tool when busy (measured: sent at 2.5 s during a 6 s tool, echoed at 7.9 s).
  Local commands such as `/context` are never echoed (skills are), so a turn that ends
  without an echo marks the oldest queued message taken (`chat::track_pickup`, unit
  test with the measured order). Not seen on screen (Mac locked).
- 2026-11-19, Claude Opus 5.5: chat text was hard to read. A screenshot showed two
  causes: message text in egui's mid-grey, and the HUD, station labels and room
  showing through the chat console at roughly a quarter strength, although its fill
  had alpha 246. The console fill is now opaque (`console::hologram`) and the chat
  transcript draws text near-white (`override_text_color` 232) with 6 px between
  items. Clippy clean, tests pass. **Not seen on screen**: the Mac locked before the
  second screenshot. If text still shows through after the opaque fill, the cause is in
  how the egui camera's output is blended (`spawn_ui_camera`, `ALPHA_BLENDING`), not
  the fill. Research for new features went into the "2026-11-19 feature research"
  backlog.
- 2026-11-19, Claude Opus 5.5: "Tell me when done" bell (`notify::Bells`): a 🔔 toggle
  on Claude's details window and each sessions-card row; the next `Stop`,
  `StopFailure` or `SessionEnd` of that session (or `end_dead_sessions` finding its
  process gone) fires one macOS notification plus a sound, then clears. M4d part 2 is
  a badge, not rooms (see M4d): `Repo::away_from` / `Repo::badge`, used by the 3D
  labels and the sessions card. Unit tests for both; clippy clean, 61 tests pass.
  **Not seen on screen** (Mac locked): the user tries the 🔔 and checks the glyph
  renders.
- 2026-11-19, Claude Opus 5.5: M26, robots work at their desk. `director::location_for`
  now keeps Claude at its main-desk seat for every pose and gives each sub-agent a
  home station from its first tool call (`station_for`, `Avatar::home`); sub-agents
  walk to the main desk only for their task, their report and when they need the user.
  Unit tests rewritten for the new rules; clippy clean, 62 tests pass. Seen on screen
  with `scripts/replay.py` of `~/.agent-room/spike/events.jsonl` at speed 3: Claude
  stayed at the main desk while its tools ran, sub-agents beamed in and walked to it
  with their reports. Their short Bash calls ended before a walk to the lab showed.
- 2026-11-19, Claude Opus 5.5: the chat console stretched past the window when Claude's
  answer held a wide table, a long code line or a long path, taking its header and close
  button off screen (the user's screenshot; reproduced with Haiku). The transcript
  (`ui::transcript`, split out of the chat body) now scrolls both ways inside the
  console's width, as the final-answer pop-up already did. A headless egui test lays out
  the real console with such an answer: 890 px wide before, within 460 px + margins
  after. Clippy clean, 63 tests pass. Not seen on screen after the fix (Mac locked).
- 2026-11-19, Claude Opus 5.5: M27, the starship bridge is the room. A grey-box
  prototype of two layouts was rejected ("soul less and bland"), so both were rebuilt
  from the HQ's own models; the user picked `front` and asked for more desks. Now:
  `world3d::Spot` (`Command` or `Desk(n)`) replaces the five station `Location`s; the
  command station (main desk model) is at the back by the window, eight crew desks
  (writing desk model, `world3d::DESKS`) in three rows in front, the teleporter front
  centre, glowing routes from command to every desk. A sub-agent takes the first free
  desk when it first works (`director::free_desk`; when all are taken it shares the
  least used) and keeps it until it leaves. The command station's sign has a button for
  Settings, History, Chat, Changes and Commands (`Console::on_command`); a click on its
  model opens Settings; their holograms open over it. `floor.glb` is re-exported with
  rings under the new spots (`environment.py` `DESKS`). Removed: the layout switch, the
  bookshelf, lab and research models and their Blender builders (`research.py`,
  `research_preview.py`, `build_bookshelf`, `build_lab`). The ship flies
  (`world3d::cruise`): while any session is busy, 400 stars move along lines of sight
  through the window and spread from its centre (a warp), with short streaks; idle,
  they stand still and the view dozes. Seen on screen with a synthetic session of
  Claude and six sub-agents (all at their own desks, stars filling the window at 40 s).
  Not done: power measured with `scripts/measure.sh` (M27 step 3).
- 2026-11-19, Claude Opus 5.5: all sound removed. The user heard other apps' audio go
  "robot" while Agent Room ran. Measured with a busy session: macOS's audio service
  idles at 0 % CPU and each notification sound (`afplay`) woke it to 10-23 %, three
  overlapping at a session's start. The user then asked to remove audio completely:
  no `afplay`, no Sound setting (`PanelPrefs.sound` gone; an old `ui.json` still
  loads), the bell is a silent notification. Re-measured: no sound process, and the
  audio service's remaining 8-10 % was there before the app started and after it quit
  (another app's audio).
- 2026-11-19, Claude Opus 5.5: fixed two leftovers of the old room seen in the user's
  screenshot: the context tank stood at the old main desk spot, now beside a crew
  desk (`view3d::tank_at`, on the command platform again), and the trophy shelf lay
  across the front-right desk (`SHELF_AT` now along the front edge right of the
  teleporter). The command sign moved up (3.5 m) so Claude's label no longer covers
  its Chat button. Clippy clean; not seen on screen (display asleep). M28 written
  from the user's "Mission Control 2.0" brief.
- 2026-11-19, Claude Opus 5.5: M28 built in four commits (3bf2d85, d6d6bd3, bb26ffb
  and the docs). Checked: clippy, 68 tests (work record, report rendering, replay
  load/play), desk halos and nameplates on screen with a synthetic busy session,
  recording end to end (the app wrote all 126 events of a replayed session to
  `missions/busy-1.jsonl`) and playback started from it without errors. **Not seen on
  screen**: the activity console (folded and open), the Replay console and badge, and
  the robot details changes; the display slept during those runs. To look:
  `AGENT_ROOM_FEED=tools`, `AGENT_ROOM_REPLAY=<recording>` with
  `AGENT_ROOM_CONSOLE=replay`, `AGENT_ROOM_SHOW_DETAILS=1`. README's privacy section
  now lists what is written to disk (mission recordings hold prompts and commands).
- 2026-11-19, Claude Opus 5.5: M28 seen on screen once the display woke: the open
  activity console (Tool calls tab) now keeps its height with the tab row clear; the
  Replay console plays a recording made by the app ("event 61 of 126", timeline,
  speeds, Exit); the room rebuilds the replayed session (six robots at their desks,
  tool counts on the nameplates). The command sign covered robot labels wherever it
  stood in the room, so it is now a fixed panel under the title, top left, with the
  REPLAY badge below it. Robot details seen earlier ("Seen by Agent Room: 9 tool
  calls", expandable calls). Not seen: the Metrics, Files and Errors tabs, and an
  exported report opening (the report text is unit tested).
- 2026-11-19, Claude Opus 5.5: M29, starship look (from the user's reference images and
  two long art briefs). Done and seen on screen: navy steel and gunmetal materials
  (Blender re-export), cool lighting with cyan and amber point lights, a lower camera
  so the panoramic window shows a big slice of space, still realistic stars (the user:
  "we dont need to move the star"; the warp/cruise is gone), a textured cloudy blue
  planet and a ringed gas giant (`space.rs`, procedural textures), nebula clouds,
  asteroids, distant ships, the glass command core, a reactor ring with nodes and a
  beam above it, the command panel at the left above the desks, a generated
  reflection environment (`space::environment`, `GeneratedEnvironmentMapLight`), and
  Settings > Graphics: Low / Medium / High (`ui::Quality`, `view3d::apply_quality`;
  High adds SSAO with FXAA and shadows from the core light; Low drops bloom, effects
  and shadows). A bug hid every unlit additive material (unlit draws base colour, not
  emissive); fixed. **Open**: the lightning bolts (`fx.rs`) spawn (logged: ~4 a second
  while busy) but did not show in any frame, including 250 ms bursts; pulses and core
  particles were not seen either. Next: find why runtime-spawned fx meshes don't draw
  (the display slept before the last probe). Not done from the briefs: AAA geometry
  (detailed walls, ceiling, floor modules), volumetric light, SSR reflections,
  hologram scanlines; power not measured with the new effects.
- 2026-11-19, Claude Opus 5.5: the user rejected the lightning ("I AM TALKING ABOUT
  LIGHTING, NOT LIGHTNING") and called the look bland: `fx.rs` (bolts, sparks, beam,
  reactor, particles, pulses) is removed. The bridge is now a ship interior
  (`hull.rs`): upper walls to a 7.5 m ceiling, the back opened into a tall framed
  window, ceiling beams with light strips and five recessed round fixtures with real
  spot lights, amber lamps under the desks; the camera sits inside under the ceiling
  with a 56° view. Seen on screen (fb00097): ceiling and fixtures across the top, the
  tall window with the ringed planet and the textured blue planet, strips on walls.
  After that, not yet seen (display asleep): a glossier, less metallic floor
  (environment.py), brighter fixtures and stronger reflections. Next: judge the floor
  reflections and light pools on screen; consider SSR on High; measure power.

- 2026-11-19, Claude Opus 5.5: M30 phase 0 (audit). Bevy 0.20.1. Camera: Camera3d,
  TonyMcMapface, Bloom NATURAL at 0.22, MSAA 4 (High: SSAO + FXAA, MSAA off), no
  Exposure, ColorGrading or DebandDither; forward rendering. Lights: one shadowed
  directional key, GlobalAmbientLight 190 (0.48,0.64,1.0), cyan point lights at the
  core and teleporter, five ceiling spot lights (`hull.rs`), eight amber desk lamps;
  a generated cubemap as GeneratedEnvironmentMapLight. Materials: glTF from Blender
  scripts (`tools/blender/`) plus StandardMaterials made in code (`world3d`, `hull`,
  `space`), no custom shaders. UI: egui (bevy_egui), not bevy_ui. Agent state: `Model`
  (sessions, agents, poses, `work.rs` calls and files, todos), `usage` (tokens,
  context, limits), `activity`, `log`. The boxes before "Command", "Replay", "Chat"
  and "Activity" were glyphs missing from egui's fonts (✦ ⏯ 💬 ▲ ▼); now plain words
  (an icon font is phase 3). The yellow floor bar is the trophy shelf (`view3d::
  celebrate`, a gold cube per finished turn), not debug. Baseline metrics (median L
  0.31 vs 0.23, dark share 7% vs 31%, saturation 0.53 vs 0.74, glow 6.9% vs 14.2%,
  dark colour #1B2E43 vs #0B2146). Not built yet from phase 0: look config with hot
  reload, inspector, hero screenshot flag, FPS overlay (screenshot env vars and the
  busy-session replay serve for now).
- 2026-11-19, Claude Opus 5.5: M30 phase 1 (value and colour) done. The user removed
  the Low/Medium/High setting ("medium and high just have weird opacity"); the plain
  look stays: no bloom, no SSAO, no shadow maps, no desk lamps. The command panel is
  centred over the command core. `look.rs` + `assets/look.json`: exposure, ambient
  (25, #1A3A7A), key light (#9CC2FF, 1800 lx), reflection strength, light scales,
  colour grading (post saturation 1.15, temperature -0.05, contrast 1.06), vignette
  0.3, sky #020A1E; a dev build re-reads the file within 0.5 s of a save. Floor,
  walls, frames in the brief's navy; no white glow (cyan #38C8FF and azure #2E8BFF);
  glow intensities lowered so the tonemapper keeps them saturated without bloom;
  holo screens saturated azure. The brief's pack is in `docs/agent-room-visual-
  overhaul/` (PROMPT.md, reference images, crops, tools). Metrics (tools/look_metrics.py
  with a scratch venv for numpy/pillow): 7/7 within tolerance, was 1/7; glow share
  6.6% vs 10.4%. Screens: baseline/phase1_hero.png, phase1_compare.png.
- 2026-11-19, Claude Opus 5.5: M30 phase 2 (lighting and reflections), first pass. The
  user: "I don't care for the performance now, just give me the best look first". The
  camera now uses deferred shading (`DefaultOpaqueRendererMethod::deferred()` in
  main.rs), TAA with MSAA off, SSAO, screen-space reflections (thickness 0.4, 24
  steps) and contact shadows. The key light and the ceiling spots cast shadow maps.
  Bloom comes back tight: `bloom_intensity` 0.12, `bloom_spread` 0.2, `bloom_threshold`
  1.0 (additive, so only strips above the threshold get a halo, no haze), all in
  look.json. Exposure ev100 is 8.9 (was 9.7), ambient 45, post/shadow saturation 1.0
  to compensate for the darker SSAO and shadows. Metrics: 5/7 within tolerance, median
  0.144 against 0.198 and dark share 52% against 36% (both too dark). The reference
  pack and baseline PNGs were removed from the two unpushed commits (filter-branch)
  and are now git-ignored, so they stay local only. The amber floor strip at the
  front right was already there in phase 1. Screens: baseline/phase2_hero.png and
  phase2_compare.png (local). **Next:** user verdict on the bloom; then framing
  (camera higher and steeper, less empty ceiling), core glow and desk screen-wash
  lights, then phase 3 (HUD).
- 2026-11-19, Claude Opus 5.5: M30 FIX 01 (the user's brief `docs/agent-room-visual-
  overhaul/FIX_01_image_quality.md`; PROMPT.md 5.2/5.5 updated from the user's second
  pack). The phase 2 temporal effects made every surface speckle and flicker ("like
  1990 TV"), so they are gone: back to forward shading, Msaa::Sample4, Hdr, DebandDither,
  Gaussian shadow filtering, tight bloom. No TAA/SSAO/SSR/contact shadows (High preset
  only, later, one at a time with the two-frame test). Focused window renders
  continuously (`pace_updates`); unfocused keeps the power-saving pace. `window::
  log_resolution` logs logical/physical size (1400x891 / 2800x1782, scale 2). Code-made
  space textures get a mip chain and 16x anisotropic sampling. Practical lights: a
  core spot and a holo-blue screen wash per desk. Camera (0,7,14) looking at (0,0,-1.7),
  vertical FOV 0.80 rad, fitted by a search to the brief's framing (window band ~10%,
  robot head ~38%, teleporter ~71%, walls at the frame edges); the camera stays under
  the ceiling so nothing outside the walls shows. Command panel back to top centre under
  the status card; it drops its mission line when it would reach the cylinder. The
  yellow floor bar was the trophy shelf (`celebrate`, amber emissive, outside the room):
  now a dark trim ledge inside the front edge. Chat and Replay icons back (💬, ⟲, both
  in egui's emoji-icon-font). Metrics 7/7, glow 7.8%, dark 33%, p95 0.82; wall and floor
  crops identical between two frames 0.5 s apart; 60 fps (debug build, 2800x1782).
  Screens (local): baseline/fix01_*.png. **Next:** wait for the user's review; do not
  start phase 4.
- 2026-11-19, Claude Opus 5.5: M30 PROMPT_02 step 1 (look pass; brief in
  `docs/agent-room-visual-overhaul/PROMPT_02_models_and_look.md`). New `probe.rs`: about
  2.5 s after start, six 90° cameras render the room into 256 px HDR faces (main
  exposure, no tonemapping), GPU readback, rows mirrored, assembled into a cube and
  swapped into the camera's GeneratedEnvironmentMapLight. The startup cubemap must
  be the same size (Bevy sizes the filter's mip chain from the first map and fails
  validation otherwise). `env_intensity` now means "as seen" ≈ 1.2·2^ev100; 850 in
  look.json. Blender: floor panels in three roughnesses with clearcoat 0.4
  (KHR_materials_clearcoat), pad and centre bezels dark trim, rings thin RingCyan
  tubes (strength 9), screen glass dark navy alpha 0.55, page content dim blue
  (no white). Desk state halos are thin tori, not 16 cm annuli. Ceiling #0B1428. The
  stray floor bar was the trophy shelf cuboid in `view3d::celebrate` (unnamed mesh):
  removed. Camera (0,6,15.6) → (0,0,-4), FOV 0.785: the whole window band shows; the
  open front edge leaves a dark band at the bottom. `AGENT_ROOM_CAMERA=<turn°>,<zoom>
  [,<pan x>,<pan z>]` starts from another angle (orbit screenshots). Lights:
  spot_scale 1.7, point_scale 1.6, ambient 90, post_saturation 0.9. Metrics 7/7:
  median 0.189, dark 39%, glow 12%, p95 0.72; walls/floor identical across 0.5 s;
  49–56 fps debug. Orbiting ±40° shows outside the shell (camera rig limits are step 8).
  Screens (local): baseline/p02s1_*.png. **Next:** user review, then step 2 (desk console).
- 2026-11-19, Claude Opus 5.5: the user chose the reference's steep view over the
  brief's framing numbers ("yes do it"). Camera (0,12.2,15.4) → (0,0,-1.4), FOV 0.70,
  36° down: robot head ~32%, teleporter ~76%, floor front edge ~95% (no dark band).
  The camera is above the ceiling, so the ceiling parts live on render layer 1
  (`hull::CEILING_LAYER`); `hull::cutaway` leaves that layer out of the main camera
  while it's above the ceiling and adds it back inside. Lights and probe cameras use
  `hull::all_layers()` so the ceiling's shadows and reflections stay. Metrics 7/7
  (median 0.22, dark 31%, glow 13%); stable. Screens (local): baseline/p02s1b_*.png.
- 2026-11-19, Claude Opus 5.5: M30 PROMPT_03 A–C in progress (brief in docs/…/
  PROMPT_03_detail_color_screens.md; new 12-metric look_metrics.py copied in).
  A: white clear-coated hardware (#D9E2EE desk shells, robot shell, black visor),
  silver #9AA6B8 / charcoal #2A3140 metal (hull trim, Blender Metal/Trim/Graphite),
  key light #B8D4FF, a 350 lx #E8F0FF top light (`look::TopLight`, `top_lux`), amber
  pad arcs and end-cap slits, violet wall strips, rack LEDs 70/12/10/8. Floor light
  circles (user): the floor coat roughness is 0.35 (was 0.08, it mirrored each
  ceiling light as a crisp disc), spot radii 0.1, and the per-desk screen-wash point
  lights are gone (they made blobs on the glossy pads); pad tops `PadTop` rough 0.35.
  C: `stations.py` `build_desk_console` (desk_console.glb, ~12.6k tris): white stadium
  console with concave front, end caps with amber slits, centre pedestal with grooves
  and vent, underglow, front lip LED, projector bar, keyboard grid, touch pad, buttons,
  tablet; pad with 24 inset lights, bolts, segment cuts. main_desk dais: dark metal
  tiers, thick cyan/amber/magenta+violet tubes, three floating arcs. teleporter.glb is
  the orrery: 12 armoured bezel segments, amber lights, dark bowl, sun/teal blips, cyan
  rim. Activity holograms float above the desk's main screen, crew desks only.
  B (user: "it is a star ship and you display code?"): `screens.rs` paints eight
  starship illustrations (star map, planet, radar, ship schematic, nebula, route chart,
  crew, cargo) and four side panels in code, unlit textured quads in front of the
  screen glass; every desk a different mix. Not yet bound to the tool category.
  D started: 1.2 m floor panels, hazard chevrons, hex patches, spine arrows; side-wall
  bays with silver pilasters, strips, bolts, grooves, status lights.
  Not yet seen on screen: the screen art and D (the Mac's screen was off/locked; the
  app renders black then). Last measured (before B/D): 7/12 metrics.
- 2026-11-19, Claude Opus 5.5 (stopped at the usage limit): user said "yes do it" to
  (1) bigger brighter screens, (2) robots not hiding desks, (3) part D, (4) part E,
  (5) per-activity screen art. Done: 1 (main 1.5×0.75, sides 0.42×0.62, luminous blue
  panel base, unlit ×4.5), 2 (`Spot::work_spot` for desks: robot at the desk end, ±1.3 m,
  half turned to the screens), part of 3 (floor panel grooves and bolts, lower-wall
  vents, grooves, status lights, light coves), colour grading post 1.1 / highlights
  1.4, exposure 9.1, top light 1300 lx, white shells with a faint 0.5 self-glow, amber
  dimmed. Last clean measurement (before the self-glow): 9/12 — edge_density 0.151,
  flat_tiles 25%, white_hardware 0.8% still fail. The final shots had a Commands
  console open (a key press reached the test window), so the self-glow change is
  unmeasured. Left: re-measure, remaining D (mullions, back ledge, racks, ceiling
  coffers), E (cube-sphere planet, atmosphere, nebula sky), 5 (screen art by tool
  category), camera rig limits.
- 2026-11-19, Claude Opus 5.5: re-measured the build from the last entry: 8/12
  (edge_density 0.147, flat_tiles 26%, warm 0.004 too high, white_hardware 0.009).
  User: "The monitor on the desk should look better", then showed the target again
  ("This is what we wanted"). A first try with bezels, white back shells and stands
  was wrong: the target's screens (crops 07, 08) are frameless holograms. Now the
  main screen rises from the projector bar (centre 1.2 m) and the side panels
  (0.36x0.54) float over small graphite emitters with a cyan lens (`stations.py`
  `holo_emitter`). The art (`screens.rs`) is on translucent luminous blue glass
  (alpha 0.72, brighter in the middle), a 2 px bright cyan edge with an inner glow,
  a 32 px grid, scanlines, a header (title, words, an accent status pill) and a
  footer tick scale with readouts; unlit multiplier 3.2 (was 4.5).
  `cargo test -p room-app dump_screens -- --ignored` with
  `AGENT_ROOM_SCREENS_DUMP=<dir>` writes every screen texture as PNG. Checked in a
  Blender render of the desk with the dumped textures; not yet seen in the app
  (display off). Next: check on screen; tone down amber (pad arcs, chevrons);
  white hardware; then the rest of D, E, screen art by tool category, camera limits.
- 2026-11-19, Claude Opus 5.5: user: the planets should look like the target; "do one
  thing better, then give me the screenshot first". Planets are now sprites rendered
  once in Blender Cycles (`tools/blender/planets.py`, `assets/textures/space/`):
  premultiplied RGBA, 2.6 radii wide, drawn as camera-facing quads
  (`space::planet_sprite`, AlphaMode::Premultiplied, unlit ×1.4). The blue planet has
  continents, soft clouds, sun from the upper left, a night side with sparse city
  lights, and a 2D-painted atmosphere (thin bright limb plus a soft halo, stronger
  towards the sun). The ringed gas giant is replaced by a small cratered moon,
  rim-lit from the right. The old noise sphere, its glow billboard and the ring are gone.
  Not yet seen in the app: the Mac was locked (the app renders black then).
- 2026-11-19, Claude Opus 5.5: user picked target crops 07 and 08 ("Ok 2 cái này nhìn
  đc"). The crew desk is rebuilt after them (`stations.py` `build_desk_console`,
  `rrect`): a 12 cm white rounded slab with a dark glass top, a silver trim line, a
  cyan line on the front face and under the front edge, raised white arms on the back
  corners (amber slit, cyan lens the side hologram rises from) joined by the
  graphite projector bar, two white pedestal blocks with a groove, vent and status
  light, a dark modesty panel, a lit 17-key-wide keyboard, two amber pads, a cyan
  tablet, two touch dials, four buttons and a small plant. `screens::SCREENS`: main
  at 1.21 m over the bar, sides at x ±0.94, 1.24 m over the arms. Activity holograms
  float at 1.92 m (was 2.15). Preview (local): baseline/p05_desk.png. Not yet seen in
  the app (Mac locked).
- 2026-11-19, Claude Opus 5.5: user: the planets still look bad. Redone after the
  target's crops 12 and 13 (2048 px sprites): the big planet is all blues (deep ocean,
  bright shallows, blue-grey land), white cloud streaks, sparse lights on the night
  side and a strong white-cyan limb, lit from above and in front; it sits low on the
  right (glass point 5.5, -1.5, radius 4.5 on the glass), so only its glowing upper
  part shows above the sill. The moon is replaced by `terra`, a small planet with tan
  continents and blue oceans, lit from the upper right, in the left window. Preview
  (local): baseline/p06_planets.png. Not yet seen in the app (Mac locked).
- 2026-11-19, Claude Opus 5.5: user (crop 07): the screen should be darker and the
  picture on it should glow, in a different colour. Each desk screen is now two quads
  (`screens::build`): dark navy see-through glass (alpha 0.68, faint grid,
  `glass_bytes`) and, 4 mm in front, the art drawn with AlphaMode::Add at ×3
  (`ART_GAIN`), so it blooms while the glass stays dim. The art is light only (black
  adds nothing), tone-mapped by its brightest channel and pushed to purer hues so
  glow stays blue/cyan instead of white; lines have a wider glow. The cargo screen is
  one big isometric crate with translucent blue faces, cyan edges, white straps, a
  glowing platform and labels (`Canvas::fill`). Preview (local):
  baseline/p07_desk_glow.png. Not yet seen in the app (Mac locked).
- 2026-11-19, Claude Opus 5.5: new working rule from the user: prototype looks in
  Blender, show the render, wait for approval before porting or starting the next item.
  Screen holograms prototyped in `tools/blender/holo_screen_proto.py` (views close,
  room, screen, bake): dark glass with an edge glow and corner brackets, header, side
  readouts, tick scale, bar chart, a 3D subject standing out of the glass on a ring
  platform, motes, a beam from the projector bar. Subjects approved by the user:
  rocket, moon, sun (plasma ball, streamer corona, two flame arcs; no measuring ring),
  station, ship, black hole (black core, photon ring, flat streaked accretion disc,
  lensed far side; no outer ring), plus the cargo crate. Ported by baking: 
  `tools/blender/bake_screens.sh` renders each as light only, straight on, ortho,
  1024x512, exposure -1.7, into `assets/textures/screens/`; `screens::SUBJECTS` puts
  one on each desk's main screen, added over the dark glass at ×3.2 (`MAIN_GAIN`).
  The painted main-screen art (`Art`, `paint_main`) is gone; side panels unchanged.
  Not yet seen in the app (Mac locked). Open: the activity holograms still float over
  the screens at 1.92 m and may clash with the subjects.
- 2026-11-19, Claude Opus 5.5: rocket reworked on request (no wireframe lines on the
  body and nose, rings instead, three swept triangular fins) and re-baked. The user is
  away from the Mac; nothing of today's look work is seen in the app yet. Next session:
  take app screenshots first (hero, desk close-up), check screen brightness
  (`MAIN_GAIN`), planet placement and whether the activity holograms clash with the
  screen subjects; then, one item at a time with Blender approval, the rest of M30.
- 2026-11-19, Claude Opus 5.5: station, ship and the moon screen redone in the
  rocket's style (smooth translucent faces with a fresnel edge glow, `rim_face`; outlines
  only on flat parts); the moon became `planet` (glowing sphere, faint latitude bands, a
  flat streaked ring, a small moon; no orbit circle). User approved and asked to port,
  commit and push: re-baked planet, station, ship; `screens::SUBJECTS` uses planet.png.
- 2026-11-19, Claude Opus 5.5: the station screen went through a ring station, an
  ISS-like one (kept as subject `iss`; the user found it fake even with a lattice truss
  and an Earth horizon, `HOLO_EXTRAS=earth`) and finally one after the user's reference
  image: a round deck with a glowing rim, a domed hub with amber windows and an amber
  foot ring, a ring of modules with screens, bridges, two antenna towers and a cargo arm,
  seen from above (`station`, no ring platform). Approved and baked.
- 2026-11-19, Claude Opus 5.5: first app screenshots of the day's work (Mac unlocked):
  desks, screen holograms and planets all show. Bug fixed: the planet sprites were
  culled (the quad's back faced the camera); `planet_sprite` turns them half a turn.
  Screen subjects read well from the hero camera. Still open: screen brightness looks
  fine but unmeasured; activity holograms vs screen subjects not yet checked closely.
  Hero (local): baseline/p21_app_hero.png.
- 2026-11-19, Claude Opus 5.5: user: sub-agents should stand in the middle of the desk,
  facing the screen, and the screen should animate while they work. `Spot::work_spot`
  for desks: slot 0 at the middle of the operator side (0.8 m in front of the desk
  centre), facing the screens (yaw π); more robots at ±0.65, ±1.3 m. Screens: the bake
  now renders 8 frames per subject (`HOLO_FRAMES`/`HOLO_FRAME`, T -1..1: the subject
  sways ±18°, the sun's plasma boils, the black hole's disc streaks flow inward, motes
  drift up, a scan band sweeps), packed 4x2 into a 2048x512 atlas
  (`tools/blender/atlas.py`, `bake_screens.sh`, ~2 min). `screens::ScreenArt` and
  `view3d::screen_animation` play it forth and back at 6 fps (uv_transform) while a
  robot works at the desk and rest on frame 3 otherwise. Checked in the app with a
  busy replay: robots at the desk middle, the working desk's screen changes frame,
  idle desks hold still. Screens (local): baseline/p22_*.png.
- 2026-11-19, Claude Opus 5.5 (stopped at the usage limit, 22:40): RESUME HERE.
  Done on main (pushed up to 12678ef; 3706e36 local, not pushed): glass cage round the
  command core removed (`world3d::command_core` gone). Blender prototypes approved by
  the user and being ported when stopped. Four sub-agents ran in parallel in git
  worktrees under `.claude/worktrees/`; all stopped, their unfinished work committed
  as "wip" on their branches (not merged, not pushed). For each: read its diff
  (`git diff main...<branch>`), finish, build, test, screenshot, then merge into main:
  1. `worktree-agent-afc4fef666ff0fe03` (wip 7dc1f3e): Vietnamese/German text shows
     wrong in the app. Added assets/fonts/ + `src/fonts.rs`, LICENSES.md, ui.rs and
     main.rs changes. Check: font covers ộ ữ ß ü in egui (and any Bevy text);
     screenshot with a replay whose prompt is Vietnamese/German.
  2. `worktree-agent-adc135299d8e93793` (wip aeabdb6): comet/meteor visitors outside
     the window. New `src/visitor.rs`, main.rs. Art: assets/textures/space/comet.png,
     meteor.png (premultiplied, head +x; `tools/blender/comets.py`; approved preview
     baseline/p23_comet_meteor.png). Wanted: one every ~2–5 min, 6–10 s across,
     comets rarer, head leads along the path, fade in/out, only costs frames while on
     screen, `AGENT_ROOM_VISITOR=comet|meteor` forces one. Quad must face the camera
     with the half turn like `space::planet_sprite`.
  3. `worktree-agent-aae5bdfa7882057c9` (wip 093424f): big command hologram instead
     of the main desk console (approved baseline/p23_command_holo*.png; prototype
     `tools/blender/command_holo_proto.py`; `STATIONS_NO_CONSOLE=1` in stations.py).
     Changed main_desk.glb, world3d.rs, view3d.rs, main.rs, Cargo.toml/lock. Was
     wiring the module, build call, systems and HUD/plan-board anchors. Live text on
     the wall: header (state, "CLAUDE · MAIN AGENT · model"), task (the HUD's "Working
     on"), current step, recent tool chips; redraw only on change; non-ASCII must
     render (see 1). Keep `world3d::inbox()` hand-overs working without the console.
  4. `worktree-agent-ac00c63e8c084d65e` (wip 97cc323): the user's newest ask: the
     robot's name tag covers the monitor, so instead of animating the monitor, show
     that desk's subject as a big 3D hologram above the working desk (replacing the
     tool-kind activity hologram there), turning slowly while it works; monitor stays
     on its rest frame; move name tags so they cover neither. Started: GLB export of
     the subjects (`tools/blender/holo_glb.py`, `export_holo_subjects.sh`,
     assets/models/holo/, a `HOLO` look registry in holo_screen_proto.py).
  5. Me, unfinished: galaxy as the 8th screen subject (no more two rockets). Approved
     look baseline/p23_galaxy.png; `galaxy()` is in holo_screen_proto.py on main;
     bake_screens.sh lists it (uncommitted change, now committed). Still to do: bake
     it (8 frames + atlas, see bake_screens.sh; ~1 min alone, slow when the machine
     is busy) and add galaxy.png as the 8th entry of `screens::SUBJECTS` (size 8).
     If item 4 replaces screen animation, bake only what it needs.
  Merge order suggestion: 1 (fonts) first, then 5, 2, 3, 4; main.rs and view3d.rs
  will conflict (systems lists). Then push. Rules still apply: Blender look approval
  before porting new looks; kill all processes you start; the user's own agent-room
  (started from their terminal, relative path) is theirs.
- 2026-11-20, Claude Opus 5.5: Comets and meteors outside the window (`visitor.rs`):
  one at a time every 2-5 min (comets about 1 in 3), crossing in 6-10 s on a slightly
  slanted path, entering and leaving off the glass, head turned along the motion as
  the camera sees it, faded at the ends. Comets at z -100 (behind the small planet,
  in front of the nebula), meteors at z -36 (in front of the big planet). Power: no
  timer of its own; it checks the clock on frames already drawn (60 s low-power
  wake-up while dozing), and only while one is on screen `pace_updates` draws at the
  walking rate (not counted as activity). `AGENT_ROOM_VISITOR=comet|meteor` sends one
  a second after start. Not yet checked visually: the screen was locked, so
  screenshots were black; check with the env var.
