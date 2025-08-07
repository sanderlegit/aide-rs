use crate::{
    agents::{
        state::{FileScope, ImplementationPlan, PlanPrompt, Task, TaskStatus, ValidationStep},
        Agent,
    },
    error::{Error, Result},
    files,
    gemini::GeminiClientWrapper,
};
use async_trait::async_trait;
use gemini_client_rs::types::{
    Content, ContentPart, FunctionCall, FunctionDeclaration, GenerateContentResponse, PartResponse,
    Role,
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
            file_list = if file_list.is_empty() { "No files in scope.".to_string() } else { file_list },
            coding_conventions = prompt.coding_conventions,
            validation_commands = prompt
                .validation_commands
                .iter()
                .map(|v| format!("- `{}` (expects exit code {})", v.command, v.expected_exit_code))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    fn create_implementation_plan_tool(&self) -> FunctionDeclaration {
        FunctionDeclaration {
            name: "create_implementation_plan".to_string(),
            description: "Creates a structured implementation plan with a list of tasks.".to_string(),
            parameters: Some(json!({
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
            })),
        }
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

        if let PartResponse::FunctionCall(FunctionCall { name, args }) = part {
            if name == "create_implementation_plan" {
                info!(?args, "Received function call to create implementation plan");

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

                let plan_args: PlanArgs = serde_json::from_value(args)?;

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

        let system_prompt = self.create_system_prompt();
        let user_prompt = self.create_user_prompt(&prompt, &files);

        let contents = vec![
            Content {
                role: Role::User,
                parts: vec![ContentPart::Text(system_prompt)],
            },
            Content {
                role: Role::Model,
                parts: vec![ContentPart::Text("Understood. I am ready to generate a plan. Please provide the details.".to_string())],
            },
            Content {
                role: Role::User,
                parts: vec![ContentPart::Text(user_prompt)],
            },
        ];

        let tools = vec![self.create_implementation_plan_tool()];

        let response = self.gemini.generate_content(contents, Some(tools)).await?;

        let plan = self.process_response(response, prompt)?;

        Ok(plan)
    }
}
