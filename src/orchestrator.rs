use crate::agents::aider::AiderWrapper;
use crate::error::{Error, Result};
use crate::file_provider;
use crate::gemini::GeminiClientWrapper;
use crate::logging::RunLogger;
use crate::session::Session;
use crate::tools::ToolExecutor;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Instant;
use tokio::process::Command;
use tracing::{error, info};

struct CommandResult {
    success: bool,
    exit_code: i32,
    stdout: String,
    stderr: String,
}

async fn run_shell_command(command_str: &str) -> CommandResult {
    let mut parts = command_str.split_whitespace();
    let command = parts.next().unwrap_or("");
    let args: Vec<&str> = parts.collect();

    if command.is_empty() {
        return CommandResult {
            success: false,
            exit_code: -1,
            stdout: String::new(),
            stderr: "No command provided.".to_string(),
        };
    }

    let output = Command::new(command).args(&args).output().await;

    match output {
        Ok(out) => CommandResult {
            success: out.status.success(),
            exit_code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        },
        Err(e) => CommandResult {
            success: false,
            exit_code: -1,
            stdout: String::new(),
            stderr: e.to_string(),
        },
    }
}

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
        #[serde(default)]
        allow_shell_commands: bool,
        #[serde(default)]
        pre_validate: bool,
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
        let default_model = "gemini-2.5-pro".to_string();
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

    #[tracing::instrument(skip(self, objective, files))]
    pub async fn research(
        &self,
        objective: String,
        files: Vec<String>,
        interactive: bool,
        output_path: Option<String>,
        model_override: Option<&str>,
    ) -> Result<PathBuf> {
        self.logger.log_summary("Starting research strategy.");
        let session = Session::new("research", &objective)?;

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
            google_search: Some(crate::gemini_types::GoogleSearch::default()),
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

        // Always save to session cache for posterity
        let session_cache_path = session.dir.join("research.md");
        std::fs::write(&session_cache_path, &research_text)?;

        // Determine the user-visible path. Default to `research/research.md` if not specified.
        let user_visible_path = if let Some(path_str) = output_path {
            PathBuf::from(path_str)
        } else {
            PathBuf::from("research/research.md")
        };

        if let Some(parent) = user_visible_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&user_visible_path, &research_text)?;

        self.logger.log_summary(&format!(
            "Research summary saved to {}",
            user_visible_path.display()
        ));

        if interactive {
            // Optional: launch aider to refine
            info!("Launching aider to review and refine the research document.");
            self.aider
                .run(
                    &session,
                    vec![user_visible_path.to_str().unwrap().to_string()],
                    "Here is the research document I generated. Please review it.",
                    false,
                    None,
                    true,
                )
                .await?;
        }

        Ok(user_visible_path)
    }

    #[tracing::instrument(skip(self, objective, files, research_context))]
    pub async fn plan(
        &self,
        objective: String,
        files: Vec<String>,
        interactive: bool,
        research_context: Option<String>,
        model_override: Option<&str>,
    ) -> Result<PathBuf> {
        self.logger.log_summary("Starting plan strategy.");
        let session = Session::new("plan", &objective)?;

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

        // Always save to session cache for posterity
        let session_cache_path = session.dir.join("plan.md");
        std::fs::write(&session_cache_path, &plan_text)?;

        // Save to a user-visible path so aider can find it in its context.
        let user_visible_path = PathBuf::from("plans/plan.md");
        if let Some(parent) = user_visible_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&user_visible_path, &plan_text)?;

        self.logger
            .log_summary(&format!("Plan saved to {}", user_visible_path.display()));

        if interactive {
            let mut files_for_aider = files;
            files_for_aider.push(user_visible_path.to_str().unwrap().to_string());

            info!("Launching aider to review and refine the plan.");
            self.aider
                .run(
                    &session,
                    files_for_aider,
                    &format!(
                        "Here is the plan I generated, stored in `{}`. Please review it and help me refine it.",
                        user_visible_path.display()
                    ),
                    false,
                    None,
                    true,
                )
                .await?;
        }

        Ok(user_visible_path)
    }

    /// Executes the implementation strategy.
    ///
    /// In automated mode (`auto = true`), this function orchestrates `aider` in a
    /// validation-driven loop. The process is as follows:
    ///
    /// 1.  `aide-rs` invokes `aider` with the provided objective.
    /// 2.  `aider` attempts to modify the code and commits its changes.
    /// 3.  After `aider` finishes, `aide-rs` runs the `validate_cmd`.
    /// 4.  **If `validate_cmd` succeeds**: `aide-rs` considers the step successful.
    ///     Depending on the `continue_on_success` flag, it will either terminate
    ///     the loop or continue with a new prompt to implement the next part of a plan.
    /// 5.  **If `validate_cmd` fails**: `aide-rs` reverts `aider`'s commit,
    ///     captures the error output, uses the Gemini model to analyze the error,
    ///     and fetches relevant documentation via the `doc_retriever` tool.
    /// 6.  The loop repeats with a new, context-enriched prompt for `aider`,
    //      containing the error and the retrieved documentation, until the
    //      `max_retries` limit is reached.
    #[tracing::instrument(skip(self, objective, files))]
    pub async fn implement(
        &self,
        objective: String,
        files: Vec<String>,
        validate_cmd: String,
        auto: bool,
        max_retries: u32,
        model_override: Option<&str>,
        allow_shell_commands: bool,
        continue_on_success: bool,
        pre_validate: bool,
    ) -> Result<()> {
        self.logger.log_summary("Starting implement strategy.");
        let session = Session::new("implement", &objective)?;

        let mut current_objective = format!(
            "Hello, can you help me with my implementation? I need to do the following:
            '{}'

            Please use the above information to get started. After you commit your changes, I will run `{}` to validate them.
            ",
            objective, validate_cmd
        );

        if !auto {
            self.aider
                .run(
                    &session,
                    files,
                    &current_objective,
                    false,
                    None,
                    allow_shell_commands,
                )
                .await?;
            self.logger
                .log_summary("Implement strategy completed (interactive).");
            return Ok(());
        }

        if pre_validate {
            info!(command = %validate_cmd, "Running pre-validation command.");
            let validation_start_time = Instant::now();
            let validation_result = run_shell_command(&validate_cmd).await;
            let validation_time_taken = validation_start_time.elapsed();

            self.logger.log_validation(crate::logging::ValidationLog {
                command: validate_cmd.clone(),
                exit_code: validation_result.exit_code,
                stdout: validation_result.stdout.clone(),
                stderr: validation_result.stderr.clone(),
                success: validation_result.success,
                time_taken_ms: validation_time_taken.as_millis(),
            });

            if !validation_result.success {
                return Err(Error::VerificationFailed(
                    "Pre-validation command failed. Please fix your tests before running with --pre-validate.".to_string(),
                ));
            }
            self.logger.log_summary("Pre-validation passed.");
        }

        // Automated loop
        for i in 0..max_retries {
            info!(attempt = i + 1, max_attempts = max_retries, "Running aider in auto mode.");

            // 1. Run aider to make changes
            let aider_start_time = Instant::now();
            let aider_result = self
                .aider
                .run(
                    &session,
                    files.clone(),
                    &current_objective,
                    true,
                    None, // We run validation ourselves
                    allow_shell_commands,
                )
                .await?;
            let aider_time_taken = aider_start_time.elapsed();

            self.logger.log_aider_run(crate::logging::AiderLog {
                success: aider_result.success,
                stdout: aider_result.stdout.clone(),
                stderr: aider_result.stderr.clone(),
                time_taken_ms: aider_time_taken.as_millis(),
            });

            if !aider_result.success {
                return Err(Error::ToolFailed(format!(
                    "Aider itself failed to run. Stderr: {}",
                    aider_result.stderr
                )));
            }

            // 2. Run validation command
            info!(command = %validate_cmd, "Running validation command.");
            let validation_start_time = Instant::now();
            let validation_result = run_shell_command(&validate_cmd).await;
            let validation_time_taken = validation_start_time.elapsed();

            self.logger.log_validation(crate::logging::ValidationLog {
                command: validate_cmd.clone(),
                exit_code: validation_result.exit_code,
                stdout: validation_result.stdout.clone(),
                stderr: validation_result.stderr.clone(),
                success: validation_result.success,
                time_taken_ms: validation_time_taken.as_millis(),
            });

            if validation_result.success {
                if aider_result.stdout.contains("No changes were applied.") {
                    self.logger.log_summary(&format!(
                        "Validation passed and Aider reported no changes on attempt {}/{}. Assuming completion.",
                        i + 1,
                        max_retries
                    ));
                    self.logger
                        .log_summary("Implement strategy completed successfully.");
                    return Ok(());
                }

                if !continue_on_success {
                    self.logger.log_summary(&format!(
                        "Validation passed on attempt {}/{}.",
                        i + 1,
                        max_retries
                    ));
                    return Ok(());
                }

                self.logger.log_summary(&format!(
                    "Validation passed on attempt {}/{}. Continuing with plan.",
                    i + 1,
                    max_retries
                ));
                current_objective =
                    "The previous changes were successful. Please continue implementing the plan."
                        .to_string();
                continue;
            }

            // 3. Validation failed, enter debug loop
            self.logger.log_summary(&format!(
                "Validation failed on attempt {}/{}. Analyzing failure...",
                i + 1,
                max_retries
            ));

            // The user has opted to not revert failed commits automatically.
            // The failed state will be part of the git history, and subsequent
            // attempts will build on it.

            let debug_prompt = format!(
                "The last attempt to fix the code failed. I need your help to figure out what to do next.
                Based on the error output below, what documentation should I look up using the `doc_retriever` tool?
                Please call the tool with the most relevant `crate_name` and `path` to get documentation that might help solve the error.

                If the tool fails to find the exact path, it will return documentation for the parent module or the whole crate, which should still be helpful. If you are unsure of the exact path, providing a guess is better than not calling the tool at all.

                Validation command STDOUT:
                {}

                Validation command STDERR:
                {}",
                validation_result.stdout, validation_result.stderr
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

            let function_call = response
                .candidates
                .as_ref()
                .and_then(|c| c.first())
                .and_then(|c| {
                    c.content
                        .parts
                        .iter()
                        .find_map(|p| p.function_call.as_ref())
                });

            if let Some(call) = function_call {
                info!(call = ?call, "Gemini requested a tool call for debugging");
                let tool_execution_result = self.tool_executor.execute(call).await;

                match tool_execution_result {
                    Ok(docs) => {
                        retrieved_docs = serde_json::to_string_pretty(&docs)
                            .unwrap_or_else(|_| "Failed to format documentation.".to_string());
                        info!(docs = %retrieved_docs, "Retrieved documentation");
                    }
                    Err(e) => {
                        error!(error = %e, "Tool execution failed on first attempt. Retrying.");

                        // Build a history for the retry prompt
                        let mut history = contents; // This is the `Vec<Content>` from the first call
                                                    // Add model's response (the tool call)
                        history.push(crate::gemini_types::Content {
                            parts: vec![crate::gemini_types::ContentPart {
                                function_call: Some(call.clone()),
                                ..Default::default()
                            }],
                            role: crate::gemini_types::Role::Model,
                        });

                        // Add tool's error response
                        let error_response = serde_json::json!({
                            "error": e.to_string(),
                            "message": "Tool execution failed."
                        });
                        history.push(crate::gemini_types::Content {
                            parts: vec![crate::gemini_types::ContentPart {
                                function_response: Some(crate::gemini_types::FunctionResponse {
                                    name: call.name.clone(),
                                    response: error_response,
                                }),
                                ..Default::default()
                            }],
                            role: crate::gemini_types::Role::Tool,
                        });

                        // Add new user prompt for retry
                        history.push(crate::gemini_types::Content {
                            parts: vec![crate::gemini_types::ContentPart::new_text(
                                "The tool call failed. Please analyze the error and try again. You may need to correct the `path` or `crate_name`.".to_string()
                            )],
                            role: crate::gemini_types::Role::User,
                        });

                        let retry_response = self
                            .gemini
                            .generate_content(history, tools.clone(), model_override)
                            .await?;

                        let retry_function_call = retry_response
                            .candidates
                            .as_ref()
                            .and_then(|c| c.first())
                            .and_then(|c| {
                                c.content
                                    .parts
                                    .iter()
                                    .find_map(|p| p.function_call.as_ref())
                            });

                        if let Some(retry_call) = retry_function_call {
                            info!(call = ?retry_call, "Gemini requested a tool call for retry");
                            match self.tool_executor.execute(retry_call).await {
                                Ok(docs) => {
                                    retrieved_docs = serde_json::to_string_pretty(&docs)
                                        .unwrap_or_else(|_| {
                                            "Failed to format documentation.".to_string()
                                        });
                                    info!(docs = %retrieved_docs, "Retrieved documentation on retry");
                                }
                                Err(e2) => {
                                    error!(error = %e2, "Tool execution failed on retry.");
                                    retrieved_docs = format!(
                                        "Failed to retrieve documentation on retry: {}",
                                        e2
                                    );
                                }
                            }
                        } else {
                            info!("Gemini did not request a tool call on retry.");
                            retrieved_docs =
                                "Gemini did not provide a new tool call on retry.".to_string();
                        }
                    }
                }
            } else {
                info!("Gemini did not request a tool call for debugging.");
            }

            current_objective = format!(
                "The last attempt failed. Here is the output from the validation command:\n\nSTDOUT:\n{}\n\nSTDERR:\n{}\n\nI tried to find relevant documentation and got this:\n\n{}\n\nPlease use this information to fix the code.",
                validation_result.stdout, validation_result.stderr, retrieved_docs
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

        let mut research_file_path: Option<PathBuf> = None;
        let total_steps = config.steps.len();
        let mut step_number = 1;

        for step in config.steps {
            match step {
                StepConfig::Research {
                    objective,
                    context,
                    model,
                    output,
                    files: extra_files,
                } => {
                    self.logger.log_summary(&format!(
                        "\n--- Starting Step {}/{}: Research ---",
                        step_number, total_steps
                    ));
                    info!("Running research step.");
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
                    self.logger.log_summary(&format!(
                        "--- Completed Step {}/{}: Research ---\n",
                        step_number, total_steps
                    ));
                }
                StepConfig::Plan {
                    objective,
                    context,
                    model,
                } => {
                    self.logger.log_summary(&format!(
                        "\n--- Starting Step {}/{}: Plan ---",
                        step_number, total_steps
                    ));
                    info!("Running plan step.");
                    let files =
                        file_provider::get_files(&[".".to_string()], Some(&context), None)?;
                    let research_content = if let Some(path) = &research_file_path {
                        Some(std::fs::read_to_string(path)?)
                    } else {
                        None
                    };
                    let _ = self
                        .plan(objective, files, false, research_content, model.as_deref())
                        .await?;
                    self.logger.log_summary(&format!(
                        "--- Completed Step {}/{}: Plan ---\n",
                        step_number, total_steps
                    ));
                }
                StepConfig::Implement {
                    objective,
                    context,
                    validate_cmd,
                    max_retries,
                    model,
                    allow_shell_commands,
                    pre_validate,
                } => {
                    self.logger.log_summary(&format!(
                        "\n--- Starting Step {}/{}: Implement ---",
                        step_number, total_steps
                    ));
                    info!("Running implement step.");
                    let files =
                        file_provider::get_files(&[".".to_string()], Some(&context), None)?;
                    let implement_objective = objective.clone();

                    // The plan and research files are now in the workspace, so we don't need
                    // to inject them into the prompt or file list. Aider will see them
                    // as part of its context.

                    self.implement(
                        implement_objective,
                        files,
                        validate_cmd,
                        true,
                        max_retries.unwrap_or(5),
                        model.as_deref(),
                        allow_shell_commands,
                        true, // Continue on success when called from `run`
                        pre_validate,
                    )
                    .await?;
                    self.logger.log_summary(&format!(
                        "--- Completed Step {}/{}: Implement ---\n",
                        step_number, total_steps
                    ));
                }
            }
            step_number += 1;
        }

        self.logger
            .log_summary(&format!("Run from {} completed.", prompt_file));

        Ok(())
    }
}
