use crate::error::Result;
use chrono::Utc;
use std::path::{Path, PathBuf};
use tracing::info;

/// Manages the directories and paths for a single execution session.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub dir: PathBuf,
    pub orchestrator_log_path: PathBuf,
    pub aider_chat_history_path: PathBuf,
}

impl Session {
    /// Creates a new session, including the necessary directories on disk.
    pub fn new(strategy: &str, objective: &str) -> Result<Self> {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        // Sanitize objective for directory name
        let sanitized_objective = objective
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .replace(' ', "_");
        let objective_slug = sanitized_objective.chars().take(30).collect::<String>();

        let session_id = format!("{}_{}_{}", timestamp, strategy, objective_slug);
        let session_dir = Path::new(".ai").join("sessions").join(&session_id);

        info!(session_id = %session_id, path = %session_dir.display(), "Creating new session");
        std::fs::create_dir_all(&session_dir)?;

        let orchestrator_log_path = session_dir.join("orchestrator.log");
        let aider_chat_history_path = session_dir.join("aider.chat.history.md");

        Ok(Self {
            id: session_id,
            dir: session_dir,
            orchestrator_log_path,
            aider_chat_history_path,
        })
    }
}
