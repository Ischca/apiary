//! 統合テスト: tmux pane に模擬出力を書き込み → capture-pane → parse_sub_agents
//!
//! Agent Teams / Subagent の出力パターンが capture-pane 経由で正しく検出されるかを
//! 実際の tmux を使って検証する。CI 環境では tmux が利用できない場合があるため
//! `#[ignore]` 属性付き。手元では `cargo test -- --ignored` で実行可能。

use std::process::Command;

/// tmux が利用可能かチェック
fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// テスト用 tmux セッションを作成し、テキストを表示させ、capture-pane で取得
fn capture_roundtrip(text: &str, tag: &str) -> String {
    let session = format!("apiary-test-{}-{}", std::process::id(), tag);

    // セッション作成
    let status = Command::new("tmux")
        .args(["new-session", "-d", "-s", &session, "-x", "120", "-y", "40"])
        .status()
        .expect("tmux new-session failed");
    assert!(status.success(), "Failed to create tmux session");

    // テキストをクリアしてから printf で出力（send-keys だと遅いので直接 cat）
    // 方法: pane 内で printf を実行
    let escaped = text
        .replace('\\', "\\\\")
        .replace('\'', "'\\''");

    let cmd = format!("printf '{}'", escaped);
    let _ = Command::new("tmux")
        .args(["send-keys", "-t", &session, &cmd, "Enter"])
        .status();

    // 少し待ってから capture
    std::thread::sleep(std::time::Duration::from_millis(500));

    let output = Command::new("tmux")
        .args(["capture-pane", "-p", "-t", &session])
        .output()
        .expect("capture-pane failed");

    // セッション削除
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", &session])
        .status();

    String::from_utf8_lossy(&output.stdout).to_string()
}

// -----------------------------------------------------------------------
// テストケース
// -----------------------------------------------------------------------

#[test]
#[ignore] // tmux が必要なため CI では skip
fn test_agent_teams_background_pattern_roundtrip() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }

    let text = "* Worked for 54s · 3 agents running in the background\n\
                1 tasks (0 done, 1 in progress)\n\
                ►► accept edits on · 3 local agents · ctrl+t to hide task";

    let captured = capture_roundtrip(text, "bg");
    eprintln!("--- captured output ---\n{}\n--- end ---", captured);

    // parse_sub_agent_count を直接呼ぶ (apiary はバイナリクレートなので
    // ここでは正規表現を直接テスト)
    let re_background = regex::Regex::new(r"(\d+)\s+agents?\s+running\s+in\s+the\s+background").unwrap();
    let re_local = regex::Regex::new(r"(\d+)\s+local\s+agents?").unwrap();

    let bg_match = re_background.captures(&captured)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<usize>().ok());
    let local_match = re_local.captures(&captured)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<usize>().ok());

    eprintln!("bg_match: {:?}, local_match: {:?}", bg_match, local_match);

    assert!(
        bg_match == Some(3) || local_match == Some(3),
        "Expected to detect 3 agents. bg={:?}, local={:?}\ncaptured:\n{}",
        bg_match, local_match, captured
    );
}

#[test]
#[ignore]
fn test_running_task_agents_pattern_roundtrip() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }

    let text = "● Running 2 Task agents… (ctrl+o to expand)\n\
                  ├─ Explore codebase · 5 tool uses · 12k tokens\n\
                  └─ Search tests · 3 tool uses · 8k tokens";

    let captured = capture_roundtrip(text, "task");
    eprintln!("--- captured output ---\n{}\n--- end ---", captured);

    let re = regex::Regex::new(r"Running\s+(\d+)\s+Task\s+agents?").unwrap();
    let count = re.captures(&captured)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<usize>().ok());

    eprintln!("count: {:?}", count);
    assert_eq!(count, Some(2), "Expected 2 Task agents.\ncaptured:\n{}", captured);
}

#[test]
#[ignore]
fn test_cooked_pattern_no_agents() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }

    // Agent Teams 完了後 → エージェント数 0 であるべき
    let text = "✻ Cooked for 1m 49s\n❯ Phase 1-1を実装して\n⏵⏵ accept edits on · ctrl+t to hide tasks";

    let captured = capture_roundtrip(text, "cooked");
    eprintln!("--- captured output ---\n{}\n--- end ---", captured);

    let re_background = regex::Regex::new(r"(\d+)\s+agents?\s+running\s+in\s+the\s+background").unwrap();
    let re_local = regex::Regex::new(r"(\d+)\s+local\s+agents?").unwrap();
    let re_running = regex::Regex::new(r"Running\s+(\d+)\s+(?:Task\s+)?agents?").unwrap();

    assert!(!re_background.is_match(&captured), "Should not match background pattern");
    assert!(!re_local.is_match(&captured), "Should not match local pattern");
    assert!(!re_running.is_match(&captured), "Should not match running pattern");
}
