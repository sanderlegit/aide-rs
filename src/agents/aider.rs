use crate::error::{Error, Result};
use crate::session::Session;
use std::env;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, instrument};

/// A wrapper for executing the `aider` command-line tool.
pub struct AiderWrapper;

/// The result of an `aider` execution.
pub struct AiderRunResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl AiderWrapper {
    /// Executes `aider` with a given set of files and an initial message.
    #[instrument(skip(self, session, message))]
    pub async fn run(
        &self,
        session: &Session,
        files_to_edit: Vec<String>,
        message: &str,
        auto: bool,
        validate_cmd: Option<String>,
    ) -> Result<AiderRunResult> {
        let base_command = env::var("AIDER_COMMAND")
            .map_err(|_| Error::Config("AIDER_COMMAND environment variable not set.".to_string()))?;

        let mut args = base_command
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<String>>();

        // The first part of the split is the command itself.
        let command_name = args.remove(0);

        // In auto mode, prevent aider from checking for updates, which can cause it
        // to crash in a non-interactive environment.
        if auto && !args.iter().any(|arg| arg == "--no-check-update") {
            args.push("--no-check-update".to_string());
        }

        // Add session-specific arguments
        args.push("--chat-history-file".to_string());
        args.push(
            session
                .aider_chat_history_path
                .to_str()
                .unwrap()
                .to_string(),
        );

        // Add files to edit
        args.extend(files_to_edit);

        // Add the initial message
        args.push("--message".to_string());
        args.push(message.to_string());

        if let Some(cmd) = validate_cmd {
            args.push("--test-cmd".to_string());
            args.push(cmd);
        }

        info!(command = %command_name, args = ?args, "Executing aider");

        let mut command = Command::new(&command_name);
        command.args(&args);

        if auto {
            let output = command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            if !output.status.success() {
                info!(status = %output.status, "aider exited with non-zero status");
            }

            Ok(AiderRunResult {
                success: output.status.success(),
                stdout,
                stderr,
            })
        } else {
            let mut child = command
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()?;

            let status = child.wait().await?;

            if !status.success() {
                return Err(Error::ToolFailed(format!(
                    "aider exited with non-zero status: {}",
                    status
                )));
            }

            Ok(AiderRunResult {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
}
