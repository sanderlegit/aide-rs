use crate::agents::aider::AiderWrapper;
use crate::error::Result;
use crate::gemini::GeminiClientWrapper;
use crate::logging::RunLogger;
use crate::session::Session;
use tracing::info;

/// The main orchestrator for managing AI workflows.
pub struct Orchestrator {
    logger: RunLogger,
    gemini: GeminiClientWrapper,
    aider: AiderWrapper,
}

impl Orchestrator {
    pub fn new() -> Result<Self> {
        let logger = RunLogger::new()?;
        // TODO: Make model configurable
        let gemini = GeminiClientWrapper::new("gemini-1.5-pro".to_string(), logger.clone())?;
        let aider = AiderWrapper;
        Ok(Self {
            logger,
            gemini,
            aider,
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn research(&self, objective: String, files: Vec<String>) -> Result<()> {
        let session = Session::new("research", &objective)?;
        info!(objective, ?files, "Starting research strategy.");

        let research_prompt = format!(
            "Please research the following topic and provide a detailed summary in a markdown document.
            I am particularly interested in the latest versions of any relevant Rust libraries and common design patterns.

            Topic: {}

            Here are some files from my project for context:
            {}",
            objective,
            files.join("\n")
        );

        // For now, we will just print the plan. Later this will call Gemini and then Aider.
        println!("\n--- RESEARCH PLAN ---\n");
        println!("Session ID: {}", session.id);
        println!("Objective: {}", objective);
        println!("Files: {:?}", files);
        println!("Prompt for Gemini:\n{}", research_prompt);
        println!("\n--- END RESEARCH PLAN ---\n");

        self.logger
            .log_summary("Research strategy completed (mock).");
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn plan(&self, objective: String, files: Vec<String>) -> Result<()> {
        let session = Session::new("plan", &objective)?;
        info!(objective, ?files, "Starting plan strategy.");

        let plan_prompt = format!(
            "Please help me plan the tasks for my implementation. I need to do the following:
            '{}'

            Based on that, and the provided file context, please create a markdown task list.",
            objective
        );

        // For now, we will just print the plan. Later this will call Gemini and then Aider.
        println!("\n--- PLAN ---\n");
        println!("Session ID: {}", session.id);
        println!("Objective: {}", objective);
        println!("Files: {:?}", files);
        println!("Prompt for Gemini -> Aider:\n{}", plan_prompt);
        println!("\n--- END PLAN ---\n");

        self.logger.log_summary("Plan strategy completed (mock).");
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn implement(
        &self,
        objective: String,
        files: Vec<String>,
        validate_cmd: String,
        auto: bool,
    ) -> Result<()> {
        let session = Session::new("implement", &objective)?;
        info!(objective, ?files, %validate_cmd, %auto, "Starting implement strategy.");

        let implement_prompt = format!(
            "Hello, can you help me with my implementation? I need to do the following:
            '{}'

            Please use the above information to get started. I will run `{}` after each of your attempts.
            ",
            objective, validate_cmd
        );

        // For now, we will just print the plan. Later this will call Aider.
        println!("\n--- IMPLEMENTATION PLAN ---\n");
        println!("Session ID: {}", session.id);
        println!("Objective: {}", objective);
        println!("Files: {:?}", files);
        println!("Auto-mode: {}", auto);
        println!("Validation Command: {}", validate_cmd);
        println!("Initial prompt for Aider:\n{}", implement_prompt);
        println!("\n--- END IMPLEMENTATION PLAN ---\n");

        self.logger
            .log_summary("Implement strategy completed (mock).");
        Ok(())
    }
}
