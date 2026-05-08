use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};

/// A shell command to execute.
///
/// Encapsulates the command string, optional working directory, and environment
/// variables. Commands are executed via `sh -c` on the system shell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProcessCommand {
    /// The shell command string to execute.
    pub command: String,
    /// Optional working directory in which to run the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<PathBuf>,
    /// Optional environment variables to set for the command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
}

/// The output captured from a process execution.
///
/// Contains the raw stdout and stderr byte streams along with the exit code.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProcessOutput {
    /// Raw bytes written to standard output.
    pub stdout: Vec<u8>,
    /// Raw bytes written to standard error.
    pub stderr: Vec<u8>,
    /// The process exit code. A value of `-1` indicates the process was
    /// terminated by a signal (no exit code available).
    pub exit_code: i32,
}

impl ProcessCommand {
    /// Creates a new command from a shell command string.
    ///
    /// The command is run through `sh -c`, so shell syntax (pipes, redirects,
    /// etc.) is supported.
    pub fn shell(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            working_dir: None,
            env: None,
        }
    }

    /// Sets the working directory in which the command will execute.
    pub fn with_working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Sets environment variables to be passed to the command.
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = Some(env);
        self
    }

    /// Executes the command asynchronously and returns the captured output.
    ///
    /// Spawns `sh -c <command>` via `tokio::process::Command` and waits for
    /// completion.
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
    /// Returns stdout interpreted as a UTF-8 string slice.
    ///
    /// Returns an error if the stdout bytes are not valid UTF-8.
    pub fn stdout_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.stdout)
    }

    /// Returns stderr interpreted as a UTF-8 string slice.
    ///
    /// Returns an error if the stderr bytes are not valid UTF-8.
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
