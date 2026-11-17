#!/usr/bin/env bash
# Builds room-hook in release mode and copies it into plugin/libexec/,
# where plugin/hooks/hooks.json expects it.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo build --release -p room-hook
mkdir -p plugin/libexec
cp target/release/room-hook plugin/libexec/room-hook
echo "plugin/libexec/room-hook updated"
