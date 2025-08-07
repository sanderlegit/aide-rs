use crate::{
    agents::{
        state::{ImplementationPlan, Task, TaskResult, TaskStatus},
        Agent,
    },
    error::{Error, Result},
    files,
    gemini::GeminiClientWrapper,
    vcs,
};
use async_trait::async_trait;
use gemini_client_rs::types::{
    Content, ContentPart, FunctionCall, FunctionDeclaration, FunctionParameters,
    GenerateContentResponse, PartResponse, Role,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};
use tracing::{error, info, warn};

pub struct ImplAgent {
    gemini: GeminiClientWrapper,
    max_retries: u32,
    auto_commit: bool,
}

fn run_command(command_str: &str) -> Result<(i32, String)> {
    info!(command = command_str, "Running command");
    let parts: Vec<&str> = command_str.split_whitespace().collect();
    if parts.is_empty() {
        return Err(Error::Config("Empty command".to_string()));
    }

    let output = Command::new(parts[0]).args(&parts[1..]).output()?;

    let exit_code = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let full_output = format!("STDOUT:\n{}\nSTDERR:\n{}", stdout, stderr);

    Ok((exit_code, full_output))
}

impl ImplAgent {
    pub fn new(max_retries: u32, auto_commit: bool) -> Result<Self> {
        let gemini = GeminiClientWrapper::new_impl_agent()?;
        Ok(Self {
            gemini,
            max_retries,
            auto_commit,
        })
    }

    fn create_file_tools(&self) -> Vec<FunctionDeclaration> {
        vec![
            FunctionDeclaration {
                name: "edit_file".to_string(),
                description: "Edits an existing file with new content. Overwrites the entire file."
                    .to_string(),
                parameters: serde_json::from_value(json!({
                    "type": "OBJECT",
                    "properties": {
                        "path": {
                            "type": "STRING",
                            "description": "The relative path to the file to be edited."
                        },
                        "new_content": {
                            "type": "STRING",
                            "description": "The complete new content of the file."
                        }
                    },
                    "required": ["path", "new_content"]
                }))
                .expect("Static schema for edit_file is invalid"),
            },
            FunctionDeclaration {
                name: "create_file".to_string(),
                description: "Creates a new file with specified content.".to_string(),
                parameters: serde_json::from_value(json!({
                    "type": "OBJECT",
                    "properties": {
                        "path": {
                            "type": "STRING",
                            "description": "The relative path for the new file."
                        },
                        "content": {
                            "type": "STRING",
                            "description": "The initial content of the new file."
                        }
                    },
                    "required": ["path", "content"]
                }))
                .expect("Static schema for create_file is invalid"),
            },
        ]
    }

    fn create_system_prompt(&self) -> String {
        "You are an expert pair programmer. Implement the user's request by calling the provided file manipulation functions. Adhere strictly to the coding conventions provided. After your final edit, run the formatter if one is specified. Finally, explain the problem and your solution.".to_string()
    }

    fn create_user_prompt(
        &self,
        task: &Task,
        plan: &ImplementationPlan,
        file_contents: &[(PathBuf, String)],
        error_context: &Option<String>,
    ) -> String {
        let file_context = file_contents
            .iter()
            .map(|(path, content)| {
                format!(
                    "--- FILE: {} ---\n```\n{}\n```",
                    path.to_string_lossy(),
                    content
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let error_prompt = if let Some(error) = error_context {
            format!(
                "\n**Correction Context:**\nOn the previous attempt, validation failed. The error was:\n```\n{}\n```\nPlease analyze the error, fix the code, and explain the fix.",
                error
            )
        } else {
            "".to_string()
        };

        format!(
            r#"
**Task:**
{task_description}

**Coding Conventions:**
{coding_conventions}

**File Context:**
{file_context}
{error_prompt}

Implement the task by calling the `edit_file` or `create_file` functions.
"#,
            task_description = task.description,
            coding_conventions = plan.original_prompt.coding_conventions,
            file_context = file_context,
            error_prompt = error_prompt,
        )
    }

    fn process_response(&self, response: &GenerateContentResponse) -> Result<String> {
        #[derive(Deserialize)]
        struct EditFileArgs {
            path: String,
            new_content: String,
        }

        #[derive(Deserialize)]
        struct CreateFileArgs {
            path: String,
            content: String,
        }

        let candidate = response
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .ok_or_else(|| Error::Config("No candidates in response".to_string()))?;

        let mut agent_tips = "".to_string();

        for part in &candidate.content.parts {
            match part {
                PartResponse::FunctionCall(FunctionCall { name, arguments }) => {
                    info!(?name, "Processing function call");
                    match name.as_str() {
                        "edit_file" => {
                            let args: EditFileArgs = serde_json::from_value(arguments.clone())?;
                            std::fs::write(&args.path, &args.new_content)?;
                            info!(path = %args.path, "Edited file");
                        }
                        "create_file" => {
                            let args: CreateFileArgs = serde_json::from_value(arguments.clone())?;
                            let path = PathBuf::from(&args.path);
                            if let Some(parent) = path.parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            std::fs::write(&path, &args.content)?;
                            info!(path = %args.path, "Created file");
                        }
                        _ => warn!(?name, "Unknown function call"),
                    }
                }
                PartResponse::Text(text) => {
                    agent_tips = text.clone();
                }
                _ => {}
            }
        }

        Ok(agent_tips)
    }

    fn run_validation(&self, task: &Task) -> std::result::Result<(), String> {
        for step in &task.validation_steps {
            match run_command(&step.command) {
                Ok((exit_code, output)) => {
                    if exit_code != step.expected_exit_code {
                        let error_msg = format!(
                            "Validation failed for command `{}`. Expected exit code {}, but got {}.\nOutput:\n{}",
                            step.command, step.expected_exit_code, exit_code, output
                        );
                        error!("{}", error_msg);
                        return Err(error_msg);
                    }
                    info!(command = %step.command, "Validation step passed");
                }
                Err(e) => {
                    let error_msg =
                        format!("Failed to execute validation command `{}`: {}", step.command, e);
                    error!("{}", error_msg);
                    return Err(error_msg);
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Agent for ImplAgent {
    type Input = PathBuf;
    type Output = ();

    async fn run(&self, plan_path: Self::Input) -> Result<Self::Output> {
        let plan_content = std::fs::read_to_string(&plan_path)?;
        let mut plan: ImplementationPlan = serde_json::from_str(&plan_content)?;

        for task in &mut plan.tasks {
            if task.status == TaskStatus::Success {
                info!(description = %task.description, "Skipping completed task");
                continue;
            }

            info!(description = %task.description, "Starting task");
            task.status = TaskStatus::Pending;

            let mut last_error: Option<String> = None;
            for attempt in 0..self.max_retries {
                task.attempts = attempt + 1;
                info!(
                    description = %task.description,
                    attempt = task.attempts,
                    max_retries = self.max_retries,
                    "Attempting task"
                );

                let workdir = Path::new(".");
                let files_in_scope = files::get_filtered_files(workdir, &task.file_scoping)?;
                let mut file_contents = Vec::new();
                for path in &files_in_scope {
                    let content = std::fs::read_to_string(path)?;
                    file_contents.push((path.clone(), content));
                }

                let tools = self.create_file_tools();
                let system_prompt = self.create_system_prompt();
                let user_prompt =
                    self.create_user_prompt(task, &plan, &file_contents, &last_error);

                let contents = vec![
                    Content {
                        role: Role::User,
                        parts: vec![ContentPart::Text(system_prompt)],
                    },
                    Content {
                        role: Role::Model,
                        parts: vec![ContentPart::Text(
                            "Understood. I am ready to implement the task.".to_string(),
                        )],
                    },
                    Content {
                        role: Role::User,
                        parts: vec![ContentPart::Text(user_prompt)],
                    },
                ];

                let response = self.gemini.generate_content(contents, Some(tools)).await?;
                let agent_tips = self.process_response(&response)?;

                let mut formatter_error: Option<String> = None;
                if let Some(formatter_cmd) = &plan.original_prompt.formatter_command {
                    info!(command = %formatter_cmd, "Running formatter");
                    match run_command(formatter_cmd) {
                        Ok((code, output)) => {
                            if code != 0 {
                                let error_msg = format!(
                                    "Formatter command `{}` failed with exit code {}.\nOutput:\n{}",
                                    formatter_cmd, code, output
                                );
                                error!("{}", error_msg);
                                formatter_error = Some(error_msg);
                            }
                        }
                        Err(e) => {
                            let error_msg = format!(
                                "Failed to execute formatter command `{}`: {}",
                                formatter_cmd, e
                            );
                            error!("{}", error_msg);
                            formatter_error = Some(error_msg);
                        }
                    }
                }

                let validation_result = if let Some(e) = formatter_error {
                    Err(e)
                } else {
                    self.run_validation(task)
                };

                match validation_result {
                    Ok(_) => {
                        info!(description = %task.description, "Task completed successfully");
                        task.status = TaskStatus::Success;
                        task.result = Some(TaskResult {
                            success: true,
                            agent_tips,
                        });
                        let plan_json = serde_json::to_string_pretty(&plan)?;
                        std::fs::write(&plan_path, plan_json)?;
                        break;
                    }
                    Err(e) => {
                        warn!(description = %task.description, "Task attempt failed");
                        last_error = Some(e);
                    }
                }
            }

            if task.status != TaskStatus::Success {
                error!(description = %task.description, "Task failed after all retries");
                task.status = TaskStatus::Failed;
                let plan_json = serde_json::to_string_pretty(&plan)?;
                std::fs::write(&plan_path, plan_json)?;
                return Err(Error::Config(format!(
                    "Task '{}' failed after {} attempts.",
                    task.description, self.max_retries
                )));
            }
        }

        if self.auto_commit {
            info!("All tasks completed. Committing changes.");
            let mut commit_message = "AI-generated changes for:\n".to_string();
            let mut changed_files = BTreeSet::new();

            for task in &plan.tasks {
                commit_message.push_str(&format!("- {}\n", task.description));
                let files = files::get_filtered_files(Path::new("."), &task.file_scoping)?;
                for file in files {
                    changed_files.insert(file);
                }
            }

            let paths_to_commit: Vec<PathBuf> = changed_files.into_iter().collect();
            vcs::add_and_commit(Path::new("."), &paths_to_commit, &commit_message)?;
            info!("Changes committed successfully.");
        }

        Ok(())
    }
}
