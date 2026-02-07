use std::process::Command;

/// デスクトップ通知を送信 (ベストエフォート)
pub fn notify(title: &str, body: &str) {
    // macOS
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            body.replace('\"', "\\\""),
            title.replace('\"', "\\\"")
        );
        let _ = Command::new("osascript")
            .args(["-e", &script])
            .output();
    }

    // Linux
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("notify-send")
            .args([title, body])
            .output();
    }
}
