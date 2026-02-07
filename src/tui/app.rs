use crate::pod::detector::{detect_member_status_with_config, parse_permission_request};
use crate::pod::discovery;
use crate::pod::{AppState, ChatMessage, Member, MemberStatus, Pod, PodStatus, PodType};
use crate::store::PodStore;
use crate::tmux::Tmux;
use anyhow::{Context, Result};
use chrono::Utc;

pub struct App {
    pub state: AppState,
    pub store: PodStore,
    pub config: crate::config::Config,
    pub hooks: crate::hooks::HooksReceiver,
    last_store_reload: std::time::Instant,
}

impl App {
    pub fn new(store: PodStore) -> Result<Self> {
        let config = crate::config::Config::load().unwrap_or_default();
        let pods = store.load_and_reconcile().unwrap_or_default();
        let mut state = AppState::new();
        state.pods = pods;
        let mut hooks = crate::hooks::HooksReceiver::new();
        hooks.init();
        Ok(Self { state, store, config, hooks, last_store_reload: std::time::Instant::now() })
    }

    /// Pod を作成
    pub fn create_pod(&mut self, name: &str, worktree: Option<&str>) -> Result<()> {
        // 同名チェック
        if self.state.pods.iter().any(|p| p.name == name) {
            anyhow::bail!("Pod '{}' already exists", name);
        }

        let start_dir = if let Some(wt_path) = worktree {
            let path = std::path::Path::new(wt_path);
            if !path.exists() {
                // worktree パスが存在しない → git worktree add を試行
                if crate::tmux::git_available() {
                    crate::tmux::create_worktree(wt_path, name)?;
                } else {
                    // git がなければディレクトリ作成だけ
                    std::fs::create_dir_all(wt_path)
                        .with_context(|| format!("Failed to create directory: {}", wt_path))?;
                }
            }
            Some(wt_path)
        } else {
            None
        };

        // tmux セッションを作成 (worktree パスをstart_dirに)
        Tmux::new_session(name, start_dir)?;

        // Pod を作成 (Solo, 1 member "claude")
        let panes = Tmux::list_panes(name)?;
        let pane_id = panes
            .first()
            .map(|p| p.id.clone())
            .unwrap_or_else(|| format!("%0"));

        let member = Member {
            role: "claude".to_string(),
            status: MemberStatus::Idle,
            tmux_pane: pane_id,
            last_change: Utc::now(),
            last_output: String::new(),
            last_polled: None,
            working_secs: 0,
        };

        let pod = Pod {
            name: name.to_string(),
            pod_type: PodType::Solo,
            members: vec![member],
            status: PodStatus::Idle,
            tmux_session: name.to_string(),
            worktree: worktree.map(|s| s.to_string()),
            created_at: Utc::now(),
            total_working_secs: 0,
        };

        self.state.pods.push(pod);
        self.save()?;

        // Claude を起動
        Tmux::start_claude_in_session(name, None)?;

        Ok(())
    }

    /// 既存 tmux セッションを Pod として取り込み
    pub fn adopt_session(&mut self, session: &str, name: Option<&str>) -> Result<()> {
        if !Tmux::session_exists(session) {
            anyhow::bail!("tmux session '{}' does not exist", session);
        }

        let pod_name = name.unwrap_or(session);

        if self.state.pods.iter().any(|p| p.name == pod_name) {
            anyhow::bail!("Pod '{}' already exists", pod_name);
        }

        let panes = Tmux::list_panes(session)?;
        let members: Vec<Member> = panes
            .iter()
            .enumerate()
            .map(|(i, pane)| Member {
                role: if i == 0 {
                    "lead".to_string()
                } else {
                    format!("member-{}", i)
                },
                status: MemberStatus::Idle,
                tmux_pane: pane.id.clone(),
                last_change: Utc::now(),
                last_output: String::new(),
                last_polled: None,
                working_secs: 0,
            })
            .collect();

        let pod_type = if members.len() > 1 {
            PodType::Team
        } else {
            PodType::Solo
        };

        let pod = Pod {
            name: pod_name.to_string(),
            pod_type,
            members,
            status: PodStatus::Idle,
            tmux_session: session.to_string(),
            worktree: None,
            created_at: Utc::now(),
            total_working_secs: 0,
        };

        self.state.pods.push(pod);
        self.save()?;

        Ok(())
    }

    /// Pod を削除 (tmux セッションも kill)
    pub fn drop_pod(&mut self, name: &str) -> Result<()> {
        let idx = self
            .state
            .pods
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(|| anyhow::anyhow!("Pod '{}' not found", name))?;

        let pod = &self.state.pods[idx];
        let session = pod.tmux_session.clone();

        // tmux セッションを kill
        if Tmux::session_exists(&session) {
            Tmux::kill_session(&session)?;
        }

        self.state.pods.remove(idx);
        self.save()?;

        // focus の調整
        if let Some(focus) = self.state.focus {
            if focus >= self.state.pods.len() {
                self.state.focus = if self.state.pods.is_empty() {
                    None
                } else {
                    Some(self.state.pods.len() - 1)
                };
            }
        }

        Ok(())
    }

    /// Pod を削除 (tmux セッションは残す)
    pub fn forget_pod(&mut self, name: &str) -> Result<()> {
        let idx = self
            .state
            .pods
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(|| anyhow::anyhow!("Pod '{}' not found", name))?;

        self.state.pods.remove(idx);
        self.save()?;

        // focus の調整
        if let Some(focus) = self.state.focus {
            if focus >= self.state.pods.len() {
                self.state.focus = if self.state.pods.is_empty() {
                    None
                } else {
                    Some(self.state.pods.len() - 1)
                };
            }
        }

        Ok(())
    }

    /// 状態を保存
    pub fn save(&self) -> Result<()> {
        self.store.save(&self.state.pods)
    }

    /// 全 Pod の状態を更新 (discovery + capture-pane + detect)
    pub fn refresh_pod_states(&mut self) {
        for pod in &mut self.state.pods {
            // セッションが生きているか確認
            if !Tmux::session_exists(&pod.tmux_session) {
                pod.status = PodStatus::Done;
                for member in &mut pod.members {
                    member.status = MemberStatus::Done;
                }
                continue;
            }

            // --- Discovery: 消えた member を除外 ---
            discovery::remove_stale_members(pod);

            // --- Discovery: 新しい member を検出 ---
            let new_members = discovery::discover_new_members(pod);
            for member in new_members {
                pod.members.push(member);
            }

            // --- Discovery: Pod type を更新 ---
            if pod.members.len() > 1 {
                pod.pod_type = PodType::Team;
            } else if pod.members.len() == 1 {
                pod.pod_type = PodType::Solo;
            }

            // --- 既存メンバーの状態検出 ---
            for member in &mut pod.members {
                if let Ok(output) = Tmux::capture_pane(&member.tmux_pane) {
                    let new_status = detect_member_status_with_config(
                        &output,
                        &self.config.detection.permission_patterns,
                        &self.config.detection.error_patterns,
                        &self.config.detection.idle_patterns,
                    );
                    if new_status != member.status {
                        // Working -> 他の状態: working_secs に差分を加算
                        if member.status == MemberStatus::Working {
                            let secs = Utc::now().signed_duration_since(member.last_change).num_seconds().max(0) as u64;
                            member.working_secs += secs;
                        }
                        member.status = new_status;
                        member.last_change = Utc::now();
                    }
                    member.last_output = output;
                }
            }
            pod.rollup_status();
        }

        // Permission 状態の member を検出して current_permission を更新
        let mut found_permission = false;
        for pod in &self.state.pods {
            for member in &pod.members {
                if member.status == MemberStatus::Permission {
                    if let Some(req) = parse_permission_request(&member.last_output) {
                        self.state.current_permission = Some(req);
                        found_permission = true;
                        break;
                    }
                }
            }
            if found_permission { break; }
        }
        if !found_permission {
            self.state.current_permission = None;
        }

        // 新たに Permission になった Pod を検出して通知
        let current_perm_pods: std::collections::HashSet<String> = self
            .state
            .pods
            .iter()
            .filter(|p| p.status == PodStatus::Permission)
            .map(|p| p.name.clone())
            .collect();

        for pod_name in &current_perm_pods {
            if !self.state.previous_permission_pods.contains(pod_name) {
                if self.config.notification.enabled {
                    crate::notify::notify(
                        "Apiary: Permission Required",
                        &format!("Pod '{}' needs your approval", pod_name),
                    );
                }
            }
        }
        self.state.previous_permission_pods = current_perm_pods;
    }

    /// 適応的ポーリング: member の状態に応じた間隔で状態更新
    pub fn selective_refresh(&mut self) {
        use std::time::{Duration, Instant};

        // hooks イベントを確認
        let hook_events = self.hooks.poll_events();
        for event in &hook_events {
            tracing::debug!("Hook event: {:?}", event);
        }

        if !hook_events.is_empty() {
            // hooks イベントに基づいて状態を直接更新 (capture-pane より優先)
            // 最後のイベントから推定される状態を適用
            if let Some(last_event) = hook_events.last() {
                if let Some(hook_status) = last_event.inferred_status() {
                    // session フィールドがあれば対応する pod を特定、なければ全体に適用
                    let target_session = last_event.session.clone();
                    for pod in &mut self.state.pods {
                        let matches = match &target_session {
                            Some(sess) => pod.tmux_session == *sess || pod.name == *sess,
                            None => true,
                        };
                        if matches {
                            for member in &mut pod.members {
                                if member.status != hook_status {
                                    // Working -> 他の状態: working_secs に差分を加算
                                    if member.status == MemberStatus::Working {
                                        let secs = chrono::Utc::now()
                                            .signed_duration_since(member.last_change)
                                            .num_seconds()
                                            .max(0) as u64;
                                        member.working_secs += secs;
                                    }
                                    member.status = hook_status.clone();
                                    member.last_change = chrono::Utc::now();
                                }
                                member.last_polled = None;
                            }
                        }
                    }
                }
            }
        }

        // --- Dynamic reload: pods.json 再読み込み + 新 member 検出 ---
        {
            let reload_interval = Duration::from_secs(2);
            if self.last_store_reload.elapsed() >= reload_interval {
                self.last_store_reload = Instant::now();

                // 1. pods.json から新しい Pod をマージ
                if let Ok(stored_pods) = self.store.load() {
                    for stored_pod in &stored_pods {
                        if !self.state.pods.iter().any(|p| p.name == stored_pod.name) {
                            self.state.pods.push(stored_pod.clone());
                        }
                    }
                    // 削除された Pod を除去 (空ファイル読み込み時のフリッカー防止)
                    if !stored_pods.is_empty() || self.state.pods.is_empty() {
                        let stored_names: std::collections::HashSet<&str> =
                            stored_pods.iter().map(|p| p.name.as_str()).collect();
                        self.state.pods.retain(|p| stored_names.contains(p.name.as_str()));
                    }

                    // focus 調整
                    if let Some(focus) = self.state.focus {
                        if focus >= self.state.pods.len() {
                            self.state.focus = if self.state.pods.is_empty() {
                                None
                            } else {
                                Some(self.state.pods.len() - 1)
                            };
                        }
                    }
                    if self.state.focus.is_none() && !self.state.pods.is_empty() {
                        self.state.focus = Some(0);
                    }
                }

                // 2. 各 Pod で新しい member を検出
                for pod in &mut self.state.pods {
                    if !Tmux::session_exists(&pod.tmux_session) {
                        continue;
                    }
                    discovery::remove_stale_members(pod);
                    let new_members = discovery::discover_new_members(pod);
                    for member in new_members {
                        pod.members.push(member);
                    }
                    if pod.members.len() > 1 {
                        pod.pod_type = PodType::Team;
                    } else if pod.members.len() == 1 {
                        pod.pod_type = PodType::Solo;
                    }
                }
            }
        }

        let now = Instant::now();
        let focus_idx = self.state.focus;

        for (pod_idx, pod) in self.state.pods.iter_mut().enumerate() {
            if !Tmux::session_exists(&pod.tmux_session) {
                pod.status = PodStatus::Done;
                for member in &mut pod.members {
                    member.status = MemberStatus::Done;
                }
                continue;
            }

            let is_focused = focus_idx == Some(pod_idx);

            for member in &mut pod.members {
                // ポーリング間隔を状態に応じて決定
                let interval = if is_focused {
                    Duration::from_millis(self.config.polling.focused_interval_ms)
                } else {
                    match member.status {
                        MemberStatus::Permission => Duration::from_millis(self.config.polling.permission_interval_ms),
                        MemberStatus::Working => Duration::from_millis(self.config.polling.working_interval_ms),
                        MemberStatus::Error => Duration::from_millis(self.config.polling.error_interval_ms),
                        MemberStatus::Idle => Duration::from_millis(self.config.polling.idle_interval_ms),
                        MemberStatus::Done => Duration::from_millis(self.config.polling.idle_interval_ms),
                    }
                };

                // 前回のポーリングから十分時間が経っているかチェック
                let should_poll = match member.last_polled {
                    Some(last) => now.duration_since(last) >= interval,
                    None => true, // 初回は必ずポーリング
                };

                if !should_poll {
                    continue;
                }

                member.last_polled = Some(now);

                if let Ok(output) = Tmux::capture_pane(&member.tmux_pane) {
                    let new_status = detect_member_status_with_config(
                        &output,
                        &self.config.detection.permission_patterns,
                        &self.config.detection.error_patterns,
                        &self.config.detection.idle_patterns,
                    );
                    if new_status != member.status {
                        // Working -> 他の状態: working_secs に差分を加算
                        if member.status == MemberStatus::Working {
                            let secs = chrono::Utc::now().signed_duration_since(member.last_change).num_seconds().max(0) as u64;
                            member.working_secs += secs;
                        }
                        member.status = new_status;
                        member.last_change = chrono::Utc::now();
                    }
                    member.last_output = output;
                }
            }
            pod.rollup_status();
        }
    }

    /// 現在の focus 位置から次の Permission Pod を巡回検索
    pub fn next_permission_pod_from_current(&self) -> Option<usize> {
        if self.state.pods.is_empty() {
            return None;
        }

        let start = self.state.focus.map(|f| f + 1).unwrap_or(0);
        let total = self.state.pods.len();

        // start から末尾まで、そして先頭から start-1 まで検索
        for i in 0..total {
            let idx = (start + i) % total;
            if self.state.pods[idx].status == PodStatus::Permission {
                return Some(idx);
            }
        }
        None
    }

    /// グリッド内でカーソルを移動
    pub fn move_focus(&mut self, direction: Direction) {
        if self.state.pods.is_empty() {
            return;
        }

        let total = self.state.pods.len();
        let cols = self.state.grid_columns.max(1);
        let current = self.state.focus.unwrap_or(0);

        let new_focus = match direction {
            Direction::Right => {
                if current + 1 < total {
                    current + 1
                } else {
                    current
                }
            }
            Direction::Left => {
                if current > 0 {
                    current - 1
                } else {
                    current
                }
            }
            Direction::Down => {
                let next = current + cols;
                if next < total {
                    next
                } else {
                    current
                }
            }
            Direction::Up => {
                if current >= cols {
                    current - cols
                } else {
                    current
                }
            }
        };

        self.state.focus = Some(new_focus);
    }

    /// コマンド文字列をパースして実行
    pub fn execute_command(&mut self, cmd: &str) -> Result<String> {
        let parts: Vec<&str> = cmd.trim().split_whitespace().collect();

        if parts.is_empty() {
            return Ok(String::new());
        }

        // "pod" prefix は省略可能
        let parts = if parts[0] == "pod" { &parts[1..] } else { &parts };

        if parts.is_empty() {
            return Ok("Available: create, adopt, drop, forget, list".to_string());
        }

        match parts[0] {
            "create" => {
                if parts.len() < 2 {
                    return Ok("Usage: create <name> [--worktree <path>]".to_string());
                }
                let name = parts[1];
                let worktree = parts
                    .iter()
                    .position(|&p| p == "--worktree")
                    .and_then(|i| parts.get(i + 1))
                    .copied();
                self.create_pod(name, worktree)?;
                Ok(format!("Pod '{}' created", name))
            }
            "adopt" => {
                if parts.len() < 2 {
                    return Ok("Usage: adopt <session> [--name <name>]".to_string());
                }
                let session = parts[1];
                let name = parts
                    .iter()
                    .position(|&p| p == "--name")
                    .and_then(|i| parts.get(i + 1))
                    .copied();
                self.adopt_session(session, name)?;
                Ok(format!("Session '{}' adopted", session))
            }
            "drop" => {
                if parts.len() < 2 {
                    return Ok("Usage: drop <name>".to_string());
                }
                self.drop_pod(parts[1])?;
                Ok(format!("Pod '{}' dropped", parts[1]))
            }
            "forget" => {
                if parts.len() < 2 {
                    return Ok("Usage: forget <name>".to_string());
                }
                self.forget_pod(parts[1])?;
                Ok(format!("Pod '{}' forgotten", parts[1]))
            }
            "list" => {
                if self.state.pods.is_empty() {
                    return Ok("No pods".to_string());
                }
                let list: Vec<String> = self
                    .state
                    .pods
                    .iter()
                    .map(|p| {
                        format!(
                            "{} {} ({}, {} members)",
                            p.status_icon(),
                            p.name,
                            p.elapsed_time(),
                            p.members.len()
                        )
                    })
                    .collect();
                Ok(list.join("\n"))
            }
            _ => Ok(format!("Unknown command: '{}'. Try: create, adopt, drop, forget, list", parts[0])),
        }
    }

    /// Chat メッセージを送信
    pub fn send_chat_message(&mut self) -> Result<()> {
        let input = self.state.chat_input.clone();
        if input.is_empty() {
            return Ok(());
        }

        // focused pod の lead/solo member を取得
        let pane_id = self
            .state
            .focused_pod()
            .and_then(|pod| pod.members.first())
            .map(|m| m.tmux_pane.clone())
            .ok_or_else(|| anyhow::anyhow!("No focused pod or member"))?;

        // スナップショット保存
        if let Ok(snapshot) = Tmux::capture_pane_lines(&pane_id, 100) {
            self.state.capture_snapshot = Some(snapshot);
        }

        // pane に送信
        Tmux::send_keys(&pane_id, &input)?;

        // chat_history に追加
        self.state.chat_history.push(ChatMessage {
            sender: "you".to_string(),
            content: input,
            timestamp: Utc::now(),
        });

        // 入力をクリア
        self.state.chat_input.clear();

        Ok(())
    }

    /// Chat モードで Claude の応答を差分検出して chat_history に追加
    pub fn refresh_chat_output(&mut self) {
        // スナップショットがない場合はスキップ
        let snapshot = match &self.state.capture_snapshot {
            Some(s) => s.clone(),
            None => return,
        };

        let pane_id = match self
            .state
            .focused_pod()
            .and_then(|pod| pod.members.first())
            .map(|m| m.tmux_pane.clone())
        {
            Some(id) => id,
            None => return,
        };

        let current = match Tmux::capture_pane_lines(&pane_id, 100) {
            Ok(c) => c,
            Err(_) => return,
        };

        // 差分を計算: スナップショットにない新しい行を抽出
        let snapshot_lines: Vec<&str> = snapshot.lines().collect();
        let current_lines: Vec<&str> = current.lines().collect();

        // スナップショットの行数より多い行が新しい出力
        if current_lines.len() > snapshot_lines.len() {
            let new_lines = &current_lines[snapshot_lines.len()..];
            let new_output = new_lines.join("\n").trim().to_string();

            if !new_output.is_empty() {
                // 既に同じ内容の応答がないか確認
                let already_added = self
                    .state
                    .chat_history
                    .last()
                    .map(|m| m.sender == "claude" && m.content == new_output)
                    .unwrap_or(false);

                if !already_added {
                    // 前回の claude メッセージを更新（差分が増えていく場合）
                    if let Some(last) = self.state.chat_history.last_mut() {
                        if last.sender == "claude" {
                            last.content = new_output;
                            return;
                        }
                    }

                    self.state.chat_history.push(ChatMessage {
                        sender: "claude".to_string(),
                        content: new_output,
                        timestamp: Utc::now(),
                    });
                }
            }
        }
    }

    /// Permission を approve
    pub fn approve_permission(&mut self) -> Result<()> {
        let pane_id = self
            .find_permission_member_pane()
            .ok_or_else(|| anyhow::anyhow!("No member awaiting permission"))?;

        Tmux::send_keys_raw(&pane_id, "y")?;
        self.state.current_permission = None;
        Ok(())
    }

    /// Permission を deny
    pub fn deny_permission(&mut self) -> Result<()> {
        let pane_id = self
            .find_permission_member_pane()
            .ok_or_else(|| anyhow::anyhow!("No member awaiting permission"))?;

        Tmux::send_keys_raw(&pane_id, "n")?;
        self.state.current_permission = None;
        Ok(())
    }

    /// Permission 状態の member の pane_id を取得
    fn find_permission_member_pane(&self) -> Option<String> {
        self.state.focused_pod().and_then(|pod| {
            pod.members
                .iter()
                .find(|m| m.status == MemberStatus::Permission)
                .map(|m| m.tmux_pane.clone())
        })
    }
}

pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}
