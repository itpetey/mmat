use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProcessCommand {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProcessOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

impl ProcessCommand {
    pub fn shell(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            working_dir: None,
            env: None,
        }
    }

    pub fn with_working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = Some(env);
        self
    }

    pub async fn execute(&self) -> Result<ProcessOutput, std::io::Error> {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(&self.command);

        if let Some(dir) = &self.working_dir {
            cmd.current_dir(dir);
        }

        if let Some(env) = &self.env {
            for (key, value) in env {
                cmd.env(key, value);
            }
        }

        let output = cmd.output().await?;

        Ok(ProcessOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

impl ProcessOutput {
    pub fn stdout_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.stdout)
    }

    pub fn stderr_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn successful_command() {
        let output = ProcessCommand::shell("echo hello").execute().await.unwrap();
        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout_str().unwrap().trim(), "hello");
        assert!(output.stderr.is_empty());
    }

    #[tokio::test]
    async fn failed_command() {
        let output = ProcessCommand::shell("exit 1").execute().await.unwrap();
        assert_eq!(output.exit_code, 1);
    }

    #[tokio::test]
    async fn working_directory() {
        let output = ProcessCommand::shell("pwd")
            .with_working_dir("/tmp")
            .execute()
            .await
            .unwrap();
        assert!(output.stdout_str().unwrap().contains("tmp"));
    }

    #[test]
    fn process_output_round_trip() {
        let output = ProcessOutput {
            stdout: b"hello".to_vec(),
            stderr: b"error".to_vec(),
            exit_code: 0,
        };
        let json = serde_json::to_string(&output).unwrap();
        let back: ProcessOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(output, back);
    }
}
