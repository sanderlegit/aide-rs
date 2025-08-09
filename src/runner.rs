use crate::error::{Error, Result};
use crate::flows::types::{Flow, History, HistoryMode, VerificationStrategy};
use crate::gemini::GeminiClientWrapper;
use crate::gemini_types::{Content, ContentPart, Role};
use crate::logging::{PromptLog, RunLogger, ToolCallLog, ToolResultLog, ValidationLog};
use crate::prompt::PromptBuilder;
use crate::tools::ToolExecutor;
use crate::vcs;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, info, warn};

struct VerificationResult {
    success: bool,
    output: serde_json::Value,
}

pub struct FlowRunner {
    logger: RunLogger,
    prompt_builder: PromptBuilder,
    // We will store the output of each block here, keyed by block.id.
    block_outputs: HashMap<String, serde_json::Value>,
    // The full conversation history.
    history: Vec<Content>,
    changed_files: HashSet<PathBuf>,
}

impl FlowRunner {
    pub fn new(logger: RunLogger) -> Result<Self> {
        // The Gemini client is now created on-demand in `execute_block`
        // to support per-block/per-flow model configuration.
        Ok(Self {
            logger,
            prompt_builder: PromptBuilder::new(),
            block_outputs: HashMap::new(),
            history: Vec::new(),
            changed_files: HashSet::new(),
        })
    }

    pub fn load_input(&mut self, id: &str, value: serde_json::Value) {
        self.block_outputs.insert(id.to_string(), value);
    }

    pub fn changed_files(&self) -> Vec<PathBuf> {
        self.changed_files.iter().cloned().collect()
    }

    pub async fn run(&mut self, flow: &Flow, prompt_path: &Path) -> Result<()> {
        self.logger.log_summary(&format!(
            "Starting flow '{}' with prompt '{}'...",
            flow.id,
            prompt_path.display()
        ));

        for block in &flow.blocks {
            let final_output = if let Some(looping_strategy) = &block.looping {
                // This block should be run in a loop.
                let list_data =
                    self.block_outputs
                        .get(&looping_strategy.over)
                        .ok_or_else(|| {
                            Error::Config(format!(
                                "Looping error: block output '{}' not found",
                                looping_strategy.over
                            ))
                        })?;

                // The output of `create_task_list` is a `TaskList` struct, which is a JSON object `{"tasks": [...]}`.
                // We need to get the array from the `tasks` field.
                // We clone the items to loop over so we don't hold an immutable borrow on `self`
                // while trying to mutably borrow it in the loop.
                let items_to_loop = list_data
                    .get("tasks")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .ok_or_else(|| {
                        Error::Config(format!(
                            "Looping error: output of block '{}' is not a valid TaskList (missing 'tasks' array)",
                            looping_strategy.over
                        ))
                    })?;

                let mut iteration_outputs = vec![];
                let original_history = if looping_strategy.clear_history_on_iteration {
                    Some(self.history.clone())
                } else {
                    None
                };

                for (i, item) in items_to_loop.iter().enumerate() {
                    self.logger.log_summary(&format!(
                        "Executing looping block '{}' (iteration {}/{})",
                        block.id,
                        i + 1,
                        items_to_loop.len()
                    ));

                    if let Some(history) = &original_history {
                        self.history = history.clone();
                    }

                    let loop_item_override = Some((looping_strategy.as_key.as_str(), item));
                    let iteration_output = self
                        .execute_block(block, flow, prompt_path, loop_item_override)
                        .await?;
                    iteration_outputs.push(iteration_output);

                    if looping_strategy.commit_on_iteration_success {
                        let changed_files = self.changed_files();
                        let task_id =
                            item.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
                        if !changed_files.is_empty() {
                            self.logger.log_summary(&format!(
                                "Loop iteration for task '{}' succeeded, committing {} file(s)...",
                                task_id,
                                changed_files.len()
                            ));
                            let commit_message = format!(
                                "aide-rs: auto-commit after task '{}' in flow '{}'",
                                task_id, flow.id
                            );
                            vcs::add_and_commit(
                                &std::env::current_dir()?,
                                &changed_files,
                                &commit_message,
                            )?;
                            self.logger.log_summary(&format!(
                                "Committed {} file(s).",
                                changed_files.len()
                            ));
                            // After committing, clear the list of changed files for the next iteration.
                            self.changed_files.clear();
                        } else {
                            self.logger.log_summary(&format!(
                                "Loop iteration for task '{}' in block '{}' succeeded, but no files were changed. Skipping commit.",
                                task_id, block.id
                            ));
                        }
                    }
                }

                // The output of the looping block is the list of outputs from each iteration.
                serde_json::to_value(iteration_outputs)?
            } else {
                // This is a standard, non-looping block.
                self.execute_block(block, flow, prompt_path, None).await?
            };

            if let Some(save_path) = &block.annotations.save_output_to {
                let output_json = serde_json::to_string_pretty(&final_output)?;
                if let Some(parent) = Path::new(save_path).parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(save_path, output_json).await?;
                self.logger.log_summary(&format!(
                    "Saved output of block '{}' to '{}'",
                    block.id, save_path
                ));
            }

            self.block_outputs
                .insert(block.id.clone(), final_output.clone());
            info!(block_id = %block.id, output = %serde_json::to_string_pretty(&final_output).unwrap_or_default(), "Stored block output");

            if block.annotations.commit_on_success {
                let changed_files = self.changed_files();
                if !changed_files.is_empty() {
                    self.logger.log_summary(&format!(
                        "Block '{}' succeeded, committing {} changed file(s)...",
                        block.id,
                        changed_files.len()
                    ));
                    let commit_message = format!(
                        "aide-rs: auto-commit after block '{}' in flow '{}'",
                        block.id, flow.id
                    );
                    vcs::add_and_commit(
                        &std::env::current_dir()?,
                        &changed_files,
                        &commit_message,
                    )?;
                    self.logger
                        .log_summary(&format!("Committed {} file(s).", changed_files.len()));
                    // After committing, clear the list of changed files for the next block.
                    self.changed_files.clear();
                } else {
                    self.logger.log_summary(&format!(
                        "Block '{}' succeeded, but no files were changed. Skipping commit.",
                        block.id
                    ));
                }
            }
        }

        self.logger
            .log_summary(&format!("Flow '{}' finished.", flow.id));
        Ok(())
    }

    async fn execute_block(
        &mut self,
        block: &crate::flows::types::Block,
        flow: &Flow,
        prompt_path: &Path,
        loop_item_override: Option<(&str, &serde_json::Value)>,
    ) -> Result<serde_json::Value> {
        if loop_item_override.is_none() {
            self.logger
                .log_summary(&format!("Executing block: '{}'...", block.id));
        }

        let mut block_output = json!(null);
        let max_retries = block.verification.as_ref().map_or(0, |v| v.max_retries);
        let mut verification_output: Option<serde_json::Value> = None;

        for attempt in 0..=max_retries {
            // Determine which model to use for this block execution.
            // Precedence: block-specific model > flow-level model > hardcoded default.
            let model_name = block
                .annotations
                .model
                .as_deref()
                .or(flow.model.as_deref())
                .unwrap_or("gemini-2.5-pro")
                .to_string();
            let gemini_client = GeminiClientWrapper::new(model_name, self.logger.clone())?;

            // 1. Determine which prompt to use
            let prompt_def = if attempt > 0 {
                // This is a retry, use the on_failure_prompt
                block
                    .verification
                    .as_ref()
                    .and_then(|v| match &v.strategy {
                        VerificationStrategy::Command {
                            on_failure_prompt, ..
                        } => Some(on_failure_prompt),
                        VerificationStrategy::Prompt { .. } => None, // Not implemented yet
                    })
                    .ok_or_else(|| {
                        Error::Config(format!(
                            "Block '{}' is in a retry loop but has no on_failure_prompt.",
                            block.id
                        ))
                    })?
            } else {
                // First attempt
                &block.prompt
            };

            // 2. Build the prompt, passing verification output and loop item if they exist
            let mut temp_block_outputs = self.block_outputs.clone();
            if let Some((key, value)) = loop_item_override {
                temp_block_outputs.insert(key.to_string(), (*value).clone());
            }

            let built_prompt = self
                .prompt_builder
                .build(
                    prompt_def,
                    prompt_path,
                    &temp_block_outputs,
                    &block.id,
                    &verification_output,
                )
                .await?;

            debug!(prompt = %built_prompt.full_prompt, "Full prompt for block '{}'", block.id);

            // 3. Execute the block's main logic (API call, tools)
            let tool_executor = ToolExecutor::new(&block.annotations.tools);
            let tool_schemas = tool_executor.schemas();

            // Create a history specific to this attempt. It starts with the global history
            // but is modified locally. It only becomes the new global history on success.
            let mut attempt_history = self.history.clone();
            let user_content = Content {
                role: Role::User,
                parts: vec![ContentPart::new_text(built_prompt.full_prompt.clone())],
            };
            attempt_history.push(user_content);

            let tools_config_for_log = if tool_schemas.is_empty() {
                None
            } else {
                Some(vec![crate::gemini_types::Tool {
                    function_declarations: tool_schemas.clone(),
                }])
            };
            self.logger.log_prompt(&PromptLog {
                model_name: gemini_client.model_name().to_string(),
                system_prompt: "".to_string(), // We are using a user-style prompt for now
                user_prompt: built_prompt.full_prompt,
                display_prompt: Some(built_prompt.display_prompt),
                tools: json!(tools_config_for_log),
            });

            // Start conversation loop for this attempt
            loop {
                let history_for_request = match &block.annotations.history {
                    History::Mode(HistoryMode::Full) => attempt_history.clone(),
                    History::Mode(HistoryMode::None) => {
                        vec![attempt_history.last().cloned().unwrap()]
                    }
                    History::LastN { last_n } => {
                        let n = *last_n as usize;
                        let len = attempt_history.len();
                        if len > n {
                            attempt_history.iter().skip(len - n).cloned().collect()
                        } else {
                            attempt_history.clone()
                        }
                    }
                };

                let tools_config = if tool_schemas.is_empty() {
                    None
                } else {
                    Some(vec![crate::gemini_types::Tool {
                        function_declarations: tool_schemas.clone(),
                    }])
                };

                let response = gemini_client
                    .generate_content(history_for_request, tools_config)
                    .await?;

                let Some(candidate) = response.candidates.and_then(|mut c| c.pop()) else {
                    return Err(Error::ApiError(
                        "No candidates received from Gemini API".to_string(),
                    ));
                };

                attempt_history.push(candidate.content.clone());

                let has_function_call = candidate
                    .content
                    .parts
                    .iter()
                    .any(|p| p.function_call.is_some());

                if !has_function_call {
                    // No tool call, this is the final response for this turn.
                    // Only set the output if it hasn't been set by a tool call already.
                    if block_output == json!(null) {
                        block_output = candidate
                            .content
                            .parts
                            .iter()
                            .find_map(|p| p.text.as_ref())
                            .map(|s| json!(s))
                            .unwrap_or(json!(null));
                    }
                    break; // Exit conversation loop
                }

                // We have at least one function call.
                for part in candidate.content.parts {
                    if let Some(call) = part.function_call {
                        let start_time = Instant::now();
                        let result = tool_executor.execute(&call).await;
                        let time_taken = start_time.elapsed();

                        let (tool_result_log, tool_output) = match result {
                            Ok(output) => (
                                ToolResultLog {
                                    success: true,
                                    stdout: serde_json::to_string_pretty(&output)
                                        .unwrap_or_default(),
                                    stderr: "".to_string(),
                                    output_json: output.clone(),
                                },
                                output,
                            ),
                            Err(e) => {
                                let error_string = e.to_string();
                                (
                                    ToolResultLog {
                                        success: false,
                                        stdout: "".to_string(),
                                        stderr: error_string.clone(),
                                        output_json: json!({ "error": error_string }),
                                    },
                                    json!({ "error": error_string }),
                                )
                            }
                        };

                        self.logger.log_tool_call(ToolCallLog {
                            tool_name: call.name.clone(),
                            tool_args: call.arguments.clone(),
                            result: tool_result_log,
                            time_taken_ms: time_taken.as_millis(),
                        });

                        if (call.name == "create_file" || call.name == "edit_file")
                            && tool_output.get("status").and_then(|s| s.as_str()) == Some("success")
                        {
                            if let Some(path_str) =
                                tool_output.get("path").and_then(|p| p.as_str())
                            {
                                self.changed_files.insert(PathBuf::from(path_str));
                            }
                        }

                        block_output = tool_output.clone();

                        let tool_response_part = ContentPart {
                            function_response: Some(crate::gemini_types::FunctionResponse {
                                name: call.name,
                                response: tool_output,
                            }),
                            ..Default::default()
                        };
                        attempt_history.push(Content {
                            role: Role::Tool,
                            parts: vec![tool_response_part],
                        });
                    }
                }
                // Continue the loop to send the tool response back to the model.
            }

            // 4. Run verification logic
            if let Some(verification) = &block.verification {
                let result = self
                    .run_verification(
                        &verification.strategy,
                        gemini_client.model_name(),
                        prompt_path,
                    )
                    .await?;
                if result.success {
                    // Success, commit the history and break the retry loop
                    self.history = attempt_history;
                    break;
                } else {
                    // Failure, store verification output for the next prompt
                    verification_output = Some(result.output);
                    if attempt == max_retries {
                        return Err(Error::VerificationFailed(format!(
                            "Block '{}' failed verification after {} retries.",
                            block.id, max_retries
                        )));
                    }
                    // Log and continue to next attempt
                    self.logger.log_summary(&format!(
                        "Verification failed for block '{}'. Retrying (attempt {}/{})...",
                        block.id,
                        attempt + 1,
                        max_retries
                    ));
                }
            } else {
                // No verification, so we're done with this block. Commit history.
                self.history = attempt_history;
                break;
            }
        }

        // After the loop (either success or no verification), return the final output.
        Ok(block_output)
    }

    async fn run_verification(
        &mut self,
        strategy: &VerificationStrategy,
        model_name: &str,
        prompt_path: &Path,
    ) -> Result<VerificationResult> {
        match strategy {
            VerificationStrategy::Command {
                command,
                expected_exit_code,
                ..
            } => {
                let start_time = Instant::now();
                let output = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output()
                    .await?;

                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(1);
                let success = exit_code == *expected_exit_code;

                self.logger.log_validation(ValidationLog {
                    command: command.clone(),
                    exit_code,
                    stdout: stdout.clone(),
                    stderr: stderr.clone(),
                    success,
                    time_taken_ms: start_time.elapsed().as_millis(),
                });

                Ok(VerificationResult {
                    success,
                    output: json!({
                        "stdout": stdout,
                        "stderr": stderr,
                        "exit_code": exit_code,
                    }),
                })
            }
            VerificationStrategy::Prompt {
                prompt,
                success_condition,
            } => {
                warn!("Prompt-based verification is experimental.");

                // 1. Build the prompt.
                let prompt_string = self
                    .prompt_builder
                    .build(prompt, prompt_path, &self.block_outputs, "", &None)
                    .await?;

                // 2. Call the LLM with a special tool for verification.
                let mut success = false;
                let mut output = json!({});

                if let Some(tool_name) = success_condition.strip_prefix("function_call:") {
                    let verification_tool = crate::gemini_types::FunctionDeclaration {
                        name: tool_name.to_string(),
                        description: "Call this function if the verification is successful."
                            .to_string(),
                        parameters: serde_json::from_str(r#"{"type": "object", "properties": {}}"#)
                            .unwrap(),
                    };
                    let tools_config = Some(vec![crate::gemini_types::Tool {
                        function_declarations: vec![verification_tool],
                    }]);

                    let user_content = Content {
                        role: Role::User,
                        parts: vec![ContentPart::new_text(prompt_string.full_prompt)],
                    };

                    // Verification prompts are ephemeral and not part of the main history.
                    let gemini_client =
                        GeminiClientWrapper::new(model_name.to_string(), self.logger.clone())?;
                    let response = gemini_client
                        .generate_content(vec![user_content], tools_config)
                        .await?;

                    // 3. Check for success condition.
                    if let Some(candidate) = response.candidates.as_ref().and_then(|c| c.first()) {
                        for part in &candidate.content.parts {
                            if let Some(call) = &part.function_call {
                                if call.name == tool_name {
                                    success = true;
                                    break;
                                }
                            }
                        }
                        output = serde_json::to_value(candidate.content.clone())?;
                    }
                } else {
                    warn!("Unsupported success_condition: {}", success_condition);
                }

                Ok(VerificationResult { success, output })
            }
        }
    }
}
