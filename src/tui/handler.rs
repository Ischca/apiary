use crate::pod::Mode;
use crate::tui::app::{App, Direction};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum Action {
    None,
    Quit,
    Render,
}

pub fn handle_key_event(app: &mut App, key: KeyEvent) -> Action {
    // Ctrl+C は常に終了
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Action::Quit;
    }

    // ? キーは全モードで Help トグル (ただし Chat/Home の入力モード中は除く)
    if key.code == KeyCode::Char('?') {
        match app.state.mode {
            Mode::Help => {
                app.state.mode = app.state.previous_mode.clone().unwrap_or(Mode::Home);
                app.state.previous_mode = None;
                return Action::Render;
            }
            Mode::Home if !app.state.command_input.is_empty() => {
                // コマンド入力中は ? を文字として処理 (下の match に流す)
            }
            Mode::Chat => {
                // Chat モードでは ? を文字として処理 (下の match に流す)
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

fn handle_home_keys(app: &mut App, key: KeyEvent) -> Action {
    // コマンド入力モード中
    if !app.state.command_input.is_empty() {
        return handle_command_input(app, key);
    }

    match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('/') => {
            // コマンド入力モードに入る (空文字列で開始はしない、/ は表示用)
            // command_input にスペースを入れてアクティブにする代わりに
            // フラグとして空文字でない状態を作る
            app.state.command_input = String::new();
            // 実際にはここで "/" を打ったら入力モードに入るが、
            // command_input が空のままだとすぐ通常モードに戻るので
            // 最初の "/" は飲み込んで入力モードのマーカーとする
            app.state.command_input.push(' ');
            app.state.status_message = None;
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
        KeyCode::Enter => {
            if app.state.focus.is_some() {
                // Permission 状態なら Permission モードへ
                let is_permission = app
                    .state
                    .focused_pod()
                    .map(|p| p.status == crate::pod::PodStatus::Permission)
                    .unwrap_or(false);
                if is_permission {
                    app.state.mode = Mode::Permission;
                } else {
                    app.state.mode = Mode::Detail;
                }
                app.state.selected_member = Some(0);
            }
            Action::Render
        }
        KeyCode::Char('n') => {
            // 次の Permission Pod にジャンプ (wrap-around)
            if let Some(idx) = app.next_permission_pod_from_current() {
                app.state.focus = Some(idx);
            }
            Action::Render
        }
        _ => Action::None,
    }
}

/// コマンド入力モードのキー処理
fn handle_command_input(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.state.command_input.clear();
            app.state.status_message = None;
            Action::Render
        }
        KeyCode::Enter => {
            let cmd = app.state.command_input.trim().to_string();
            app.state.command_input.clear();

            if cmd.is_empty() {
                return Action::Render;
            }

            match app.execute_command(&cmd) {
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
            Action::Render
        }
        KeyCode::Backspace => {
            app.state.command_input.pop();
            Action::Render
        }
        KeyCode::Char(c) => {
            app.state.command_input.push(c);
            Action::Render
        }
        _ => Action::None,
    }
}

fn handle_detail_keys(app: &mut App, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => {
            app.state.mode = Mode::Home;
            app.state.selected_member = None;
            Action::Render
        }
        KeyCode::Char('c') => {
            app.state.mode = Mode::Chat;
            app.state.chat_input.clear();
            Action::Render
        }
        KeyCode::Char('p') => {
            // Permission モードへ (該当 member がいる場合)
            let has_permission = app
                .state
                .focused_pod()
                .map(|p| {
                    p.members
                        .iter()
                        .any(|m| m.status == crate::pod::MemberStatus::Permission)
                })
                .unwrap_or(false);
            if has_permission {
                app.state.mode = Mode::Permission;
            }
            Action::Render
        }
        KeyCode::Up | KeyCode::Char('k') => {
            // member 選択を上に移動
            if let Some(ref mut sel) = app.state.selected_member {
                if *sel > 0 {
                    *sel -= 1;
                }
            }
            Action::Render
        }
        KeyCode::Down | KeyCode::Char('j') => {
            // member 選択を下に移動
            let member_count = app
                .state
                .focused_pod()
                .map(|p| p.members.len())
                .unwrap_or(0);
            if let Some(ref mut sel) = app.state.selected_member {
                if *sel + 1 < member_count {
                    *sel += 1;
                }
            }
            Action::Render
        }
        _ => Action::None,
    }
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
