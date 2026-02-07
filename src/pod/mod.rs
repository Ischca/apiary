pub mod detector;
pub mod discovery;

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemberStatus {
    Idle,
    Working,
    Permission,
    Error,
    Done,
}

impl MemberStatus {
    pub fn icon(&self) -> &str {
        match self {
            MemberStatus::Permission => "\u{26a0}",
            MemberStatus::Error => "\u{274c}",
            MemberStatus::Working => "\u{1f504}",
            MemberStatus::Idle => "\u{23f8}",
            MemberStatus::Done => "\u{2705}",
        }
    }

    pub fn priority(&self) -> u8 {
        match self {
            MemberStatus::Permission => 4,
            MemberStatus::Error => 3,
            MemberStatus::Working => 2,
            MemberStatus::Idle => 1,
            MemberStatus::Done => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PodStatus {
    Idle,
    Working,
    Permission,
    Error,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PodType {
    Solo,
    Team,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub role: String,
    pub status: MemberStatus,
    pub tmux_pane: String,
    pub last_change: DateTime<Utc>,
    pub last_output: String,
    #[serde(skip)]
    pub last_polled: Option<std::time::Instant>,
    #[serde(default)]
    pub working_secs: u64,
}

impl Member {
    pub fn status_icon(&self) -> &str {
        self.status.icon()
    }

    pub fn elapsed(&self) -> String {
        format_elapsed(self.last_change)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pod {
    pub name: String,
    pub pod_type: PodType,
    pub members: Vec<Member>,
    pub status: PodStatus,
    pub tmux_session: String,
    pub worktree: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub total_working_secs: u64,
}

impl Pod {
    pub fn rollup_status(&mut self) {
        if self.members.is_empty() {
            self.status = PodStatus::Idle;
            return;
        }

        let max_priority = self
            .members
            .iter()
            .map(|m| m.status.priority())
            .max()
            .unwrap_or(0);

        self.status = match max_priority {
            4 => PodStatus::Permission,
            3 => PodStatus::Error,
            2 => PodStatus::Working,
            1 => PodStatus::Idle,
            _ => PodStatus::Done,
        };
    }

    pub fn elapsed_time(&self) -> String {
        format_elapsed(self.created_at)
    }

    pub fn add_member(&mut self, member: Member) {
        self.members.push(member);
    }

    pub fn status_icon(&self) -> &str {
        match self.status {
            PodStatus::Permission => "\u{26a0}",
            PodStatus::Error => "\u{274c}",
            PodStatus::Working => "\u{1f504}",
            PodStatus::Idle => "\u{23f8}",
            PodStatus::Done => "\u{2705}",
        }
    }

    /// 全 member の working 秒数の合計
    pub fn total_working_time(&self) -> u64 {
        self.members.iter().map(|m| m.working_secs).sum::<u64>() + self.total_working_secs
    }

    /// 全体の経過時間（秒）
    pub fn total_elapsed_secs(&self) -> u64 {
        Utc::now().signed_duration_since(self.created_at).num_seconds().max(0) as u64
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Mode {
    Home,
    Detail,
    Chat,
    Permission,
    Help,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub sender: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub pods: Vec<Pod>,
    pub focus: Option<usize>,
    pub selected_member: Option<usize>,
    pub mode: Mode,
    pub command_input: String,
    pub chat_input: String,
    pub chat_history: Vec<ChatMessage>,
    pub capture_snapshot: Option<String>,
    pub grid_columns: usize,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub current_permission: Option<crate::pod::detector::PermissionRequest>,
    pub previous_permission_pods: HashSet<String>,
    pub previous_mode: Option<Mode>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            pods: Vec::new(),
            focus: None,
            selected_member: None,
            mode: Mode::Home,
            command_input: String::new(),
            chat_input: String::new(),
            chat_history: Vec::new(),
            capture_snapshot: None,
            grid_columns: 3,
            should_quit: false,
            status_message: None,
            current_permission: None,
            previous_permission_pods: HashSet::new(),
            previous_mode: None,
        }
    }

    pub fn focused_pod(&self) -> Option<&Pod> {
        self.focus.and_then(|i| self.pods.get(i))
    }

    pub fn focused_pod_mut(&mut self) -> Option<&mut Pod> {
        self.focus.and_then(|i| self.pods.get_mut(i))
    }

    pub fn next_permission_pod(&self) -> Option<usize> {
        self.pods
            .iter()
            .position(|p| p.status == PodStatus::Permission)
    }

    pub fn pods_summary(&self) -> (usize, usize, usize) {
        let total_pods = self.pods.len();
        let permission_count = self
            .pods
            .iter()
            .filter(|p| p.status == PodStatus::Permission)
            .count();
        let total_members: usize = self.pods.iter().map(|p| p.members.len()).sum();
        (total_pods, permission_count, total_members)
    }
}

pub fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 { format!("{}m", m) } else { format!("{}m{}s", m, s) }
    } else {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 { format!("{}h", h) } else { format!("{}h{}m", h, m) }
    }
}

fn format_elapsed(since: DateTime<Utc>) -> String {
    let duration = Utc::now().signed_duration_since(since);
    let seconds = duration.num_seconds();

    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86400)
    }
}
