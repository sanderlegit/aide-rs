use crate::{
    agents::{
        Agent,
        state::{FileScope, ImplementationPlan, PlanPrompt, Task, TaskStatus, ValidationStep},
    },
    error::{Error, Result},
    files,
    gemini::GeminiClientWrapper,
};
use async_trait::async_trait;
use gemini_client_rs::types::{
    Content, ContentPart, DynamicRetrieval, DynamicRetrievalConfig, FunctionCall,
    FunctionDeclaration, FunctionParameters, GenerateContentResponse, PartResponse, Role,
    ToolConfig, ToolConfigFunctionDeclaration,
};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use tracing::info;

pub struct PlanAgent {
    gemini: GeminiClientWrapper,
}

impl PlanAgent {
    pub fn new() -> Result<Self> {
        let gemini = GeminiClientWrapper::new_plan_agent()?;
        Ok(Self { gemini })
    }

    fn create_system_prompt(&self) -> String {
        "You are an expert software architect. Your role is to analyze a user's objective, the provided project context, and create a detailed, step-by-step implementation plan. You must break down the objective into small, verifiable tasks. For each task, you must define the specific files that need to be touched and the commands to validate the changes. You must call the `create_implementation_plan` function with the generated plan.".to_string()
    }

    fn create_user_prompt(&self, prompt: &PlanPrompt, files: &[PathBuf]) -> String {
        let file_list = files
            .iter()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"
Please create an implementation plan for the following objective:

**Objective:**
{objective}

**Project Context (Scoped Files):**
```
{file_list}
```

**Coding Conventions:**
{coding_conventions}

**Validation Commands (to be run after each task):**
{validation_commands}

Generate a detailed implementation plan by calling the `create_implementation_plan` function.
"#,
            objective = prompt.objective,
            file_list = if file_list.is_empty() {
                "No files in scope.".to_string()
            } else {
                file_list
            },
            coding_conventions = prompt.coding_conventions,
            validation_commands = prompt
                .validation_commands
                .iter()
                .map(|v| format!(
                    "- `{}` (expects exit code {})",
                    v.command, v.expected_exit_code
                ))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    fn create_implementation_plan_tool(&self) -> FunctionDeclaration {
        let params_json = json!({
            "type": "OBJECT",
            "properties": {
                "tasks": {
                    "type": "ARRAY",
                    "description": "The list of tasks to be executed.",
                    "items": {
                        "type": "OBJECT",
                        "properties": {
                            "description": {
                                "type": "STRING",
                                "description": "A detailed description of the task."
                            },
                            "file_scoping": {
                                "type": "OBJECT",
                                "description": "The files relevant to this task.",
                                "properties": {
                                    "include": {
                                        "type": "ARRAY",
                                        "description": "Glob patterns for files to include.",
                                        "items": { "type": "STRING" }
                                    },
                                    "exclude": {
                                        "type": "ARRAY",
                                        "description": "Glob patterns for files to exclude.",
                                        "items": { "type": "STRING" }
                                    }
                                },
                                "required": ["include"]
                            },
                            "validation_steps": {
                                "type": "ARRAY",
                                "description": "Commands to validate the task's completion.",
                                "items": {
                                    "type": "OBJECT",
                                    "properties": {
                                        "command": {
                                            "type": "STRING",
                                            "description": "The validation command to run."
                                        },
                                        "expected_exit_code": {
                                            "type": "NUMBER",
                                            "description": "The expected exit code for the command."
                                        }
                                    },
                                    "required": ["command", "expected_exit_code"]
                                }
                            }
                        },
                        "required": ["description", "file_scoping", "validation_steps"]
                    }
                }
            },
            "required": ["tasks"]
        });
        let parameters: FunctionParameters = serde_json::from_value(params_json)
            .expect("Internal error: static schema for create_implementation_plan is invalid");

        FunctionDeclaration {
            name: "create_implementation_plan".to_string(),
            description: "Creates a structured implementation plan with a list of tasks."
                .to_string(),
            parameters,
        }
    }

    fn create_dependency_research_prompt(&self, prompt: &PlanPrompt) -> String {
        format!(
            "Based on the following objective, please use Google Search to find the best and most up-to-date Rust crates (libraries) to accomplish the task. Provide their names and latest versions.

Objective: {}

Focus on libraries that are well-maintained, popular, and fit the requirements. List them clearly.",
            prompt.objective
        )
    }

    fn create_google_search_tool(&self) -> ToolConfig {
        ToolConfig::DynamicRetieval {
            google_search_retrieval: DynamicRetrieval {
                dynamic_retrieval_config: Some(DynamicRetrievalConfig {
                    mode: "MODE_DYNAMIC".to_string(),
                    dynamic_threshold: Some(0.5),
                }),
            },
        }
    }

    fn process_search_response(&self, response: &GenerateContentResponse) -> Result<String> {
        let candidate = response
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .ok_or_else(|| Error::Config("No candidates in search response".to_string()))?;

        if let Some(part) = candidate.content.parts.first() {
            if let PartResponse::Text(text) = part {
                info!(search_result = %text, "Successfully got search result");
                return Ok(text.clone());
            }
        }

        Err(Error::Config(
            "Expected a text part in the search response".to_string(),
        ))
    }

    fn process_response(
        &self,
        response: GenerateContentResponse,
        prompt: PlanPrompt,
    ) -> Result<ImplementationPlan> {
        let candidate = response
            .candidates
            .and_then(|mut c| c.pop())
            .ok_or_else(|| Error::Config("No candidates in response".to_string()))?;

        let part = candidate
            .content
            .parts
            .into_iter()
            .next()
            .ok_or_else(|| Error::Config("No parts in candidate".to_string()))?;

        if let PartResponse::FunctionCall(FunctionCall { name, arguments }) = part {
            if name == "create_implementation_plan" {
                info!(
                    ?arguments,
                    "Received function call to create implementation plan"
                );

                #[derive(Deserialize)]
                struct PlanArgs {
                    tasks: Vec<TaskArgs>,
                }
                #[derive(Deserialize)]
                struct TaskArgs {
                    description: String,
                    file_scoping: FileScope,
                    validation_steps: Vec<ValidationStep>,
                }

                let plan_args: PlanArgs = serde_json::from_value(arguments)?;

                let tasks = plan_args
                    .tasks
                    .into_iter()
                    .map(|task_arg| Task {
                        description: task_arg.description,
                        file_scoping: task_arg.file_scoping,
                        validation_steps: task_arg.validation_steps,
                        status: TaskStatus::Pending,
                        attempts: 0,
                        result: None,
                    })
                    .collect();

                return Ok(ImplementationPlan {
                    original_prompt: prompt,
                    tasks,
                });
            }
        }

        Err(Error::Config(
            "Expected a function call to `create_implementation_plan`".to_string(),
        ))
    }
}

#[async_trait]
impl Agent for PlanAgent {
    type Input = PlanPrompt;
    type Output = ImplementationPlan;

    async fn run(&self, prompt: Self::Input) -> Result<Self::Output> {
        let files = files::get_filtered_files(Path::new("."), &prompt.file_scoping)?;
        info!(?files, "Found files for planning context");

        let mut user_prompt = self.create_user_prompt(&prompt, &files);

        if prompt.use_google_search_for_deps {
            info!("Using Google Search to find up-to-date libraries.");
            let search_prompt = self.create_dependency_research_prompt(&prompt);
            let search_contents = vec![Content {
                role: Role::User,
                parts: vec![ContentPart::Text(search_prompt)],
            }];
            let search_tools = vec![self.create_google_search_tool()];

            let search_response = self
                .gemini
                .generate_content(search_contents, Some(search_tools))
                .await?;

            let search_result_text = self.process_search_response(&search_response)?;
            user_prompt = format!(
                "{}\n\n**Suggested Libraries (from Google Search):**\n{}",
                user_prompt, search_result_text
            );
        }

        let system_prompt = self.create_system_prompt();

        let contents = vec![
            Content {
                role: Role::User,
                parts: vec![ContentPart::Text(system_prompt)],
            },
            Content {
                role: Role::Model,
                parts: vec![ContentPart::Text(
                    "Understood. I am ready to generate a plan. Please provide the details."
                        .to_string(),
                )],
            },
            Content {
                role: Role::User,
                parts: vec![ContentPart::Text(user_prompt)],
            },
        ];

        let tools = vec![self.create_implementation_plan_tool()];
        let tool_config = vec![ToolConfig::FunctionDeclaration(
            ToolConfigFunctionDeclaration {
                function_declarations: tools,
            },
        )];

        let response = self.gemini.generate_content(contents, Some(tool_config)).await?;

        let plan = self.process_response(response, prompt)?;

        Ok(plan)
    }
}
