use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Defines a complete, end-to-end workflow.
/// Parsed from a `flows/*.yml` file.
#[derive(Debug, Serialize, Deserialize)]
pub struct Flow {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub model: Option<String>,
    pub blocks: Vec<Block>,
}

/// A single, atomic step within a Flow.
#[derive(Debug, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    #[serde(default)]
    pub description: String,
    pub prompt: Prompt,
    #[serde(default)]
    pub annotations: Annotations,
    #[serde(default)]
    pub verification: Option<Verification>,
    #[serde(default)]
    pub looping: Option<LoopingStrategy>,
}

/// Defines how to construct a prompt for the LLM.
#[derive(Debug, Serialize, Deserialize)]
pub struct Prompt {
    pub composition: Vec<PromptPart>,
}

/// A component of a prompt.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PromptPart {
    #[serde(rename = "static_text")]
    StaticText {
        content: String,
        #[serde(default)]
        hide_in_stdout: bool,
    },
    #[serde(rename = "file_contents")]
    FileContents {
        #[serde(default)]
        scopes: Vec<String>, // Names of scope files in `ctx/` dir, e.g., ["base", "ai"]. "prompt" is a special name for the scope from the user's prompt file.
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        hide_in_stdout: bool,
    },
    #[serde(rename = "file_list")]
    FileList {
        #[serde(default)]
        scopes: Vec<String>,
        #[serde(default)]
        prefix: String,
        #[serde(default)]
        hide_in_stdout: bool,
    },
    #[serde(rename = "prompt_file_field")]
    PromptFileField {
        field: String,
        prefix: String,
        #[serde(default)]
        hide_in_stdout: bool,
    },
    #[serde(rename = "previous_output")]
    PreviousOutput {
        block_id: String,
        prefix: String,
        #[serde(default)]
        hide_in_stdout: bool,
    },
}

/// Modifies the execution behavior of a block.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Annotations {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub history: History,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub structured_output_schema: Option<String>,
    #[serde(default)]
    pub save_output_to: Option<String>,
    #[serde(default)]
    pub commit_on_success: bool,
}

/// Defines how to validate a block's output and whether to loop on failure.
#[derive(Debug, Serialize, Deserialize)]
pub struct Verification {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    pub strategy: VerificationStrategy,
}

fn default_max_retries() -> u32 {
    5
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum VerificationStrategy {
    #[serde(rename = "command")]
    Command {
        command: String,
        expected_exit_code: i32,
        on_failure_prompt: Prompt,
    },
    #[serde(rename = "prompt")]
    Prompt {
        prompt: Prompt,
        success_condition: String, // e.g., "function_call:verification_passed"
    },
}

/// Defines file include/exclude rules.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct FileScope {
    #[serde(default)]
    pub description: Option<String>,
    pub include: Vec<String>, // Glob patterns
    pub exclude: Vec<String>, // Glob patterns
}

impl FileScope {
    /// Loads a FileScope from a YAML file.
    pub fn from_yaml_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        serde_yaml::from_str(&content).map_err(Into::into)
    }

    /// Merges another FileScope into this one, deduplicating patterns.
    /// The description from `other` is ignored.
    pub fn merge(&mut self, other: Self) {
        self.include.extend(other.include);
        self.include.sort();
        self.include.dedup();

        self.exclude.extend(other.exclude);
        self.exclude.sort();
        self.exclude.dedup();
    }
}

/// How much of the conversation history to include.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum History {
    Mode(HistoryMode),
    LastN { last_n: u32 },
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HistoryMode {
    Full,
    None,
}

impl Default for History {
    fn default() -> Self {
        History::Mode(HistoryMode::Full)
    }
}

// The following structs are used for structured task generation and execution.

/// A list of tasks, typically the output of a planning block.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TaskList {
    pub tasks: Vec<Task>,
}

/// A high-level task description.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: String,
    pub description: String,
}

/// A detailed, just-in-time plan for implementing a specific task or batch of tasks.
/// This is generated by a sub-prompt right before execution.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DetailedTaskPlan {
    pub task_ids: Vec<String>,
    pub plan_description: String,
    pub file_scopes: Vec<FileScope>,
    pub validation_steps: Vec<ValidationStep>,
}

/// A single command to be run for validation.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ValidationStep {
    pub command: String,
    pub expected_exit_code: i32,
}

/// Defines looping behavior for a block.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LoopingStrategy {
    /// The ID of the block whose output (which should be a list, e.g., TaskList) to iterate over.
    pub over: String,
    /// The name to expose each item of the list as in the `block_outputs` for this block's prompt composition.
    #[serde(rename = "as")]
    pub as_key: String,
    /// If true, clears the conversation history at the start of each iteration to keep context focused.
    #[serde(default)]
    pub clear_history_on_iteration: bool,
}
