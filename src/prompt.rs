use crate::error::Result;
use crate::flows::types::Prompt;

/// Constructs a final prompt string from a `Prompt` definition.
pub struct PromptBuilder {}

impl PromptBuilder {
    pub fn new() -> Self {
        Self {}
    }

    /// Builds the prompt, resolving all composition parts.
    pub async fn build(&self, _prompt_def: &Prompt) -> Result<String> {
        // TODO: Implement the logic to process the prompt definition.
        // - Handle static_text
        // - Handle prompt_file_field
        // - Handle file_contents by loading scopes and files
        // - Handle previous_output by looking up results from a state map
        Ok("This is a stub prompt.".to_string())
    }
}
