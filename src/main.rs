mod config;
mod hooks;
mod notify;
mod pod;
mod store;
mod tmux;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
use std::time::{Duration, Instant};

use crate::store::PodStore;
use crate::tui::app::App;
use crate::tui::handler::{handle_key_event, Action};
use crate::tui::ui::draw;

const TICK_RATE_MS: u64 = 250;

#[derive(Parser)]
#[command(name = "apiary", bin_name = "apiary", version, about = "Claude Code Multi-Session Manager")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new pod with a tmux session and Claude Code
    Create {
        /// Pod name
        name: String,
        /// Git worktree path (optional)
        #[arg(long)]
        worktree: Option<String>,
    },
    /// Adopt an existing tmux session as a pod
    Adopt {
        /// tmux session name
        session: String,
        /// Pod name (defaults to session name)
        #[arg(long)]
        name: Option<String>,
    },
    /// Drop a pod and kill its tmux session
    Drop {
        /// Pod name
        name: String,
    },
    /// List all pods
    List,
    /// Show status summary of all pods
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // ログ初期化
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("apiary=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    // tmux チェック
    if !tmux::Tmux::is_available() {
        eprintln!("Error: tmux is not installed or not in PATH.");
        eprintln!("Apiary requires tmux >= 3.2");
        std::process::exit(1);
    }

    match cli.command {
        Some(cmd) => run_cli(cmd),
        None => run_tui(),
    }
}

fn run_cli(cmd: Commands) -> Result<()> {
    let store = PodStore::new()?;
    let mut app = App::new(store)?;

    match cmd {
        Commands::Create { name, worktree } => {
            app.create_pod(&name, worktree.as_deref())?;
            println!("Pod '{}' created", name);
        }
        Commands::Adopt { session, name } => {
            app.adopt_session(&session, name.as_deref())?;
            println!("Session '{}' adopted as pod", session);
        }
        Commands::Drop { name } => {
            app.drop_pod(&name)?;
            println!("Pod '{}' dropped", name);
        }
        Commands::List => {
            app.refresh_pod_states();
            if app.state.pods.is_empty() {
                println!("No pods");
            } else {
                for pod in &app.state.pods {
                    println!(
                        "{} {} ({}, {} members, {})",
                        pod.status_icon(),
                        pod.name,
                        format!("{:?}", pod.pod_type).to_lowercase(),
                        pod.members.len(),
                        pod.elapsed_time(),
                    );
                }
            }
        }
        Commands::Status => {
            app.refresh_pod_states();
            let (total, warnings, members) = app.state.pods_summary();
            println!(
                "Pods: {} | Warnings: {} | Members: {}",
                total, warnings, members
            );
            for pod in &app.state.pods {
                println!(
                    "  {} {} [{:?}] - {} members",
                    pod.status_icon(),
                    pod.name,
                    pod.status,
                    pod.members.len(),
                );
                for member in &pod.members {
                    println!(
                        "    {} {} ({})",
                        member.status_icon(),
                        member.role,
                        member.elapsed(),
                    );
                }
            }
        }
    }
    app.save()?;
    Ok(())
}

fn run_tui() -> Result<()> {
    // PodStore 初期化
    let store = PodStore::new()?;

    // App 初期化
    let mut app = App::new(store)?;

    // Terminal 初期化
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // メインループ
    let result = run_app(&mut terminal, &mut app);

    // Terminal 復元
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // 状態を保存
    let _ = app.save();

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    let tick_rate = Duration::from_millis(TICK_RATE_MS);
    let mut last_tick = Instant::now();
    let mut last_refresh = Instant::now();

    // 初回描画
    terminal.draw(|frame| draw(frame, app))?;

    // 初回の状態更新
    app.refresh_pod_states();
    terminal.draw(|frame| draw(frame, app))?;

    loop {
        // イベント待ち (tick_rate でタイムアウト)
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                let action = handle_key_event(app, key);
                match action {
                    Action::Quit => {
                        app.state.should_quit = true;
                        break;
                    }
                    Action::Render => {
                        terminal.draw(|frame| draw(frame, app))?;
                    }
                    Action::None => {}
                }
            }
        }

        // Tick 処理
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();

            // グリッドカラム数を更新
            let size = terminal.size()?;
            let grid_width = (size.width as f32 * 0.65) as usize;
            app.state.grid_columns = (grid_width / 23).max(1);

            // Chat モード中は高頻度で応答を取得
            if app.state.mode == crate::pod::Mode::Chat {
                app.refresh_chat_output();
                terminal.draw(|frame| draw(frame, app))?;
            }
        }

        // 定期的に Pod 状態を更新 (適応的ポーリング)
        // 毎 tick で呼ぶが、内部で member ごとの間隔制御をする
        if last_refresh.elapsed() >= Duration::from_millis(500) {
            last_refresh = Instant::now();
            app.selective_refresh();
            terminal.draw(|frame| draw(frame, app))?;
        }
    }

    Ok(())
}
