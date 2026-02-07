use crate::pod::{format_duration, MemberStatus, Mode, PodStatus};
use crate::tui::app::App;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

const CARD_WIDTH: u16 = 22;
const CARD_HEIGHT: u16 = 6;
const CARD_GAP: u16 = 1;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // ステータスバー用に最下行を確保
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    // 左右分割 (35% / 65%)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(main_chunks[0]);

    // 左ペイン: Context Panel
    render_context_panel(frame, app, chunks[0]);

    // 右ペイン: Pods Grid
    render_pods_grid(frame, app, chunks[1]);

    // ステータスバー
    render_status_bar(frame, app, main_chunks[1]);
}

/// 左ペイン: モードに応じて内容を切り替え
fn render_context_panel(frame: &mut Frame, app: &App, area: Rect) {
    match app.state.mode {
        Mode::Home => render_home(frame, app, area),
        Mode::Detail => render_detail(frame, app, area),
        Mode::Chat => render_chat(frame, app, area),
        Mode::Permission => render_permission(frame, app, area),
        Mode::Help => render_help(frame, app, area),
    }
}

/// Home モード: コマンド入力
fn render_home(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Home ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 || inner.width < 2 {
        return;
    }

    // 上部: コマンド一覧、下部: 入力
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(inner);

    // コマンドヘルプ
    let help_lines = vec![
        Line::from(Span::styled(
            "Commands:",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  create ", Style::default().fg(Color::Green)),
            Span::raw("<name> [--worktree <p>]"),
        ]),
        Line::from(vec![
            Span::styled("  adopt  ", Style::default().fg(Color::Green)),
            Span::raw("<session> [--name <n>]"),
        ]),
        Line::from(vec![
            Span::styled("  drop   ", Style::default().fg(Color::Green)),
            Span::raw("<name>"),
        ]),
        Line::from(vec![
            Span::styled("  forget ", Style::default().fg(Color::Green)),
            Span::raw("<name>"),
        ]),
        Line::from(vec![
            Span::styled("  list   ", Style::default().fg(Color::Green)),
            Span::raw(""),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Keys:",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  arrows  Navigate pods"),
        Line::from("  Enter   Open detail"),
        Line::from("  n       Next warning"),
        Line::from("  /       Command mode"),
        Line::from("  q       Quit"),
    ];

    let help = Paragraph::new(help_lines);
    frame.render_widget(help, sections[0]);

    // ステータスメッセージまたは入力
    let input_text = if let Some(ref msg) = app.state.status_message {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(msg.as_str(), Style::default().fg(Color::Yellow)),
        ])
    } else {
        Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(app.state.command_input.as_str()),
            Span::styled("_", Style::default().fg(Color::Gray)),
        ])
    };

    let input_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));
    let input_inner = input_block.inner(sections[1]);
    frame.render_widget(input_block, sections[1]);
    frame.render_widget(Paragraph::new(input_text), input_inner);
}

/// Pod Detail モード
fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let pod = match app.state.focused_pod() {
        Some(p) => p,
        None => {
            let block = Block::default()
                .title(" Detail ")
                .borders(Borders::ALL);
            let msg = Paragraph::new("No pod selected").block(block);
            frame.render_widget(msg, area);
            return;
        }
    };

    // タイトル: team Pod は member 数を表示
    let title = match pod.pod_type {
        crate::pod::PodType::Solo => format!(" {} {} ", pod.status_icon(), pod.name),
        crate::pod::PodType::Team => format!(" {} {} ({}) ", pod.status_icon(), pod.name, pod.members.len()),
    };
    let block = Block::default()
        .title(title.as_str())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(status_color(&pod.status)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 || inner.width < 2 {
        return;
    }

    // 時間サマリー行
    let total_elapsed = format_duration(pod.total_elapsed_secs());
    let total_working = format_duration(pod.total_working_time());
    let time_summary_height: u16 = 2; // サマリー行 + 空行

    // 上部: 時間サマリー + member 一覧, 下部: 選択 member の出力
    let member_area_height = (pod.members.len() as u16 + time_summary_height + 2).min(inner.height / 2).max(3 + time_summary_height);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(member_area_height), Constraint::Min(3)])
        .split(inner);

    // 時間サマリーを描画
    let mut detail_lines: Vec<Line> = Vec::new();
    detail_lines.push(Line::from(vec![
        Span::styled("Total: ", Style::default().fg(Color::DarkGray)),
        Span::styled(&total_elapsed, Style::default().fg(Color::White)),
        Span::styled(" | Working: ", Style::default().fg(Color::DarkGray)),
        Span::styled(&total_working, Style::default().fg(Color::Blue)),
    ]));
    detail_lines.push(Line::from(""));

    // Member 一覧 (スクロール対応)
    let selected_member = app.state.selected_member.unwrap_or(0);
    let visible_height = sections[0].height as usize;
    let total_members = pod.members.len();

    // 選択 member を中心にスクロールオフセットを計算
    let scroll_offset = if total_members <= visible_height {
        0
    } else {
        let half = visible_height / 2;
        if selected_member < half {
            0
        } else if selected_member + half >= total_members {
            total_members.saturating_sub(visible_height)
        } else {
            selected_member.saturating_sub(half)
        }
    };

    let member_lines: Vec<Line> = pod
        .members
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(i, m)| {
            let marker = if i == selected_member { "> " } else { "  " };
            let style = if i == selected_member {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(marker, style),
                Span::styled(
                    m.status_icon(),
                    Style::default().fg(member_status_color(&m.status)),
                ),
                Span::styled(
                    format!(" {} ", m.role),
                    style.fg(Color::White),
                ),
                Span::styled(
                    m.elapsed(),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        })
        .collect();

    // スクロールインジケーター
    let scroll_indicator = if total_members > visible_height {
        format!(" [{}-{}/{}] ", scroll_offset + 1, (scroll_offset + visible_height).min(total_members), total_members)
    } else {
        String::new()
    };

    if !scroll_indicator.is_empty() {
        let indicator_line = Line::from(Span::styled(
            scroll_indicator,
            Style::default().fg(Color::DarkGray),
        ));
        let mut all_lines = member_lines;
        // 最後の行をインジケーターに置き換え (スクロールが必要な場合)
        if all_lines.len() >= visible_height && visible_height > 1 {
            all_lines.pop();
            all_lines.push(indicator_line);
        }
        detail_lines.extend(all_lines);
        let members_widget = Paragraph::new(detail_lines);
        frame.render_widget(members_widget, sections[0]);
    } else {
        detail_lines.extend(member_lines);
        let members_widget = Paragraph::new(detail_lines);
        frame.render_widget(members_widget, sections[0]);
    }

    // 選択 member の last_output
    let output_text = pod
        .members
        .get(selected_member)
        .map(|m| m.last_output.as_str())
        .unwrap_or("");

    let output_block = Block::default()
        .title(" Output ")
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));

    let output = Paragraph::new(output_text)
        .block(output_block)
        .wrap(Wrap { trim: false });
    frame.render_widget(output, sections[1]);
}

/// Chat モード
fn render_chat(frame: &mut Frame, app: &App, area: Rect) {
    let pod_name = app
        .state
        .focused_pod()
        .map(|p| p.name.as_str())
        .unwrap_or("?");

    let title = format!(" Chat: {} ", pod_name);
    let block = Block::default()
        .title(title.as_str())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 || inner.width < 2 {
        return;
    }

    // 上部: チャット履歴, 下部: 入力
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(inner);

    // Chat 履歴: 各メッセージを複数行に展開
    let available_height = sections[0].height as usize;

    let mut all_lines: Vec<Line> = Vec::new();
    for msg in &app.state.chat_history {
        let sender_color = if msg.sender == "you" {
            Color::Green
        } else {
            Color::Cyan
        };
        let prefix = format!("[{}] ", msg.sender);
        let prefix_len = prefix.len();

        for (i, line) in msg.content.lines().enumerate() {
            if i == 0 {
                // 最初の行: sender prefix 付き
                all_lines.push(Line::from(vec![
                    Span::styled(
                        prefix.clone(),
                        Style::default().fg(sender_color),
                    ),
                    Span::raw(line.to_string()),
                ]));
            } else {
                // 続行行: prefix 分の空白でインデント
                all_lines.push(Line::from(vec![
                    Span::raw(" ".repeat(prefix_len)),
                    Span::raw(line.to_string()),
                ]));
            }
        }
    }

    // 末尾からスクロール表示: 表示可能な行数に収まるように
    let skip = if all_lines.len() > available_height {
        all_lines.len() - available_height
    } else {
        0
    };
    let visible_lines: Vec<Line> = all_lines.into_iter().skip(skip).collect();

    let history = Paragraph::new(visible_lines).wrap(Wrap { trim: false });
    frame.render_widget(history, sections[0]);

    // 入力エリア
    let input_line = Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Cyan)),
        Span::raw(app.state.chat_input.as_str()),
        Span::styled("_", Style::default().fg(Color::Gray)),
    ]);
    let input_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));
    let input_inner = input_block.inner(sections[1]);
    frame.render_widget(input_block, sections[1]);
    frame.render_widget(Paragraph::new(input_line), input_inner);
}

/// Permission モード
fn render_permission(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Permission Required ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 || inner.width < 2 {
        return;
    }

    let mut lines = Vec::new();

    // Pod / Member 情報
    if let Some(pod) = app.state.focused_pod() {
        lines.push(Line::from(vec![
            Span::styled("Pod:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(pod.name.as_str(), Style::default().fg(Color::White)),
        ]));

        if let Some(member) = pod.members.iter().find(|m| m.status == MemberStatus::Permission) {
            lines.push(Line::from(vec![
                Span::styled("Member: ", Style::default().fg(Color::DarkGray)),
                Span::styled(member.role.as_str(), Style::default().fg(Color::White)),
            ]));
        }
    }

    lines.push(Line::from(""));

    // PermissionRequest があれば構造化表示
    if let Some(ref req) = app.state.current_permission {
        lines.push(Line::from(vec![
            Span::styled("Tool:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(req.tool.as_str(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));

        if !req.command.is_empty() {
            lines.push(Line::from(Span::styled("Command:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
            // コマンド内容をコードブロック風に表示
            lines.push(Line::from(Span::styled(
                "\u{250c}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2510}",
                Style::default().fg(Color::DarkGray),
            )));
            for cmd_line in req.command.lines() {
                lines.push(Line::from(vec![
                    Span::styled("\u{2502} ", Style::default().fg(Color::DarkGray)),
                    Span::styled(cmd_line, Style::default().fg(Color::White)),
                ]));
            }
            lines.push(Line::from(Span::styled(
                "\u{2514}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2518}",
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        // フォールバック: last_output の末尾を表示
        if let Some(pod) = app.state.focused_pod() {
            if let Some(member) = pod.members.iter().find(|m| m.status == MemberStatus::Permission) {
                lines.push(Line::from(Span::styled("Tool output:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
                for line in member.last_output.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev() {
                    lines.push(Line::from(Span::styled(line, Style::default().fg(Color::White))));
                }
            }
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("[A]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("pprove  "),
        Span::styled("[D]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw("eny  "),
        Span::styled("[S]", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
        Span::raw("kip  "),
        Span::styled("[Esc]", Style::default().fg(Color::DarkGray)),
        Span::raw(" Back"),
    ]));

    let content = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(content, inner);
}

/// Help モード
fn render_help(frame: &mut Frame, _app: &App, area: Rect) {
    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 || inner.width < 2 {
        return;
    }

    let lines = vec![
        Line::from(Span::styled(
            "Apiary - Claude Code Multi-Session Manager",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Global Keys:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  hjkl        Navigate pods"),
        Line::from("  Enter       Open pod detail"),
        Line::from("  Esc         Back / Home"),
        Line::from("  n           Jump to next pod"),
        Line::from("  /           Command input"),
        Line::from("  ?           Toggle this help"),
        Line::from("  q           Quit"),
        Line::from(""),
        Line::from(Span::styled(
            "Detail Mode:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  j/k         Select member"),
        Line::from("  c           Open chat"),
        Line::from("  p           Permission mode"),
        Line::from(""),
        Line::from(Span::styled(
            "Chat Mode:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  Enter       Send message"),
        Line::from("  Esc         Back to detail"),
        Line::from(""),
        Line::from(Span::styled(
            "Permission Mode:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  a           Approve"),
        Line::from("  d           Deny"),
        Line::from("  s           Skip"),
        Line::from(""),
        Line::from(Span::styled(
            "Commands (/ to enter):",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  create <name> [--worktree <p>]"),
        Line::from("  adopt <session> [--name <n>]"),
        Line::from("  drop <name>"),
        Line::from("  list"),
        Line::from(""),
        Line::from(Span::styled(
            "Press Esc or ? to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let help = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(help, inner);
}

/// 右ペイン: Pod カードのグリッド
fn render_pods_grid(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Pods ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let min_card_height = 4u16;
    if inner.width < CARD_WIDTH || inner.height < min_card_height {
        return;
    }

    if app.state.pods.is_empty() {
        let empty_msg = Paragraph::new(Line::from(vec![
            Span::styled(
                "  No pods. Use ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled("create", Style::default().fg(Color::Green)),
            Span::styled(
                " or ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled("adopt", Style::default().fg(Color::Green)),
            Span::styled(
                " to add pods.",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        frame.render_widget(empty_msg, inner);
        return;
    }

    // カラム数を計算
    let cols = ((inner.width) / (CARD_WIDTH + CARD_GAP)).max(1) as usize;

    let focus_idx = app.state.focus;

    // 行ごとにグループ化して最大カード高さを計算
    let rows: Vec<Vec<(usize, &crate::pod::Pod)>> = app
        .state
        .pods
        .iter()
        .enumerate()
        .collect::<Vec<_>>()
        .chunks(cols)
        .map(|chunk| chunk.to_vec())
        .collect();

    let mut y_offset: u16 = 0;

    for row_pods in &rows {
        // この行の最大カード高さを計算
        let row_height = row_pods
            .iter()
            .map(|(_, pod)| card_height(pod))
            .max()
            .unwrap_or(CARD_HEIGHT);

        for (col_idx, (i, pod)) in row_pods.iter().enumerate() {
            let x = inner.x + (col_idx as u16) * (CARD_WIDTH + CARD_GAP);
            let y = inner.y + y_offset;
            let h = card_height(pod);

            // 描画エリアに収まるかチェック
            if x + CARD_WIDTH > inner.x + inner.width || y + h > inner.y + inner.height {
                continue;
            }

            let card_area = Rect::new(x, y, CARD_WIDTH, h);
            render_pod_card(frame, pod, card_area, focus_idx == Some(*i));
        }

        y_offset += row_height + CARD_GAP;

        // 残りの高さがなければ打ち切り
        if y_offset >= inner.height {
            break;
        }
    }
}

/// Pod の member 数に応じたカード高さを計算
fn card_height(pod: &crate::pod::Pod) -> u16 {
    match pod.pod_type {
        crate::pod::PodType::Solo => CARD_HEIGHT,
        crate::pod::PodType::Team => {
            let member_lines = pod.members.len().min(5) as u16; // 最大5行 (4 members + "+N more")
            (member_lines + 2).max(4).min(8) // border 2行分 + member行、最小4最大8
        }
    }
}

/// 個々の Pod カードを描画
fn render_pod_card(frame: &mut Frame, pod: &crate::pod::Pod, area: Rect, focused: bool) {
    let border_color = status_color(&pod.status);
    let border_style = if focused {
        Style::default()
            .fg(border_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(border_color)
    };

    // タイトル: team Pod は member 数を表示
    let title = match pod.pod_type {
        crate::pod::PodType::Solo => format!(" {} {} ", pod.name, pod.status_icon()),
        crate::pod::PodType::Team => format!(" {} {} ({}) ", pod.name, pod.status_icon(), pod.members.len()),
    };

    let block = Block::default()
        .title(title.as_str())
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 1 || inner.width < 2 {
        return;
    }

    let mut lines = Vec::new();
    let max_display_members = (inner.height as usize).min(4);

    for (i, member) in pod.members.iter().enumerate() {
        if i >= max_display_members {
            let remaining = pod.members.len() - max_display_members;
            lines.push(Line::from(Span::styled(
                format!("+{} more", remaining),
                Style::default().fg(Color::DarkGray),
            )));
            break;
        }

        // role名を固定幅で表示
        let role_display = if member.role.len() > 10 {
            format!("{:.10}", member.role)
        } else {
            format!("{:<10}", member.role)
        };

        lines.push(Line::from(vec![
            Span::styled(
                member.status_icon(),
                Style::default().fg(member_status_color(&member.status)),
            ),
            Span::raw(" "),
            Span::styled(
                role_display,
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!(" {}", member.elapsed()),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    let content = Paragraph::new(lines);
    frame.render_widget(content, inner);
}

/// ステータスバー
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let (total_pods, warnings, total_members) = app.state.pods_summary();
    let total_working: u64 = app.state.pods.iter().map(|p| p.total_working_time()).sum();

    let bar = Line::from(vec![
        Span::styled(
            " apiary ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{} pods", total_pods),
            Style::default().fg(Color::White),
        ),
        Span::styled(" / ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{} warnings", warnings),
            if warnings > 0 {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
        Span::styled(" / ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{} members", total_members),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            format!(" | Work: {}", format_duration(total_working)),
            Style::default().fg(Color::Blue),
        ),
        Span::styled(
            format!(
                " | Mode: {:?}",
                app.state.mode
            ),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let status_bar = Paragraph::new(bar)
        .style(Style::default().bg(Color::Black));
    frame.render_widget(status_bar, area);
}

/// PodStatus に応じた色を返す
fn status_color(status: &PodStatus) -> Color {
    match status {
        PodStatus::Permission => Color::Yellow,
        PodStatus::Error => Color::Red,
        PodStatus::Working => Color::Blue,
        PodStatus::Idle => Color::DarkGray,
        PodStatus::Done => Color::Green,
    }
}

/// MemberStatus に応じた色を返す
fn member_status_color(status: &MemberStatus) -> Color {
    match status {
        MemberStatus::Permission => Color::Yellow,
        MemberStatus::Error => Color::Red,
        MemberStatus::Working => Color::Blue,
        MemberStatus::Idle => Color::DarkGray,
        MemberStatus::Done => Color::Green,
    }
}
