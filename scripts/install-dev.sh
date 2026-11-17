#!/usr/bin/env bash
# Installs this checkout's agent-room and room-hook into ~/.cargo/bin, the same place
# `cargo install --git` puts them, so the plugin's hook shim finds them.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cargo install --locked --path "$root/crates/room-hook"
cargo install --locked --path "$root/crates/room-app"
echo "Installed agent-room and room-hook into ~/.cargo/bin"
