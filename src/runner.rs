use crate::error::{Error, Result};
use crate::flows::types::{Flow, History, HistoryMode, VerificationStrategy};
use crate::gemini::GeminiClientWrapper;
use crate::gemini_types::{Content, ContentPart, Role};
use crate::logging::{PromptLog, RunLogger, ToolCallLog, ToolResultLog, ValidationLog};
use crate::prompt::PromptBuilder;
use crate::tools::ToolExecutor;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use tracing::{info, warn};

struct VerificationResult {
    success: bool,
    output: serde_json::Value,
}

pub struct FlowRunner {
    logger: RunLogger,
    prompt_builder: PromptBuilder,
    gemini_client: GeminiClientWrapper,
    // We will store the output of each block here, keyed by block.id.
    block_outputs: HashMap<String, serde_json::Value>,
    // The full conversation history.
    history: Vec<Content>,
}

impl FlowRunner {
    pub fn new(logger: RunLogger) -> Result<Self> {
        // TODO: The model should probably be configurable per-flow or per-block
        let gemini_client =
            GeminiClientWrapper::new("gemini-1.5-flash-latest".to_string(), logger.clone())?;
        Ok(Self {
            logger,
            prompt_builder: PromptBuilder::new(),
            gemini_client,
            block_outputs: HashMap::new(),
            history: Vec::new(),
        })
    }

    pub async fn run(&mut self, flow: &Flow, prompt_path: &Path) -> Result<()> {
        self.logger.log_summary(&format!(
            "Starting flow '{}' with prompt '{}'...",
            flow.id,
            prompt_path.display()
        ));

        for block in &flow.blocks {
            if let Some(looping_strategy) = &block.looping {
                // This block should be run in a loop.
                let list_data = self.block_outputs.get(&looping_strategy.over).ok_or_else(|| {
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
                        .execute_block(block, prompt_path, loop_item_override)
                        .await?;
                    iteration_outputs.push(iteration_output);
                }

                // The output of the looping block is the list of outputs from each iteration.
                let final_output = serde_json::to_value(iteration_outputs)?;
                self.block_outputs.insert(block.id.clone(), final_output);
            } else {
                // This is a standard, non-looping block.
                let block_output = self.execute_block(block, prompt_path, None).await?;
                self.block_outputs
                    .insert(block.id.clone(), block_output.clone());
                info!(block_id = %block.id, output = %serde_json::to_string_pretty(&block_output).unwrap_or_default(), "Stored block output");
            }
        }

        self.logger
            .log_summary(&format!("Flow '{}' finished.", flow.id));
        Ok(())
    }

    async fn execute_block(
        &mut self,
        block: &crate::flows::types::Block,
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
            // 1. Determine which prompt to use
            let prompt_def = if attempt > 0 {
                // This is a retry, use the on_failure_prompt
                block.verification.as_ref().and_then(|v| match &v.strategy {
                        VerificationStrategy::Command { on_failure_prompt, .. } => Some(on_failure_prompt),
                        VerificationStrategy::Prompt { .. } => None, // Not implemented yet
                    }).ok_or_else(|| Error::Config(format!("Block '{}' is in a retry loop but has no on_failure_prompt.", block.id)))?
            } else {
                // First attempt
                &block.prompt
            };

            // 2. Build the prompt, passing verification output and loop item if they exist
            let mut temp_block_outputs = self.block_outputs.clone();
            if let Some((key, value)) = loop_item_override {
                temp_block_outputs.insert(key.to_string(), (*value).clone());
            }

            let prompt_string = self
                .prompt_builder
                .build(
                    prompt_def,
                    prompt_path,
                    &temp_block_outputs,
                    &block.id,
                    &verification_output,
                )
                .await?;

            // 3. Execute the block's main logic (API call, tools)
            let tool_executor = ToolExecutor::new(&block.annotations.tools);
            let tool_schemas = tool_executor.schemas();

            let user_content = Content {
                role: Role::User,
                parts: vec![ContentPart::new_text(prompt_string.clone())],
            };
            self.history.push(user_content);

            let history_for_request = match &block.annotations.history {
                History::Mode(HistoryMode::Full) => self.history.clone(),
                History::Mode(HistoryMode::None) => vec![self.history.last().cloned().unwrap()],
                History::LastN { last_n } => {
                    let n = *last_n as usize;
                    let len = self.history.len();
                    if len > n {
                        self.history.iter().skip(len - n).cloned().collect()
                    } else {
                        self.history.clone()
                    }
                }
            };

            let tools_config = if tool_schemas.is_empty() {
                None
            } else {
                Some(vec![
                    crate::gemini_types::ToolConfig::FunctionDeclaration(
                        crate::gemini_types::ToolConfigFunctionDeclaration {
                            function_declarations: tool_schemas,
                        },
                    ),
                ])
            };

            self.logger.log_prompt(PromptLog {
                model_name: self.gemini_client.model_name().to_string(),
                system_prompt: "".to_string(), // We are using a user-style prompt for now
                user_prompt: prompt_string,
                tools: json!(tools_config),
            });

            let response = self
                .gemini_client
                .generate_content(history_for_request, tools_config)
                .await?;

            if let Some(candidate) = response.candidates.and_then(|mut c| c.pop()) {
                self.history.push(candidate.content.clone());
                block_output = json!(null); // Reset block output for this attempt

                for part in candidate.content.parts {
                    if let Some(text) = part.text {
                        info!(%text, "Got text response from model");
                        block_output = json!(text);
                    }
                    if let Some(call) = part.function_call {
                        info!(tool_call = %call.name, "Got function call from model");
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

                        block_output = tool_output.clone();

                        let tool_response_part = ContentPart {
                            function_response: Some(crate::gemini_types::FunctionResponse {
                                name: call.name,
                                response: crate::gemini_types::FunctionResponsePayload {
                                    content: tool_output,
                                },
                            }),
                            ..Default::default()
                        };
                        self.history.push(Content {
                            role: Role::Tool,
                            parts: vec![tool_response_part],
                        });
                    }
                }
            } else {
                return Err(Error::ApiError(
                    "No candidates received from Gemini API".to_string(),
                ));
            }

            // 4. Run verification logic
            if let Some(verification) = &block.verification {
                let result = self.run_verification(&verification.strategy).await?;
                if result.success {
                    // Success, break the retry loop
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
                // No verification, so we're done with this block.
                break;
            }
        }

        // After the loop (either success or no verification), return the final output.
        Ok(block_output)
    }

    async fn run_verification(
        &mut self,
        strategy: &VerificationStrategy,
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
            VerificationStrategy::Prompt { .. } => {
                warn!("Prompt-based verification is not yet implemented.");
                // For now, we'll just assume it passes.
                Ok(VerificationResult {
                    success: true,
                    output: json!({}),
                })
            }
        }
    }
}
