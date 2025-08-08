use crate::error::{Error, Result};
use crate::flows::types::{Flow, History, HistoryMode};
use crate::gemini::GeminiClientWrapper;
use crate::gemini_types::{Content, ContentPart, Role};
use crate::logging::{PromptLog, RunLogger, ToolCallLog, ToolResultLog};
use crate::prompt::PromptBuilder;
use crate::tools::ToolExecutor;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use tracing::info;

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
            self.logger
                .log_summary(&format!("Executing block: '{}'...", block.id));

            // 1. Initialize the tool executor for this block.
            let tool_executor = ToolExecutor::new(&block.annotations.tools);
            let tool_schemas = tool_executor.schemas();

            // 2. Build the prompt.
            let prompt_string = self
                .prompt_builder
                .build(&block.prompt, prompt_path, &self.block_outputs)
                .await?;

            // 3. Add the user prompt to the history.
            let user_content = Content {
                role: Role::User,
                parts: vec![ContentPart::new_text(prompt_string.clone())],
            };
            self.history.push(user_content);

            // 4. Call Gemini API with the prompt and tool schemas.
            let history_for_request = match &block.annotations.history {
                History::Mode(HistoryMode::Full) => self.history.clone(),
                History::Mode(HistoryMode::None) => {
                    vec![self.history.last().cloned().unwrap()]
                }
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
                Some(vec![crate::gemini_types::ToolConfig::FunctionDeclaration(
                    crate::gemini_types::ToolConfigFunctionDeclaration {
                        function_declarations: tool_schemas,
                    },
                )])
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

            // 5. Handle response, execute tools if necessary.
            if let Some(candidate) = response.candidates.and_then(|mut c| c.pop()) {
                self.history.push(candidate.content.clone());
                let mut block_output = json!(null);

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

                        // Add tool response to history for next turn
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
                // 6. Store block output in self.block_outputs.
                self.block_outputs
                    .insert(block.id.clone(), block_output.clone());
                info!(block_id = %block.id, output = %serde_json::to_string_pretty(&block_output).unwrap_or_default(), "Stored block output");
            } else {
                return Err(Error::ApiError(
                    "No candidates received from Gemini API".to_string(),
                ));
            }

            // 7. TODO: Run verification logic.
        }

        self.logger
            .log_summary(&format!("Flow '{}' finished.", flow.id));
        Ok(())
    }
}
