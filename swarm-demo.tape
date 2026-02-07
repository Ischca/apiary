# Apiary Swarm Demo - VHS Recording
# Shows the realistic workflow:
# 1. Launch TUI
# 2. Create pods via /create command
# 3. Claude Code (fake) starts in each pod
# 4. Teammates spawn automatically (simulating Claude Code Team)
# 5. TUI discovers new members dynamically

Output assets/swarm-demo.gif
Output assets/swarm-demo.webm

Set Shell "bash"
Set FontFamily "Fira Code"
Set FontSize 15
Set Width 1400
Set Height 700
Set Padding 20
Set Framerate 30
Set PlaybackSpeed 0.8
Set TypingSpeed 50ms
Set Theme {
  "name": "Apiary",
  "black": "#1a1a2e",
  "red": "#ef4444",
  "green": "#22c55e",
  "yellow": "#f59e0b",
  "blue": "#3b82f6",
  "magenta": "#8b5cf6",
  "cyan": "#22d3ee",
  "white": "#e2e8f0",
  "brightBlack": "#475569",
  "brightRed": "#f87171",
  "brightGreen": "#4ade80",
  "brightYellow": "#fbbf24",
  "brightBlue": "#60a5fa",
  "brightMagenta": "#a78bfa",
  "brightCyan": "#67e8f9",
  "brightWhite": "#f8fafc",
  "background": "#0f0d1a",
  "foreground": "#e2e8f0",
  "selection": "#334155",
  "cursor": "#f59e0b"
}

# ============================================================
# Hidden setup
# ============================================================
Hide

# Clean state
Type "tmux kill-server 2>/dev/null; rm -f \"$HOME/Library/Application Support/apiary/pods.json\" \"$HOME/Library/Application Support/apiary/pods.json.tmp\""
Enter
Sleep 500ms

# Put fake claude first in PATH
Type "export PATH=\"$PWD:$PATH\"; ln -sf demo-fake-claude.sh claude"
Enter
Sleep 200ms

# Start teammate-spawner in background
Type "bash ./swarm-demo-orchestrator.sh &"
Enter
Sleep 500ms

# Alias apiary to suppress logs
Type "alias apiary='RUST_LOG=error command apiary'"
Enter
Sleep 200ms

Type "clear"
Enter
Sleep 500ms

Show

# ============================================================
# Phase 1 (0-3s): Launch TUI — empty dashboard
# ============================================================
Type "apiary"
Enter
Sleep 3s

# ============================================================
# Phase 2 (3-9s): /create bugfix-503 — first pod appears
# ============================================================
Type "/"
Sleep 500ms
Type "create bugfix-503"
Enter
Sleep 5s

# ============================================================
# Phase 3 (9-15s): /create auth-refactor — second pod appears
# ============================================================
Type "/"
Sleep 500ms
Type "create auth-refactor"
Enter
Sleep 5s

# ============================================================
# Phase 4 (15-21s): auth-refactor grows — impl teammate spawns
# (orchestrator splits pane ~6s after session creation)
# ============================================================
Sleep 6s

# ============================================================
# Phase 5 (21-27s): tests teammate spawns, create api-migration
# ============================================================
Type "/"
Sleep 500ms
Type "create api-migration"
Enter
Sleep 5s

# ============================================================
# Phase 6 (27-31s): Browse the grid, watch api-migration grow
# ============================================================
Right
Sleep 1s
Right
Sleep 1s
Left
Sleep 1s
Left
Sleep 1s

# ============================================================
# Phase 7 (31-35s): Jump to Permission pod (n), enter Detail
# ============================================================
Type "n"
Sleep 1.5s
Enter
Sleep 2s

# ============================================================
# Phase 8 (35-39s): Browse members in Detail
# ============================================================
Down
Sleep 1s
Down
Sleep 1s
Up
Sleep 1s

# ============================================================
# Phase 9 (39-43s): Permission mode — approve
# ============================================================
Type "p"
Sleep 2s
Type "a"
Sleep 2s

# ============================================================
# Phase 10 (43-47s): Navigate to api-migration Detail
# ============================================================
Escape
Sleep 1s
Right
Sleep 0.5s
Right
Sleep 0.5s
Enter
Sleep 1.5s

# ============================================================
# Phase 11 (47-53s): Chat mode — send message
# ============================================================
Type "c"
Sleep 1s
Type "focus on the v2 endpoints"
Sleep 1.5s
Enter
Sleep 2s

# ============================================================
# Phase 12 (53-57s): Back to Home — final overview
# ============================================================
Escape
Sleep 1s
Escape
Sleep 2s

# ============================================================
# Phase 13 (57-59s): Quit
# ============================================================
Type "q"
Sleep 1s
