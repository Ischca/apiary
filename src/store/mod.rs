use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing::{info, warn};

use crate::pod::Pod;
use crate::tmux::Tmux;

pub struct PodStore {
    path: PathBuf,
}

impl PodStore {
    /// 新しい PodStore を作成。パスは ~/.config/apiary/pods.json
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("Failed to determine config directory")?
            .join("apiary");

        if !config_dir.exists() {
            std::fs::create_dir_all(&config_dir)
                .with_context(|| format!("Failed to create config directory: {:?}", config_dir))?;
        }

        let path = config_dir.join("pods.json");
        Ok(Self { path })
    }

    /// カスタムパスで PodStore を作成（テスト用）
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// pods.json を読み込んで Pod の Vec を返す
    /// ファイルが存在しない場合は空 Vec を返す
    pub fn load(&self) -> Result<Vec<Pod>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&self.path)
            .with_context(|| format!("Failed to read pods file: {:?}", self.path))?;

        if content.trim().is_empty() {
            return Ok(Vec::new());
        }

        let pods: Vec<Pod> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse pods file: {:?}", self.path))?;

        Ok(pods)
    }

    /// Pod の Vec を pods.json に保存 (アトミック: tmp → rename)
    pub fn save(&self, pods: &[Pod]) -> Result<()> {
        let content = serde_json::to_string_pretty(pods)
            .context("Failed to serialize pods")?;

        let tmp_path = self.path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &content)
            .with_context(|| format!("Failed to write temp pods file: {:?}", tmp_path))?;
        std::fs::rename(&tmp_path, &self.path)
            .with_context(|| format!("Failed to rename temp pods file: {:?}", tmp_path))?;

        Ok(())
    }

    /// 読み込んだ Pod を tmux の実態と照合し、存在しないセッションの Pod を除外
    /// 残った Pod の各 member の tmux_pane が存在するかも確認
    pub fn load_and_reconcile(&self) -> Result<Vec<Pod>> {
        let mut pods = self.load()?;

        // 存在する全ペインの ID を取得（一括取得で tmux 呼び出しを減らす）
        let all_panes = Tmux::list_all_panes().unwrap_or_default();
        let pane_ids: std::collections::HashSet<String> =
            all_panes.iter().map(|p| p.id.clone()).collect();

        pods.retain(|pod| {
            if !Tmux::session_exists(&pod.tmux_session) {
                info!(
                    session = %pod.tmux_session,
                    pod = %pod.name,
                    "Removing pod: tmux session no longer exists"
                );
                return false;
            }
            true
        });

        for pod in &mut pods {
            let before_count = pod.members.len();
            pod.members.retain(|member| {
                let exists = pane_ids.contains(&member.tmux_pane);
                if !exists {
                    warn!(
                        pane = %member.tmux_pane,
                        role = %member.role,
                        pod = %pod.name,
                        "Removing member: tmux pane no longer exists"
                    );
                }
                exists
            });
            if pod.members.len() != before_count {
                pod.rollup_status();
            }
        }

        // 整合後の状態を保存
        self.save(&pods)?;

        Ok(pods)
    }

    /// Pod を追加して保存
    pub fn add_pod(&self, pods: &mut Vec<Pod>, pod: Pod) -> Result<()> {
        pods.push(pod);
        self.save(pods)
    }

    /// Pod を名前で削除して保存
    pub fn remove_pod(&self, pods: &mut Vec<Pod>, name: &str) -> Result<bool> {
        let before_len = pods.len();
        pods.retain(|p| p.name != name);
        let removed = pods.len() < before_len;

        if removed {
            self.save(pods)?;
        }

        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pod::{Member, MemberStatus, PodStatus, PodType};
    use chrono::Utc;
    use std::fs;
    use tempfile::NamedTempFile;

    fn make_test_pod(name: &str) -> Pod {
        Pod {
            name: name.to_string(),
            pod_type: PodType::Solo,
            members: vec![Member {
                role: "leader".to_string(),
                status: MemberStatus::Idle,
                tmux_pane: "%0".to_string(),
                last_change: Utc::now(),
                last_output: String::new(),
                last_polled: None,
                working_secs: 0,
            }],
            status: PodStatus::Idle,
            tmux_session: format!("apiary-{}", name),
            worktree: None,
            created_at: Utc::now(),
            total_working_secs: 0,
        }
    }

    #[test]
    fn test_load_nonexistent_file() {
        let store = PodStore::with_path(PathBuf::from("/tmp/apiary_test_nonexistent.json"));
        let pods = store.load().unwrap();
        assert!(pods.is_empty());
    }

    #[test]
    fn test_save_and_load() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let store = PodStore::with_path(path);

        let pods = vec![make_test_pod("test1"), make_test_pod("test2")];
        store.save(&pods).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "test1");
        assert_eq!(loaded[1].name, "test2");
    }

    #[test]
    fn test_load_empty_file() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        fs::write(&path, "").unwrap();

        let store = PodStore::with_path(path);
        let pods = store.load().unwrap();
        assert!(pods.is_empty());
    }

    #[test]
    fn test_add_pod() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let store = PodStore::with_path(path);

        let mut pods = Vec::new();
        store.add_pod(&mut pods, make_test_pod("new-pod")).unwrap();

        assert_eq!(pods.len(), 1);
        assert_eq!(pods[0].name, "new-pod");

        // ファイルからも読み込めることを確認
        let loaded = store.load().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "new-pod");
    }

    #[test]
    fn test_remove_pod() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let store = PodStore::with_path(path);

        let mut pods = vec![make_test_pod("a"), make_test_pod("b"), make_test_pod("c")];
        store.save(&pods).unwrap();

        let removed = store.remove_pod(&mut pods, "b").unwrap();
        assert!(removed);
        assert_eq!(pods.len(), 2);
        assert_eq!(pods[0].name, "a");
        assert_eq!(pods[1].name, "c");

        // 存在しない名前を削除しようとした場合
        let removed = store.remove_pod(&mut pods, "nonexistent").unwrap();
        assert!(!removed);
        assert_eq!(pods.len(), 2);
    }
}
