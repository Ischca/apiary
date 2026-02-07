use crate::pod::MemberStatus;
use serde::Deserialize;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;

const HOOKS_FILE: &str = "/tmp/apiary-hooks.jsonl";

#[derive(Debug, Clone, Deserialize)]
pub struct HookEvent {
    pub event: String,       // "tool_start", "tool_end", "permission", "error"
    pub tool: Option<String>,
    pub session: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
}

impl HookEvent {
    /// hooks イベントから推定される MemberStatus を返す
    pub fn inferred_status(&self) -> Option<MemberStatus> {
        match self.event.as_str() {
            "tool_start" => Some(MemberStatus::Working),
            "tool_end" => Some(MemberStatus::Working), // ツール終了後もまだ処理中
            "permission" => Some(MemberStatus::Permission),
            "error" => Some(MemberStatus::Error),
            _ => None,
        }
    }
}

pub struct HooksReceiver {
    path: PathBuf,
    last_position: u64,
}

impl HooksReceiver {
    pub fn new() -> Self {
        Self {
            path: PathBuf::from(HOOKS_FILE),
            last_position: 0,
        }
    }

    /// 初期化: 現在のファイル末尾位置を記録
    pub fn init(&mut self) {
        if let Ok(metadata) = fs::metadata(&self.path) {
            self.last_position = metadata.len();
        }
    }

    /// 新しいイベントを読み取る
    pub fn poll_events(&mut self) -> Vec<HookEvent> {
        let mut events = Vec::new();

        let file = match fs::File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return events,
        };

        let metadata = match file.metadata() {
            Ok(m) => m,
            Err(_) => return events,
        };

        // ファイルが小さくなった場合（truncate等）はリセット
        if metadata.len() < self.last_position {
            self.last_position = 0;
        }

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(self.last_position)).is_err() {
            return events;
        }

        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    self.last_position += n as u64;
                    if let Ok(event) = serde_json::from_str::<HookEvent>(line.trim()) {
                        events.push(event);
                    }
                }
                Err(_) => break,
            }
        }

        events
    }

    /// hooks が有効か (ファイルが存在するか)
    pub fn is_available(&self) -> bool {
        self.path.exists()
    }
}

/// hooks 設定テンプレートを出力
pub fn print_hooks_setup() {
    println!("Add the following to ~/.claude/settings.json to enable hooks integration:");
    println!();
    println!(r#"{{
  "hooks": {{
    "preToolUse": [{{
      "type": "command",
      "command": "echo '{{\"event\":\"tool_start\",\"tool\":\"$TOOL_NAME\"}}' >> /tmp/apiary-hooks.jsonl"
    }}],
    "postToolUse": [{{
      "type": "command",
      "command": "echo '{{\"event\":\"tool_end\",\"tool\":\"$TOOL_NAME\"}}' >> /tmp/apiary-hooks.jsonl"
    }}]
  }}
}}"#);
}
