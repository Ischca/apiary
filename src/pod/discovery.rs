use crate::pod::{Member, MemberStatus, Pod};
use crate::tmux::Tmux;
use chrono::Utc;
use regex::Regex;

/// Claude Code の特徴的なパターン
const CLAUDE_CODE_PATTERNS: &[&str] = &[
    r"(?i)claude",
    r"\u{276f}",          // ❯ プロンプト
    r"(?i)tool use",
    r"(?i)\bBash\b.*\bRead\b",
    r"(?i)anthropic",
];

/// Pod 内の tmux セッションから新しい member を検出する
pub fn discover_new_members(pod: &Pod) -> Vec<Member> {
    let panes = match Tmux::list_panes(&pod.tmux_session) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let known_panes: std::collections::HashSet<&str> = pod.members.iter()
        .map(|m| m.tmux_pane.as_str())
        .collect();

    let mut new_members = Vec::new();

    for pane in &panes {
        if known_panes.contains(pane.id.as_str()) {
            continue;
        }

        // capture-pane で出力を確認
        let output = match Tmux::capture_pane(&pane.id) {
            Ok(o) => o,
            Err(_) => continue,
        };

        // Claude Code が動いているペインかチェック
        if !is_claude_code_pane(&output) {
            continue;
        }

        let role = detect_role_name(&output, new_members.len() + pod.members.len());

        new_members.push(Member {
            role,
            status: MemberStatus::Working,
            tmux_pane: pane.id.clone(),
            last_change: Utc::now(),
            last_output: output,
            last_polled: None,
            working_secs: 0,
        });
    }

    new_members
}

/// capture-pane 出力から Claude Code が動いているペインかどうか判定
pub fn is_claude_code_pane(output: &str) -> bool {
    if output.trim().is_empty() {
        return false;
    }

    let mut match_count = 0;
    for pattern in CLAUDE_CODE_PATTERNS {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(output) {
                match_count += 1;
            }
        }
    }

    // 1つ以上のパターンにマッチすれば Claude Code と判定
    match_count >= 1
}

/// capture-pane 出力から役割名を推定
pub fn detect_role_name(output: &str, fallback_index: usize) -> String {
    // Lead / team-lead パターン
    let lead_re = Regex::new(r"(?i)\b(lead|team.?lead|leader)\b").ok();
    if let Some(re) = &lead_re {
        if re.is_match(output) {
            return "lead".to_string();
        }
    }

    // teammate 名パターン: "name: xxx" 形式のヘッダーを探す
    let name_patterns = [
        r"(?i)(?:agent|teammate|worker)\s*(?:name)?[:\s]+([a-zA-Z][a-zA-Z0-9_-]+)",
        r"(?i)I am\s+([a-zA-Z][a-zA-Z0-9_-]+)",
    ];

    for pattern in &name_patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(caps) = re.captures(output) {
                if let Some(name) = caps.get(1) {
                    let name_str = name.as_str();
                    // 一般的な単語を除外
                    if !["the", "a", "an", "this", "that", "claude", "code"].contains(&name_str.to_lowercase().as_str()) {
                        return name_str.to_string();
                    }
                }
            }
        }
    }

    // フォールバック
    format!("member-{}", fallback_index)
}

/// Pod から消えたペインを検出して member を除外
pub fn remove_stale_members(pod: &mut Pod) {
    let panes = match Tmux::list_panes(&pod.tmux_session) {
        Ok(p) => p,
        Err(_) => return,
    };

    let active_pane_ids: std::collections::HashSet<String> = panes.iter()
        .map(|p| p.id.clone())
        .collect();

    pod.members.retain(|m| active_pane_ids.contains(&m.tmux_pane));
}
