use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Defines a complete, end-to-end workflow.
/// Parsed from a `flows/*.yml` file.
#[derive(Debug, Serialize, Deserialize)]
pub struct Flow {
    pub id: String,
    pub description: String,
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
    StaticText { content: String },
    #[serde(rename = "file_contents")]
    FileContents {
        #[serde(default)]
        scopes: Vec<String>, // Names of scope files in `ctx/` dir, e.g., ["base", "ai"]. "prompt" is a special name for the scope from the user's prompt file.
        #[serde(default)]
        prefix: String,
    },
    #[serde(rename = "prompt_file_field")]
    PromptFileField { field: String, prefix: String },
    #[serde(rename = "previous_output")]
    PreviousOutput { block_id: String, prefix: String },
}

/// Modifies the execution behavior of a block.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Annotations {
    #[serde(default)]
    pub history: History,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub structured_output_schema: Option<String>,
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
