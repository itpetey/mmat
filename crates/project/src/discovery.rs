//! Repository discovery and project type detection.
//!
//! This module scans a directory for well-known project marker files
//! (e.g. `Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`) and
//! extracts project metadata such as name and language-specific source files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

/// Errors that can occur during project discovery.
#[derive(Error, Debug)]
pub enum DiscoveryError {
    /// An I/O operation failed while reading filesystem entries.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Parsing a JSON manifest (e.g. `package.json`) failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// No recognised project was found at the given path.
    #[error("No project found at: {0}")]
    NoProjectFound(String),
}

/// The detected programming language or framework for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectType {
    /// A Rust project (marker: `Cargo.toml`).
    Rust,
    /// A Node.js project (marker: `package.json`).
    Node,
    /// A Python project (marker: `pyproject.toml` or `requirements.txt`).
    Python,
    /// A Go project (marker: `go.mod`).
    Go,
    /// An unrecognised project type with no known marker file.
    Unknown,
}

/// Information about a detected project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    /// The programming language or framework of the project.
    pub project_type: ProjectType,
    /// The root directory path of the project.
    pub root_path: PathBuf,
    /// The human-readable name extracted from the project manifest.
    pub name: String,
    /// Source files belonging to the project, filtered by language extension.
    pub language_files: Vec<String>,
}

/// Stateless entry point for detecting and describing a project at a given path.
pub struct RepoDiscovery;

impl RepoDiscovery {
    /// Scan the given directory for a known project marker and return
    /// [`ProjectInfo`] if one is found.
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

/// Scan a directory for valid project subdirectories.
///
/// Returns a list of [`ProjectInfo`] for each subdirectory that contains
/// a recognised project marker file.
pub fn discover_projects(work_dir: &Path) -> Result<Vec<ProjectInfo>, DiscoveryError> {
    let mut projects = Vec::new();
    let entries = std::fs::read_dir(work_dir).map_err(DiscoveryError::Io)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && let Ok(info) = RepoDiscovery::detect(&path)
        {
            projects.push(info);
        }
    }
    Ok(projects)
}
