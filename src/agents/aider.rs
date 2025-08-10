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
        allow_shell_commands: bool,
    ) -> Result<AiderRunResult> {
        let base_command = env::var("AIDER_COMMAND")
            .map_err(|_| Error::Config("AIDER_COMMAND environment variable not set.".to_string()))?;

        let mut args = base_command
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<String>>();

        // The first part of the split is the command itself.
        let command_name = args.remove(0);

        // In auto mode, add flags to ensure it runs non-interactively and predictably.
        if auto {
            // Prevent aider from checking for updates, which can crash in non-interactive CI
            if !args.iter().any(|arg| arg == "--no-check-update") {
                args.push("--no-check-update".to_string());
            }
            // Always say yes to prompts in auto mode
            if !args.iter().any(|arg| arg == "--yes-always" || arg == "--yes") {
                args.push("--yes-always".to_string());
            }
            // Disable streaming for cleaner logs in auto mode
            if !args.iter().any(|arg| arg == "--no-stream") {
                args.push("--no-stream".to_string());
            }
            // Do not allow using URLs
            if !args.iter().any(|arg| arg == "--no-detect-urls") {
                args.push("--no-detect-urls".to_string());
            }
            // Do not allow running commands unless explicitly allowed
            if !allow_shell_commands && !args.iter().any(|arg| arg == "--no-suggest-shell-commands")
            {
                args.push("--no-suggest-shell-commands".to_string());
            }
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

        // Redact the message for cleaner logs
        let mut log_args = args.clone();
        if let Some(i) = log_args.iter().position(|arg| arg == "--message") {
            if i + 1 < log_args.len() {
                log_args[i + 1] = "<message content redacted>".to_string();
            }
        }

        info!(command = %command_name, args = ?log_args, "Executing aider");

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
