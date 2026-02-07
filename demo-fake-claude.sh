#!/usr/bin/env bash
# Fake "claude" command for demo recordings.
# Detects which tmux session it's in and outputs appropriate fake output.

SESSION=$(tmux display-message -p '#{session_name}' 2>/dev/null || echo "unknown")

case "$SESSION" in
  bugfix-503)
    echo "Claude Code"
    sleep 1
    echo "Reading src/server/connection.rs..."
    sleep 1
    echo "tool use: Read src/server/pool.rs"
    sleep 1
    echo "Editing src/server/pool.rs..."
    sleep 1
    echo "Fixed the 503 error in connection pool."
    echo "All changes applied successfully."
    echo ""
    echo "Session ended"
    sleep 3600
    ;;
  auth-refactor)
    echo "Claude Code  agent: lead"
    sleep 1
    echo "Reading src/auth/handler.rs..."
    sleep 1
    echo "tool use: Read src/auth/types.rs"
    sleep 1
    echo "Editing src/auth/middleware.rs..."
    sleep 3600
    ;;
  api-migration)
    echo "Claude Code  I am lead"
    sleep 1
    echo "Analyzing API endpoints..."
    sleep 1
    echo "tool use: Read src/api/v2/routes.rs"
    sleep 3600
    ;;
  *)
    echo "Claude Code"
    sleep 1
    echo "tool use: Read README.md"
    sleep 3600
    ;;
esac
