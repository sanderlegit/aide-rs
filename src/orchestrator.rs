use crate::agents::aider::AiderWrapper;
use crate::error::{Error, Result};
use crate::gemini::GeminiClientWrapper;
use crate::logging::RunLogger;
use crate::session::Session;
use crate::tools::ToolExecutor;
use crate::vcs;
use tracing::{error, info};

/// The main orchestrator for managing AI workflows.
pub struct Orchestrator {
    logger: RunLogger,
    gemini: GeminiClientWrapper,
    aider: AiderWrapper,
    tool_executor: ToolExecutor,
}

impl Orchestrator {
    pub fn new() -> Result<Self> {
        let logger = RunLogger::new()?;
        // TODO: Make model configurable
        let gemini = GeminiClientWrapper::new("gemini-1.5-pro".to_string(), logger.clone())?;
        let aider = AiderWrapper;
        // For now, enable all tools. Later this could be configured per-strategy.
        let tool_executor = ToolExecutor::new(&["doc_retriever".to_string()]);
        Ok(Self {
            logger,
            gemini,
            aider,
            tool_executor,
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

        let contents = vec![crate::gemini_types::Content {
            parts: vec![crate::gemini_types::ContentPart::new_text(research_prompt)],
            role: crate::gemini_types::Role::User,
        }];

        let research_tool = crate::gemini_types::Tool {
            google_search_retrieval: Some(crate::gemini_types::GoogleSearchRetrieval::default()),
            ..Default::default()
        };
        let tools = Some(vec![research_tool]);

        let response = self.gemini.generate_content(contents, tools).await?;

        let research_text = response
            .candidates
            .and_then(|mut c| c.pop())
            .and_then(|c| c.content.parts.into_iter().next())
            .and_then(|p| p.text)
            .unwrap_or_else(|| "No response text from Gemini.".to_string());

        let research_file_path = session.dir.join("research.md");
        std::fs::write(&research_file_path, &research_text)?;

        self.logger.log_summary(&format!(
            "Research summary saved to {}",
            research_file_path.display()
        ));

        // Optional: launch aider to refine
        info!("Launching aider to review and refine the research document.");
        self.aider
            .run(
                &session,
                vec![research_file_path.to_str().unwrap().to_string()],
                "Here is the research document I generated. Please review it.",
                false,
                None,
            )
            .await?;

        self.logger.log_summary("Research strategy completed.");
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn plan(&self, objective: String, files: Vec<String>) -> Result<()> {
        let session = Session::new("plan", &objective)?;
        info!(objective, ?files, "Starting plan strategy.");

        let mut file_context = String::new();
        for file_path in &files {
            match std::fs::read_to_string(file_path) {
                Ok(content) => {
                    file_context
                        .push_str(&format!("\n---\nFile: {}\n---\n{}\n", file_path, content));
                }
                Err(e) => {
                    // Log a warning but continue, as some files might not be critical for planning
                    tracing::warn!(file_path, error = %e, "Failed to read file for planning context");
                }
            }
        }

        let plan_prompt = format!(
            "Please help me plan the tasks for my implementation. I need to do the following:
            '{}'

            Based on that, and the provided file context, please create a markdown task list.
            {}",
            objective, file_context
        );

        let contents = vec![crate::gemini_types::Content {
            parts: vec![crate::gemini_types::ContentPart::new_text(plan_prompt)],
            role: crate::gemini_types::Role::User,
        }];

        let response = self.gemini.generate_content(contents, None).await?;

        let plan_text = response
            .candidates
            .and_then(|mut c| c.pop())
            .and_then(|c| c.content.parts.into_iter().next())
            .and_then(|p| p.text)
            .unwrap_or_else(|| "No response text from Gemini.".to_string());

        let plan_file_path = session.dir.join("plan.md");
        std::fs::write(&plan_file_path, &plan_text)?;

        self.logger
            .log_summary(&format!("Plan saved to {}", plan_file_path.display()));

        let mut files_for_aider = files;
        files_for_aider.push(plan_file_path.to_str().unwrap().to_string());

        info!("Launching aider to review and refine the plan.");
        self.aider
            .run(
                &session,
                files_for_aider,
                &format!(
                    "Here is the plan I generated, stored in `{}`. Please review it and help me refine it.",
                    plan_file_path.display()
                ),
                false,
                None,
            )
            .await?;

        self.logger.log_summary("Plan strategy completed.");
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

        let mut current_objective = format!(
            "Hello, can you help me with my implementation? I need to do the following:
            '{}'

            Please use the above information to get started. I will run `{}` after each of your attempts.
            ",
            objective, validate_cmd
        );

        if !auto {
            self.aider
                .run(&session, files, &current_objective, false, None)
                .await?;
            self.logger
                .log_summary("Implement strategy completed (interactive).");
            return Ok(());
        }

        // Automated loop
        let max_retries = 5;
        for i in 0..max_retries {
            info!(attempt = i + 1, max_attempts = max_retries, "Running aider in auto mode.");

            let result = self
                .aider
                .run(
                    &session,
                    files.clone(),
                    &current_objective,
                    true,
                    Some(validate_cmd.clone()),
                )
                .await?;

            if result.success {
                self.logger.log_summary(&format!(
                    "Aider succeeded on attempt {}/{}.",
                    i + 1,
                    max_retries
                ));
                let commit_message = format!("Implement: {}", objective);
                self.logger.log_summary(&format!(
                    "Committing changes with message: {}",
                    commit_message
                ));
                let repo_path = std::env::current_dir()?;
                let file_paths = files.iter().map(std::path::PathBuf::from).collect::<Vec<_>>();
                vcs::add_and_commit(&repo_path, &file_paths, &commit_message)?;
                return Ok(());
            }

            self.logger.log_summary(&format!(
                "Aider failed on attempt {}/{}. Analyzing failure...",
                i + 1,
                max_retries
            ));

            // Implement Gemini-based debugging.
            let debug_prompt = format!(
                "The last attempt to fix the code failed. I need your help to figure out what to do next.
                Based on the error output below, what documentation should I look up using the `doc_retriever` tool?
                Please call the tool with the most relevant `crate_name` and `path` to get documentation that might help solve the error.

                Validation command STDOUT:
                {}

                Validation command STDERR:
                {}",
                result.stdout, result.stderr
            );

            let contents = vec![crate::gemini_types::Content {
                parts: vec![crate::gemini_types::ContentPart::new_text(debug_prompt)],
                role: crate::gemini_types::Role::User,
            }];

            let tools = Some(vec![crate::gemini_types::Tool {
                function_declarations: self.tool_executor.schemas(),
                ..Default::default()
            }]);

            let response = self.gemini.generate_content(contents, tools).await?;

            let mut retrieved_docs = "No documentation was retrieved.".to_string();

            if let Some(function_call) = response
                .candidates
                .as_ref()
                .and_then(|c| c.first())
                .and_then(|c| c.content.parts.first())
                .and_then(|p| p.function_call.as_ref())
            {
                info!(call = ?function_call, "Gemini requested a tool call for debugging");
                match self.tool_executor.execute(function_call).await {
                    Ok(docs) => {
                        retrieved_docs = serde_json::to_string_pretty(&docs)
                            .unwrap_or_else(|_| "Failed to format documentation.".to_string());
                        info!(docs = %retrieved_docs, "Retrieved documentation");
                    }
                    Err(e) => {
                        error!(error = %e, "Tool execution failed");
                        retrieved_docs = format!("Failed to retrieve documentation: {}", e);
                    }
                }
            } else {
                info!("Gemini did not request a tool call for debugging.");
            }

            current_objective = format!(
                "The last attempt failed. Here is the output from the validation command:\n\nSTDOUT:\n{}\n\nSTDERR:\n{}\n\nI tried to find relevant documentation and got this:\n\n{}\n\nPlease use this information to fix the code.",
                result.stdout, result.stderr, retrieved_docs
            );
        }

        self.logger
            .log_summary("Implement strategy failed after max retries.");
        Err(Error::ToolFailed(
            "Aider failed to complete the objective after maximum retries.".to_string(),
        ))
    }
}
