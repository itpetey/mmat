use std::path::Path;

use thiserror::Error;
use tracing::info;

use crate::discovery::ProjectType;

#[derive(Error, Debug)]
pub enum ScaffoldError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Directory already exists: {0}")]
    DirectoryExists(String),

    #[error("Unsupported project type: {0}")]
    UnsupportedType(String),
}

pub struct ProjectScaffold;

impl ProjectScaffold {
    pub fn create_rust_project(path: &Path, name: &str) -> Result<(), ScaffoldError> {
        info!("Creating Rust project: {} at {}", name, path.display());

        std::fs::create_dir_all(path.join("src"))?;

        let cargo_toml = format!(
            r#"[package]
name = "{}"
version = "0.1.0"
edition = "2024"

[dependencies]
"#,
            name
        );
        std::fs::write(path.join("Cargo.toml"), cargo_toml)?;

        let main_rs = r#"fn main() {
    println!("Hello, world!");
}
"#;
        std::fs::write(path.join("src/main.rs"), main_rs)?;

        info!("Rust project created successfully");
        Ok(())
    }

    pub fn create_node_project(path: &Path, name: &str) -> Result<(), ScaffoldError> {
        info!("Creating Node project: {} at {}", name, path.display());

        std::fs::create_dir_all(path.join("src"))?;

        let package_json = serde_json::json!({
            "name": name,
            "version": "0.1.0",
            "description": "",
            "main": "src/index.js",
            "scripts": {
                "start": "node src/index.js",
                "test": "echo \"Error: no test specified\" && exit 1"
            },
            "keywords": [],
            "author": "",
            "license": "ISC"
        });
        std::fs::write(
            path.join("package.json"),
            serde_json::to_string_pretty(&package_json)?,
        )?;

        let index_js = r#"console.log("Hello, world!");
"#;
        std::fs::write(path.join("src/index.js"), index_js)?;

        info!("Node project created successfully");
        Ok(())
    }

    pub fn create_python_project(path: &Path, name: &str) -> Result<(), ScaffoldError> {
        info!("Creating Python project: {} at {}", name, path.display());

        std::fs::create_dir_all(path.join(name))?;
        std::fs::write(path.join(name).join("__init__.py"), "")?;

        let pyproject_toml = format!(
            r#"[project]
name = "{}"
version = "0.1.0"
description = ""
requires-python = ">=3.12"
"#,
            name
        );
        std::fs::write(path.join("pyproject.toml"), pyproject_toml)?;

        info!("Python project created successfully");
        Ok(())
    }

    pub fn create(
        path: &Path,
        name: &str,
        project_type: &ProjectType,
    ) -> Result<(), ScaffoldError> {
        if path.exists() {
            return Err(ScaffoldError::DirectoryExists(path.display().to_string()));
        }

        std::fs::create_dir_all(path)?;

        match project_type {
            ProjectType::Rust => Self::create_rust_project(path, name),
            ProjectType::Node => Self::create_node_project(path, name),
            ProjectType::Python => Self::create_python_project(path, name),
            ProjectType::Go => Err(ScaffoldError::UnsupportedType("Go".to_string())),
            ProjectType::Unknown => Err(ScaffoldError::UnsupportedType("Unknown".to_string())),
        }
    }
}
