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

    /// セッションを終了
    pub fn kill_session(name: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args(["kill-session", "-t", name])
            .output()
            .with_context(|| format!("Failed to kill tmux session '{}'", name))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux kill-session failed for '{}': {}", name, stderr.trim());
        }

        Ok(())
    }

    /// セッションが存在するか確認
    pub fn session_exists(name: &str) -> bool {
        Command::new("tmux")
            .args(["has-session", "-t", name])
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
