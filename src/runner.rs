use crate::error::Result;
use crate::flows::types::Flow;
use crate::logging::RunLogger;
use crate::prompt::PromptBuilder;
use crate::tools::ToolExecutor;
use std::collections::HashMap;
use std::path::Path;

pub struct FlowRunner {
    logger: RunLogger,
    prompt_builder: PromptBuilder,
    // We will store the output of each block here, keyed by block.id.
    block_outputs: HashMap<String, serde_json::Value>,
}

impl FlowRunner {
    pub fn new(logger: RunLogger) -> Result<Self> {
        Ok(Self {
            logger,
            prompt_builder: PromptBuilder::new(),
            block_outputs: HashMap::new(),
        })
    }

    pub async fn run(&mut self, flow: &Flow, prompt_path: &Path) -> Result<()> {
        self.logger.log_summary(&format!(
            "Starting flow '{}' with prompt '{}'...",
            flow.id,
            prompt_path.display()
        ));

        for block in &flow.blocks {
            self.logger
                .log_summary(&format!("Executing block: '{}'...", block.id));

            // 1. Initialize the tool executor for this block.
            let _tool_executor = ToolExecutor::new(&block.annotations.tools);

            // 2. Build the prompt.
            let prompt_string = self
                .prompt_builder
                .build(&block.prompt, prompt_path, &self.block_outputs)
                .await?;
            println!("PROMPT for block '{}':\n{}", block.id, prompt_string);

            // 3. TODO: Call Gemini API with the prompt and tool schemas.

            // 4. TODO: Handle response, execute tools if necessary.

            // 5. TODO: Run verification logic.

            // 6. TODO: Store block output in self.block_outputs.
        }

        self.logger
            .log_summary(&format!("Flow '{}' finished.", flow.id));
        Ok(())
    }
}
