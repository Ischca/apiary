use crate::pod::{Member, MemberStatus, Pod, PodStatus, PodType};
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
    // Agent Teams teammate pane: Claude Code ツール名が行頭インデント付きで出現するパターン
    r"(?m)^\s{2}(Read|Write|Edit|Grep|Glob|Bash|Task)\s",
];

/// Pod 内の tmux セッションから新しい member を検出する
/// all_pods: 同じ session を共有する全 Pod の pane を重複検出しないために使用
pub fn discover_new_members(pod: &Pod, all_pods: &[Pod]) -> Vec<Member> {
    let panes = match Tmux::list_panes(&pod.tmux_session) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    // 同じ tmux session を共有する全 Pod の pane を known として収集
    let known_panes: std::collections::HashSet<&str> = all_pods.iter()
        .filter(|p| p.tmux_session == pod.tmux_session)
        .flat_map(|p| p.members.iter())
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
            last_output_ansi: String::new(),
            pane_size: (80, 24),
            last_polled: None,
            working_secs: 0,
            sub_agents: Vec::new(),
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
    // Agent Teams @name パターン: "@main @reader-detector @reader-main" のような行から抽出
    // 最初の @name を採用 (leader pane に表示される)
    if let Ok(re) = Regex::new(r"@([a-zA-Z][a-zA-Z0-9_-]*)") {
        if let Some(caps) = re.captures(output) {
            if let Some(name) = caps.get(1) {
                return name.as_str().to_string();
            }
        }
    }

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

/// Leader pane 出力から Agent Teams メンバーの @name リストを抽出
pub fn extract_teammate_names(output: &str) -> Vec<String> {
    let re = Regex::new(r"@([a-zA-Z][a-zA-Z0-9_-]*)").unwrap();
    re.captures_iter(output)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
        .collect()
}

/// 検出された Member から子 Pod を作成し、親 Pod の group を設定する
///
/// - 各 Member に対して `{parent.name}/{member.role}` の子 Pod を生成
/// - 親 Pod の group が None なら `parent.name` で初期化
/// - 子 Pod の group は親の group を継承
pub fn create_child_pods(parent: &mut Pod, discovered: Vec<Member>) -> Vec<Pod> {
    if discovered.is_empty() {
        return Vec::new();
    }

    let group_name = parent.group.clone().unwrap_or_else(|| parent.name.clone());

    // 親 Pod にグループを設定（初回のみ）
    if parent.group.is_none() {
        parent.group = Some(parent.name.clone());
    }

    discovered
        .into_iter()
        .map(|member| {
            let child_name = format!("{}/{}", parent.name, member.role);
            Pod {
                name: child_name,
                pod_type: PodType::Solo,
                tmux_session: parent.tmux_session.clone(),
                project: parent.project.clone(),
                group: Some(group_name.clone()),
                status: PodStatus::Idle,
                members: vec![member],
                created_at: Utc::now(),
                total_working_secs: 0,
            }
        })
        .collect()
}

/// member が 0 の子 Pod を除去する
///
/// - group が Some で、同じ group に他の Pod が存在する子 Pod のみ対象
/// - 親 Pod（group 名と Pod 名が一致）は除去しない
/// - group が None の空 Pod は手動作成の可能性があるため除去しない
pub fn remove_orphan_child_pods(pods: &mut Vec<Pod>) {
    // group ごとに Pod 名を収集して、親 Pod を特定
    let parent_names: std::collections::HashSet<String> = pods
        .iter()
        .filter_map(|p| {
            p.group.as_ref().and_then(|g| {
                if g == &p.name { Some(p.name.clone()) } else { None }
            })
        })
        .collect();

    pods.retain(|pod| {
        // member が残っている → 残す
        if !pod.members.is_empty() {
            return true;
        }

        // group が None → 手動作成 Pod なので残す
        let group = match &pod.group {
            Some(g) => g,
            None => return true,
        };

        // 親 Pod → 残す
        if &pod.name == group || parent_names.contains(&pod.name) {
            return true;
        }

        // 同じ group の他の Pod が存在する場合のみ「孤立子」として除去
        // (group が存在するが他に誰もいない → 最後の子なので除去)
        false
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pod::{PodStatus, PodType};

    /// テスト用 Member を作成するヘルパー
    fn make_member(role: &str, pane: &str) -> Member {
        Member {
            role: role.to_string(),
            status: MemberStatus::Working,
            tmux_pane: pane.to_string(),
            last_change: Utc::now(),
            last_output: String::new(),
            last_output_ansi: String::new(),
            pane_size: (80, 24),
            last_polled: None,
            working_secs: 0,
            sub_agents: Vec::new(),
        }
    }

    /// テスト用 Pod を作成するヘルパー
    fn make_pod(name: &str, session: &str, members: Vec<Member>, group: Option<&str>) -> Pod {
        Pod {
            name: name.to_string(),
            pod_type: if members.len() > 1 { PodType::Team } else { PodType::Solo },
            members,
            status: PodStatus::Working,
            tmux_session: session.to_string(),
            project: Some("my-project".to_string()),
            group: group.map(|s| s.to_string()),
            created_at: Utc::now(),
            total_working_secs: 0,
        }
    }

    // -----------------------------------------------------------------------
    // create_child_pods
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_child_pods_basic() {
        let mut parent = make_pod("auth", "auth-session", vec![make_member("lead", "%0")], None);
        let discovered = vec![
            make_member("reader-detector", "%1"),
            make_member("reader-main", "%2"),
        ];

        let children = create_child_pods(&mut parent, discovered);

        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name, "auth/reader-detector");
        assert_eq!(children[1].name, "auth/reader-main");
        // 子 Pod は親の session を共有
        assert_eq!(children[0].tmux_session, "auth-session");
        assert_eq!(children[1].tmux_session, "auth-session");
        // 子 Pod は親の group を継承
        assert_eq!(children[0].group, Some("auth".to_string()));
        assert_eq!(children[1].group, Some("auth".to_string()));
        // 子 Pod は親の project を継承
        assert_eq!(children[0].project, Some("my-project".to_string()));
        // 各子 Pod に member が1つ
        assert_eq!(children[0].members.len(), 1);
        assert_eq!(children[0].members[0].role, "reader-detector");
    }

    #[test]
    fn test_create_child_pods_sets_parent_group() {
        let mut parent = make_pod("auth", "auth-session", vec![make_member("lead", "%0")], None);
        assert!(parent.group.is_none());

        let discovered = vec![make_member("worker", "%1")];
        let _children = create_child_pods(&mut parent, discovered);

        // 親の group が parent.name に設定される
        assert_eq!(parent.group, Some("auth".to_string()));
    }

    #[test]
    fn test_create_child_pods_preserves_existing_group() {
        let mut parent = make_pod(
            "auth",
            "auth-session",
            vec![make_member("lead", "%0")],
            Some("project-group"),
        );

        let discovered = vec![make_member("worker", "%1")];
        let children = create_child_pods(&mut parent, discovered);

        // 親の既存 group は変わらない
        assert_eq!(parent.group, Some("project-group".to_string()));
        // 子 Pod は親の既存 group を継承
        assert_eq!(children[0].group, Some("project-group".to_string()));
    }

    #[test]
    fn test_create_child_pods_empty_discovery() {
        let mut parent = make_pod("auth", "auth-session", vec![make_member("lead", "%0")], None);

        let children = create_child_pods(&mut parent, Vec::new());

        assert!(children.is_empty());
        // 空の discovery では親の group は変わらない
        assert!(parent.group.is_none());
    }

    // -----------------------------------------------------------------------
    // remove_orphan_child_pods
    // -----------------------------------------------------------------------

    #[test]
    fn test_remove_orphan_child_pods_basic() {
        let mut pods = vec![
            // 親 Pod (member あり)
            make_pod("auth", "auth-session", vec![make_member("lead", "%0")], Some("auth")),
            // 子 Pod (member あり) → 残る
            make_pod("auth/worker", "auth-session", vec![make_member("worker", "%1")], Some("auth")),
            // 子 Pod (member 空) → 除去
            make_pod("auth/reader", "auth-session", Vec::new(), Some("auth")),
        ];

        remove_orphan_child_pods(&mut pods);

        assert_eq!(pods.len(), 2);
        assert_eq!(pods[0].name, "auth");
        assert_eq!(pods[1].name, "auth/worker");
    }

    #[test]
    fn test_remove_orphan_child_pods_keeps_non_empty() {
        let mut pods = vec![
            make_pod("auth", "auth-session", vec![make_member("lead", "%0")], Some("auth")),
            make_pod("auth/worker", "auth-session", vec![make_member("worker", "%1")], Some("auth")),
        ];

        remove_orphan_child_pods(&mut pods);

        // member がある子 Pod は残る
        assert_eq!(pods.len(), 2);
    }

    #[test]
    fn test_remove_orphan_child_pods_keeps_ungrouped() {
        let mut pods = vec![
            // group=None で member 空の Pod → 手動作成なので残す
            make_pod("manual-pod", "manual-session", Vec::new(), None),
            // group ありの親 Pod
            make_pod("auth", "auth-session", vec![make_member("lead", "%0")], Some("auth")),
        ];

        remove_orphan_child_pods(&mut pods);

        assert_eq!(pods.len(), 2);
        assert!(pods.iter().any(|p| p.name == "manual-pod"));
    }

    #[test]
    fn test_remove_orphan_child_pods_keeps_parent_even_if_empty() {
        // 親 Pod の member が空になっても、group 名と一致するなら残す
        let mut pods = vec![
            make_pod("auth", "auth-session", Vec::new(), Some("auth")),
            make_pod("auth/worker", "auth-session", vec![make_member("worker", "%1")], Some("auth")),
        ];

        remove_orphan_child_pods(&mut pods);

        assert_eq!(pods.len(), 2);
        assert!(pods.iter().any(|p| p.name == "auth"));
    }

    // -----------------------------------------------------------------------
    // is_claude_code_pane — Agent Teams patterns
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_claude_code_pane_agent_teams_teammate() {
        // v5 テストで確認: teammate pane は Claude Code TUI を表示
        // "esc to interrupt" と "/ide for Visual Studio Code" が表示される
        let output = concat!(
            "─────────────────────────────────────────────\n",
            "esc to interrupt                                 ◯ /ide for Visual Studio Code\n",
            "\n",
            "I'll read the detector module to understand the current implementation.\n",
            "\n",
            "  Read src/pod/detector.rs\n",
            "\n",
            "● Reading file...\n",
        );
        // 出力に Claude 関連パターンがないため false — teammate pane は
        // 必ずしも "claude" を含まない場合がある
        // ただし "Read" や tool use 系パターンが含まれていれば true
        let result = is_claude_code_pane(output);
        // "Read" が含まれているので少なくとも1パターンマッチ
        assert!(result, "Teammate pane with tool usage should be detected as Claude Code pane");
    }

    #[test]
    fn test_is_claude_code_pane_agent_teams_leader() {
        // v5 テストで確認: leader pane には @name 表示と Claude 表示がある
        let output = concat!(
            "  @main @reader-detector @reader-main · shift+↑ to expand\n",
            "  2 teammates · esc to interrupt · ctrl+t to show teammates\n",
            "\n",
            "Claude is thinking...\n",
        );
        assert!(is_claude_code_pane(output), "Leader pane should be detected as Claude Code pane");
    }

    #[test]
    fn test_is_claude_code_pane_empty_pane() {
        assert!(!is_claude_code_pane(""), "Empty output should not be Claude Code pane");
        assert!(!is_claude_code_pane("   \n  \n  "), "Whitespace-only output should not be Claude Code pane");
    }

    #[test]
    fn test_is_claude_code_pane_plain_shell() {
        let output = "shota@mac ~ % ls\nDesktop  Documents  Downloads\nshota@mac ~ %";
        assert!(!is_claude_code_pane(output), "Plain shell should not be detected as Claude Code pane");
    }

    #[test]
    fn test_is_claude_code_pane_teammate_esc_to_interrupt() {
        // teammate pane が Claude Code の tool use を表示していない初期状態
        // "esc to interrupt" のみ — これだけでは Claude パターンに一致しない
        let output = "─────────────────────────\nesc to interrupt                 ◯ /ide for Visual Studio Code\n";
        // 現在のパターンでは "claude", "❯", "tool use", "Bash.*Read", "anthropic" のいずれにもマッチしない
        let result = is_claude_code_pane(output);
        // これは現在 false — Agent Teams のパターン拡張候補
        assert!(!result, "Bare 'esc to interrupt' without Claude patterns should not match (yet)");
    }

    // -----------------------------------------------------------------------
    // detect_role_name — @name pattern from Agent Teams
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_role_name_at_prefix() {
        // v5: leader pane に "@main @reader-detector @reader-main" が表示される
        let output = "  @main @reader-detector @reader-main · shift+↑ to expand\n\
                       2 teammates · esc to interrupt · ctrl+t to show teammates\n";
        let role = detect_role_name(output, 0);
        // @name パターンから最初の名前を抽出すべき
        // 現在の実装では対応していないので RED → Phase 3 で GREEN にする
        assert!(
            role == "main" || role == "reader-detector" || role == "reader-main",
            "Expected @name extraction, got: {}",
            role
        );
    }

    #[test]
    fn test_detect_role_name_teammate_self_output() {
        // teammate pane 自身の出力: Claude Code の典型的な working 出力
        let output = "I'll read the detector module.\n\n  Read src/pod/detector.rs\n\n● Reading file...\n";
        let role = detect_role_name(output, 2);
        // 特定の名前パターンがないのでフォールバック
        assert_eq!(role, "member-2");
    }

    #[test]
    fn test_detect_role_name_fallback() {
        let output = "some random output without any name patterns\n";
        assert_eq!(detect_role_name(output, 5), "member-5");
    }

    #[test]
    fn test_detect_role_name_existing_lead_pattern() {
        let output = "I am the team lead for this project.\nWorking on...\n";
        assert_eq!(detect_role_name(output, 0), "lead");
    }

    // -----------------------------------------------------------------------
    // extract_teammate_names — @name list extraction
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_teammate_names_leader_output() {
        let output = "  @main @reader-detector @reader-main · shift+↑ to expand\n";
        let names = extract_teammate_names(output);
        assert_eq!(names, vec!["main", "reader-detector", "reader-main"]);
    }

    #[test]
    fn test_extract_teammate_names_no_at_names() {
        let output = "Just some normal output without @ patterns\n";
        let names = extract_teammate_names(output);
        assert!(names.is_empty());
    }

    #[test]
    fn test_extract_teammate_names_single() {
        let output = "  @worker · working\n";
        let names = extract_teammate_names(output);
        assert_eq!(names, vec!["worker"]);
    }
}
