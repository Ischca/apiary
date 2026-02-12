use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    cursor,
    event::{self, Event, EnableBracketedPaste, DisableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;
use std::time::{Duration, Instant};

use apiary::project;
use apiary::store::PodStore;
use apiary::tmux;
use apiary::tui::app::App;
use apiary::tui::handler::{handle_key_event, handle_paste_event, Action};
use apiary::tui::ui::draw;

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
        /// Project name or path (defaults to cwd)
        #[arg(long, alias = "worktree")]
        project: Option<String>,
        /// Group name (optional)
        #[arg(long)]
        group: Option<String>,
    },
    /// Adopt an existing tmux session as a pod
    Adopt {
        /// tmux session name
        session: String,
        /// Pod name (defaults to session name)
        #[arg(long)]
        name: Option<String>,
        /// Group name (optional)
        #[arg(long)]
        group: Option<String>,
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
    /// Manage project registry
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
}

#[derive(Subcommand)]
enum ProjectAction {
    /// List registered projects
    List,
    /// Register a project
    Add {
        /// Project path
        path: String,
        /// Project name (defaults to directory name)
        #[arg(long)]
        name: Option<String>,
    },
    /// Unregister a project
    Remove {
        /// Project name
        name: String,
    },
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
        eprintln!();
        eprintln!("Install tmux:");
        if cfg!(target_os = "macos") {
            eprintln!("  brew install tmux");
        } else {
            eprintln!("  Ubuntu/Debian: sudo apt install tmux");
            eprintln!("  Fedora:        sudo dnf install tmux");
            eprintln!("  Arch:          sudo pacman -S tmux");
        }
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
        Commands::Create { name, project, group } => {
            app.create_pod(&name, project.as_deref(), group.as_deref(), None)?;
            println!("Pod '{}' created", name);
        }
        Commands::Adopt { session, name, group } => {
            app.adopt_session(&session, name.as_deref(), group.as_deref())?;
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
        Commands::Project { action } => {
            let project_store = project::ProjectStore::new()?;
            match action {
                ProjectAction::List => {
                    let projects = project_store.list()?;
                    if projects.is_empty() {
                        println!("No projects registered");
                    } else {
                        for p in &projects {
                            println!("  {} → {}", p.name, p.path);
                        }
                    }
                }
                ProjectAction::Add { path, name } => {
                    if let Some(name) = name {
                        let project = project::Project {
                            name: name.clone(),
                            path: path.clone(),
                        };
                        project_store.register(&project)?;
                        println!("Project '{}' registered → {}", name, path);
                    } else {
                        let project = project::resolve_project(&project_store, &path)?;
                        println!("Project '{}' registered → {}", project.name, project.path);
                    }
                }
                ProjectAction::Remove { name } => {
                    if project_store.unregister(&name)? {
                        println!("Project '{}' removed", name);
                    } else {
                        println!("Project '{}' not found", name);
                    }
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
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // メインループ
    let result = run_app(&mut terminal, &mut app);

    // Terminal 復元
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableBracketedPaste)?;
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
            match event::read()? {
                Event::Key(key) => {
                let action = handle_key_event(app, key);
                match action {
                    Action::Quit => {
                        app.state.should_quit = true;
                        break;
                    }
                    Action::Render => {
                        terminal.draw(|frame| draw(frame, app))?;
                    }
                    Action::AttachTmux(session) => {
                        if !tmux::Tmux::session_exists(&session) {
                            app.state.status_message = Some(format!("Session '{}' not found", session));
                            terminal.draw(|frame| draw(frame, app))?;
                            continue;
                        }

                        let is_inside_tmux = std::env::var("TMUX").is_ok();

                        if !is_inside_tmux {
                            // TUI 一時停止
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show)?;

                            let prefix = tmux::Tmux::get_prefix();
                            println!("Attaching to '{}'. Detach with {}, d to return to apiary.", session, prefix);

                            // blocking attach
                            let _ = tmux::Tmux::attach_session(&session);

                            // TUI 復帰
                            enable_raw_mode()?;
                            execute!(terminal.backend_mut(), EnterAlternateScreen, cursor::Hide, EnableBracketedPaste)?;
                            terminal.clear()?;

                            app.refresh_pod_states();
                            terminal.draw(|frame| draw(frame, app))?;
                        } else {
                            // switch-client (non-blocking)
                            match tmux::Tmux::attach_session(&session) {
                                Ok(_) => {
                                    let prefix = tmux::Tmux::get_prefix();
                                    app.state.status_message = Some(format!("Switched to '{}'. Use {}, s to return.", session, prefix));
                                }
                                Err(e) => {
                                    app.state.status_message = Some(format!("Switch error: {}", e));
                                }
                            }
                            terminal.draw(|frame| draw(frame, app))?;
                        }
                    }
                    Action::None => {}
                }
                }
                Event::Paste(text) => {
                    handle_paste_event(app, &text);
                    terminal.draw(|frame| draw(frame, app))?;
                }
                _ => {}
            }
        }

        // Tick 処理
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();

            // グリッドカラム数を更新
            let size = terminal.size()?;
            let grid_width = (size.width as f32 * 0.65) as usize;
            app.state.grid_columns = (grid_width / 23).max(1);

            // Detail モード: PTY ストリームから drain して再描画
            if app.state.mode == apiary::pod::Mode::Detail {
                if let Some(ref mut stream) = app.detail_pty_stream {
                    if stream.drain() > 0 {
                        terminal.draw(|frame| draw(frame, app))?;
                    }
                }
            }

            // Chat モード中は高頻度で応答を取得
            if app.state.mode == apiary::pod::Mode::Chat {
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
