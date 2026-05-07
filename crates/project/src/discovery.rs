use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

#[derive(Error, Debug)]
pub enum DiscoveryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("No project found at: {0}")]
    NoProjectFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub project_type: ProjectType,
    pub root_path: PathBuf,
    pub name: String,
    pub language_files: Vec<String>,
}

pub struct RepoDiscovery;

impl RepoDiscovery {
    pub fn detect(path: &Path) -> Result<ProjectInfo, DiscoveryError> {
        info!("Detecting project at: {}", path.display());

        let markers = [
            ("Cargo.toml", ProjectType::Rust),
            ("package.json", ProjectType::Node),
            ("pyproject.toml", ProjectType::Python),
            ("requirements.txt", ProjectType::Python),
            ("go.mod", ProjectType::Go),
        ];

        for (file, project_type) in &markers {
            let marker_path = path.join(file);
            if marker_path.exists() {
                let name = Self::extract_name(path, file)?;
                let language_files = Self::find_language_files(path, project_type);

                info!(
                    "Detected {:?} project: {} at {}",
                    project_type,
                    name,
                    path.display()
                );

                return Ok(ProjectInfo {
                    project_type: project_type.clone(),
                    root_path: path.to_path_buf(),
                    name,
                    language_files,
                });
            }
        }

        Err(DiscoveryError::NoProjectFound(path.display().to_string()))
    }

    fn extract_name(path: &Path, marker: &str) -> Result<String, DiscoveryError> {
        match marker {
            "Cargo.toml" => Self::extract_cargo_name(path),
            "package.json" => Self::extract_package_name(path),
            _ => Ok(path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()),
        }
    }

    fn extract_cargo_name(path: &Path) -> Result<String, DiscoveryError> {
        let cargo_path = path.join("Cargo.toml");
        let content = std::fs::read_to_string(cargo_path)?;

        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("name")
                && let Some(value) = line.split('=').nth(1)
            {
                return Ok(value.trim().trim_matches('"').to_string());
            }
        }

        Ok(path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string())
    }

    fn extract_package_name(path: &Path) -> Result<String, DiscoveryError> {
        let package_path = path.join("package.json");
        let content = std::fs::read_to_string(package_path)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;

        Ok(json["name"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            }))
    }

    fn find_language_files(path: &Path, project_type: &ProjectType) -> Vec<String> {
        let extensions: Vec<&str> = match project_type {
            ProjectType::Rust => vec![".rs"],
            ProjectType::Node => vec![".ts", ".js", ".tsx", ".jsx"],
            ProjectType::Python => vec![".py"],
            ProjectType::Go => vec![".go"],
            ProjectType::Unknown => vec![],
        };

        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file()
                    && let Some(ext) = path.extension()
                {
                    let ext_str = format!(".{}", ext.to_string_lossy());
                    if extensions.contains(&ext_str.as_str())
                        && let Some(name) = path.file_name()
                    {
                        files.push(name.to_string_lossy().to_string());
                    }
                }
            }
        }

        files
    }
}
