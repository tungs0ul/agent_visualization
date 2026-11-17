# Agent instructions

- Read `PLAN.md` first. It holds all decisions, architecture, milestones and current status.
- Do not re-open decisions listed in PLAN.md section 2 unless the user asks.
- Bevy API changes between versions: check the examples for the exact Bevy version in
  `Cargo.lock` before writing Bevy code.
- Hooks ship in the plugin under `plugin/`. Test with `claude --plugin-dir plugin`.
  Never write hooks into `~/.claude/settings.json`.
- Before finishing a session: kill every process you started (`room-app`, test servers,
  replay scripts), then update PLAN.md "Status" and "Handoff log".
- Never make the app or `room-hook` answer "allow" without an explicit user action.
