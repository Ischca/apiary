use crate::pod::{InlinePrompt, Mode, PaneFocus};
use crate::tui::app::{App, Direction, generate_pod_name};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum Action {
    None,
    Quit,
    Render,
    AttachTmux(String),
}

pub fn handle_key_event(app: &mut App, key: KeyEvent) -> Action {
    // Detail モード (パススルー) では Ctrl+C も pane に転送するため、ここでは除外
    if app.state.mode != Mode::Detail {
        // Ctrl+C は終了
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
    }

    // ? キーは全モードで Help トグル (ただし Chat/Home の入力モード中/Detail パススルー中は除く)
    if key.code == KeyCode::Char('?') {
        match app.state.mode {
            Mode::Help => {
                app.state.mode = app.state.previous_mode.clone().unwrap_or(Mode::Home);
                app.state.previous_mode = None;
                return Action::Render;
            }
            Mode::Home if app.state.pane_focus == PaneFocus::Left
                || app.state.inline_prompt != InlinePrompt::None => {
                // 左ペイン入力中またはインラインプロンプト中は ? を文字として処理
            }
            Mode::Chat | Mode::Detail => {
                // Chat モード / Detail パススルーモードでは ? を文字として処理
            }
            _ => {
                app.state.previous_mode = Some(app.state.mode.clone());
                app.state.mode = Mode::Help;
                return Action::Render;
            }
        }
    }

    match app.state.mode {
        Mode::Home => handle_home_keys(app, key),
        Mode::Detail => handle_detail_keys(app, key),
        Mode::Chat => handle_chat_keys(app, key),
        Mode::Permission => handle_permission_keys(app, key),
        Mode::Help => handle_help_keys(app, key),
    }
}

pub fn handle_paste_event(app: &mut App, text: &str) {
    match app.state.mode {
        Mode::Home => {
            if app.state.inline_prompt == InlinePrompt::None {
                app.state.pane_focus = PaneFocus::Left;
                if app.state.inline_input.is_empty() {
                    app.state.status_message = None;
                }
            }
            if app.state.inline_prompt == InlinePrompt::None
                || matches!(app.state.inline_prompt, InlinePrompt::AdoptSession)
            {
                app.state.inline_input.push_str(text);
            }
        }
        Mode::Chat => {
            app.state.chat_input.push_str(text);
        }
        Mode::Detail => {
            if let Err(e) = app.forward_paste_to_pane(text) {
                app.state.status_message = Some(format!("Paste error: {}", e));
            }
        }
        _ => {}
    }
}

fn handle_home_keys(app: &mut App, key: KeyEvent) -> Action {
    // インラインプロンプト中 (drop 確認, adopt)
    if app.state.inline_prompt != InlinePrompt::None {
        return handle_inline_prompt(app, key);
    }

    match app.state.pane_focus {
        PaneFocus::Right => handle_home_right_keys(app, key),
        PaneFocus::Left => handle_home_left_keys(app, key),
    }
}

/// 右ペインフォーカス時: Pod ナビゲーション + ショートカット
fn handle_home_right_keys(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Tab | KeyCode::Char('n') => {
            // 左ペインにフォーカス切り替え
            app.state.pane_focus = PaneFocus::Left;
            app.state.inline_input.clear();
            Action::Render
        }
        KeyCode::Left => {
            app.move_focus(Direction::Left);
            Action::Render
        }
        KeyCode::Right => {
            app.move_focus(Direction::Right);
            Action::Render
        }
        KeyCode::Up => {
            app.move_focus(Direction::Up);
            Action::Render
        }
        KeyCode::Down => {
            app.move_focus(Direction::Down);
            Action::Render
        }
        KeyCode::Char('h') => {
            app.move_focus(Direction::Left);
            Action::Render
        }
        KeyCode::Char('l') => {
            app.move_focus(Direction::Right);
            Action::Render
        }
        KeyCode::Char('k') => {
            app.move_focus(Direction::Up);
            Action::Render
        }
        KeyCode::Char('j') => {
            app.move_focus(Direction::Down);
            Action::Render
        }
        KeyCode::Enter | KeyCode::Char('i') => {
            // Detail モード (Permission 状態なら Permission モードへ)
            if let Some(pod) = app.state.focused_pod() {
                if pod.status == crate::pod::PodStatus::Permission {
                    app.state.mode = Mode::Permission;
                } else {
                    app.state.mode = Mode::Detail;
                    app.state.selected_member = Some(0);
                    app.start_detail_pty_stream();
                }
                app.state.selected_member = Some(0);
                app.state.chat_input.clear();
            }
            Action::Render
        }
        KeyCode::Char('t') => {
            // tmux セッションにアタッチ
            if let Some(pod) = app.state.focused_pod() {
                let session = pod.tmux_session.clone();
                return Action::AttachTmux(session);
            }
            Action::Render
        }
        KeyCode::Char('N') => {
            // 次の Permission Pod にジャンプ
            if let Some(idx) = app.next_permission_pod_from_current() {
                app.state.focus = Some(idx);
            }
            Action::Render
        }
        KeyCode::Char('a') => {
            // Adopt セッション (インラインプロンプト)
            app.state.inline_prompt = InlinePrompt::AdoptSession;
            app.state.inline_input.clear();
            app.state.status_message = None;
            Action::Render
        }
        KeyCode::Char('d') => {
            // Drop 確認 (インラインプロンプト)
            if let Some(pod) = app.state.focused_pod() {
                let name = pod.name.clone();
                app.state.inline_prompt = InlinePrompt::DropConfirm(name);
                app.state.inline_input.clear();
                app.state.status_message = None;
            }
            Action::Render
        }
        KeyCode::Char('p') => {
            // ディレクトリブラウザを開く
            app.open_browser(None);
            Action::Render
        }
        KeyCode::Char(c) => {
            // ショートカットに該当しない文字 → 左ペインに切り替えて1文字目として入力
            app.state.pane_focus = PaneFocus::Left;
            app.state.inline_input.clear();
            app.state.inline_input.push(c);
            Action::Render
        }
        _ => Action::None,
    }
}

/// 左ペインフォーカス時: 指示入力 + スラッシュコマンド
fn handle_home_left_keys(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc | KeyCode::Tab => {
            app.state.pane_focus = PaneFocus::Right;
            Action::Render
        }
        KeyCode::Enter => {
            let input = app.state.inline_input.trim().to_string();
            app.state.inline_input.clear();

            if input.is_empty() {
                return Action::Render;
            }

            if let Some(cmd) = input.strip_prefix('/') {
                // スラッシュコマンド: 先頭の / を取り除いて execute_command に渡す
                match app.execute_command(cmd) {
                    Ok(msg) => {
                        app.state.status_message = if msg.is_empty() {
                            None
                        } else {
                            Some(msg)
                        };
                    }
                    Err(e) => {
                        app.state.status_message = Some(format!("Error: {}", e));
                    }
                }
            } else {
                // 指示 → Pod 自動作成
                let (instruction, project_input) = parse_at_project(&input);
                let names: Vec<String> = app.state.pods.iter().map(|p| p.name.clone()).collect();
                let name = generate_pod_name(&instruction, &names);
                match app.create_pod(&name, project_input.as_deref(), None, Some(&instruction)) {
                    Ok(()) => {
                        // 新しい Pod にフォーカス
                        let new_idx = app.state.pods.len().saturating_sub(1);
                        app.state.focus = Some(new_idx);
                        app.state.status_message = Some(format!("Pod '{}' created", name));
                    }
                    Err(e) => {
                        app.state.status_message = Some(format!("Error: {}", e));
                    }
                }
            }

            Action::Render
        }
        KeyCode::Backspace => {
            app.state.inline_input.pop();
            Action::Render
        }
        KeyCode::Char(c) => {
            // 入力開始時に前回の結果メッセージをクリア
            if app.state.inline_input.is_empty() {
                app.state.status_message = None;
            }
            app.state.inline_input.push(c);
            Action::Render
        }
        _ => Action::None,
    }
}

/// "instruction @project" 構文をパース
fn parse_at_project(input: &str) -> (String, Option<String>) {
    if let Some(at_pos) = input.rfind('@') {
        let instruction = input[..at_pos].trim().to_string();
        let project = input[at_pos + 1..].trim().to_string();
        if project.is_empty() {
            (input.to_string(), None)
        } else {
            (instruction, Some(project))
        }
    } else {
        (input.to_string(), None)
    }
}

/// インラインプロンプトのキー処理
fn handle_inline_prompt(app: &mut App, key: KeyEvent) -> Action {
    if app.state.inline_prompt == InlinePrompt::Browse {
        return handle_browser_keys(app, key);
    }

    match key.code {
        KeyCode::Esc => {
            app.state.inline_prompt = InlinePrompt::None;
            app.state.inline_input.clear();
            Action::Render
        }
        KeyCode::Enter => {
            let input = app.state.inline_input.trim().to_string();
            let prompt = app.state.inline_prompt.clone();
            app.state.inline_prompt = InlinePrompt::None;
            app.state.inline_input.clear();

            match prompt {
                InlinePrompt::AdoptSession => {
                    if input.is_empty() {
                        return Action::Render;
                    }
                    let parts: Vec<&str> = input.split_whitespace().collect();
                    let session = parts[0];
                    let group = parts
                        .iter()
                        .position(|&p| p == "--group")
                        .and_then(|i| parts.get(i + 1))
                        .copied();
                    match app.adopt_session(session, None, group) {
                        Ok(()) => {
                            app.state.status_message =
                                Some(format!("Session '{}' adopted", session));
                        }
                        Err(e) => {
                            app.state.status_message = Some(format!("Error: {}", e));
                        }
                    }
                }
                InlinePrompt::DropConfirm(name) => {
                    if input == "y" || input == "yes" {
                        match app.drop_pod(&name) {
                            Ok(()) => {
                                app.state.status_message = Some(format!("Pod '{}' dropped", name));
                            }
                            Err(e) => {
                                app.state.status_message = Some(format!("Error: {}", e));
                            }
                        }
                    } else {
                        app.state.status_message = Some("Drop cancelled".to_string());
                    }
                }
                InlinePrompt::Browse => {} // handled above
                InlinePrompt::None => {}
            }
            Action::Render
        }
        KeyCode::Backspace => {
            app.state.inline_input.pop();
            Action::Render
        }
        KeyCode::Char(c) => {
            app.state.inline_input.push(c);
            Action::Render
        }
        _ => Action::None,
    }
}

/// ブラウザモードのキー処理
fn handle_browser_keys(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.browser_cancel();
            Action::Render
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(bs) = &mut app.state.browser_state {
                if !bs.entries.is_empty() && bs.selected < bs.entries.len() - 1 {
                    bs.selected += 1;
                }
            }
            Action::Render
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if let Some(bs) = &mut app.state.browser_state {
                if bs.selected > 0 {
                    bs.selected -= 1;
                }
            }
            Action::Render
        }
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
            app.browser_enter_dir();
            Action::Render
        }
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => {
            app.browser_go_parent();
            Action::Render
        }
        KeyCode::Char(' ') => {
            match app.browser_select_current() {
                Ok(msg) => {
                    app.state.status_message = Some(msg);
                }
                Err(e) => {
                    app.state.status_message = Some(format!("Error: {}", e));
                }
            }
            Action::Render
        }
        _ => Action::None,
    }
}

fn handle_detail_keys(app: &mut App, key: KeyEvent) -> Action {
    // Esc でパススルー終了 → Home に戻る
    if key.code == KeyCode::Esc {
        app.restore_detail_window_size();
        app.state.mode = Mode::Home;
        app.state.selected_member = None;
        return Action::Render;
    }

    // Pod が Dead なら Home に戻す (dead pane にキーを送っても意味がない)
    let is_dead = app.state.focused_pod()
        .map(|p| p.status == crate::pod::PodStatus::Dead)
        .unwrap_or(true);
    if is_dead {
        app.restore_detail_window_size();
        app.state.mode = Mode::Home;
        app.state.selected_member = None;
        return Action::Render;
    }

    // 全キーを pane に転送 (パススルーモード)
    if let Err(e) = app.forward_key_to_pane(&key) {
        app.state.status_message = Some(format!("Key error: {}", e));
    }
    Action::Render
}

fn handle_chat_keys(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.state.mode = Mode::Detail;
            Action::Render
        }
        KeyCode::Enter => {
            if !app.state.chat_input.is_empty() {
                if let Err(e) = app.send_chat_message() {
                    app.state.status_message = Some(format!("Send error: {}", e));
                }
            }
            Action::Render
        }
        KeyCode::Backspace => {
            app.state.chat_input.pop();
            Action::Render
        }
        KeyCode::Char(c) => {
            app.state.chat_input.push(c);
            Action::Render
        }
        _ => Action::None,
    }
}

fn handle_help_keys(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc | KeyCode::Char('?') => {
            app.state.mode = app.state.previous_mode.clone().unwrap_or(Mode::Home);
            app.state.previous_mode = None;
            Action::Render
        }
        _ => Action::None,
    }
}

fn handle_permission_keys(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.state.mode = Mode::Detail;
            Action::Render
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            if let Err(e) = app.approve_permission() {
                app.state.status_message = Some(format!("Approve error: {}", e));
            } else {
                app.state.status_message = Some("Permission approved".to_string());
                app.state.mode = Mode::Detail;
            }
            Action::Render
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            if let Err(e) = app.deny_permission() {
                app.state.status_message = Some(format!("Deny error: {}", e));
            } else {
                app.state.status_message = Some("Permission denied".to_string());
                app.state.mode = Mode::Detail;
            }
            Action::Render
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            // Skip - Detail に戻るだけ
            app.state.mode = Mode::Detail;
            Action::Render
        }
        _ => Action::None,
    }
}
