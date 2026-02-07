# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
cargo build                  # Dev build
cargo build --release        # Release build
cargo install --path . --locked  # Install binary (use --locked to pin deps)
cargo test                   # Run all tests
cargo test --lib detector    # Run detector tests only
cargo clippy                 # Lint
cargo fmt                    # Format
RUST_LOG=apiary=debug cargo run  # Run TUI with debug logging (stderr)
```

Minimum Rust version: check Cargo.lock for compatibility (some transitive deps require 1.88+; `--locked` avoids pulling newer versions).

## Architecture

Apiary is a synchronous TUI dashboard (ratatui + crossterm) that manages multiple Claude Code sessions running in tmux. There is no async in the main loop — everything is tick-based polling.

### Core Concept: Pod = tmux session, Member = tmux pane

```
Pod (tmux session "auth-refactor")
├── Member: lead   (pane %0) — Working
├── Member: impl   (pane %3) — Permission
└── Member: tests  (pane %5) — Idle
```

A Solo pod has 1 member. When `discover_new_members()` finds additional panes with Claude Code output, it becomes a Team pod.

### Main Loop (`src/main.rs` → `run_app()`)

Three nested timing loops drive the app:
- **250ms tick**: recalculate grid layout, poll chat output in Chat mode
- **500ms refresh**: `app.selective_refresh()` — adaptive per-member polling + render
- **2s reload**: inside `selective_refresh()`, reload `pods.json` and run member discovery

### Data Flow

```
pods.json ──load──► App.state.pods ──selective_refresh──► capture_pane ──► detect_status ──► render
                         ▲                                                       │
                         └──────────── rollup_status ◄───────────────────────────┘
```

1. **PodStore** (`src/store/mod.rs`): JSON persistence at `~/Library/Application Support/apiary/pods.json` (macOS). Uses atomic writes (tmp → rename).
2. **Discovery** (`src/pod/discovery.rs`): Lists tmux panes, checks if output matches Claude Code patterns (`is_claude_code_pane`), extracts role names.
3. **Detection** (`src/pod/detector.rs`): Regex patterns on last ~15 lines of pane output. Priority: Permission > Error > Working > Idle > Done.
4. **Rollup**: Pod status = highest-priority member status.

### Adaptive Polling (`src/tui/app.rs` → `selective_refresh()`)

Each member has an independent `last_polled` timestamp. Polling interval depends on state:
- Focused pod / Permission: 1s
- Working: 3s
- Error: 5s
- Idle: 10s

This avoids hammering tmux for idle sessions.

### Dynamic Reload (also in `selective_refresh()`)

Every 2 seconds:
1. Re-read `pods.json` — merge new pods, remove deleted ones (with empty-store safety guard)
2. Run `discover_new_members()` on each pod — detect new panes (teammate spawns)
3. Update `pod_type` (Solo ↔ Team) based on member count

### Module Responsibilities

| Module | Role |
|--------|------|
| `src/tui/app.rs` | App struct, state mutations, command execution, polling logic |
| `src/tui/handler.rs` | Key event → Action dispatch per Mode (Home/Detail/Chat/Permission/Help) |
| `src/tui/ui.rs` | All ratatui rendering — grid cards, detail panel, chat, status bar |
| `src/pod/mod.rs` | Data models (Pod, Member, AppState, enums), rollup_status |
| `src/pod/detector.rs` | Status detection from pane output, permission request parsing |
| `src/pod/discovery.rs` | New member discovery, stale member removal, Claude Code heuristics |
| `src/store/mod.rs` | PodStore — load/save/reconcile pods.json |
| `src/tmux/mod.rs` | Stateless tmux CLI wrapper (all calls are `Command::new("tmux")`) |
| `src/hooks.rs` | Optional fast-path: poll `/tmp/apiary-hooks.jsonl` for real-time events |
| `src/config.rs` | Config from `~/.config/apiary/config.toml` (polling intervals, detection patterns, notifications) |

### Key Handler Flow

`/` enters command mode in Home. Commands: `create <name>`, `adopt <session>`, `drop <name>`, `forget <name>`, `list`. The `create` command creates a tmux session, adds a Pod to state, saves to store, then sends `claude` + Enter to the pane.

### Permission Flow

1. `detect_member_status_with_config()` finds Permission pattern in pane output
2. `parse_permission_request()` extracts tool name + command via regex
3. User presses `a` (approve) or `d` (deny) in Permission mode
4. `Tmux::send_keys_raw(pane, "y"/""n")` sends keystroke to the pane

## Platform Notes

- **Config path**: `dirs::config_dir()` on macOS = `~/Library/Application Support/`, NOT `~/.config/`
- **Store path**: `~/Library/Application Support/apiary/pods.json`
- **Config file**: `~/.config/apiary/config.toml` (loaded separately via custom logic in config.rs)
- **Atomic writes**: Store uses tmp file + rename to prevent corruption from concurrent reads

## Testing

Tests live in `src/pod/detector.rs` (12 tests for status detection/permission parsing) and `src/store/mod.rs` (6 tests for persistence). Detector tests cover edge cases like empty output, priority ordering, and tail-only matching.
