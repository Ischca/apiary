---
name: vhs-demo
description: Create, debug, and record VHS demo tapes for this TUI application. Use when the user asks to create demo GIFs, fix demo recordings, or troubleshoot VHS tape issues.
allowed-tools: Read, Write, Edit, Bash, Glob, Grep
argument-hint: "[create|debug|record <tape-file>]"
---

# VHS Demo Recording Skill

You are an expert at creating VHS (https://github.com/charmbracelet/vhs) demo recordings for TUI applications, especially ratatui-based apps running inside tmux.

## Usage

- `/vhs-demo create <name>` — Create a new .tape file
- `/vhs-demo debug <tape-file>` — Debug a broken recording
- `/vhs-demo record <tape-file>` — Record and verify a tape

---

## Critical Knowledge (Lessons Learned)

### 1. macOS Config Path

`dirs::config_dir()` on macOS returns `~/Library/Application Support/`, **NOT** `~/.config/`.

```bash
# WRONG
rm -f ~/.config/apiary/pods.json

# CORRECT
rm -f "$HOME/Library/Application Support/apiary/pods.json"
```

Always verify the actual path by checking the Rust source (`dirs::config_dir()`) or running the app with debug output.

### 2. Background Processes Corrupt TUI Display

Any background process that writes to stdout/stderr **will corrupt the TUI's alternate screen**. This is the #1 cause of "display is broken" issues.

```bash
# WRONG — stdout from `apiary adopt` bleeds into TUI
bash ./orchestrator.sh &

# CORRECT — silence all output
exec >/dev/null 2>&1   # Add at top of background scripts
```

This applies to:
- Background orchestrator scripts
- `apiary adopt` (prints "Session 'xxx' adopted as pod")
- Any `echo`, `printf`, or command that produces output

### 3. Atomic File Writes

`std::fs::write()` is **NOT atomic**. If a TUI reads a file while another process is writing it, you get partial/empty data, causing flicker or data loss.

```rust
// WRONG — truncates then writes (race window)
std::fs::write(&path, content)?;

// CORRECT — write to tmp, then atomic rename
let tmp = path.with_extension("json.tmp");
std::fs::write(&tmp, content)?;
std::fs::rename(&tmp, &path)?;
```

Also add safety checks in the reader to handle empty loads gracefully.

### 4. tmux Session PATH Propagation

VHS `Set Shell "bash"` only affects the **recording shell**, NOT tmux sessions created later.

tmux sessions inherit the **server's** environment, but the shell (especially zsh on macOS) re-reads its profile and **overrides PATH**.

To inject a fake command (e.g., fake `claude`) into tmux sessions:

```bash
# 1. Create fake binary in a known directory
mkdir -p /tmp/apiary-demo-bin
cp demo-fake-claude.sh /tmp/apiary-demo-bin/claude
chmod +x /tmp/apiary-demo-bin/claude

# 2. Start tmux server with controlled environment
tmux new-session -d -s _daemon
tmux set-option -g default-command "bash --norc --noprofile"
tmux set-environment -g PATH "/tmp/apiary-demo-bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
```

Key points:
- `default-command "bash --norc --noprofile"` prevents shell profile from overriding PATH
- `set-environment -g PATH` sets PATH for all future sessions
- The `_daemon` session keeps the server alive
- New sessions created by `tmux new-session` will inherit these settings

### 5. GIF Verification

The `Read` tool only shows **one static frame** of an animated GIF. To verify a recording:

```bash
# Extract frames at specific timestamps
mkdir -p /tmp/frames
ffmpeg -y -i assets/demo.webm \
  -vf "select='eq(n\,150)+eq(n\,300)+eq(n\,450)+eq(n\,600)'" \
  -vsync vfr /tmp/frames/frame_%03d.png

# Then read individual PNGs to inspect
```

Also check file size as a sanity signal:
- **< 100KB** for a 30+ second recording = probably empty/broken
- **200KB-2MB** = normal for a TUI recording

### 6. Silent Script Failures

`set -euo pipefail` + `exec >/dev/null 2>&1` = **silent death**. The script fails and you never know.

Debug approach:
```bash
# Run with trace output to a FILE (not stdout)
bash -x ./orchestrator.sh 2>/tmp/orch-debug.log &
```

### 7. VHS Tape Timing

- `Sleep` in VHS controls recording time, NOT process execution time
- Background scripts run in real time regardless of VHS Sleep
- Account for: tmux session creation (~0.3s), command execution (~0.5s), TUI refresh cycle (0.5s), dynamic reload interval (2s)
- Always add buffer time after creating sessions before expecting them to appear in TUI

### 8. TUI Create Flow

When the TUI's `/create` command runs:
1. `Tmux::new_session(name, start_dir)` — creates tmux session
2. `Tmux::list_panes(name)` — gets pane ID
3. Pod is added to state and saved to store
4. `Tmux::start_claude_in_session(name, None)` — sends `claude` + Enter to pane

The `claude` command runs inside the tmux session's shell, which has its own PATH (see point 4 above).

---

## VHS Tape Template

```tape
Output assets/<name>.gif
Output assets/<name>.webm

Set Shell "bash"
Set FontFamily "Fira Code"
Set FontSize 15
Set Width 1400
Set Height 700
Set Padding 20
Set Framerate 30
Set PlaybackSpeed 0.8
Set TypingSpeed 50ms
Set Theme { ... }

# ============================================================
# Hidden setup — ALWAYS redirect background process output
# ============================================================
Hide

# Clean state (use correct macOS path!)
Type "tmux kill-server 2>/dev/null; rm -f \"$HOME/Library/Application Support/apiary/pods.json\" \"$HOME/Library/Application Support/apiary/pods.json.tmp\""
Enter
Sleep 500ms

# Background scripts MUST suppress output
Type "bash ./orchestrator.sh &"   # orchestrator has exec >/dev/null 2>&1
Enter
Sleep 500ms

Type "clear"
Enter
Sleep 500ms

Show

# ============================================================
# Demo content — use phases with comments
# ============================================================
# Phase 1: ...
Type "apiary"
Enter
Sleep 3s
```

---

## Debug Checklist

When a recording is broken, check in this order:

1. **File size** — Is the GIF suspiciously small?
2. **Extract frames** — Use ffmpeg to check specific timestamps
3. **Config path** — Is cleanup using the correct OS-specific path?
4. **Background stdout** — Are any background processes writing to the terminal?
5. **Script failures** — Run orchestrator with `bash -x` to a log file
6. **tmux sessions** — Do `tmux list-sessions` to verify sessions exist
7. **pods.json** — Check the correct path: `~/Library/Application Support/apiary/pods.json`
8. **Timing** — Is the TUI refresh interval (2s for dynamic reload) accounted for?
9. **PATH** — Does the tmux session's shell find the expected commands?
10. **Atomic writes** — Could a race condition cause empty/partial reads?

---

## Recording Workflow

```bash
# 1. Clean state
tmux kill-server 2>/dev/null
rm -f "$HOME/Library/Application Support/apiary/pods.json"

# 2. Record
vhs <tape-file>.tape

# 3. Verify
ls -lh assets/<name>.gif   # Check file size
ffmpeg -y -i assets/<name>.webm \
  -vf "select='eq(n\,150)+eq(n\,450)+eq(n\,750)'" \
  -vsync vfr /tmp/verify_%03d.png
# Then Read each PNG to inspect content

# 4. If broken, debug
bash -x ./orchestrator.sh 2>/tmp/debug.log &
sleep 10
cat /tmp/debug.log
```
