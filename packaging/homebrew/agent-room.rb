# Homebrew formula for Agent Room. It builds from source, so the binaries are compiled on
# the user's Mac and need no code signing. Publish it in a tap repository
# (github.com/tungs0ul/homebrew-tap, file Formula/agent-room.rb), then users run
#   brew install tungs0ul/tap/agent-room
# Before publishing: point `url` at a release tag and fill in `sha256`
# (`shasum -a 256 <tarball>`).
class AgentRoom < Formula
  desc "Watch Claude Code and its sub-agents work, and answer permission prompts"
  homepage "https://github.com/tungs0ul/agent_visualization"
  url "https://github.com/tungs0ul/agent_visualization/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "<sha256 of the tarball>"
  license "MIT"
  head "https://github.com/tungs0ul/agent_visualization.git", branch: "main"

  depends_on "rust" => :build
  depends_on :macos

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/room-hook")
    system "cargo", "install", *std_cargo_args(path: "crates/room-app")
  end

  def caveats
    <<~EOS
      Add the Claude Code plugin that feeds Agent Room, inside Claude Code:
        /plugin marketplace add tungs0ul/agent_visualization
        /plugin install agent-room@agent-room
      Then start the app with: agent-room
    EOS
  end

  test do
    # The hook must stay silent and succeed when the app isn't running.
    output = pipe_output("#{bin}/room-hook", '{"session_id":"t","hook_event_name":"Stop"}')
    assert_empty output
  end
end
