use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct TmuxSession {
    pub name: String,
    pub windows: usize,
    pub created: String,
}

#[derive(Debug, Clone)]
pub struct TmuxPane {
    pub id: String,
    pub session: String,
    pub window_index: usize,
    pub pane_index: usize,
    pub active: bool,
    pub title: String,
    pub pid: Option<u32>,
}

pub struct Tmux;

impl Tmux {
    /// tmux が利用可能かチェック
    pub fn is_available() -> bool {
        Command::new("tmux")
            .arg("-V")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// tmux サーバーが起動しているかチェック
    pub fn has_server() -> bool {
        Command::new("tmux")
            .arg("list-sessions")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// 全セッション一覧を取得
    pub fn list_sessions() -> Result<Vec<TmuxSession>> {
        let output = Command::new("tmux")
            .args(["list-sessions", "-F", "#{session_name}|#{session_windows}|#{session_created}"])
            .output()
            .context("Failed to execute tmux list-sessions")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // サーバーが起動していない場合は空Vecを返す
            if stderr.contains("no server running") || stderr.contains("error connecting") {
                return Ok(Vec::new());
            }
            anyhow::bail!("tmux list-sessions failed: {}", stderr.trim());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut sessions = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() < 3 {
                continue;
            }
            sessions.push(TmuxSession {
                name: parts[0].to_string(),
                windows: parts[1].parse().unwrap_or(0),
                created: parts[2].to_string(),
            });
        }

        Ok(sessions)
    }

    /// セッション内の全ペインを取得
    pub fn list_panes(session: &str) -> Result<Vec<TmuxPane>> {
        let output = Command::new("tmux")
            .args([
                "list-panes",
                "-t", session,
                "-s",
                "-F", "#{pane_id}|#{session_name}|#{window_index}|#{pane_index}|#{pane_active}|#{pane_title}|#{pane_pid}",
            ])
            .output()
            .context("Failed to execute tmux list-panes")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux list-panes failed for session '{}': {}", session, stderr.trim());
        }

        parse_panes(&String::from_utf8_lossy(&output.stdout))
    }

    /// 全セッションの全ペインを取得
    pub fn list_all_panes() -> Result<Vec<TmuxPane>> {
        let output = Command::new("tmux")
            .args([
                "list-panes",
                "-a",
                "-F", "#{pane_id}|#{session_name}|#{window_index}|#{pane_index}|#{pane_active}|#{pane_title}|#{pane_pid}",
            ])
            .output()
            .context("Failed to execute tmux list-panes -a")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("no server running") || stderr.contains("error connecting") {
                return Ok(Vec::new());
            }
            anyhow::bail!("tmux list-panes -a failed: {}", stderr.trim());
        }

        parse_panes(&String::from_utf8_lossy(&output.stdout))
    }

    /// ペインの出力をキャプチャ (最新50行)
    pub fn capture_pane(pane_id: &str) -> Result<String> {
        Self::capture_pane_lines(pane_id, 50)
    }

    /// ペインの出力をキャプチャ (行数指定)
    pub fn capture_pane_lines(pane_id: &str, lines: i32) -> Result<String> {
        let start = format!("-{}", lines);
        let output = Command::new("tmux")
            .args(["capture-pane", "-t", pane_id, "-p", "-S", &start])
            .output()
            .with_context(|| format!("Failed to capture pane '{}'", pane_id))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux capture-pane failed for '{}': {}", pane_id, stderr.trim());
        }

        let content = String::from_utf8_lossy(&output.stdout);
        Ok(content.trim_end().to_string())
    }

    /// ペインにキー入力を送信 (Enter 付き)
    pub fn send_keys(pane_id: &str, keys: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args(["send-keys", "-t", pane_id, keys, "Enter"])
            .output()
            .with_context(|| format!("Failed to send keys to pane '{}'", pane_id))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux send-keys failed for '{}': {}", pane_id, stderr.trim());
        }

        Ok(())
    }

    /// ANSI エスケープ付きで pane の可視領域をキャプチャ (描画用)
    pub fn capture_pane_ansi(pane_id: &str) -> Result<String> {
        let output = Command::new("tmux")
            .args(["capture-pane", "-e", "-p", "-t", pane_id])
            .output()
            .with_context(|| format!("Failed to capture pane '{}'", pane_id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("capture-pane -e failed for '{}': {}", pane_id, stderr.trim());
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// pane をリサイズ
    pub fn resize_pane(pane_id: &str, width: u16, height: u16) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "resize-pane", "-t", pane_id,
                "-x", &width.to_string(),
                "-y", &height.to_string(),
            ])
            .output()
            .with_context(|| format!("Failed to resize pane '{}'", pane_id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux resize-pane failed for '{}': {}", pane_id, stderr.trim());
        }
        Ok(())
    }

    /// pane が属する window をリサイズ
    pub fn resize_window(pane_id: &str, width: u16, height: u16) -> Result<()> {
        // pane → window ターゲット解決
        let out = Command::new("tmux")
            .args(["display-message", "-t", pane_id, "-p", "#{session_name}:#{window_index}"])
            .output()
            .with_context(|| format!("Failed to resolve window for pane '{}'", pane_id))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            anyhow::bail!("display-message failed for '{}': {}", pane_id, stderr.trim());
        }
        let window_target = String::from_utf8_lossy(&out.stdout).trim().to_string();

        let output = Command::new("tmux")
            .args([
                "resize-window", "-t", &window_target,
                "-x", &width.to_string(),
                "-y", &height.to_string(),
            ])
            .output()
            .with_context(|| format!("Failed to resize window '{}'", window_target))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux resize-window failed for '{}': {}", window_target, stderr.trim());
        }
        Ok(())
    }

    /// pane が属する window のサイズを取得
    pub fn get_window_size(pane_id: &str) -> Result<(u16, u16)> {
        let output = Command::new("tmux")
            .args(["display-message", "-t", pane_id, "-p", "#{window_width}|#{window_height}"])
            .output()
            .with_context(|| format!("Failed to get window size for pane '{}'", pane_id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("display-message failed for '{}': {}", pane_id, stderr.trim());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = text.trim().split('|').collect();
        if parts.len() != 2 {
            anyhow::bail!("Unexpected window size format: {}", text.trim());
        }
        let cols: u16 = parts[0].parse().unwrap_or(80);
        let rows: u16 = parts[1].parse().unwrap_or(24);
        Ok((cols, rows))
    }

    /// pane のサイズ (cols, rows) を取得
    pub fn get_pane_size(pane_id: &str) -> Result<(u16, u16)> {
        let output = Command::new("tmux")
            .args(["display-message", "-t", pane_id, "-p", "#{pane_width}|#{pane_height}"])
            .output()
            .with_context(|| format!("Failed to get pane size '{}'", pane_id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("display-message failed for '{}': {}", pane_id, stderr.trim());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = text.trim().split('|').collect();
        if parts.len() != 2 {
            anyhow::bail!("Unexpected pane size format: {}", text.trim());
        }
        let cols: u16 = parts[0].parse().unwrap_or(80);
        let rows: u16 = parts[1].parse().unwrap_or(24);
        Ok((cols, rows))
    }

    /// リテラルテキスト送信 (-l フラグで特殊文字をエスケープせずそのまま送信)
    pub fn send_keys_literal(pane_id: &str, text: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args(["send-keys", "-l", "-t", pane_id, text])
            .output()
            .with_context(|| format!("Failed to send literal keys to '{}'", pane_id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("send-keys -l failed for '{}': {}", pane_id, stderr.trim());
        }
        Ok(())
    }

    /// ペインにキー入力を送信 (Enter なし)
    pub fn send_keys_raw(pane_id: &str, keys: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args(["send-keys", "-t", pane_id, keys])
            .output()
            .with_context(|| format!("Failed to send raw keys to pane '{}'", pane_id))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux send-keys (raw) failed for '{}': {}", pane_id, stderr.trim());
        }

        Ok(())
    }

    /// 新しいセッションを作成
    pub fn new_session(name: &str, start_dir: Option<&str>) -> Result<String> {
        let mut cmd = Command::new("tmux");
        cmd.args(["new-session", "-d", "-s", name]);

        if let Some(dir) = start_dir {
            cmd.args(["-c", dir]);
        }

        let output = cmd
            .output()
            .with_context(|| format!("Failed to create tmux session '{}'", name))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux new-session failed for '{}': {}", name, stderr.trim());
        }

        Ok(name.to_string())
    }

    /// セッション内で Claude Code を起動
    pub fn start_claude_in_session(session: &str, prompt: Option<&str>) -> Result<()> {
        Self::send_keys(session, "claude")?;

        if let Some(p) = prompt {
            // Claude の起動を待つために少し遅延
            std::thread::sleep(std::time::Duration::from_secs(2));
            Self::send_keys(session, p)?;
        }

        Ok(())
    }

    /// ペインを終了
    pub fn kill_pane(pane_id: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args(["kill-pane", "-t", pane_id])
            .output()
            .with_context(|| format!("Failed to kill tmux pane '{}'", pane_id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux kill-pane failed for '{}': {}", pane_id, stderr.trim());
        }
        Ok(())
    }

    /// セッションを終了
    pub fn kill_session(name: &str) -> Result<()> {
        let exact = format!("={}", name);
        let output = Command::new("tmux")
            .args(["kill-session", "-t", &exact])
            .output()
            .with_context(|| format!("Failed to kill tmux session '{}'", name))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux kill-session failed for '{}': {}", name, stderr.trim());
        }

        Ok(())
    }

    /// 現在の tmux prefix キーを取得 (例: "C-b", "C-a")
    pub fn get_prefix() -> String {
        Command::new("tmux")
            .args(["show-options", "-gv", "prefix"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| "C-b".to_string())
    }

    /// pipe-pane でペインの PTY 出力をファイルにストリーム開始
    pub fn pipe_pane_start(pane_id: &str, output_path: &str) -> Result<()> {
        // 既存の pipe を停止
        let _ = Self::pipe_pane_stop(pane_id);

        let cmd = format!("cat >> {}", output_path);
        let output = Command::new("tmux")
            .args(["pipe-pane", "-O", "-t", pane_id, &cmd])
            .output()
            .with_context(|| format!("Failed to start pipe-pane for '{}'", pane_id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux pipe-pane start failed for '{}': {}", pane_id, stderr.trim());
        }
        Ok(())
    }

    /// pipe-pane を停止
    pub fn pipe_pane_stop(pane_id: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args(["pipe-pane", "-t", pane_id])
            .output()
            .with_context(|| format!("Failed to stop pipe-pane for '{}'", pane_id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux pipe-pane stop failed for '{}': {}", pane_id, stderr.trim());
        }
        Ok(())
    }

    /// セッションにアタッチ (tmux外: attach-session, tmux内: switch-client)
    /// 戻り値: Ok(true) = blocking attach, Ok(false) = switch-client
    pub fn attach_session(name: &str) -> Result<bool> {
        if std::env::var("TMUX").is_ok() {
            // tmux 内: switch-client (non-blocking)
            let output = Command::new("tmux")
                .args(["switch-client", "-t", name])
                .output()
                .with_context(|| format!("Failed to switch to tmux session '{}'", name))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("tmux switch-client failed for '{}': {}", name, stderr.trim());
            }
            Ok(false)
        } else {
            // tmux 外: attach-session (blocking, stdio 継承)
            let status = Command::new("tmux")
                .args(["attach-session", "-t", name])
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()
                .with_context(|| format!("Failed to attach to tmux session '{}'", name))?;

            if !status.success() {
                anyhow::bail!("tmux attach-session failed for '{}'", name);
            }
            Ok(true)
        }
    }

    /// セッションが存在するか確認
    pub fn session_exists(name: &str) -> bool {
        // "=" プレフィックスで完全一致（tmux はデフォルトでプレフィックスマッチする）
        let exact = format!("={}", name);
        Command::new("tmux")
            .args(["has-session", "-t", &exact])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// ペインのプロセスが生きているか確認
    pub fn pane_has_process(pane_id: &str) -> bool {
        let output = Command::new("tmux")
            .args([
                "display-message",
                "-t", pane_id,
                "-p", "#{pane_pid}",
            ])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let pid_str = String::from_utf8_lossy(&o.stdout);
                let pid_str = pid_str.trim();
                if pid_str.is_empty() {
                    return false;
                }
                // PID が取得できたら /proc もしくは kill -0 で生存確認
                if let Ok(pid) = pid_str.parse::<u32>() {
                    // macOS / Linux 両対応: kill -0 で確認
                    Command::new("kill")
                        .args(["-0", &pid.to_string()])
                        .output()
                        .map(|o| o.status.success())
                        .unwrap_or(false)
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

/// git worktree を作成 (branch名 = name)
pub fn create_worktree(path: &str, branch: &str) -> Result<()> {
    // まず branch が存在するか確認
    let branch_exists = Command::new("git")
        .args(["branch", "--list", branch])
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);

    let output = if branch_exists {
        Command::new("git")
            .args(["worktree", "add", path, branch])
            .output()
            .context("Failed to create git worktree")?
    } else {
        // 新しいブランチを作成
        Command::new("git")
            .args(["worktree", "add", "-b", branch, path])
            .output()
            .context("Failed to create git worktree")?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add failed: {}", stderr.trim());
    }

    Ok(())
}

/// git が利用可能かチェック
pub fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// tmux list-panes の出力をパースする共通関数
fn parse_panes(stdout: &str) -> Result<Vec<TmuxPane>> {
    let mut panes = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(7, '|').collect();
        if parts.len() < 7 {
            continue;
        }

        panes.push(TmuxPane {
            id: parts[0].to_string(),
            session: parts[1].to_string(),
            window_index: parts[2].parse().unwrap_or(0),
            pane_index: parts[3].parse().unwrap_or(0),
            active: parts[4] == "1",
            title: parts[5].to_string(),
            pid: parts[6].trim().parse().ok(),
        });
    }

    Ok(panes)
}
