use crate::pod::{format_duration, BrowserState, InlinePrompt, MemberStatus, Mode, PaneFocus, PodStatus};
use crate::tui::app::App;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const CARD_WIDTH: u16 = 20;
const CARD_HEIGHT: u16 = 8;
const CARD_GAP: u16 = 1;
const DEAD_CARD_HEIGHT: u16 = 4;

/// 文字列を指定した表示幅に切り詰める（CJK文字対応）
/// 幅を超える場合は末尾を "…" に置き換える
fn truncate_to_width(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }
    let mut result = String::new();
    let mut current_width = 0;
    let ellipsis_width = 1; // "…" is 1 column wide in most terminals
    let target = max_width.saturating_sub(ellipsis_width);
    for ch in s.chars() {
        let w = ch.width().unwrap_or(0);
        if current_width + w > target {
            break;
        }
        result.push(ch);
        current_width += w;
    }
    result.push('…');
    result
}

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // ステータスバー用に最下2行を確保
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
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

/// Home モード: 指示入力 + ガイド
fn render_home(frame: &mut Frame, app: &App, area: Rect) {
    if app.state.inline_prompt == InlinePrompt::Browse {
        if let Some(ref bs) = app.state.browser_state {
            render_browser(frame, bs, area);
            return;
        }
    }

    let is_focused = app.state.pane_focus == PaneFocus::Left;
    let border_color = if is_focused { Color::Cyan } else { Color::DarkGray };

    let block = Block::default()
        .title(" New Task ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 || inner.width < 2 {
        return;
    }

    // 入力テキストの表示行数を計算（折り返し考慮）
    let input_width = inner.width.saturating_sub(3) as usize; // "> " prefix + margin
    let input_lines = if input_width > 0 && !app.state.inline_input.is_empty() {
        // Unicode 表示幅ベースで行数を推定（CJK文字は2カラム幅）
        let text_width = format!("> {}_", app.state.inline_input).width();
        (text_width / input_width.max(1)) + 1
    } else {
        1
    };
    let input_height = (input_lines as u16 + 1).max(2); // +1 for border, min 2

    // 3分割: ワークスペース表示 / ガイドテキスト / 入力
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(3), Constraint::Length(input_height)])
        .split(inner);

    // ワークスペース表示
    let workspace_line = if let Some(ref project) = app.state.current_project {
        let max_len = (inner.width as usize).saturating_sub(4);
        let path = &project.path;
        let display_path = if path.len() > max_len {
            format!("...{}", &path[path.len() - max_len + 3..])
        } else {
            path.clone()
        };
        Line::from(vec![
            Span::styled(" \u{1f4c2} ", Style::default().fg(Color::Cyan)),
            Span::styled(display_path, Style::default().fg(Color::White)),
        ])
    } else {
        Line::from(Span::styled(
            " No workspace set (p to browse)",
            Style::default().fg(Color::DarkGray),
        ))
    };
    let workspace = Paragraph::new(vec![workspace_line, Line::from("")]);
    frame.render_widget(workspace, sections[0]);

    // ガイドテキスト
    let guide_lines = if let Some(ref msg) = app.state.status_message {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                msg.as_str(),
                Style::default().fg(Color::Yellow),
            )),
        ]
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Give an instruction",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "  to start Claude.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  /drop, /adopt, /forget",
                Style::default().fg(Color::Rgb(80, 85, 95)),
            )),
            Line::from(Span::styled(
                "  /project, /browse, /list",
                Style::default().fg(Color::Rgb(80, 85, 95)),
            )),
            Line::from(Span::styled(
                "  for commands",
                Style::default().fg(Color::Rgb(80, 85, 95)),
            )),
        ]
    };

    let guide = Paragraph::new(guide_lines);
    frame.render_widget(guide, sections[1]);

    // 入力エリア
    let cursor_style = if is_focused {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let input_text = Line::from(vec![
        Span::styled("> ", Style::default().fg(if is_focused { Color::Cyan } else { Color::DarkGray })),
        Span::styled(app.state.inline_input.as_str(), cursor_style),
        if is_focused {
            Span::styled("_", Style::default().fg(Color::Gray))
        } else {
            Span::styled("", Style::default())
        },
    ]);

    let input_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));
    let input_inner = input_block.inner(sections[2]);
    frame.render_widget(input_block, sections[2]);
    frame.render_widget(Paragraph::new(input_text).wrap(Wrap { trim: false }), input_inner);
}

/// ディレクトリブラウザ
fn render_browser(frame: &mut Frame, bs: &BrowserState, area: Rect) {
    // タイトル: 現在のパス（長ければ末尾を表示）
    let path_str = bs.current_path.to_string_lossy().to_string();
    let max_title_len = (area.width as usize).saturating_sub(4);
    let display_path = if path_str.len() > max_title_len {
        format!("...{}", &path_str[path_str.len() - max_title_len + 3..])
    } else {
        path_str
    };
    let title = format!(" {} ", display_path);

    let block = Block::default()
        .title(title.as_str())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 2 || inner.width < 2 {
        return;
    }

    // 本体 + ヒントバー
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let visible_height = sections[0].height as usize;

    if bs.entries.is_empty() {
        let empty = Paragraph::new(Line::from(Span::styled(
            "  (empty directory)",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(empty, sections[0]);
    } else {
        // スクロールオフセットを計算
        let scroll_offset = if bs.selected >= visible_height {
            bs.selected - visible_height + 1
        } else {
            0
        };

        let lines: Vec<Line> = bs.entries.iter().enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, entry)| {
                let is_selected = i == bs.selected;
                let (prefix, name_color) = if entry.is_dir {
                    ("/ ", Color::Blue)
                } else {
                    ("  ", Color::Rgb(160, 165, 175))
                };

                let style = if is_selected {
                    Style::default().fg(Color::White).bg(Color::Rgb(40, 60, 100)).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(name_color)
                };

                let marker = if is_selected { "> " } else { "  " };
                Line::from(vec![
                    Span::styled(marker, if is_selected {
                        Style::default().fg(Color::Cyan).bg(Color::Rgb(40, 60, 100))
                    } else {
                        Style::default().fg(Color::DarkGray)
                    }),
                    Span::styled(prefix, style),
                    Span::styled(entry.name.as_str(), style),
                ])
            })
            .collect();

        let list = Paragraph::new(lines);
        frame.render_widget(list, sections[0]);
    }

    // ヒントバー
    let hint = Line::from(vec![
        Span::styled(" ↑↓", Style::default().fg(Color::Cyan)),
        Span::styled("Nav ", Style::default().fg(Color::DarkGray)),
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::styled("Open ", Style::default().fg(Color::DarkGray)),
        Span::styled("h", Style::default().fg(Color::Cyan)),
        Span::styled("Parent ", Style::default().fg(Color::DarkGray)),
        Span::styled("Space", Style::default().fg(Color::Cyan)),
        Span::styled("Select ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::styled("Cancel", Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(hint), sections[1]);
}

/// Pod Detail モード: パススルー + ANSI カラー表示
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

    let selected_member = app.state.selected_member.unwrap_or(0);

    // タイトル: ステータスアイコン + Pod名 + 経過時間 + subagent数 + Esc exit
    // Pod名をブロック幅に収まるよう切り詰め（CJK対応）
    let icon = pod.status_icon();
    let elapsed = pod.elapsed_time();
    let sub_count = pod.total_sub_agents();
    let sub_info = if sub_count > 0 {
        format!(" \u{26a1}{}", sub_count)
    } else {
        String::new()
    };
    let member_info = if pod.members.len() > 1 {
        let member_name = pod.members.get(selected_member)
            .map(|m| m.role.as_str())
            .unwrap_or("?");
        // 固定部分: " icon  elapsed sub_info [member]  Esc exit "
        let fixed_width = format!(" {}  {}{} [{}]  Esc exit ", icon, elapsed, sub_info, member_name).width();
        let available = (area.width as usize).saturating_sub(fixed_width + 2); // +2 for borders
        let name = truncate_to_width(&pod.name, available.max(1));
        format!(" {} {} {}{} [{}]  Esc exit ", icon, name, elapsed, sub_info, member_name)
    } else {
        // 固定部分: " icon  elapsed sub_info  Esc exit "
        let fixed_width = format!(" {}  {}{}  Esc exit ", icon, elapsed, sub_info).width();
        let available = (area.width as usize).saturating_sub(fixed_width + 2);
        let name = truncate_to_width(&pod.name, available.max(1));
        format!(" {} {} {}{}  Esc exit ", icon, name, elapsed, sub_info)
    };

    let block = Block::default()
        .title(member_info.as_str())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(status_color(&pod.status)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 1 || inner.width < 2 {
        return;
    }

    // ストリームがあればその永続パーサーから描画
    if let Some(ref stream) = app.detail_pty_stream {
        let screen = stream.screen();
        let (pane_cols, pane_rows) = stream.size();
        let start_row = pane_rows.saturating_sub(inner.height);
        let display_cols = inner.width.min(pane_cols);
        let lines: Vec<Line> = (0..inner.height)
            .map(|r| render_vt100_row(screen, start_row + r, display_cols))
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    // フォールバック: 既存の last_output_ansi パース
    let ansi_output = pod.members.get(selected_member)
        .map(|m| m.last_output_ansi.as_str())
        .unwrap_or("");

    if ansi_output.is_empty() {
        let output_text = pod.members.get(selected_member)
            .map(|m| m.last_output.as_str())
            .unwrap_or("");
        let output_lines: Vec<&str> = output_text.lines().collect();
        let skip = output_lines.len().saturating_sub(inner.height as usize);
        let visible_lines: Vec<Line> = output_lines
            .iter()
            .skip(skip)
            .map(|line| Line::from(Span::raw(*line)))
            .collect();
        let output = Paragraph::new(visible_lines);
        frame.render_widget(output, inner);
        return;
    }

    let (pane_cols, pane_rows) = pod.members.get(selected_member)
        .map(|m| m.pane_size)
        .unwrap_or((inner.width, inner.height));

    let parse_cols = if pane_cols > 0 { pane_cols } else { inner.width };
    let parse_rows = if pane_rows > 0 { pane_rows } else { inner.height };

    let mut parser = vt100::Parser::new(parse_rows, parse_cols, 0);
    parser.process(ansi_output.as_bytes());
    let screen = parser.screen();

    let start_row = parse_rows.saturating_sub(inner.height);
    let display_cols = inner.width.min(parse_cols);

    let lines: Vec<Line> = (0..inner.height)
        .map(|r| render_vt100_row(screen, start_row + r, display_cols))
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

/// vt100::Screen の 1 行を ratatui::Line に変換するヘルパー
fn render_vt100_row(screen: &vt100::Screen, row: u16, display_cols: u16) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    let mut col: u16 = 0;
    while col < display_cols {
        if let Some(cell) = screen.cell(row, col) {
            if cell.is_wide_continuation() {
                col += 1;
                continue;
            }

            let fg = convert_vt100_color(cell.fgcolor());
            let bg = convert_vt100_color(cell.bgcolor());
            let mut style = Style::default().fg(fg).bg(bg);
            if cell.bold() { style = style.add_modifier(Modifier::BOLD); }
            if cell.italic() { style = style.add_modifier(Modifier::ITALIC); }
            if cell.underline() { style = style.add_modifier(Modifier::UNDERLINED); }
            if cell.inverse() { style = style.add_modifier(Modifier::REVERSED); }

            let contents = cell.contents();
            if contents.is_empty() {
                spans.push(Span::styled(" ".to_string(), style));
                col += 1;
            } else if cell.is_wide() {
                if col + 2 > display_cols {
                    spans.push(Span::styled(" ".to_string(), style));
                    col += 1;
                } else {
                    spans.push(Span::styled(contents.to_string(), style));
                    col += 2;
                }
            } else {
                spans.push(Span::styled(contents.to_string(), style));
                col += 1;
            }
        } else {
            spans.push(Span::styled(" ".to_string(), Style::default()));
            col += 1;
        }
    }
    Line::from(spans)
}

/// vt100::Color → ratatui::Color 変換
fn convert_vt100_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(idx) => {
            // 標準 16 色をマッピング
            match idx {
                0 => Color::Black,
                1 => Color::Red,
                2 => Color::Green,
                3 => Color::Yellow,
                4 => Color::Blue,
                5 => Color::Magenta,
                6 => Color::Cyan,
                7 => Color::White,
                8 => Color::DarkGray,
                9 => Color::LightRed,
                10 => Color::LightGreen,
                11 => Color::LightYellow,
                12 => Color::LightBlue,
                13 => Color::LightMagenta,
                14 => Color::LightCyan,
                15 => Color::White,
                _ => Color::Indexed(idx),
            }
        }
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
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
            "Home (Right Pane):",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  hjkl/arrows Navigate pods"),
        Line::from("  Enter/i     Open pod detail"),
        Line::from("  t           Attach tmux session"),
        Line::from("  n/Tab       New task (left pane)"),
        Line::from("  a           Adopt session"),
        Line::from("  d           Drop pod"),
        Line::from("  p           Browse directories"),
        Line::from("  N           Next warning pod"),
        Line::from("  ?           Toggle this help"),
        Line::from("  q           Quit"),
        Line::from(""),
        Line::from(Span::styled(
            "Home (Left Pane - Input):",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  Type        Instruction for Claude"),
        Line::from("  Enter       Create pod & send"),
        Line::from("  /cmd        Slash commands"),
        Line::from("  @project    Specify project"),
        Line::from("  Esc/Tab     Back to right pane"),
        Line::from(""),
        Line::from(Span::styled(
            "Detail Mode (Passthrough):",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  All keys    Forwarded to pane"),
        Line::from("  Esc         Back to Home"),
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
            "Slash Commands (in left pane):",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("  /create <name> [--project <p>]"),
        Line::from("  /adopt <session> [--name <n>]"),
        Line::from("  /drop <name>"),
        Line::from("  /forget <name>"),
        Line::from("  /list"),
        Line::from("  /project list|add|remove"),
        Line::from("  /browse"),
        Line::from(""),
        Line::from(Span::styled(
            "Press Esc or ? to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let help = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(help, inner);
}

/// 右ペイン: Pod カードのグリッド（グループ / 非グループ / Dead の3セクション）
fn render_pods_grid(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.state.pane_focus == PaneFocus::Right;
    let border_color = if is_focused { Color::Cyan } else { Color::DarkGray };

    let block = Block::default()
        .title(" Pods ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let min_card_height = 4u16;
    if inner.width < CARD_WIDTH || inner.height < min_card_height {
        return;
    }

    if app.state.pods.is_empty() {
        let empty_msg = Paragraph::new(Line::from(vec![
            Span::styled(
                "  No pods. Type an instruction or press ",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled("n", Style::default().fg(Color::Green)),
            Span::styled(
                " to start.",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        frame.render_widget(empty_msg, inner);
        return;
    }

    let cols = (inner.width / (CARD_WIDTH + CARD_GAP)).max(1) as usize;
    let focus_idx = app.state.focus;

    // Pod をカテゴリ分け: グループ / 非グループ / Dead
    let mut group_order: Vec<String> = Vec::new();
    let mut group_map: std::collections::HashMap<String, Vec<(usize, &crate::pod::Pod)>> =
        std::collections::HashMap::new();
    let mut ungrouped: Vec<(usize, &crate::pod::Pod)> = Vec::new();
    let mut dead: Vec<(usize, &crate::pod::Pod)> = Vec::new();

    for (i, pod) in app.state.pods.iter().enumerate() {
        if pod.status == PodStatus::Dead {
            dead.push((i, pod));
        } else if let Some(ref group) = pod.group {
            if !group_map.contains_key(group) {
                group_order.push(group.clone());
            }
            group_map.entry(group.clone()).or_default().push((i, pod));
        } else {
            ungrouped.push((i, pod));
        }
    }

    let mut y_offset: u16 = 0;

    // --- グループ描画 ---
    for group_name in &group_order {
        let group_pods = &group_map[group_name];
        // グループ内のカラム数（ボーダー分 2 を引く）
        let cols_in_group = ((inner.width.saturating_sub(2)) / (CARD_WIDTH + CARD_GAP)).max(1) as usize;
        let num_rows = (group_pods.len() + cols_in_group - 1) / cols_in_group;
        let group_height = 2 + (num_rows as u16) * (CARD_HEIGHT + CARD_GAP) - CARD_GAP;

        if y_offset + group_height > inner.height {
            break;
        }

        let group_area = Rect::new(inner.x, inner.y + y_offset, inner.width, group_height);

        let group_block = Block::default()
            .title(format!(" {} ", group_name))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(55, 60, 70)));

        let group_inner = group_block.inner(group_area);
        frame.render_widget(group_block, group_area);

        for (idx, (i, pod)) in group_pods.iter().enumerate() {
            let col = idx % cols_in_group;
            let row = idx / cols_in_group;
            let x = group_inner.x + (col as u16) * (CARD_WIDTH + CARD_GAP);
            let y = group_inner.y + (row as u16) * (CARD_HEIGHT + CARD_GAP);

            if x + CARD_WIDTH > group_inner.x + group_inner.width
                || y + CARD_HEIGHT > group_inner.y + group_inner.height
            {
                continue;
            }

            let card_area = Rect::new(x, y, CARD_WIDTH, CARD_HEIGHT);
            render_pod_card(frame, pod, card_area, focus_idx == Some(*i));
        }

        y_offset += group_height + CARD_GAP;
    }

    // --- 非グループ Pod 描画 ---
    let ungrouped_rows: Vec<&[(usize, &crate::pod::Pod)]> = ungrouped.chunks(cols).collect();
    for row_pods in &ungrouped_rows {
        if y_offset + CARD_HEIGHT > inner.height {
            break;
        }

        for (col_idx, (i, pod)) in row_pods.iter().enumerate() {
            let x = inner.x + (col_idx as u16) * (CARD_WIDTH + CARD_GAP);
            let y = inner.y + y_offset;

            if x + CARD_WIDTH > inner.x + inner.width {
                continue;
            }

            let card_area = Rect::new(x, y, CARD_WIDTH, CARD_HEIGHT);
            render_pod_card(frame, pod, card_area, focus_idx == Some(*i));
        }

        y_offset += CARD_HEIGHT + CARD_GAP;
    }

    // --- Dead セクション ---
    if !dead.is_empty() && y_offset + 1 + DEAD_CARD_HEIGHT <= inner.height {
        // セパレーター
        let sep_area = Rect::new(inner.x, inner.y + y_offset, inner.width, 1);
        let mut sep_text = String::from("\u{2500}\u{2500} Dead ");
        let remaining = (inner.width as usize).saturating_sub(sep_text.len());
        for _ in 0..remaining {
            sep_text.push('\u{2500}');
        }
        let sep = Paragraph::new(Line::from(Span::styled(
            sep_text,
            Style::default().fg(Color::Rgb(55, 55, 60)),
        )));
        frame.render_widget(sep, sep_area);
        y_offset += 1 + CARD_GAP;

        // Dead Pod をコンパクトカードで描画
        let dead_rows: Vec<&[(usize, &crate::pod::Pod)]> = dead.chunks(cols).collect();
        for row_pods in &dead_rows {
            if y_offset + DEAD_CARD_HEIGHT > inner.height {
                break;
            }

            for (col_idx, (i, pod)) in row_pods.iter().enumerate() {
                let x = inner.x + (col_idx as u16) * (CARD_WIDTH + CARD_GAP);
                let y = inner.y + y_offset;

                if x + CARD_WIDTH > inner.x + inner.width {
                    continue;
                }

                let card_area = Rect::new(x, y, CARD_WIDTH, DEAD_CARD_HEIGHT);
                render_pod_card(frame, pod, card_area, focus_idx == Some(*i));
            }

            y_offset += DEAD_CARD_HEIGHT + CARD_GAP;
        }
    }
}

/// 個々の Pod カードを描画（角丸 + ステータス背景色）
fn render_pod_card(frame: &mut Frame, pod: &crate::pod::Pod, area: Rect, focused: bool) {
    let is_dead = pod.status == PodStatus::Dead;
    let bg = status_bg_color(&pod.status);

    let border_style = if focused {
        Style::default()
            .fg(Color::White)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(status_border_color(&pod.status)).bg(bg)
    };

    // タイトル: ステータスアイコン + 表示名 + 経過時間 + subagent数（カード幅に収める）
    let icon = pod.status_icon();
    let elapsed = pod.elapsed_time();
    let sub_count = pod.total_sub_agents();
    let sub_suffix = if sub_count > 0 {
        format!(" \u{26a1}{}", sub_count)  // ⚡N
    } else {
        String::new()
    };
    let raw_name = if let Some(ref group) = pod.group {
        if pod.name != *group {
            // 子 Pod: グループ名を省略 "../impl"
            format!(
                "../{}",
                pod.name
                    .strip_prefix(&format!("{}/", group))
                    .unwrap_or(&pod.name)
            )
        } else {
            pod.name.clone()
        }
    } else {
        pod.name.clone()
    };
    let marker = if focused { "\u{25b6} " } else { "" };
    // 固定部分: " marker icon  elapsed sub_suffix "
    let fixed_width = format!(" {}{}  {}{} ", marker, icon, elapsed, sub_suffix).width();
    let available = (area.width as usize).saturating_sub(fixed_width + 2); // +2 for borders
    let display_name = truncate_to_width(&raw_name, available.max(1));
    let title = format!(" {}{} {} {}{} ", marker, icon, display_name, elapsed, sub_suffix);

    let block = Block::default()
        .title(title.as_str())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(bg));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 1 || inner.width < 2 {
        return;
    }

    let text_color = if is_dead {
        Color::Rgb(80, 80, 85)
    } else {
        Color::Rgb(200, 205, 215)
    };

    // Pane 出力プレビュー: 最初の member の last_output 末尾を表示
    let output = pod
        .members
        .first()
        .map(|m| m.last_output.as_str())
        .unwrap_or("");

    let available_lines = inner.height as usize;
    let width = inner.width as usize;
    let output_lines: Vec<&str> = output.lines().collect();
    let skip = output_lines.len().saturating_sub(available_lines);

    let mut lines: Vec<Line> = output_lines
        .iter()
        .skip(skip)
        .map(|line| {
            // カード幅に切り詰め（マルチバイト対応: char 単位で切る）
            let truncated: String = line.chars().take(width).collect();
            Line::from(Span::styled(
                truncated,
                Style::default().fg(text_color).bg(bg),
            ))
        })
        .collect();

    // 残りの行を背景色で埋める
    while lines.len() < available_lines {
        lines.push(Line::from(Span::styled(
            " ".repeat(width),
            Style::default().bg(bg),
        )));
    }

    let content = Paragraph::new(lines).style(Style::default().bg(bg));
    frame.render_widget(content, inner);
}

/// ステータスバー (2行: 統計情報 + キーヒント)
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    // --- 1行目: 統計情報 ---
    let (total_pods, warnings, total_members) = app.state.pods_summary();
    let total_working: u64 = app.state.pods.iter().map(|p| p.total_working_time()).sum();
    let total_subagents: usize = app.state.pods.iter().map(|p| p.total_sub_agents()).sum();

    let mut bar_spans = vec![
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
    ];

    if total_subagents > 0 {
        bar_spans.push(Span::styled(" / ", Style::default().fg(Color::DarkGray)));
        bar_spans.push(Span::styled(
            format!("\u{26a1}{} agents", total_subagents),
            Style::default().fg(Color::Magenta),
        ));
    }

    bar_spans.push(Span::styled(
        format!(" | Work: {}", format_duration(total_working)),
        Style::default().fg(Color::Blue),
    ));

    let bar = Line::from(bar_spans);

    let status_bar = Paragraph::new(bar)
        .style(Style::default().bg(Color::Black));
    frame.render_widget(status_bar, rows[0]);

    // --- 2行目: キーヒント ---
    let key_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(Color::DarkGray);

    let hint_line = match app.state.mode {
        Mode::Home => {
            // インラインプロンプト中
            if app.state.inline_prompt != InlinePrompt::None {
                if app.state.inline_prompt == InlinePrompt::Browse {
                    Line::from(vec![
                        Span::styled(" Browse ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::raw(" "),
                        Span::styled("[j/k]", key_style),
                        Span::styled("Nav ", label_style),
                        Span::styled("[Enter/l]", key_style),
                        Span::styled("Open ", label_style),
                        Span::styled("[h/BS]", key_style),
                        Span::styled("Parent ", label_style),
                        Span::styled("[Space]", key_style),
                        Span::styled("Select ", label_style),
                        Span::styled("[Esc]", key_style),
                        Span::styled("Cancel", label_style),
                    ])
                } else {
                let prompt_label = match &app.state.inline_prompt {
                    InlinePrompt::AdoptSession => "Session name: ",
                    InlinePrompt::DropConfirm(_) => "",
                    InlinePrompt::Browse | InlinePrompt::None => "",
                };

                // DropConfirm は特別なフォーマット
                if let InlinePrompt::DropConfirm(ref name) = app.state.inline_prompt {
                    Line::from(vec![
                        Span::styled(
                            format!(" Drop '{}'? (y/yes): ", name),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::styled(
                            app.state.inline_input.as_str(),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled("_ ", Style::default().fg(Color::Gray)),
                        Span::styled("[Enter]", key_style),
                        Span::styled("OK ", label_style),
                        Span::styled("[Esc]", key_style),
                        Span::styled("Cancel", label_style),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled(format!(" {}", prompt_label), Style::default().fg(Color::Yellow)),
                        Span::styled(
                            app.state.inline_input.as_str(),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled("_ ", Style::default().fg(Color::Gray)),
                        Span::styled("[Enter]", key_style),
                        Span::styled("OK ", label_style),
                        Span::styled("[Esc]", key_style),
                        Span::styled("Cancel", label_style),
                    ])
                }
                } // close Browse else
            } else if app.state.pane_focus == PaneFocus::Left {
                // 左ペインフォーカス中
                Line::from(vec![
                    Span::styled(" [Enter]", key_style),
                    Span::styled("Send ", label_style),
                    Span::styled("[/]", key_style),
                    Span::styled("Command ", label_style),
                    Span::styled("[@]", key_style),
                    Span::styled("Project ", label_style),
                    Span::styled("[Esc]", key_style),
                    Span::styled("Cancel", label_style),
                ])
            } else {
                // 右ペインフォーカス (通常)
                Line::from(vec![
                    Span::styled(" [n]", key_style),
                    Span::styled("New ", label_style),
                    Span::styled("[Enter]", key_style),
                    Span::styled("Detail ", label_style),
                    Span::styled("[t]", key_style),
                    Span::styled("Attach ", label_style),
                    Span::styled("[d]", key_style),
                    Span::styled("Drop ", label_style),
                    Span::styled("[a]", key_style),
                    Span::styled("Adopt ", label_style),
                    Span::styled("[p]", key_style),
                    Span::styled("Browse ", label_style),
                    Span::styled("[N]", key_style),
                    Span::styled("Warn ", label_style),
                    Span::styled("[?]", key_style),
                    Span::styled("Help ", label_style),
                    Span::styled("[q]", key_style),
                    Span::styled("Quit", label_style),
                ])
            }
        }
        Mode::Detail => {
            Line::from(vec![
                Span::styled(" Passthrough ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled("All keys → pane ", label_style),
                Span::styled("[Esc]", key_style),
                Span::styled("Back ", label_style),
            ])
        }
        Mode::Chat => {
            Line::from(vec![
                Span::styled(" [Enter]", key_style),
                Span::styled("Send ", label_style),
                Span::styled("[Esc]", key_style),
                Span::styled("Back", label_style),
            ])
        }
        Mode::Permission => {
            Line::from(vec![
                Span::styled(" [a]", key_style),
                Span::styled("Approve ", label_style),
                Span::styled("[d]", key_style),
                Span::styled("Deny ", label_style),
                Span::styled("[s]", key_style),
                Span::styled("Skip ", label_style),
                Span::styled("[Esc]", key_style),
                Span::styled("Back", label_style),
            ])
        }
        Mode::Help => {
            Line::from(vec![
                Span::styled(" [?/Esc]", key_style),
                Span::styled("Close help", label_style),
            ])
        }
    };

    let hint_bar = Paragraph::new(hint_line)
        .style(Style::default().bg(Color::Black));
    frame.render_widget(hint_bar, rows[1]);
}

/// PodStatus に応じたボーダー色（Detail パネル等で使用）
fn status_color(status: &PodStatus) -> Color {
    match status {
        PodStatus::Permission => Color::Rgb(200, 170, 80),
        PodStatus::Error => Color::Rgb(200, 90, 95),
        PodStatus::Working => Color::Rgb(80, 130, 200),
        PodStatus::Idle => Color::Rgb(100, 105, 115),
        PodStatus::Done => Color::Rgb(80, 180, 120),
        PodStatus::Dead => Color::Rgb(70, 70, 75),
    }
}

/// Pod カードの背景色（ニュアンスカラー）
fn status_bg_color(status: &PodStatus) -> Color {
    match status {
        PodStatus::Working => Color::Rgb(18, 28, 48),
        PodStatus::Permission => Color::Rgb(48, 38, 18),
        PodStatus::Error => Color::Rgb(48, 18, 22),
        PodStatus::Idle => Color::Rgb(26, 28, 32),
        PodStatus::Done => Color::Rgb(18, 40, 28),
        PodStatus::Dead => Color::Rgb(18, 18, 20),
    }
}

/// Pod カードのボーダー色（背景色より少し明るい）
fn status_border_color(status: &PodStatus) -> Color {
    match status {
        PodStatus::Working => Color::Rgb(35, 55, 85),
        PodStatus::Permission => Color::Rgb(85, 70, 35),
        PodStatus::Error => Color::Rgb(85, 35, 40),
        PodStatus::Idle => Color::Rgb(45, 48, 55),
        PodStatus::Done => Color::Rgb(35, 65, 48),
        PodStatus::Dead => Color::Rgb(32, 32, 35),
    }
}

/// MemberStatus に応じた色を返す
fn member_status_color(status: &MemberStatus) -> Color {
    match status {
        MemberStatus::Permission => Color::Rgb(200, 170, 80),
        MemberStatus::Error => Color::Rgb(200, 90, 95),
        MemberStatus::Working => Color::Rgb(80, 130, 200),
        MemberStatus::Idle => Color::Rgb(100, 105, 115),
        MemberStatus::Done => Color::Rgb(80, 180, 120),
        MemberStatus::Dead => Color::Rgb(70, 70, 75),
    }
}
