use crate::{
    agents::{
        state::{FileScope, ImplementationPlan, PlanPrompt, Task, TaskStatus, ValidationStep},
        Agent,
    },
    error::{Error, Result},
    files,
    gemini::GeminiClientWrapper,
    gemini_types::{
        Content, ContentPart, DynamicRetrieval, DynamicRetrievalConfig, FunctionCall,
        GenerateContentResponse, Role, ToolConfig,
    },
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
    file_scoping: FileScope,
    validation_steps: Vec<ValidationStep>,
}

pub struct PlanAgent {
    gemini: GeminiClientWrapper,
}

impl PlanAgent {
    pub fn new() -> Result<Self> {
        let gemini = GeminiClientWrapper::new_plan_agent()?;
        Ok(Self { gemini })
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

Please provide the file scoping and validation steps for this specific task by calling the `create_task_details` function. The validation steps must be a subset of the following available commands:
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
            "description": "Creates the details (file scope, validation) for a single task.",
            "parameters": {
                "type": "OBJECT",
                "properties": {
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
                "required": ["file_scoping", "validation_steps"]
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

    // Google Search related functions
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
                dynamic_retrieval_config: DynamicRetrievalConfig {
                    mode: "MODE_DYNAMIC".to_string(),
                    dynamic_threshold: 0.5,
                },
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
            if let Some(text) = &part.text {
                info!(search_result = %text, "Successfully got search result");
                return Ok(text.clone());
            }
        }

        Err(Error::Config(
            "Expected a text part in the search response".to_string(),
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

        let mut user_prompt_for_descriptions =
            self.create_description_generation_user_prompt(&prompt, &files);

        if prompt.use_google_search_for_deps {
            info!("Using Google Search to find up-to-date libraries.");
            let search_prompt = self.create_dependency_research_prompt(&prompt);
            info!(
                prompt = %format!("\n---\n{}\n---", search_prompt),
                "Sending dependency research prompt to Gemini"
            );
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
            user_prompt_for_descriptions = format!(
                "{}\n\n**Suggested Libraries (from Google Search):**\n{}",
                user_prompt_for_descriptions, search_result_text
            );
        }

        // STEP 1: Get task descriptions
        info!("Step 1: Generating task descriptions...");
        let system_prompt = self.create_description_generation_system_prompt();
        info!(
            prompt = %format!("\n--- SYSTEM ---\n{}\n--- USER ---\n{}\n---", system_prompt, user_prompt_for_descriptions),
            "Sending description generation prompt to Gemini"
        );

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

        let function_declarations = vec![self.create_task_descriptions_tool()];
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
            info!(
                prompt = %format!("\n--- SYSTEM ---\n{}\n--- USER ---\n{}\n---", system_prompt, user_prompt),
                "Sending task detailing prompt to Gemini"
            );

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

            let function_declarations = vec![self.create_task_details_tool()];
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
                file_scoping: details.file_scoping,
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
