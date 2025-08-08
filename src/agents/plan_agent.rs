use crate::{
    agents::{
        state::{ImplementationPlan, PlanPrompt, Task, TaskStatus, ValidationStep},
        Agent,
    },
    error::{Error, Result},
    files,
    gemini::GeminiClientWrapper,
    gemini_types::{Content, ContentPart, FunctionCall, GenerateContentResponse, Role},
    logging::{PromptLog, RunLogger},
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Deserialize)]
struct TaskDescriptions {
    tasks: Vec<String>,
}

#[derive(Deserialize)]
struct TaskDetails {
    validation_steps: Vec<ValidationStep>,
}

pub struct PlanAgent {
    gemini: GeminiClientWrapper,
    logger: RunLogger,
}

impl PlanAgent {
    pub fn new(logger: RunLogger) -> Result<Self> {
        let gemini = GeminiClientWrapper::new_plan_agent(logger.clone())?;
        Ok(Self { gemini, logger })
    }

    // STEP 1: Get task descriptions
    fn create_description_generation_system_prompt(&self) -> String {
        "You are an expert software architect. Your role is to analyze a user's objective and the provided project context, and break it down into a high-level list of task descriptions. Focus on the logical steps and do not include implementation details like file paths or validation commands yet. You must call the `create_task_descriptions` function with the generated list of descriptions.".to_string()
    }

    fn create_description_generation_user_prompt(
        &self,
        prompt: &PlanPrompt,
        files: &[PathBuf],
    ) -> String {
        let file_list = files
            .iter()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"
Please create a list of high-level task descriptions for the following objective:

**Objective:**
{objective}

**Project Context (Scoped Files):**
```
{file_list}
```

**Coding Conventions:**
{coding_conventions}

Generate a list of task descriptions by calling the `create_task_descriptions` function.
"#,
            objective = prompt.objective,
            file_list = if file_list.is_empty() {
                "No files in scope.".to_string()
            } else {
                file_list
            },
            coding_conventions = prompt.coding_conventions,
        )
    }

    fn create_task_descriptions_tool(&self) -> serde_json::Value {
        json!({
            "name": "create_task_descriptions",
            "description": "Creates a list of high-level task descriptions for an implementation plan.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
                    "tasks": {
                        "type": "ARRAY",
                        "description": "The list of task descriptions.",
                        "items": { "type": "STRING" }
                    }
                },
                "required": ["tasks"]
            }
        })
    }

    fn process_description_response(
        &self,
        response: GenerateContentResponse,
    ) -> Result<Vec<String>> {
        let candidate = response
            .candidates
            .and_then(|mut c| c.pop())
            .ok_or_else(|| Error::Config("No candidates in description response".to_string()))?;

        let part = candidate.content.parts.into_iter().next().ok_or_else(|| {
            Error::Config("No parts in candidate for description response".to_string())
        })?;

        if let Some(FunctionCall { name, arguments }) = part.function_call {
            if name == "create_task_descriptions" {
                info!(
                    ?arguments,
                    "Received function call to create task descriptions"
                );
                let args: TaskDescriptions = serde_json::from_value(arguments)?;
                return Ok(args.tasks);
            }
        }

        Err(Error::Config(
            "Expected a function call to `create_task_descriptions`".to_string(),
        ))
    }

    // STEP 2: Detail each task
    fn create_task_detailing_system_prompt(&self) -> String {
        "You are an expert software architect. Your role is to detail a single task for an implementation plan. Given the overall objective, project context, and a specific task description, you must determine which files are relevant (`file_scoping`) and what commands are needed to validate the task (`validation_steps`). You must call the `create_task_details` function with this information.".to_string()
    }

    fn create_task_detailing_user_prompt(
        &self,
        original_prompt: &PlanPrompt,
        files: &[PathBuf],
        task_description: &str,
    ) -> String {
        let file_list = files
            .iter()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");

        let validation_commands_list = original_prompt
            .validation_commands
            .iter()
            .map(|v| format!("- `{}`", v.command))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"
**Overall Objective:**
{objective}

**Project Context (Scoped Files):**
```
{file_list}
```

**Coding Conventions:**
{coding_conventions}

**Task to Detail:**
{task_description}

Please provide the validation steps for this specific task by calling the `create_task_details` function. The validation steps must be a subset of the following available commands:
{validation_commands_list}
"#,
            objective = original_prompt.objective,
            file_list = if file_list.is_empty() {
                "No files in scope.".to_string()
            } else {
                file_list
            },
            coding_conventions = original_prompt.coding_conventions,
            task_description = task_description,
            validation_commands_list = validation_commands_list,
        )
    }

    fn create_task_details_tool(&self) -> serde_json::Value {
        json!({
            "name": "create_task_details",
            "description": "Creates the details (validation steps) for a single task.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
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
                "required": ["validation_steps"]
            }
        })
    }

    fn process_detailing_response(&self, response: GenerateContentResponse) -> Result<TaskDetails> {
        let candidate = response
            .candidates
            .and_then(|mut c| c.pop())
            .ok_or_else(|| Error::Config("No candidates in detailing response".to_string()))?;

        let part = candidate
            .content
            .parts
            .into_iter()
            .next()
            .ok_or_else(|| Error::Config("No parts in candidate for detailing response".to_string()))?;

        if let Some(FunctionCall { name, arguments }) = part.function_call {
            if name == "create_task_details" {
                info!(
                    ?arguments,
                    "Received function call to create task details"
                );
                let args: TaskDetails = serde_json::from_value(arguments)?;
                return Ok(args);
            }
        }

        Err(Error::Config(
            "Expected a function call to `create_task_details`".to_string(),
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

        let user_prompt_for_descriptions =
            self.create_description_generation_user_prompt(&prompt, &files);

        // STEP 1: Get task descriptions
        info!("Step 1: Generating task descriptions...");
        let system_prompt = self.create_description_generation_system_prompt();
        let function_declarations = vec![self.create_task_descriptions_tool()];

        self.logger.log_prompt(PromptLog {
            agent_type: "PlanAgent (Descriptions)".to_string(),
            system_prompt: system_prompt.clone(),
            user_prompt: user_prompt_for_descriptions.clone(),
            tools: json!(function_declarations),
        });

        let contents = vec![
            Content {
                role: Role::User,
                parts: vec![ContentPart::Text(system_prompt)],
            },
            Content {
                role: Role::Model,
                parts: vec![ContentPart::Text(
                    "Understood. I am ready to generate task descriptions.".to_string(),
                )],
            },
            Content {
                role: Role::User,
                parts: vec![ContentPart::Text(user_prompt_for_descriptions)],
            },
        ];

        let tool_config = json!([{
            "functionDeclarations": function_declarations
        }]);

        let response = self
            .gemini
            .generate_content(contents, Some(tool_config))
            .await?;

        let task_descriptions = self.process_description_response(response)?;
        info!(?task_descriptions, "Generated task descriptions");

        // STEP 2: Detail each task
        info!("Step 2: Detailing each task...");
        let mut tasks = Vec::new();
        for description in &task_descriptions {
            info!(task = %description, "Detailing task");

            let system_prompt = self.create_task_detailing_system_prompt();
            let user_prompt =
                self.create_task_detailing_user_prompt(&prompt, &files, description);
            let function_declarations = vec![self.create_task_details_tool()];

            self.logger.log_prompt(PromptLog {
                agent_type: "PlanAgent (Details)".to_string(),
                system_prompt: system_prompt.clone(),
                user_prompt: user_prompt.clone(),
                tools: json!(function_declarations),
            });

            let contents = vec![
                Content {
                    role: Role::User,
                    parts: vec![ContentPart::Text(system_prompt)],
                },
                Content {
                    role: Role::Model,
                    parts: vec![ContentPart::Text(
                        "Understood. I am ready to detail the task.".to_string(),
                    )],
                },
                Content {
                    role: Role::User,
                    parts: vec![ContentPart::Text(user_prompt)],
                },
            ];

            let tool_config = json!([{
                "functionDeclarations": function_declarations
            }]);

            let response = self
                .gemini
                .generate_content(contents, Some(tool_config))
                .await?;

            let details = self.process_detailing_response(response)?;

            tasks.push(Task {
                description: description.clone(),
                validation_steps: details.validation_steps,
                status: TaskStatus::Pending,
                attempts: 0,
                result: None,
            });
        }

        // STEP 3: Assemble and return plan
        Ok(ImplementationPlan {
            original_prompt: prompt,
            tasks,
        })
    }
}
