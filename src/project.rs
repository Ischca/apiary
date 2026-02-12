use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub path: String,
}

pub struct ProjectStore {
    path: PathBuf,
}

impl ProjectStore {
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("Failed to determine config directory")?
            .join("apiary");

        if !config_dir.exists() {
            std::fs::create_dir_all(&config_dir)
                .with_context(|| format!("Failed to create config directory: {:?}", config_dir))?;
        }

        let path = config_dir.join("projects.json");
        Ok(Self { path })
    }

    pub fn load(&self) -> Result<Vec<Project>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&self.path)
            .with_context(|| format!("Failed to read projects file: {:?}", self.path))?;

        if content.trim().is_empty() {
            return Ok(Vec::new());
        }

        let projects: Vec<Project> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse projects file: {:?}", self.path))?;

        Ok(projects)
    }

    pub fn save(&self, projects: &[Project]) -> Result<()> {
        let content = serde_json::to_string_pretty(projects)
            .context("Failed to serialize projects")?;

        let tmp_path = self.path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &content)
            .with_context(|| format!("Failed to write temp projects file: {:?}", tmp_path))?;
        std::fs::rename(&tmp_path, &self.path)
            .with_context(|| format!("Failed to rename temp projects file: {:?}", tmp_path))?;

        Ok(())
    }

    pub fn find_by_name(&self, name: &str) -> Result<Option<Project>> {
        let projects = self.load()?;
        Ok(projects.into_iter().find(|p| p.name == name))
    }

    pub fn register(&self, project: &Project) -> Result<()> {
        let mut projects = self.load()?;
        // Update if same name exists
        if let Some(existing) = projects.iter_mut().find(|p| p.name == project.name) {
            existing.path = project.path.clone();
        } else {
            projects.push(project.clone());
        }
        self.save(&projects)
    }

    pub fn unregister(&self, name: &str) -> Result<bool> {
        let mut projects = self.load()?;
        let before = projects.len();
        projects.retain(|p| p.name != name);
        let removed = projects.len() < before;
        if removed {
            self.save(&projects)?;
        }
        Ok(removed)
    }

    pub fn list(&self) -> Result<Vec<Project>> {
        self.load()
    }
}

/// Detect git repository root from a given path
fn detect_git_root(path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        None
    } else {
        Some(root)
    }
}

/// Derive project name from a directory path (last component)
fn project_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".to_string())
}

/// Resolve a project from input string.
/// Input can be a registered project name or a filesystem path.
pub fn resolve_project(store: &ProjectStore, input: &str) -> Result<Project> {
    // 1. Check if input matches a registered project name
    if let Some(project) = store.find_by_name(input)? {
        return Ok(project);
    }

    // 2. Treat input as a path
    let path = std::path::Path::new(input);
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    let abs_str = abs_path.to_string_lossy().to_string();

    // Try to detect git root
    let project_path = if abs_path.exists() {
        detect_git_root(&abs_str).unwrap_or(abs_str)
    } else {
        // Path doesn't exist yet, use as-is
        abs_str
    };

    let name = project_name_from_path(&project_path);

    // Check if same name with different path already exists
    if let Some(existing) = store.find_by_name(&name)? {
        if existing.path == project_path {
            return Ok(existing);
        }
        anyhow::bail!(
            "Project '{}' already registered with different path: {}\nUse --name to specify a different name",
            name,
            existing.path
        );
    }

    // Auto-register new project
    let project = Project {
        name: name.clone(),
        path: project_path,
    };
    store.register(&project)?;

    Ok(project)
}

/// Resolve project from optional input, falling back to current directory.
pub fn resolve_project_or_cwd(store: &ProjectStore, input: Option<&str>) -> Result<Project> {
    match input {
        Some(input) => resolve_project(store, input),
        None => {
            let cwd = std::env::current_dir()
                .context("Failed to get current directory")?;
            let cwd_str = cwd.to_string_lossy().to_string();

            // Try git root detection
            let project_path = detect_git_root(&cwd_str).unwrap_or(cwd_str);
            let name = project_name_from_path(&project_path);

            // Check existing registration
            if let Some(existing) = store.find_by_name(&name)? {
                if existing.path == project_path {
                    return Ok(existing);
                }
                // Same name, different path — use path-based name with suffix
                let unique_name = format!("{}-{}", name, &project_path.len());
                let project = Project {
                    name: unique_name,
                    path: project_path,
                };
                store.register(&project)?;
                return Ok(project);
            }

            let project = Project {
                name,
                path: project_path,
            };
            store.register(&project)?;

            Ok(project)
        }
    }
}
