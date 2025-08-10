use crate::agents::aider::AiderWrapper;
use crate::error::{Error, Result};
use crate::file_provider;
use crate::gemini::GeminiClientWrapper;
use crate::logging::RunLogger;
use crate::session::Session;
use crate::tools::ToolExecutor;
use crate::vcs;
use serde::Deserialize;
use std::path::PathBuf;
use tracing::{error, info};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
enum StepConfig {
    Research {
        objective: String,
        context: String,
        model: Option<String>,
        output: Option<String>,
        #[serde(default)]
        files: Option<Vec<String>>,
    },
    Plan {
        objective: String,
        context: String,
        model: Option<String>,
    },
    Implement {
        objective: String,
        context: String,
        #[serde(default = "default_validate_cmd")]
        validate_cmd: String,
        max_retries: Option<u32>,
        model: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunConfig {
    steps: Vec<StepConfig>,
}

fn default_validate_cmd() -> String {
    "make test".to_string()
}

/// The main orchestrator for managing AI workflows.
pub struct Orchestrator {
    logger: RunLogger,
    gemini: GeminiClientWrapper,
    aider: AiderWrapper,
    tool_executor: ToolExecutor,
}

impl Orchestrator {
    pub fn new(model_override: Option<String>) -> Result<Self> {
        let logger = RunLogger::new()?;
        let default_model = "gemini-1.5-pro".to_string();
        let model_name = model_override.unwrap_or(default_model);
        let gemini = GeminiClientWrapper::new(model_name, logger.clone())?;
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
    pub async fn research(
        &self,
        objective: String,
        files: Vec<String>,
        interactive: bool,
        output_path: Option<String>,
        model_override: Option<&str>,
    ) -> Result<PathBuf> {
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

        let response = self
            .gemini
            .generate_content(contents, tools, model_override)
            .await?;

        let research_text = response
            .candidates
            .and_then(|mut c| c.pop())
            .and_then(|c| c.content.parts.into_iter().next())
            .and_then(|p| p.text)
            .unwrap_or_else(|| "No response text from Gemini.".to_string());

        let research_file_path = if let Some(path_str) = output_path {
            PathBuf::from(path_str)
        } else {
            session.dir.join("research.md")
        };

        if let Some(parent) = research_file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&research_file_path, &research_text)?;

        self.logger.log_summary(&format!(
            "Research summary saved to {}",
            research_file_path.display()
        ));

        if interactive {
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
        }

        self.logger.log_summary("Research strategy completed.");
        Ok(research_file_path)
    }

    #[tracing::instrument(skip(self))]
    pub async fn plan(
        &self,
        objective: String,
        files: Vec<String>,
        interactive: bool,
        research_context: Option<String>,
        model_override: Option<&str>,
    ) -> Result<PathBuf> {
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

        let research_prompt_addition = research_context
            .map(|ctx| format!("\n\nHere is some research context to consider:\n{}", ctx))
            .unwrap_or_default();

        let plan_prompt = format!(
            "Please help me plan the tasks for my implementation. I need to do the following:
            '{}'
            {}
            Based on that, and the provided file context, please create a markdown task list.
            {}",
            objective, research_prompt_addition, file_context
        );

        let contents = vec![crate::gemini_types::Content {
            parts: vec![crate::gemini_types::ContentPart::new_text(plan_prompt)],
            role: crate::gemini_types::Role::User,
        }];

        let response = self
            .gemini
            .generate_content(contents, None, model_override)
            .await?;

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

        if interactive {
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
        }

        self.logger.log_summary("Plan strategy completed.");
        Ok(plan_file_path)
    }

    #[tracing::instrument(skip(self))]
    pub async fn implement(
        &self,
        objective: String,
        files: Vec<String>,
        validate_cmd: String,
        auto: bool,
        max_retries: u32,
        model_override: Option<&str>,
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

            let response = self
                .gemini
                .generate_content(contents, tools, model_override)
                .await?;

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

    #[tracing::instrument(skip(self))]
    pub async fn run(&self, prompt_file: String) -> Result<()> {
        info!(prompt_file, "Starting run from config file.");

        let file_content = std::fs::read_to_string(&prompt_file)?;
        let config: RunConfig = serde_yaml::from_str(&file_content)?;

        let mut plan_file_path: Option<PathBuf> = None;
        let mut research_file_path: Option<PathBuf> = None;
        let mut last_objective: Option<String> = None;

        for step in config.steps {
            match step {
                StepConfig::Research {
                    objective,
                    context,
                    model,
                    output,
                    files: extra_files,
                } => {
                    info!(objective = %objective, "Running research step.");
                    last_objective = Some(objective.clone());
                    let mut files =
                        file_provider::get_files(&[".".to_string()], Some(&context), None)?;
                    if let Some(mut new_files) = extra_files {
                        files.append(&mut new_files);
                    }
                    files.sort();
                    files.dedup();

                    let path = self
                        .research(objective, files, false, output, model.as_deref())
                        .await?;
                    research_file_path = Some(path);
                }
                StepConfig::Plan {
                    objective,
                    context,
                    model,
                } => {
                    info!(objective = %objective, "Running plan step.");
                    last_objective = Some(objective.clone());
                    let files =
                        file_provider::get_files(&[".".to_string()], Some(&context), None)?;
                    let research_content = if let Some(path) = &research_file_path {
                        Some(std::fs::read_to_string(path)?)
                    } else {
                        None
                    };
                    let path = self
                        .plan(objective, files, false, research_content, model.as_deref())
                        .await?;
                    plan_file_path = Some(path);
                }
                StepConfig::Implement {
                    objective,
                    context,
                    validate_cmd,
                    max_retries,
                    model,
                } => {
                    info!(objective = %objective, "Running implement step.");
                    let mut files =
                        file_provider::get_files(&[".".to_string()], Some(&context), None)?;
                    let mut implement_objective = objective.clone();

                    if let Some(plan_path) = &plan_file_path {
                        let original_objective = last_objective.as_deref().unwrap_or(&objective);
                        implement_objective = format!(
                            "Implement the tasks described in the plan file `{}`. The original objective was: {}",
                            plan_path.display(),
                            original_objective
                        );
                        files.push(plan_path.to_str().unwrap().to_string());
                    }

                    if let Some(research_path) = &research_file_path {
                        files.push(research_path.to_str().unwrap().to_string());
                    }

                    self.implement(
                        implement_objective,
                        files,
                        validate_cmd,
                        true,
                        max_retries.unwrap_or(5),
                        model.as_deref(),
                    )
                    .await?;
                }
            }
        }

        self.logger
            .log_summary(&format!("Run from {} completed.", prompt_file));

        Ok(())
    }
}
