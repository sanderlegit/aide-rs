use crate::error::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::info;

#[derive(Serialize, Deserialize, Debug)]
struct LogEntry<T> {
    timestamp: DateTime<Utc>,
    payload: T,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PromptLog {
    pub model_name: String,
    pub system_prompt: String,
    pub user_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_prompt: Option<String>,
    pub tools: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResponseLog {
    pub model_name: String,
    pub response: crate::gemini_types::GenerateContentResponse,
    pub time_taken_ms: u128,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallLog {
    pub tool_name: String,
    pub tool_args: serde_json::Value,
    pub result: ToolResultLog,
    pub time_taken_ms: u128,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultLog {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub output_json: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ValidationLog {
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub time_taken_ms: u128,
}

#[derive(Clone)]
pub struct RunLogger {
    _log_dir: PathBuf,
    summary_log_path: PathBuf,
    complete_log_path: PathBuf,
    // Using Mutex for interior mutability to be able to write from `&self` methods.
    summary_file: Arc<Mutex<File>>,
    complete_file: Arc<Mutex<File>>,
}

impl RunLogger {
    pub fn new() -> Result<Self> {
        let run_id = Utc::now().format("%Y%m%d_%H%M%S");
        let log_dir = PathBuf::from(format!(".ai/logs/{}", run_id));
        std::fs::create_dir_all(&log_dir)?;

        let summary_log_path = log_dir.join("summary.log");
        let complete_log_path = log_dir.join("complete.log.jsonl");

        let summary_file = Arc::new(Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&summary_log_path)?,
        ));
        let complete_file = Arc::new(Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&complete_log_path)?,
        ));

        let logger = Self {
            _log_dir: log_dir,
            summary_log_path,
            complete_log_path,
            summary_file,
            complete_file,
        };

        logger.log_summary(&format!("Log run started at: {}\n", run_id));
        logger.log_summary(&format!(
            "Full logs at: {}\n",
            logger.complete_log_path.display()
        ));
        logger.log_summary(&format!(
            "Summary log at: {}\n\n",
            logger.summary_log_path.display()
        ));

        Ok(logger)
    }

    fn log_complete<T: Serialize>(&self, payload: T) {
        let entry = LogEntry {
            timestamp: Utc::now(),
            payload,
        };
        if let Ok(json_string) = serde_json::to_string(&entry) {
            if let Ok(mut file) = self.complete_file.lock() {
                let _ = writeln!(file, "{}", json_string);
            }
        }
    }

    pub fn log_summary(&self, message: &str) {
        info!("{}", message);
        if let Ok(mut file) = self.summary_file.lock() {
            let _ = writeln!(file, "{}", message);
        }
    }

    pub fn log_prompt(&self, mut log: PromptLog) {
        let summary = if let Some(display) = log.display_prompt.take() {
            display
        } else {
            // Fallback for prompts that don't use the new system, or for older logs.
            format!(
                "> {}\n... (full prompt in complete.log.jsonl)",
                log.user_prompt.lines().next().unwrap_or("").trim()
            )
        };

        self.log_summary(&format!(
            "[{}] PROMPT to {}:\n{}\n",
            Utc::now().to_rfc3339(),
            log.model_name,
            summary
        ));
        self.log_complete(log);
    }

    pub fn log_response(&self, log: ResponseLog) {
        let text_parts = log.response.candidates.as_ref().map_or(String::new(), |c| {
            c.iter()
                .flat_map(|cand| &cand.content.parts)
                .filter_map(|part| part.text.as_ref())
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        });
        let function_calls = log.response.candidates.as_ref().map_or(String::new(), |c| {
            c.iter()
                .flat_map(|cand| &cand.content.parts)
                .filter_map(|part| part.function_call.as_ref())
                .map(|fc| format!("CALL: {}(...)", fc.name))
                .collect::<Vec<_>>()
                .join("\n")
        });

        self.log_summary(&format!(
            "[{}] RESPONSE from {} ({}ms):\n--- TEXT ---\n{}\n--- CALLS ---\n{}\n---\n",
            Utc::now().to_rfc3339(),
            log.model_name,
            log.time_taken_ms,
            if text_parts.trim().is_empty() {
                "None"
            } else {
                &text_parts
            },
            if function_calls.is_empty() {
                "None"
            } else {
                &function_calls
            }
        ));
        self.log_complete(log);
    }

    pub fn log_tool_call(&self, log: ToolCallLog) {
        self.log_summary(&format!(
            "[{}] TOOL CALL: {} ({}ms)\n--- ARGS ---\n{}\n--- RESULT ---\nSuccess: {}\nStdout: {}\nStderr: {}\n---\n",
            Utc::now().to_rfc3339(),
            log.tool_name,
            log.time_taken_ms,
            serde_json::to_string_pretty(&log.tool_args).unwrap_or_default(),
            log.result.success,
            log.result.stdout,
            log.result.stderr
        ));
        self.log_complete(log);
    }

    pub fn log_validation(&self, log: ValidationLog) {
        self.log_summary(&format!(
            "[{}] VALIDATION: `{}` ({}ms)\n--- RESULT ---\nSuccess: {}\nExit Code: {}\nStdout: {}\nStderr: {}\n---\n",
            Utc::now().to_rfc3339(),
            log.command,
            log.time_taken_ms,
            log.success,
            log.exit_code,
            log.stdout,
            log.stderr
        ));
        self.log_complete(log);
    }
}
