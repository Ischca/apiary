use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub polling: PollingConfig,
    pub notification: NotificationConfig,
    pub detection: DetectionConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct PollingConfig {
    /// フォーカス中 Pod のポーリング間隔 (ms)
    pub focused_interval_ms: u64,
    /// Permission 状態のポーリング間隔 (ms)
    pub permission_interval_ms: u64,
    /// Working 状態のポーリング間隔 (ms)
    pub working_interval_ms: u64,
    /// Idle 状態のポーリング間隔 (ms)
    pub idle_interval_ms: u64,
    /// Error 状態のポーリング間隔 (ms)
    pub error_interval_ms: u64,
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            focused_interval_ms: 1000,
            permission_interval_ms: 1000,
            working_interval_ms: 3000,
            idle_interval_ms: 10000,
            error_interval_ms: 5000,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct NotificationConfig {
    /// デスクトップ通知を有効にするか
    pub enabled: bool,
    /// 通知音を鳴らすか
    pub sound: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sound: false,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct DetectionConfig {
    /// 追加の Permission 検出パターン (正規表現)
    pub permission_patterns: Vec<String>,
    /// 追加の Error 検出パターン (正規表現)
    pub error_patterns: Vec<String>,
    /// 追加の Idle 検出パターン (正規表現)
    pub idle_patterns: Vec<String>,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            permission_patterns: Vec::new(),
            error_patterns: Vec::new(),
            idle_patterns: Vec::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            polling: PollingConfig::default(),
            notification: NotificationConfig::default(),
            detection: DetectionConfig::default(),
        }
    }
}

impl Config {
    /// ~/.config/apiary/config.toml を読み込む。なければデフォルト。
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config: {:?}", path))?;

        if content.trim().is_empty() {
            return Ok(Self::default());
        }

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config: {:?}", path))?;

        Ok(config)
    }

    fn config_path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("Failed to determine config directory")?
            .join("apiary");
        Ok(dir.join("config.toml"))
    }
}
