use crate::{
    agents::{
        Agent,
        state::{ImplementationPlan, Task, TaskResult, TaskStatus},
    },
    error::{Error, Result},
    files,
    gemini::GeminiClientWrapper,
    gemini_types::{
        Content, ContentPart, FunctionCall, FunctionDeclaration, GenerateContentResponse, Role,
    },
    vcs,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use tracing::{error, info, warn};

async fn summarize_error(error_output: &str) -> Result<String> {
    info!("Summarizing error output...");
    let gemini = GeminiClientWrapper::new_summarize_agent()?;
    let prompt = format!(
        "Summarize this compiler/tool error into its most critical message, keeping it concise and focusing on the root cause: {}",
        error_output
    );

    let contents = vec![Content {
        role: Role::User,
        parts: vec![ContentPart::Text(prompt)],
    }];

    let response = gemini.generate_content::<()>(contents, None).await?;

    let candidate = response
        .candidates
        .and_then(|mut c| c.pop())
        .ok_or_else(|| Error::Config("No candidates in summarization response".to_string()))?;

    if let Some(part) = candidate.content.parts.into_iter().next() {
        if let Some(text) = part.text {
            info!(summary = %text, "Successfully summarized error");
            return Ok(text);
        }
    }

    Err(Error::Config(
        "Expected a text part in the summarization response".to_string(),
    ))
}


pub struct ImplAgent {
    gemini: GeminiClientWrapper,
    max_retries: u32,
    auto_commit: bool,
}

fn run_command(command_str: &str) -> Result<(i32, String)> {
    info!(command = command_str, "Running command");
    if command_str.is_empty() {
        return Err(Error::Config("Empty command".to_string()));
    }

    let output = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", command_str]).output()?
    } else {
        Command::new("sh").arg("-c").arg(command_str).output()?
    };

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
                parameters: json!({
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
                }),
            },
            FunctionDeclaration {
                name: "create_file".to_string(),
                description: "Creates a new file with specified content.".to_string(),
                parameters: json!({
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
                }),
            },
        ]
    }

    fn create_system_prompt(&self) -> String {
        "You are an expert pair programmer. Implement the user's request by calling the provided file manipulation functions. Adhere strictly to the coding conventions provided. After your final edit, run the formatter if one is specified. Finally, explain the problem and your solution.".to_string()
    }

    fn create_user_prompt(
        &self,
        current_task_index: usize,
        plan: &ImplementationPlan,
        file_contents: &[(PathBuf, String)],
        error_context: &Option<String>,
    ) -> String {
        let task = &plan.tasks[current_task_index];
        let original_prompt = &plan.original_prompt;

        let tasks_overview = plan
            .tasks
            .iter()
            .enumerate()
            .map(|(idx, t)| {
                let status_marker = match t.status {
                    TaskStatus::Success => "[✓]",
                    TaskStatus::Pending => "[ ]",
                    TaskStatus::Failed => "[✗]",
                };
                let current_marker = if idx == current_task_index {
                    ">>"
                } else {
                    "  "
                };
                format!(
                    "{} {} {}. {}",
                    current_marker,
                    status_marker,
                    idx + 1,
                    t.description
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

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
                "\n**Correction Context:**\nThe last attempt failed validation. The error was:\n```\n{}\n```\nPlease analyze the error, fix the code, and explain the fix.",
                error
            )
        } else {
            "".to_string()
        };

        format!(
            r#"
**Overall Plan:**
{tasks_overview}

**Current Task:**
{task_description}

**Coding Conventions:**
{coding_conventions}

**Project File Context:**
{file_context}
{error_prompt}

Implement the current task by calling the `edit_file` or `create_file` functions.
"#,
            tasks_overview = tasks_overview,
            task_description = task.description,
            coding_conventions = original_prompt.coding_conventions,
            file_context = file_context,
            error_prompt = error_prompt,
        )
    }

    fn process_response(
        &self,
        response: &GenerateContentResponse,
    ) -> Result<(String, Vec<PathBuf>)> {
        use std::io::Write;

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
        let mut modified_files = Vec::new();

        for part in &candidate.content.parts {
            if let Some(FunctionCall { name, arguments }) = &part.function_call {
                info!(?name, "Processing function call");
                match name.as_str() {
                    "edit_file" => {
                        let args: EditFileArgs = serde_json::from_value(arguments.clone())?;
                        let path = PathBuf::from(&args.path);
                        std::fs::write(&path, &args.new_content)?;
                        info!(path = %args.path, "Edited file");
                        modified_files.push(path);
                    }
                    "create_file" => {
                        let args: CreateFileArgs = serde_json::from_value(arguments.clone())?;
                        let path = PathBuf::from(&args.path);
                        if let Some(parent) = path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let mut file = std::fs::File::create(&path)?;
                        file.write_all(args.content.as_bytes())?;
                        file.sync_all()?;
                        info!(path = %args.path, "Created file");
                        modified_files.push(path);
                    }
                    _ => warn!(?name, "Unknown function call"),
                }
            }
            if let Some(text) = &part.text {
                agent_tips = text.clone();
            }
        }

        Ok((agent_tips, modified_files))
    }

    fn run_validation_steps(
        &self,
        steps: &[crate::agents::state::ValidationStep],
    ) -> std::result::Result<(), String> {
        for step in steps {
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
                    let error_msg = format!(
                        "Failed to execute validation command `{}`: {}",
                        step.command, e
                    );
                    error!("{}", error_msg);
                    return Err(error_msg);
                }
            }
        }
        Ok(())
    }

    fn run_validation(&self, task: &Task) -> std::result::Result<(), String> {
        self.run_validation_steps(&task.validation_steps)
    }
}

#[async_trait]
impl Agent for ImplAgent {
    type Input = PathBuf;
    type Output = ();

    async fn run(&self, plan_path: Self::Input) -> Result<Self::Output> {
        let plan_content = std::fs::read_to_string(&plan_path)?;
        let mut plan: ImplementationPlan = toml::from_str(&plan_content)?;

        let mut initial_error_context: Option<String> = None;
        info!("Running initial validation of the current project state...");
        if let Err(e) = self.run_validation_steps(&plan.original_prompt.validation_commands) {
            warn!(error = %e, "Initial validation failed. This error will be added to the first task's context.");
            initial_error_context = Some(e);
        }

        let mut is_first_pending_task = true;

        for i in 0..plan.tasks.len() {
            let task_succeeded = {
                if plan.tasks[i].status == TaskStatus::Success {
                    info!(description = %plan.tasks[i].description, "Skipping completed task");
                    continue;
                }

                let mut task = plan.tasks[i].clone();

                info!(description = %task.description, "Starting task");
                task.status = TaskStatus::Pending;
                info!(status = ?task.status, "Task status updated");

                let mut last_error: Option<String> = None;
                if is_first_pending_task {
                    if let Some(initial_error) = initial_error_context.take() {
                        last_error = Some(format!(
                            "The project failed initial validation before starting the first task. Please fix this issue first.\nError:\n{}",
                            initial_error
                        ));
                    }
                    is_first_pending_task = false;
                }

                let mut succeeded = false;
                for attempt in 0..self.max_retries {
                    task.attempts = attempt + 1;
                    info!(
                        description = %task.description,
                        attempt = task.attempts,
                        max_retries = self.max_retries,
                        "Attempting task"
                    );

                    let workdir = Path::new(".");

                    // Use all files from the original prompt's scope as context.
                    let files_for_context =
                        files::get_filtered_files(workdir, &plan.original_prompt.file_scoping)?;

                    let mut file_contents = Vec::new();
                    for path in &files_for_context {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            file_contents.push((path.clone(), content));
                        } else {
                            warn!(path = %path.display(), "Could not read file for context, it may have been deleted.");
                        }
                    }

                    let tools = self.create_file_tools();
                    let system_prompt = self.create_system_prompt();
                    let user_prompt =
                        self.create_user_prompt(i, &plan, &file_contents, &last_error);

                    let full_prompt = format!("{}\n\n{}", system_prompt, user_prompt);

                    info!(
                        prompt = %format!("\n---\n{}\n---", full_prompt),
                        "Sending implementation prompt to Gemini"
                    );

                    let contents = vec![Content {
                        role: Role::User,
                        parts: vec![ContentPart::Text(full_prompt)],
                    }];

                    let tool_config = json!([{
                        "functionDeclarations": tools
                    }]);
                    let response = self
                        .gemini
                        .generate_content(contents, Some(tool_config))
                        .await?;
                    let (agent_tips, modified_files) = self.process_response(&response)?;

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
                        self.run_validation(&task)
                    };

                    match validation_result {
                        Ok(_) => {
                            info!(description = %task.description, "Task completed successfully");
                            task.status = TaskStatus::Success;
                            info!(status = ?task.status, "Task status updated");
                            task.result = Some(TaskResult {
                                success: true,
                                agent_tips,
                                modified_files: modified_files
                                    .into_iter()
                                    .map(|p| p.to_string_lossy().to_string())
                                    .collect(),
                            });
                            succeeded = true;
                            break;
                        }
                        Err(e) => {
                            warn!(description = %task.description, "Task attempt failed");
                            const SUMMARIZATION_THRESHOLD: usize = 1000;
                            let error_for_prompt = if e.len() > SUMMARIZATION_THRESHOLD {
                                match summarize_error(&e).await {
                                    Ok(summary) => summary,
                                    Err(summary_err) => {
                                        error!(error = %summary_err, "Failed to summarize error, using full error text");
                                        e
                                    }
                                }
                            } else {
                                e
                            };
                            last_error = Some(error_for_prompt);
                        }
                    }
                }

                if succeeded {
                    plan.tasks[i] = task;
                }
                succeeded
            };

            if task_succeeded {
                if self.auto_commit {
                    if let Some(result) = &plan.tasks[i].result {
                        if !result.modified_files.is_empty() {
                            let commit_message = format!("AI: {}", plan.tasks[i].description);
                            let paths_to_commit: Vec<PathBuf> = result
                                .modified_files
                                .iter()
                                .map(PathBuf::from)
                                .collect();
                            info!(commit_message = %commit_message, "Committing changes for successful task.");
                            vcs::add_and_commit(Path::new("."), &paths_to_commit, &commit_message)?;
                            info!("Changes committed successfully.");
                        } else {
                            info!("No files were modified for this task, skipping commit.");
                        }
                    }
                }

                let plan_toml = toml::to_string_pretty(&plan)?;
                std::fs::write(&plan_path, plan_toml)?;
            } else {
                plan.tasks[i].status = TaskStatus::Failed;
                info!(status = ?plan.tasks[i].status, "Task status updated");
                error!(description = %plan.tasks[i].description, "Task failed after all retries");
                let plan_toml = toml::to_string_pretty(&plan)?;
                std::fs::write(&plan_path, plan_toml)?;
                return Err(Error::Config(format!(
                    "Task '{}' failed after {} attempts.",
                    plan.tasks[i].description, self.max_retries
                )));
            }
        }

        Ok(())
    }
}
