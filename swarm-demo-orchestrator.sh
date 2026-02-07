#!/usr/bin/env bash
# swarm-demo-orchestrator.sh
# Watches for tmux sessions created by TUI's /create command.
# After detecting them, injects teammate panes to simulate Claude Code Team.
# Run in background BEFORE starting the TUI.

set -euo pipefail
exec >/dev/null 2>&1

# ======================================================================
# Wait for sessions to appear, then add teammates
# ======================================================================

# --- Wait for auth-refactor session (created by TUI) ---
while ! tmux has-session -t auth-refactor 2>/dev/null; do
  sleep 0.5
done

# Let the fake claude get established first
sleep 6

# Add impl teammate (Permission status)
tmux split-window -t auth-refactor
sleep 0.5
tmux send-keys -t "auth-refactor:0.1" "printf 'Claude Code  agent: impl\nI need to run a command:\n  Bash: rm -rf ./tmp/cache && npm run build\nAllow this action? (y/n)\n'" Enter

# Add tests teammate
sleep 5
tmux split-window -t auth-refactor
sleep 0.5
tmux send-keys -t "auth-refactor:0.2" "printf 'Claude Code  agent name: tests\nRunning test suite...\ntool use: Bash cargo test --lib\n'" Enter

# --- Wait for api-migration session (created by TUI) ---
while ! tmux has-session -t api-migration 2>/dev/null; do
  sleep 0.5
done

# Let the fake claude get established
sleep 5

# Add researcher teammate
tmux split-window -t api-migration
sleep 0.5
tmux send-keys -t "api-migration:0.1" "printf 'Claude Code  agent: researcher\nDone researching.\n❯ '" Enter

# Done
sleep 30
