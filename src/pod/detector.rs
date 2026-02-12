use regex::Regex;

use crate::pod::MemberStatus;
use crate::pod::SubAgent;

/// 許可リクエストの詳細
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub tool: String,
    pub command: String,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// パターン定義 (将来的に設定ファイルへ外部化可能)
// ---------------------------------------------------------------------------

/// Permission 検出パターン
const PERMISSION_PATTERNS: &[&str] = &[
    r"(?i)allow.*\(y/n\)",
    r"(?i)allow.*\by\b.*\bn\b",
    r"(?i)\bapprove\b.*\bdeny\b",
    r"(?i)do you want to\b",
    r"(?i)permission requested",
    r"(?i)allow\s+(once|always)",
];

/// Error 検出パターン
const ERROR_PATTERNS: &[&str] = &[
    r"(?m)^.*\bError:.*$",
    r"(?m)^.*\berror:.*$",
    r"(?i)\bfailed\b",
    r"(?i)\bpanic\b",
    r"(?i)thread\s+'.*'\s+panicked",
];

/// Done 検出パターン
const DONE_PATTERNS: &[&str] = &[
    r"(?i)session ended",
    r"(?i)process exited",
    r"(?i)connection closed",
];

/// Idle 検出パターン (プロンプト待ち)
const IDLE_PATTERNS: &[&str] = &[
    r"^\s*[\u{276f}\u{2771}>]\s*$",  // ❯ or ❱ or >
    r"^\s*\$\s*$",                    // bare $ prompt
    r"^\s*%\s*$",                     // bare % prompt (zsh)
];

/// ツール名検出パターン
const TOOL_PATTERNS: &[&str] = &[
    r"(?i)\b(bash|write|read|edit|grep|glob|search|notebook)\b",
];

// ---------------------------------------------------------------------------
// 検出関数
// ---------------------------------------------------------------------------

/// カスタムパターン付きで capture-pane の出力からメンバーの状態を検出する。
///
/// `extra_*` にユーザー定義の追加パターンを渡すと、組み込みパターンに加えて検出に使用される。
pub fn detect_member_status_with_config(
    output: &str,
    extra_permission: &[String],
    extra_error: &[String],
    extra_idle: &[String],
) -> MemberStatus {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return MemberStatus::Done;
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let tail_start = if lines.len() > 15 { lines.len() - 15 } else { 0 };
    let tail = &lines[tail_start..];
    let tail_text = tail.join("\n");

    // 1. Permission 検出 (最優先)
    if matches_any(&tail_text, PERMISSION_PATTERNS) || matches_any_dynamic(&tail_text, extra_permission) {
        return MemberStatus::Permission;
    }

    // 2. Error 検出
    if matches_any(&tail_text, ERROR_PATTERNS) || matches_any_dynamic(&tail_text, extra_error) {
        return MemberStatus::Error;
    }

    // 3. Done 検出
    if matches_any(&tail_text, DONE_PATTERNS) {
        return MemberStatus::Done;
    }

    // 4. Idle 検出 (最終行がプロンプト)
    if let Some(last) = tail.last() {
        if matches_any(last, IDLE_PATTERNS) || matches_any_dynamic(last, extra_idle) {
            return MemberStatus::Idle;
        }
    }

    // 5. デフォルト: Working
    MemberStatus::Working
}

/// capture-pane の出力からメンバーの状態を検出する。
///
/// 検出優先度:
///   1. Permission (最優先) -- 許可プロンプトが表示されている
///   2. Error              -- エラーメッセージが出ている
///   3. Done               -- 空出力やセッション終了
///   4. Idle               -- プロンプト待ち状態
///   5. Working (デフォルト)
pub fn detect_member_status(output: &str) -> MemberStatus {
    // 空出力 = プロセスが終了している可能性が高い
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return MemberStatus::Done;
    }

    // 最後の数行を重点的に見る (最新の状態を反映)
    let lines: Vec<&str> = trimmed.lines().collect();
    let tail_start = if lines.len() > 15 { lines.len() - 15 } else { 0 };
    let tail = &lines[tail_start..];
    let tail_text = tail.join("\n");

    // --- 1. Permission 検出 (最優先) ---
    if matches_any(&tail_text, PERMISSION_PATTERNS) {
        return MemberStatus::Permission;
    }

    // --- 2. Error 検出 ---
    if matches_any(&tail_text, ERROR_PATTERNS) {
        return MemberStatus::Error;
    }

    // --- 3. Done 検出 ---
    if matches_any(&tail_text, DONE_PATTERNS) {
        return MemberStatus::Done;
    }

    // --- 4. Idle 検出 (最終行がプロンプト) ---
    if let Some(last) = tail.last() {
        if matches_any(last, IDLE_PATTERNS) {
            return MemberStatus::Idle;
        }
    }

    // --- 5. デフォルト: Working ---
    MemberStatus::Working
}

/// capture-pane 出力から許可リクエストの内容をパースする。
///
/// ツール名、コマンド内容、詳細テキストを抽出する。
/// 検出できない場合は `None` を返す。
pub fn parse_permission_request(output: &str) -> Option<PermissionRequest> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }

    // まず Permission 状態かどうかチェック
    let lines: Vec<&str> = trimmed.lines().collect();
    let tail_start = if lines.len() > 20 { lines.len() - 20 } else { 0 };
    let tail = &lines[tail_start..];
    let tail_text = tail.join("\n");

    if !matches_any(&tail_text, PERMISSION_PATTERNS) {
        return None;
    }

    // ツール名を検出
    let tool = extract_first_match(&tail_text, TOOL_PATTERNS)
        .unwrap_or_else(|| "unknown".to_string());

    // コマンド内容を検出 (コードブロック ``` ... ``` 内のテキスト)
    let command = extract_code_block(&tail_text)
        .unwrap_or_default();

    // 詳細テキスト: Permission 行を含むコンテキスト
    let detail = tail_text.clone();

    Some(PermissionRequest {
        tool,
        command,
        detail,
    })
}

/// Pod の member 状態からロールアップ状態を計算する。
///
/// 最も優先度が高い状態を返す。空の場合は `Idle`。
pub fn rollup_status(statuses: &[MemberStatus]) -> MemberStatus {
    if statuses.is_empty() {
        return MemberStatus::Idle;
    }

    statuses
        .iter()
        .max_by_key(|s| s.priority())
        .cloned()
        .unwrap_or(MemberStatus::Idle)
}

// ---------------------------------------------------------------------------
// Subagent 検出
// ---------------------------------------------------------------------------

/// capture-pane 出力から実行中の Subagent (Task ツール) を検出する。
///
/// Claude Code の実際の表示パターン:
///   * Worked for 54s · 3 agents running in the background
///   ►► accept edits on · 3 local agents · ctrl+t to hide task
///   ● Running 3 Task agents… (ctrl+o to expand)
///     ├─ description · N tool uses · Nk tokens
pub fn parse_sub_agents(output: &str) -> Vec<SubAgent> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let expected_count = parse_sub_agent_count(trimmed);
    if expected_count == 0 {
        return Vec::new();
    }

    let mut agents = Vec::new();

    // 個別エージェントの詳細行を検出
    //   ├─ description · N tool uses · Nk tokens
    //   └─ description · N tool uses · Nk tokens
    let detail_re = Regex::new(r"[├└]─\s*(.+?)(?:\s+·\s+\d+\s+tool\s+uses?)?(?:\s+·\s+[\d.]+k?\s+tokens?)?$").ok();

    if let Some(ref re) = detail_re {
        for line in trimmed.lines() {
            let line = line.trim();
            if let Some(caps) = re.captures(line) {
                if let Some(desc) = caps.get(1) {
                    let description = desc.as_str().trim().to_string();
                    let agent_type = infer_agent_type(&description);
                    agents.push(SubAgent {
                        agent_type,
                        description,
                    });
                }
            }
        }
    }

    // 詳細行が取れなかった場合、expected_count 分のプレースホルダーを作成
    if agents.is_empty() && expected_count > 0 {
        for i in 0..expected_count {
            agents.push(SubAgent {
                agent_type: "Task".to_string(),
                description: format!("agent {}", i + 1),
            });
        }
    }

    agents
}

/// description からエージェントタイプを推定
fn infer_agent_type(description: &str) -> String {
    let lower = description.to_lowercase();
    if lower.contains("explore") || lower.contains("search") || lower.contains("find") {
        "Explore".to_string()
    } else if lower.contains("plan") || lower.contains("design") {
        "Plan".to_string()
    } else if lower.contains("test") || lower.contains("build") {
        "Bash".to_string()
    } else {
        "Task".to_string()
    }
}

/// capture-pane 出力からエージェント数を返す (0 = なし)
///
/// 複数のパターンに対応:
///   - "N agents running in the background"
///   - "N local agents"
///   - "Running N Task agents"
pub fn parse_sub_agent_count(output: &str) -> usize {
    // 複数パターンを試行、最大値を返す
    let patterns = [
        r"(\d+)\s+agents?\s+running\s+in\s+the\s+background",
        r"(\d+)\s+local\s+agents?",
        r"Running\s+(\d+)\s+Task\s+agents?",
        r"Running\s+(\d+)\s+agents?",
    ];

    let mut max_count: usize = 0;
    for pattern in &patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(caps) = re.captures(output) {
                if let Some(m) = caps.get(1) {
                    if let Ok(n) = m.as_str().parse::<usize>() {
                        max_count = max_count.max(n);
                    }
                }
            }
        }
    }
    max_count
}

// ---------------------------------------------------------------------------
// ヘルパー関数
// ---------------------------------------------------------------------------

/// 与えられたテキストが、パターン配列のいずれかにマッチするかチェック
fn matches_any(text: &str, patterns: &[&str]) -> bool {
    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(text) {
                return true;
            }
        }
    }
    false
}

/// 動的パターン (String の Vec) に対してマッチチェック
fn matches_any_dynamic(text: &str, patterns: &[String]) -> bool {
    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            if re.is_match(text) {
                return true;
            }
        }
    }
    false
}

/// パターン配列の最初にマッチしたキャプチャグループ (group 1) を返す
fn extract_first_match(text: &str, patterns: &[&str]) -> Option<String> {
    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(caps) = re.captures(text) {
                if let Some(m) = caps.get(1) {
                    return Some(m.as_str().to_string());
                }
            }
        }
    }
    None
}

/// テキストからコードブロック (``` ... ```) の中身を抽出する
fn extract_code_block(text: &str) -> Option<String> {
    let re = Regex::new(r"(?s)```[^\n]*\n(.*?)```").ok()?;
    re.captures(text)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
}

// ---------------------------------------------------------------------------
// テスト
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_output_is_done() {
        assert_eq!(detect_member_status(""), MemberStatus::Done);
        assert_eq!(detect_member_status("   \n  \n  "), MemberStatus::Done);
    }

    #[test]
    fn test_permission_allow_yn() {
        let output = "Some output\nAllow this action? (y/n)";
        assert_eq!(detect_member_status(output), MemberStatus::Permission);
    }

    #[test]
    fn test_permission_approve_deny() {
        let output = "Tool wants to run bash\napprove / deny";
        assert_eq!(detect_member_status(output), MemberStatus::Permission);
    }

    #[test]
    fn test_permission_do_you_want_to() {
        let output = "Do you want to allow this tool?";
        assert_eq!(detect_member_status(output), MemberStatus::Permission);
    }

    #[test]
    fn test_permission_allow_once_always() {
        let output = "Some text\nAllow once | Allow always";
        assert_eq!(detect_member_status(output), MemberStatus::Permission);
    }

    #[test]
    fn test_error_detection() {
        assert_eq!(
            detect_member_status("Compiling...\nError: something went wrong"),
            MemberStatus::Error
        );
        assert_eq!(
            detect_member_status("Build failed"),
            MemberStatus::Error
        );
        assert_eq!(
            detect_member_status("thread 'main' panicked at ..."),
            MemberStatus::Error
        );
    }

    #[test]
    fn test_done_session_ended() {
        let output = "Working...\nSession ended";
        assert_eq!(detect_member_status(output), MemberStatus::Done);
    }

    #[test]
    fn test_idle_prompt() {
        assert_eq!(detect_member_status("some output\n\u{276f}"), MemberStatus::Idle);
        assert_eq!(detect_member_status("some output\n$"), MemberStatus::Idle);
        assert_eq!(detect_member_status("some output\n  $ "), MemberStatus::Idle);
    }

    #[test]
    fn test_working_default() {
        let output = "Compiling project...\n[=====>    ] 50%";
        assert_eq!(detect_member_status(output), MemberStatus::Working);
    }

    #[test]
    fn test_permission_has_priority_over_error() {
        // Permission が Error より優先
        let output = "Error: something\nAllow this? (y/n)";
        assert_eq!(detect_member_status(output), MemberStatus::Permission);
    }

    #[test]
    fn test_parse_permission_request_basic() {
        let output = "Tool: bash\n```\nrm -rf /tmp/test\n```\nAllow this action? (y/n)";
        let req = parse_permission_request(output).unwrap();
        assert_eq!(req.tool, "bash");
        assert_eq!(req.command, "rm -rf /tmp/test");
    }

    #[test]
    fn test_parse_permission_request_no_permission() {
        let output = "Just some regular output";
        assert!(parse_permission_request(output).is_none());
    }

    #[test]
    fn test_parse_permission_request_unknown_tool() {
        let output = "Some tool wants access\nAllow once | Allow always";
        let req = parse_permission_request(output).unwrap();
        assert_eq!(req.tool, "unknown");
    }

    #[test]
    fn test_rollup_empty() {
        assert_eq!(rollup_status(&[]), MemberStatus::Idle);
    }

    #[test]
    fn test_rollup_single() {
        assert_eq!(
            rollup_status(&[MemberStatus::Working]),
            MemberStatus::Working
        );
    }

    #[test]
    fn test_rollup_permission_wins() {
        let statuses = vec![
            MemberStatus::Idle,
            MemberStatus::Working,
            MemberStatus::Permission,
            MemberStatus::Error,
        ];
        assert_eq!(rollup_status(&statuses), MemberStatus::Permission);
    }

    #[test]
    fn test_rollup_error_over_working() {
        let statuses = vec![
            MemberStatus::Working,
            MemberStatus::Error,
            MemberStatus::Idle,
        ];
        assert_eq!(rollup_status(&statuses), MemberStatus::Error);
    }

    #[test]
    fn test_rollup_all_done() {
        let statuses = vec![MemberStatus::Done, MemberStatus::Done];
        assert_eq!(rollup_status(&statuses), MemberStatus::Done);
    }

    #[test]
    fn test_working_with_spinner() {
        // スピナー文字が表示されている場合は Working
        let output = "\u{280b} Running tests...";
        assert_eq!(detect_member_status(output), MemberStatus::Working);
    }

    #[test]
    fn test_long_output_uses_tail() {
        // 長い出力の場合、末尾の Permission が検出される
        let mut output = String::new();
        for i in 0..100 {
            output.push_str(&format!("Line {}\n", i));
        }
        output.push_str("Allow this? (y/n)");
        assert_eq!(detect_member_status(&output), MemberStatus::Permission);
    }

    // -----------------------------------------------------------------------
    // Subagent 検出テスト
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_sub_agents_empty() {
        assert!(parse_sub_agents("").is_empty());
        assert!(parse_sub_agents("some regular output").is_empty());
    }

    #[test]
    fn test_parse_sub_agents_running_count_only() {
        let output = "● Running 3 Task agents… (ctrl+o to expand)";
        let agents = parse_sub_agents(output);
        assert_eq!(agents.len(), 3);
        assert_eq!(agents[0].agent_type, "Task");
    }

    #[test]
    fn test_parse_sub_agents_with_details() {
        let output = r#"● Running 2 Task agents… (ctrl+o to expand)
  ├─ Explore codebase structure · 5 tool uses · 12k tokens
  └─ Search for test files · 3 tool uses · 8k tokens"#;
        let agents = parse_sub_agents(output);
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].agent_type, "Explore");
        assert!(agents[0].description.contains("Explore codebase"));
        assert_eq!(agents[1].agent_type, "Explore"); // "Search" triggers Explore
    }

    #[test]
    fn test_parse_sub_agent_count() {
        assert_eq!(parse_sub_agent_count("Running 3 Task agents"), 3);
        assert_eq!(parse_sub_agent_count("Running 1 Task agent"), 1);
        assert_eq!(parse_sub_agent_count("no agents here"), 0);
        assert_eq!(parse_sub_agent_count(""), 0);
    }

    #[test]
    fn test_parse_sub_agent_count_real_formats() {
        // 実際の Claude Code 出力フォーマット
        assert_eq!(
            parse_sub_agent_count("* Worked for 54s · 3 agents running in the background"),
            3
        );
        assert_eq!(
            parse_sub_agent_count("►► accept edits on · 3 local agents · ctrl+t to hide task"),
            3
        );
    }

    #[test]
    fn test_parse_sub_agents_real_background() {
        let output = "some output\n* Worked for 54s · 3 agents running in the background\n1 tasks (0 done, 1 in progress)";
        let agents = parse_sub_agents(output);
        assert_eq!(agents.len(), 3);
    }

    #[test]
    fn test_parse_sub_agents_real_local() {
        let output = "►► accept edits on · 2 local agents · ctrl+t to hide task";
        let agents = parse_sub_agents(output);
        assert_eq!(agents.len(), 2);
    }
}
