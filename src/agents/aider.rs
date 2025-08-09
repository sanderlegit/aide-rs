use crate::error::{Error, Result};
use crate::session::Session;
use std::env;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, instrument};

/// A wrapper for executing the `aider` command-line tool.
pub struct AiderWrapper;

impl AiderWrapper {
    /// Executes `aider` with a given set of files and an initial message.
    #[instrument(skip(self, session))]
    pub async fn run(
        &self,
        session: &Session,
        files_to_edit: Vec<String>,
        message: &str,
    ) -> Result<()> {
        let base_command = env::var("AIDER_COMMAND")
            .map_err(|_| Error::Config("AIDER_COMMAND environment variable not set.".to_string()))?;

        let mut args = base_command
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<String>>();

        // The first part of the split is the command itself.
        let command_name = args.remove(0);

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

        info!(command = %command_name, args = ?args, "Executing aider");

        let mut child = Command::new(&command_name)
            .args(&args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        let status = child.await?;

        if !status.success() {
            return Err(Error::ToolFailed(format!(
                "aider exited with non-zero status: {}",
                status
            )));
        }

        Ok(())
    }
}
