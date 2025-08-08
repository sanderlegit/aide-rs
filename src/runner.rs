use crate::error::Result;
use crate::flows::types::Flow;
use crate::logging::RunLogger;
use std::path::Path;

pub struct FlowRunner {
    logger: RunLogger,
}

impl FlowRunner {
    pub fn new(logger: RunLogger) -> Result<Self> {
        Ok(Self { logger })
    }

    pub async fn run(&self, flow: &Flow, prompt_path: &Path) -> Result<()> {
        self.logger.log_summary(&format!(
            "Starting flow '{}' with prompt '{}'...",
            flow.id,
            prompt_path.display()
        ));
        // TODO: Implement the full flow execution logic here.
        // - Load prompt file
        // - Iterate through blocks
        // - Construct prompts
        // - Call Gemini API
        // - Handle responses and tools
        // - Run verification
        println!("Flow '{}' is executing (stub).", flow.id);
        self.logger
            .log_summary(&format!("Flow '{}' finished.", flow.id));
        Ok(())
    }
}
