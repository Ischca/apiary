use crate::pod::detector::{detect_member_status_with_config, parse_permission_request, parse_sub_agents};
use crate::pod::discovery;
use crate::pod::{AppState, BrowserEntry, BrowserState, ChatMessage, InlinePrompt, Member, MemberStatus, Mode, PaneFocus, Pod, PodStatus, PodType};
use crate::project::ProjectStore;
use crate::store::PodStore;
use crate::tmux::Tmux;
use anyhow::{Context, Result};
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// pipe-pane ストリーミング + 永続 vt100 パーサー
pub struct DetailPtyStream {
    parser: vt100::Parser,
    file: std::fs::File,
    file_path: PathBuf,
    pane_id: String,
    cols: u16,
    rows: u16,
}

impl DetailPtyStream {
    pub fn start(pane_id: &str, cols: u16, rows: u16) -> Result<Self> {
        let file_path = PathBuf::from(format!("/tmp/apiary-pty-{}.raw", pane_id.replace('%', "")));

        // ファイルを作成 (既存を truncate)
        std::fs::File::create(&file_path)
            .with_context(|| format!("Failed to create PTY stream file: {:?}", file_path))?;

        // pipe-pane 開始
        Tmux::pipe_pane_start(pane_id, file_path.to_str().unwrap())?;

        // resize して SIGWINCH → アプリが全画面再描画 → pipe がキャプチャ
        let _ = Tmux::resize_window(pane_id, cols, rows);

        // 読み取りハンドルをオープン
        let file = std::fs::File::open(&file_path)
            .with_context(|| format!("Failed to open PTY stream file: {:?}", file_path))?;

        let parser = vt100::Parser::new(rows, cols, 0);

        Ok(Self {
            parser,
            file,
            file_path,
            pane_id: pane_id.to_string(),
            cols,
            rows,
        })
    }

    /// ファイルから新バイトを読み取り → パーサーに処理。読み取ったバイト数を返す。
    pub fn drain(&mut self) -> usize {
        let mut buf = [0u8; 16384];
        let mut total = 0;
        loop {
            match self.file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.parser.process(&buf[..n]);
                    total += n;
                }
                Err(_) => break,
            }
        }
        total
    }

    /// サイズ変更 (変化時のみ実行)
    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols != self.cols || rows != self.rows {
            self.cols = cols;
            self.rows = rows;
            self.parser.set_size(rows, cols);
            let _ = Tmux::resize_window(&self.pane_id, cols, rows);
        }
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    /// pipe-pane 停止 + ファイル削除
    pub fn stop(self) {
        let _ = Tmux::pipe_pane_stop(&self.pane_id);
        let _ = std::fs::remove_file(&self.file_path);
    }
}

pub struct App {
    pub state: AppState,
    pub store: PodStore,
    pub project_store: ProjectStore,
    pub config: crate::config::Config,
    pub hooks: crate::hooks::HooksReceiver,
    pub detail_pty_stream: Option<DetailPtyStream>,
    last_store_reload: std::time::Instant,
}

impl App {
    pub fn new(store: PodStore) -> Result<Self> {
        let config = crate::config::Config::load().unwrap_or_default();
        let project_store = ProjectStore::new()?;
        let pods = store.load_and_reconcile().unwrap_or_default();
        let mut state = AppState::new();
        state.pods = pods;
        // 起動時に cwd からワークスペースを初期化
        state.current_project = crate::project::resolve_project_or_cwd(&project_store, None).ok();
        let mut hooks = crate::hooks::HooksReceiver::new();
        hooks.init();
        Ok(Self { state, store, project_store, config, hooks, detail_pty_stream: None, last_store_reload: std::time::Instant::now() })
    }

    /// Pod を作成
    pub fn create_pod(&mut self, name: &str, project_input: Option<&str>, group: Option<&str>, prompt: Option<&str>) -> Result<()> {
        // 同名チェック
        if self.state.pods.iter().any(|p| p.name == name) {
            anyhow::bail!("Pod '{}' already exists", name);
        }

        // プロジェクト解決: @project 指定 > current_project > cwd フォールバック
        let project = if let Some(input) = project_input {
            crate::project::resolve_project(&self.project_store, input)?
        } else if let Some(ref cp) = self.state.current_project {
            cp.clone()
        } else {
            crate::project::resolve_project_or_cwd(&self.project_store, None)?
        };

        // tmux セッションを作成 (プロジェクトパスを start_dir に)
        Tmux::new_session(name, Some(project.path.as_str()))?;

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
            last_output_ansi: String::new(),
            pane_size: (80, 24),
            last_polled: None,
            working_secs: 0,
            sub_agents: Vec::new(),
        };

        let pod = Pod {
            name: name.to_string(),
            pod_type: PodType::Solo,
            members: vec![member],
            status: PodStatus::Idle,
            tmux_session: name.to_string(),
            project: Some(project.name.clone()),
            group: group.map(|s| s.to_string())
                .or_else(|| Some(project.name.clone())),
            created_at: Utc::now(),
            total_working_secs: 0,
        };

        self.state.pods.push(pod);
        self.save()?;

        // Claude を起動
        Tmux::start_claude_in_session(name, prompt)?;

        Ok(())
    }

    /// 既存 tmux セッションを Pod として取り込み
    pub fn adopt_session(&mut self, session: &str, name: Option<&str>, group: Option<&str>) -> Result<()> {
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
                last_output_ansi: String::new(),
                pane_size: (80, 24),
                last_polled: None,
                working_secs: 0,
                sub_agents: Vec::new(),
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
            project: None,
            group: group.map(|s| s.to_string()),
            created_at: Utc::now(),
            total_working_secs: 0,
        };

        self.state.pods.push(pod);
        self.save()?;

        Ok(())
    }

    /// Pod を削除 (同一 session を共有する Pod がなければ session ごと kill、あれば pane 単位で kill)
    pub fn drop_pod(&mut self, name: &str) -> Result<()> {
        let idx = self
            .state
            .pods
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(|| anyhow::anyhow!("Pod '{}' not found", name))?;

        let pod = &self.state.pods[idx];
        let session = pod.tmux_session.clone();
        let pane_ids: Vec<String> = pod.members.iter().map(|m| m.tmux_pane.clone()).collect();

        // 同一 session を使う他の Pod があるか
        let shared = self.state.pods.iter()
            .any(|p| p.name != name && p.tmux_session == session);

        if shared {
            // pane 単位で kill（session は残す）
            for pane_id in &pane_ids {
                let _ = Tmux::kill_pane(pane_id);
            }
        } else {
            // 最後の Pod → session ごと kill
            if Tmux::session_exists(&session) {
                Tmux::kill_session(&session)?;
            }
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
        let mut new_pods: Vec<Pod> = Vec::new();
        let pod_count = self.state.pods.len();

        for idx in 0..pod_count {
            let pod = &mut self.state.pods[idx];

            // セッションが生きているか確認
            if !Tmux::session_exists(&pod.tmux_session) {
                if pod.status != PodStatus::Dead {
                    pod.status = PodStatus::Dead;
                    for member in &mut pod.members {
                        member.status = MemberStatus::Dead;
                    }
                }
                continue;
            } else if pod.status == PodStatus::Dead {
                // セッションが復活した場合、Dead から復帰
                for member in &mut pod.members {
                    if member.status == MemberStatus::Dead {
                        member.status = MemberStatus::Idle;
                        member.last_change = Utc::now();
                    }
                }
            }

            // --- Discovery: 消えた member を除外 ---
            discovery::remove_stale_members(pod);

            // --- Discovery: 新しい pane を検出 → 子 Pod 作成 ---
            {
                let all_known: Vec<Pod> = self.state.pods.iter()
                    .chain(new_pods.iter())
                    .cloned()
                    .collect();
                let pod = &self.state.pods[idx];
                let discovered = discovery::discover_new_members(pod, &all_known);

                let pod = &mut self.state.pods[idx];
                let children = discovery::create_child_pods(pod, discovered);
                new_pods.extend(children);
            }

            // --- 既存メンバーの状態検出 ---
            let pod = &mut self.state.pods[idx];
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
                    // Subagent 検出 (pane 出力から)
                    member.sub_agents = parse_sub_agents(&output);
                    member.last_output = output;
                }
            }
            pod.rollup_status();
        }

        // 新 Pod を state に追加
        if !new_pods.is_empty() {
            self.state.pods.extend(new_pods);
            self.save().ok();
        }

        // 孤立子 Pod のクリーンアップ
        discovery::remove_orphan_child_pods(&mut self.state.pods);

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

            // Subagent hooks イベントを処理
            for event in &hook_events {
                if !event.is_subagent_event() {
                    continue;
                }
                let agent_type = event.agent_type.clone().unwrap_or_else(|| "Task".to_string());
                let agent_id = event.agent_id.clone().unwrap_or_default();
                let target_session = event.session.clone();

                for pod in &mut self.state.pods {
                    let matches = match &target_session {
                        Some(sess) => pod.tmux_session == *sess || pod.name == *sess,
                        None => true,
                    };
                    if !matches { continue; }

                    for member in &mut pod.members {
                        match event.event.as_str() {
                            "subagent_start" => {
                                // 既存の同一 agent_id がなければ追加
                                if !member.sub_agents.iter().any(|a| a.description == agent_id) {
                                    member.sub_agents.push(crate::pod::SubAgent {
                                        agent_type: agent_type.clone(),
                                        description: agent_id.clone(),
                                    });
                                }
                            }
                            "subagent_stop" => {
                                member.sub_agents.retain(|a| a.description != agent_id);
                            }
                            _ => {}
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

                // 2. 各 Pod で新しい pane を検出 → 子 Pod 作成
                let mut new_pods: Vec<Pod> = Vec::new();
                let pod_count = self.state.pods.len();
                for idx in 0..pod_count {
                    {
                        let pod = &mut self.state.pods[idx];
                        if !Tmux::session_exists(&pod.tmux_session) {
                            continue;
                        }
                        discovery::remove_stale_members(pod);
                    }

                    // all_known: 既存の全 Pod + 今回の新 Pod
                    let all_known: Vec<Pod> = self.state.pods.iter()
                        .chain(new_pods.iter())
                        .cloned()
                        .collect();
                    let discovered = discovery::discover_new_members(&self.state.pods[idx], &all_known);

                    let pod = &mut self.state.pods[idx];
                    let children = discovery::create_child_pods(pod, discovered);
                    new_pods.extend(children);
                }
                if !new_pods.is_empty() {
                    self.state.pods.extend(new_pods);
                    self.save().ok();
                }

                // 3. 孤立子 Pod のクリーンアップ
                discovery::remove_orphan_child_pods(&mut self.state.pods);
            }
        }

        let now = Instant::now();
        let focus_idx = self.state.focus;

        for (pod_idx, pod) in self.state.pods.iter_mut().enumerate() {
            if !Tmux::session_exists(&pod.tmux_session) {
                if pod.status != PodStatus::Dead {
                    pod.status = PodStatus::Dead;
                    for member in &mut pod.members {
                        member.status = MemberStatus::Dead;
                    }
                }
                continue;
            } else if pod.status == PodStatus::Dead {
                // セッションが復活した場合、Dead から復帰
                for member in &mut pod.members {
                    if member.status == MemberStatus::Dead {
                        member.status = MemberStatus::Idle;
                        member.last_change = chrono::Utc::now();
                    }
                }
                // rollup_status() がループ末尾で呼ばれて pod.status も更新される
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
                        MemberStatus::Dead => Duration::from_millis(self.config.polling.idle_interval_ms),
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
                    // Subagent / Agent Teams 検出 (pane 出力から)
                    let detected = parse_sub_agents(&output);
                    if !detected.is_empty() || !member.sub_agents.is_empty() {
                        tracing::debug!(
                            pane = %member.tmux_pane,
                            detected = detected.len(),
                            "sub_agents detected from pane output"
                        );
                    }
                    member.sub_agents = detected;
                    member.last_output = output;
                }

                // Detail モード: ストリームがあればそこから drain + リサイズ追従
                if is_focused && self.state.mode == Mode::Detail {
                    if let Some(ref mut stream) = self.detail_pty_stream {
                        if let Ok((term_cols, term_rows)) = crossterm::terminal::size() {
                            let w = (term_cols * 35 / 100).saturating_sub(2);
                            let h = term_rows.saturating_sub(4);
                            if w > 0 && h > 0 {
                                stream.resize(w, h);
                            }
                        }
                        stream.drain();
                        member.pane_size = stream.size();
                    }
                }
            }
            pod.rollup_status();
        }

        // Detail モードで focused pod が Dead になったら自動で Home に戻る
        if self.state.mode == Mode::Detail {
            let is_dead = self.state.focused_pod()
                .map(|p| p.status == PodStatus::Dead)
                .unwrap_or(true);
            if is_dead {
                self.restore_detail_window_size();
                self.state.mode = Mode::Home;
                self.state.selected_member = None;
            }
        }
    }

    /// Detail モード開始時に PTY ストリームを開始
    pub fn start_detail_pty_stream(&mut self) {
        let selected = self.state.selected_member.unwrap_or(0);
        let pane_id = match self.state.focused_pod()
            .and_then(|pod| pod.members.get(selected))
            .map(|m| m.tmux_pane.clone())
        {
            Some(id) => id,
            None => return,
        };

        // ターミナルサイズから Detail 表示エリアを算出
        let (cols, rows) = if let Ok((term_cols, term_rows)) = crossterm::terminal::size() {
            let w = (term_cols * 35 / 100).saturating_sub(2);
            let h = term_rows.saturating_sub(4);
            if w > 0 && h > 0 { (w, h) } else { (80, 24) }
        } else {
            (80, 24)
        };

        // オリジナル window サイズを保存
        if self.state.detail_original_window_size.is_none() {
            if let Ok(orig) = Tmux::get_window_size(&pane_id) {
                self.state.detail_original_window_size = Some((pane_id.clone(), orig.0, orig.1));
            }
        }

        match DetailPtyStream::start(&pane_id, cols, rows) {
            Ok(stream) => {
                self.detail_pty_stream = Some(stream);
            }
            Err(e) => {
                tracing::warn!("Failed to start PTY stream: {}", e);
            }
        }
    }

    /// PTY ストリームを停止
    pub fn stop_detail_pty_stream(&mut self) {
        if let Some(stream) = self.detail_pty_stream.take() {
            stream.stop();
        }
    }

    /// Detail モード終了時に window サイズを復元
    pub fn restore_detail_window_size(&mut self) {
        self.stop_detail_pty_stream();
        if let Some((pane_id, cols, rows)) = self.state.detail_original_window_size.take() {
            let _ = Tmux::resize_window(&pane_id, cols, rows);
        }
        self.state.detail_just_resized = false;
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
            return Ok("Available: create, adopt, drop, forget, list, project, browse".to_string());
        }

        match parts[0] {
            "create" => {
                if parts.len() < 2 {
                    return Ok("Usage: create <name> [--project <p>] [--group <g>]".to_string());
                }
                let name = parts[1];
                let project = parts
                    .iter()
                    .position(|&p| p == "--project")
                    .and_then(|i| parts.get(i + 1))
                    .copied();
                let group = parts
                    .iter()
                    .position(|&p| p == "--group")
                    .and_then(|i| parts.get(i + 1))
                    .copied();
                self.create_pod(name, project, group, None)?;
                Ok(format!("Pod '{}' created", name))
            }
            "adopt" => {
                if parts.len() < 2 {
                    return Ok("Usage: adopt <session> [--name <n>] [--group <g>]".to_string());
                }
                let session = parts[1];
                let name = parts
                    .iter()
                    .position(|&p| p == "--name")
                    .and_then(|i| parts.get(i + 1))
                    .copied();
                let group = parts
                    .iter()
                    .position(|&p| p == "--group")
                    .and_then(|i| parts.get(i + 1))
                    .copied();
                self.adopt_session(session, name, group)?;
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
            "project" => {
                if parts.len() < 2 {
                    return Ok("Usage: project list | project add <path> [--name <n>] | project remove <name>".to_string());
                }
                match parts[1] {
                    "list" => {
                        let projects = self.project_store.list()?;
                        if projects.is_empty() {
                            return Ok("No projects registered".to_string());
                        }
                        let list: Vec<String> = projects
                            .iter()
                            .map(|p| format!("  {} → {}", p.name, p.path))
                            .collect();
                        Ok(format!("Projects:\n{}", list.join("\n")))
                    }
                    "add" => {
                        if parts.len() < 3 {
                            return Ok("Usage: project add <path> [--name <n>]".to_string());
                        }
                        let path = parts[2];
                        let name = parts
                            .iter()
                            .position(|&p| p == "--name")
                            .and_then(|i| parts.get(i + 1))
                            .copied();
                        if let Some(name) = name {
                            let project = crate::project::Project {
                                name: name.to_string(),
                                path: path.to_string(),
                            };
                            self.project_store.register(&project)?;
                            Ok(format!("Project '{}' registered → {}", name, path))
                        } else {
                            let project = crate::project::resolve_project(&self.project_store, path)?;
                            Ok(format!("Project '{}' registered → {}", project.name, project.path))
                        }
                    }
                    "remove" => {
                        if parts.len() < 3 {
                            return Ok("Usage: project remove <name>".to_string());
                        }
                        let name = parts[2];
                        if self.project_store.unregister(name)? {
                            Ok(format!("Project '{}' removed", name))
                        } else {
                            Ok(format!("Project '{}' not found", name))
                        }
                    }
                    _ => Ok(format!("Unknown project command: '{}'. Try: list, add, remove", parts[1])),
                }
            }
            "browse" => {
                self.open_browser(None);
                Ok(String::new())
            }
            _ => Ok(format!("Unknown command: '{}'. Try: create, adopt, drop, forget, list, project, browse", parts[0])),
        }
    }

    /// Detail モードから pane にテキストを送信
    pub fn send_input_to_pane(&mut self) -> Result<()> {
        let input = self.state.chat_input.clone();
        if input.is_empty() {
            return Ok(());
        }

        let selected = self.state.selected_member.unwrap_or(0);
        let pane_id = self
            .state
            .focused_pod()
            .and_then(|pod| pod.members.get(selected))
            .map(|m| m.tmux_pane.clone())
            .ok_or_else(|| anyhow::anyhow!("No focused pod or member"))?;

        Tmux::send_keys(&pane_id, &input)?;
        self.state.chat_input.clear();

        // 送信後すぐに pane 出力を更新（即時フィードバック）
        if let Ok(output) = Tmux::capture_pane(&pane_id) {
            if let Some(pod) = self.state.focused_pod_mut() {
                if let Some(member) = pod.members.get_mut(selected) {
                    member.last_output = output;
                }
            }
        }

        Ok(())
    }

    /// Detail パススルー: キーストロークを pane に転送
    pub fn forward_key_to_pane(&mut self, key: &KeyEvent) -> Result<()> {
        let selected = self.state.selected_member.unwrap_or(0);
        let pane_id = self.state.focused_pod()
            .and_then(|pod| pod.members.get(selected))
            .map(|m| m.tmux_pane.clone())
            .ok_or_else(|| anyhow::anyhow!("No focused pod or member"))?;

        match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    Tmux::send_keys_raw(&pane_id, &format!("C-{}", c))?;
                } else {
                    Tmux::send_keys_literal(&pane_id, &c.to_string())?;
                }
            }
            KeyCode::Enter => Tmux::send_keys_raw(&pane_id, "Enter")?,
            KeyCode::Esc => Tmux::send_keys_raw(&pane_id, "Escape")?,
            KeyCode::Backspace => Tmux::send_keys_raw(&pane_id, "BSpace")?,
            KeyCode::Tab => Tmux::send_keys_raw(&pane_id, "Tab")?,
            KeyCode::Up => Tmux::send_keys_raw(&pane_id, "Up")?,
            KeyCode::Down => Tmux::send_keys_raw(&pane_id, "Down")?,
            KeyCode::Left => Tmux::send_keys_raw(&pane_id, "Left")?,
            KeyCode::Right => Tmux::send_keys_raw(&pane_id, "Right")?,
            KeyCode::Delete => Tmux::send_keys_raw(&pane_id, "DC")?,
            KeyCode::Home => Tmux::send_keys_raw(&pane_id, "Home")?,
            KeyCode::End => Tmux::send_keys_raw(&pane_id, "End")?,
            KeyCode::PageUp => Tmux::send_keys_raw(&pane_id, "PPage")?,
            KeyCode::PageDown => Tmux::send_keys_raw(&pane_id, "NPage")?,
            _ => return Ok(()),
        }

        // ストリームがあれば drain で即時反映
        if let Some(ref mut stream) = self.detail_pty_stream {
            std::thread::sleep(std::time::Duration::from_millis(10));
            stream.drain();
        }
        Ok(())
    }

    /// Detail パススルー: ペーストテキストを pane に転送
    pub fn forward_paste_to_pane(&mut self, text: &str) -> Result<()> {
        let selected = self.state.selected_member.unwrap_or(0);
        let pane_id = self.state.focused_pod()
            .and_then(|pod| pod.members.get(selected))
            .map(|m| m.tmux_pane.clone())
            .ok_or_else(|| anyhow::anyhow!("No focused pod or member"))?;

        Tmux::send_keys_literal(&pane_id, text)?;

        if let Some(ref mut stream) = self.detail_pty_stream {
            std::thread::sleep(std::time::Duration::from_millis(10));
            stream.drain();
        }
        Ok(())
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

    /// ディレクトリブラウザを開く
    pub fn open_browser(&mut self, start_path: Option<&str>) {
        let path = match start_path {
            Some(p) => std::path::PathBuf::from(p),
            None => {
                // current_project があればそのパスを起点、なければ $HOME
                self.state.current_project.as_ref()
                    .map(|p| std::path::PathBuf::from(&p.path))
                    .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/")))
            }
        };
        let entries = Self::read_directory(&path);
        self.state.browser_state = Some(BrowserState {
            current_path: path,
            entries,
            selected: 0,
            scroll_offset: 0,
        });
        self.state.inline_prompt = InlinePrompt::Browse;
        self.state.pane_focus = PaneFocus::Left;
    }

    /// ディレクトリの内容を読み取り（隠しファイル除外、ディレクトリ優先ソート）
    pub fn read_directory(path: &Path) -> Vec<BrowserEntry> {
        let mut entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(path) {
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                entries.push(BrowserEntry { name, is_dir });
            }
        }
        // ディレクトリ優先、名前順ソート
        entries.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        entries
    }

    /// ブラウザ: 選択中のディレクトリに入る
    pub fn browser_enter_dir(&mut self) {
        let (new_path, is_dir) = {
            let bs = match &self.state.browser_state {
                Some(bs) => bs,
                None => return,
            };
            let entry = match bs.entries.get(bs.selected) {
                Some(e) => e,
                None => return,
            };
            if !entry.is_dir {
                return;
            }
            (bs.current_path.join(&entry.name), entry.is_dir)
        };
        if !is_dir {
            return;
        }
        let entries = Self::read_directory(&new_path);
        if let Some(bs) = &mut self.state.browser_state {
            bs.current_path = new_path;
            bs.entries = entries;
            bs.selected = 0;
            bs.scroll_offset = 0;
        }
    }

    /// ブラウザ: 親ディレクトリへ移動
    pub fn browser_go_parent(&mut self) {
        let parent = {
            let bs = match &self.state.browser_state {
                Some(bs) => bs,
                None => return,
            };
            match bs.current_path.parent() {
                Some(p) => p.to_path_buf(),
                None => return,
            }
        };
        let entries = Self::read_directory(&parent);
        if let Some(bs) = &mut self.state.browser_state {
            bs.current_path = parent;
            bs.entries = entries;
            bs.selected = 0;
            bs.scroll_offset = 0;
        }
    }

    /// ブラウザ: 現在のディレクトリをワークスペースとして設定
    pub fn browser_select_current(&mut self) -> Result<String> {
        let path_str = {
            let bs = self.state.browser_state.as_ref()
                .ok_or_else(|| anyhow::anyhow!("No browser state"))?;
            bs.current_path.to_string_lossy().to_string()
        };
        let project = crate::project::resolve_project(&self.project_store, &path_str)?;
        let msg = format!("Workspace set → {}", project.path);
        self.state.current_project = Some(project);
        self.browser_cancel();
        Ok(msg)
    }

    /// ブラウザ: キャンセル
    pub fn browser_cancel(&mut self) {
        self.state.browser_state = None;
        self.state.inline_prompt = InlinePrompt::None;
    }
}

pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// 指示文からPod名を自動生成
/// Primary: Claude Haiku で kebab-case 名を生成
/// Fallback: ストップワード除去 + 先頭3語 → kebab-case
pub fn generate_pod_name(instruction: &str, existing_names: &[String]) -> String {
    let base = generate_name_with_haiku(instruction)
        .unwrap_or_else(|| generate_name_fallback(instruction));

    deduplicate_name(&base, existing_names)
}

fn generate_name_with_haiku(instruction: &str) -> Option<String> {
    let prompt_text = format!(
        "Generate a short kebab-case name (2-3 words, max 30 chars) for this task. Output ONLY the name, nothing else: {}",
        instruction
    );

    let output = std::process::Command::new("claude")
        .args(["-p", "--model", "haiku", "--no-session-persistence"])
        .arg(&prompt_text)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() || name.len() > 50 {
        return None;
    }

    Some(sanitize_tmux_name(&name))
}

fn generate_name_fallback(instruction: &str) -> String {
    let stop_words: std::collections::HashSet<&str> = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "shall", "can", "to", "of", "in", "for",
        "on", "with", "at", "by", "from", "as", "into", "through", "during",
        "before", "after", "above", "below", "between", "and", "but", "or",
        "not", "no", "so", "if", "then", "that", "this", "it", "its",
    ].iter().cloned().collect();

    let words: Vec<&str> = instruction
        .split_whitespace()
        .filter(|w| !stop_words.contains(&w.to_lowercase().as_str()))
        .take(3)
        .collect();

    let name = if words.is_empty() {
        "task".to_string()
    } else {
        words.join("-").to_lowercase()
    };

    sanitize_tmux_name(&name)
}

fn sanitize_tmux_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn deduplicate_name(base: &str, existing_names: &[String]) -> String {
    if !existing_names.contains(&base.to_string()) {
        return base.to_string();
    }
    for i in 2..100 {
        let candidate = format!("{}-{}", base, i);
        if !existing_names.contains(&candidate) {
            return candidate;
        }
    }
    format!("{}-{}", base, chrono::Utc::now().timestamp())
}
