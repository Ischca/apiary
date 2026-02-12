//! 統合テスト: Agent Teams の teammate pane 検出・除去・ライフサイクル
//!
//! 実際の tmux を使って split-pane → discover → stale removal を検証。
//! CI 環境では `#[ignore]` で skip。手元では:
//!   cargo test --test agent_teams_discovery -- --ignored --nocapture

use apiary::pod::discovery::{
    create_child_pods, discover_new_members, extract_teammate_names,
    remove_orphan_child_pods, remove_stale_members,
};
use apiary::pod::{Member, MemberStatus, Pod, PodStatus, PodType};
use chrono::Utc;
use std::process::Command;

/// tmux が利用可能かチェック
fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// ユニークなセッション名を生成
fn unique_session(tag: &str) -> String {
    format!("apiary-test-at-{}-{}", std::process::id(), tag)
}

/// テスト用 tmux セッションを作成
fn create_session(name: &str) -> bool {
    Command::new("tmux")
        .args(["new-session", "-d", "-s", name, "-x", "120", "-y", "40"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// セッション内で split-window して新しい pane を追加
fn split_window(session: &str) -> Option<String> {
    let output = Command::new("tmux")
        .args([
            "split-window", "-t", session, "-h",
            "-P", "-F", "#{pane_id}",
        ])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// pane に Claude Code 風テキストを表示
fn inject_claude_output(pane_id: &str, text: &str) {
    let escaped = text.replace('\\', "\\\\").replace('\'', "'\\''");
    let cmd = format!("printf '{}'", escaped);
    let _ = Command::new("tmux")
        .args(["send-keys", "-t", pane_id, &cmd, "Enter"])
        .status();
    std::thread::sleep(std::time::Duration::from_millis(300));
}

/// セッションの全 pane ID を取得
fn list_pane_ids(session: &str) -> Vec<String> {
    let output = Command::new("tmux")
        .args(["list-panes", "-t", session, "-s", "-F", "#{pane_id}"])
        .output()
        .unwrap_or_else(|_| panic!("Failed to list panes for {}", session));
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// セッション削除 (cleanup)
fn kill_session(name: &str) {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", name])
        .status();
}

/// テスト用 Pod を作成
fn make_test_pod(name: &str, session: &str, pane_id: &str) -> Pod {
    Pod {
        name: name.to_string(),
        pod_type: PodType::Solo,
        members: vec![Member {
            role: "lead".to_string(),
            status: MemberStatus::Working,
            tmux_pane: pane_id.to_string(),
            last_change: Utc::now(),
            last_output: String::new(),
            last_output_ansi: String::new(),
            pane_size: (80, 24),
            last_polled: None,
            working_secs: 0,
            sub_agents: Vec::new(),
        }],
        status: PodStatus::Working,
        tmux_session: session.to_string(),
        project: None,
        group: None,
        created_at: Utc::now(),
        total_working_secs: 0,
    }
}

// -----------------------------------------------------------------------
// テストケース
// -----------------------------------------------------------------------

/// 2-1: teammate pane の検出
///
/// 1 leader + 2 teammate pane を持つ tmux session を作成し、
/// discover_new_members() が新 pane を検出することを確認
#[test]
#[ignore]
fn test_discover_agent_teams_teammates() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }

    let session = unique_session("discover");
    assert!(create_session(&session), "Failed to create session");

    // leader pane ID を取得
    let initial_panes = list_pane_ids(&session);
    assert_eq!(initial_panes.len(), 1);
    let leader_pane = &initial_panes[0];

    // leader に Claude 出力を注入
    inject_claude_output(
        leader_pane,
        "  @main @reader-detector @reader-main \\u00b7 shift+\\u2191 to expand\\n\
         2 teammates \\u00b7 esc to interrupt \\u00b7 ctrl+t to show teammates\\n\
         Claude is thinking...\\n",
    );

    // teammate pane を2つ追加 (split-window)
    let teammate1 = split_window(&session).expect("Failed to split for teammate 1");
    let teammate2 = split_window(&session).expect("Failed to split for teammate 2");

    // teammate に Claude Code 風出力を注入
    inject_claude_output(
        &teammate1,
        "I will read the detector module.\\n\\n  Read src/pod/detector.rs\\n\\nClaude is working...\\n",
    );
    inject_claude_output(
        &teammate2,
        "Searching for test files.\\n\\n  Bash  find . -name '*_test.rs'\\n  Read src/store/mod.rs\\n\\nClaude code running...\\n",
    );

    // Pod 作成 (leader のみ登録済み)
    let pod = make_test_pod("test-pod", &session, leader_pane);
    let all_pods = vec![pod.clone()];

    // discover
    let new_members = discover_new_members(&pod, &all_pods);

    eprintln!("Discovered {} new members:", new_members.len());
    for m in &new_members {
        eprintln!("  role={}, pane={}", m.role, m.tmux_pane);
    }

    // teammate 2つが検出されるはず
    assert!(
        new_members.len() >= 2,
        "Expected at least 2 new members, got {}",
        new_members.len()
    );

    // 検出された pane が teammate1, teammate2 であること
    let discovered_panes: Vec<&str> = new_members.iter().map(|m| m.tmux_pane.as_str()).collect();
    assert!(
        discovered_panes.contains(&teammate1.as_str()),
        "Teammate 1 ({}) not discovered in {:?}",
        teammate1,
        discovered_panes
    );
    assert!(
        discovered_panes.contains(&teammate2.as_str()),
        "Teammate 2 ({}) not discovered in {:?}",
        teammate2,
        discovered_panes
    );

    // cleanup
    kill_session(&session);
}

/// 2-2: stale teammate の除去
///
/// 3 pane session から teammate pane を kill → remove_stale_members() で除去確認
#[test]
#[ignore]
fn test_remove_stale_agent_teams_members() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }

    let session = unique_session("stale");
    assert!(create_session(&session), "Failed to create session");

    let initial_panes = list_pane_ids(&session);
    let leader_pane = initial_panes[0].clone();

    // teammate pane を2つ追加
    let teammate1 = split_window(&session).expect("split 1");
    let teammate2 = split_window(&session).expect("split 2");

    // Pod に 3 member 全て登録
    let mut pod = Pod {
        name: "test-stale".to_string(),
        pod_type: PodType::Team,
        members: vec![
            Member {
                role: "lead".to_string(),
                status: MemberStatus::Working,
                tmux_pane: leader_pane.clone(),
                last_change: Utc::now(),
                last_output: String::new(),
                last_output_ansi: String::new(),
                pane_size: (80, 24),
                last_polled: None,
                working_secs: 0,
                sub_agents: Vec::new(),
            },
            Member {
                role: "reader-detector".to_string(),
                status: MemberStatus::Working,
                tmux_pane: teammate1.clone(),
                last_change: Utc::now(),
                last_output: String::new(),
                last_output_ansi: String::new(),
                pane_size: (80, 24),
                last_polled: None,
                working_secs: 0,
                sub_agents: Vec::new(),
            },
            Member {
                role: "reader-main".to_string(),
                status: MemberStatus::Working,
                tmux_pane: teammate2.clone(),
                last_change: Utc::now(),
                last_output: String::new(),
                last_output_ansi: String::new(),
                pane_size: (80, 24),
                last_polled: None,
                working_secs: 0,
                sub_agents: Vec::new(),
            },
        ],
        status: PodStatus::Working,
        tmux_session: session.clone(),
        project: None,
        group: None,
        created_at: Utc::now(),
        total_working_secs: 0,
    };

    assert_eq!(pod.members.len(), 3);

    // teammate pane を kill (Agent Teams 完了をシミュレート)
    let _ = Command::new("tmux")
        .args(["kill-pane", "-t", &teammate1])
        .status();
    let _ = Command::new("tmux")
        .args(["kill-pane", "-t", &teammate2])
        .status();
    std::thread::sleep(std::time::Duration::from_millis(200));

    // stale removal
    remove_stale_members(&mut pod);

    eprintln!("After stale removal: {} members", pod.members.len());
    for m in &pod.members {
        eprintln!("  role={}, pane={}", m.role, m.tmux_pane);
    }

    // leader のみ残る
    assert_eq!(pod.members.len(), 1, "Expected 1 member after stale removal");
    assert_eq!(pod.members[0].role, "lead");
    assert_eq!(pod.members[0].tmux_pane, leader_pane);

    kill_session(&session);
}

/// 2-3: extract_teammate_names からの子 Pod 作成ロジック検証
///
/// leader の @name 出力から teammate 名を抽出し、子 Pod を構築できることを確認
#[test]
#[ignore]
fn test_child_pod_creation_from_teammates() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }

    let session = unique_session("child");
    assert!(create_session(&session), "Failed to create session");

    let panes = list_pane_ids(&session);
    let leader_pane = &panes[0];

    // leader に @name 出力を注入
    inject_claude_output(
        leader_pane,
        "@main @reader-detector @reader-main \\u00b7 shift+up to expand\\n",
    );
    std::thread::sleep(std::time::Duration::from_millis(300));

    // capture して names を抽出
    let captured = Command::new("tmux")
        .args(["capture-pane", "-p", "-t", leader_pane])
        .output()
        .expect("capture failed");
    let output = String::from_utf8_lossy(&captured.stdout);
    eprintln!("Captured leader output:\n{}", output);

    let names = extract_teammate_names(&output);
    eprintln!("Extracted names: {:?}", names);

    assert!(
        names.contains(&"main".to_string()),
        "Expected 'main' in names: {:?}",
        names
    );

    // 子 Pod 構築シミュレーション
    let parent_name = "my-project";
    for name in &names {
        let child_name = format!("{}/{}", parent_name, name);
        let child_pod = Pod {
            name: child_name.clone(),
            pod_type: PodType::Solo,
            members: Vec::new(),
            status: PodStatus::Working,
            tmux_session: session.clone(),
            project: None,
            group: Some(parent_name.to_string()),
            created_at: Utc::now(),
            total_working_secs: 0,
        };
        assert_eq!(child_pod.group, Some(parent_name.to_string()));
        assert_eq!(child_pod.tmux_session, session);
        eprintln!("Created child pod: {}", child_name);
    }

    kill_session(&session);
}

/// 2-4: フルライフサイクル
///
/// session 作成 → pane 追加 → discover → pane 削除 → stale removal
#[test]
#[ignore]
fn test_agent_teams_full_lifecycle() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }

    let session = unique_session("lifecycle");
    assert!(create_session(&session), "Failed to create session");

    let initial_panes = list_pane_ids(&session);
    let leader_pane = initial_panes[0].clone();

    // leader に Claude 出力を注入
    inject_claude_output(
        &leader_pane,
        "Claude is working...\\nI will analyze the codebase.\\n",
    );

    // Phase 1: Solo — leader のみ
    let mut pod = make_test_pod("lifecycle-test", &session, &leader_pane);
    let all_pods = vec![pod.clone()];
    let new = discover_new_members(&pod, &all_pods);
    assert_eq!(new.len(), 0, "No new members expected in solo phase");
    eprintln!("Phase 1 (Solo): {} members", pod.members.len());

    // Phase 2: Team — teammate 追加
    let teammate1 = split_window(&session).expect("split 1");
    let teammate2 = split_window(&session).expect("split 2");

    inject_claude_output(
        &teammate1,
        "Claude is searching codebase...\\n  Read src/main.rs\\n",
    );
    inject_claude_output(
        &teammate2,
        "Claude is checking tests...\\n  Bash cargo test\\n  Read Cargo.toml\\n",
    );
    // printf が実行されるまで追加で待機
    std::thread::sleep(std::time::Duration::from_millis(500));

    let all_pods = vec![pod.clone()];
    let new = discover_new_members(&pod, &all_pods);
    eprintln!("Phase 2 (Team): discovered {} new members", new.len());
    for m in &new {
        eprintln!("  role={}, pane={}", m.role, m.tmux_pane);
        pod.add_member(m.clone());
    }

    assert!(pod.members.len() >= 3, "Expected at least 3 members after teammate join");
    pod.pod_type = PodType::Team;

    // Phase 3: teammate pane 消滅 (Agent Teams 完了)
    let _ = Command::new("tmux")
        .args(["kill-pane", "-t", &teammate1])
        .status();
    let _ = Command::new("tmux")
        .args(["kill-pane", "-t", &teammate2])
        .status();
    std::thread::sleep(std::time::Duration::from_millis(200));

    remove_stale_members(&mut pod);
    eprintln!("Phase 3 (Post-cleanup): {} members", pod.members.len());

    assert_eq!(pod.members.len(), 1, "Expected only leader after cleanup");
    assert_eq!(pod.members[0].tmux_pane, leader_pane);

    kill_session(&session);
}

/// 2-5: 子 Pod ライフサイクル — create_child_pods → stale removal → orphan cleanup
///
/// 1. session 作成 → leader Pod
/// 2. split-pane → discover → create_child_pods → 子 Pod 確認
/// 3. kill-pane → remove_stale → remove_orphan → 子 Pod 消滅確認
#[test]
#[ignore]
fn test_child_pod_lifecycle_with_tmux() {
    if !tmux_available() {
        eprintln!("tmux not available, skipping");
        return;
    }

    let session = unique_session("child-lifecycle");
    assert!(create_session(&session), "Failed to create session");

    let initial_panes = list_pane_ids(&session);
    let leader_pane = initial_panes[0].clone();

    // leader に Claude 出力を注入
    inject_claude_output(
        &leader_pane,
        "Claude is working...\\nI will analyze the codebase.\\n",
    );

    // --- Phase 1: Solo Pod ---
    let mut parent = make_test_pod("my-project", &session, &leader_pane);
    let mut all_pods = vec![parent.clone()];

    // teammate pane を追加 (split-window)
    let teammate1 = split_window(&session).expect("split 1");
    let teammate2 = split_window(&session).expect("split 2");

    inject_claude_output(
        &teammate1,
        "Claude is searching...\\n  Read src/main.rs\\n",
    );
    inject_claude_output(
        &teammate2,
        "Claude is testing...\\n  Bash cargo test\\n  Read Cargo.toml\\n",
    );
    std::thread::sleep(std::time::Duration::from_millis(500));

    // --- Phase 2: discover → create_child_pods ---
    let discovered = discover_new_members(&parent, &all_pods);
    eprintln!("Discovered {} new members", discovered.len());
    assert!(
        discovered.len() >= 2,
        "Expected at least 2 discovered members, got {}",
        discovered.len()
    );

    let children = create_child_pods(&mut parent, discovered);
    eprintln!("Created {} child pods", children.len());
    assert!(children.len() >= 2, "Expected at least 2 child pods");

    // 親の group が設定される
    assert_eq!(parent.group, Some("my-project".to_string()));

    // 子 Pod の名前・group を検証
    for child in &children {
        assert!(child.name.starts_with("my-project/"), "Child name: {}", child.name);
        assert_eq!(child.group, Some("my-project".to_string()));
        assert_eq!(child.tmux_session, session);
        assert_eq!(child.members.len(), 1);
    }

    // all_pods に追加
    all_pods = vec![parent.clone()];
    all_pods.extend(children);
    eprintln!("Total pods: {}", all_pods.len());

    // --- Phase 3: kill panes → stale removal → orphan cleanup ---
    let _ = Command::new("tmux")
        .args(["kill-pane", "-t", &teammate1])
        .status();
    let _ = Command::new("tmux")
        .args(["kill-pane", "-t", &teammate2])
        .status();
    std::thread::sleep(std::time::Duration::from_millis(200));

    // stale removal: 各子 Pod の member を除去
    for pod in &mut all_pods {
        remove_stale_members(pod);
    }
    eprintln!("After stale removal:");
    for pod in &all_pods {
        eprintln!("  {} members={}", pod.name, pod.members.len());
    }

    // 子 Pod は member が 0 になっているはず
    let orphan_count = all_pods.iter()
        .filter(|p| p.name != "my-project" && p.members.is_empty())
        .count();
    assert!(orphan_count >= 2, "Expected at least 2 orphan child pods, got {}", orphan_count);

    // orphan cleanup
    remove_orphan_child_pods(&mut all_pods);
    eprintln!("After orphan cleanup: {} pods", all_pods.len());
    for pod in &all_pods {
        eprintln!("  {} members={}", pod.name, pod.members.len());
    }

    // 親 Pod のみ残る
    assert_eq!(all_pods.len(), 1, "Expected only parent pod after orphan cleanup");
    assert_eq!(all_pods[0].name, "my-project");
    assert_eq!(all_pods[0].members.len(), 1); // leader は生存

    kill_session(&session);
}
