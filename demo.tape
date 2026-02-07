# Apiary Demo - VHS Recording
# Demonstrates the TUI dashboard with Pod management

Output assets/demo.gif
Output assets/demo.webm

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

# Cleanup and set up alias to suppress stderr/logs (hidden from recording)
Hide
Type "tmux kill-server 2>/dev/null; rm -f \"$HOME/Library/Application Support/apiary/pods.json\" \"$HOME/Library/Application Support/apiary/pods.json.tmp\""
Enter
Sleep 500ms
Type "alias apiary='RUST_LOG=error command apiary'"
Enter
Sleep 200ms
Type "clear"
Enter
Sleep 500ms
Show

# Step 1: Show help
Sleep 300ms
Type "apiary --help"
Enter
Sleep 3s

# Step 2: Create pods
Type "apiary create issue-1234"
Enter
Sleep 1.5s

Type "apiary create hotfix-99"
Enter
Sleep 1.5s

# Step 3: List pods
Type "apiary list"
Enter
Sleep 2s

# Step 4: Status
Type "apiary status"
Enter
Sleep 2.5s

# Step 5: Launch TUI
Type "apiary"
Enter
Sleep 3s

# Step 6: Navigate pods
Right
Sleep 1s
Left
Sleep 1s

# Step 7: Enter detail view
Enter
Sleep 2s

# Step 8: Back to home
Escape
Sleep 1.5s

# Step 9: Help screen
Type "?"
Sleep 3s

# Step 10: Exit
Type "?"
Sleep 1s
Type "q"
Sleep 1s
