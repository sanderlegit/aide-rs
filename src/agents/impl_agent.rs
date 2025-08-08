use crate::{
    agents::{
        Agent,
        state::{ImplementationPlan, Task, TaskResult, TaskStatus},
    },
    error::{Error, Result},
    files,
    gemini::GeminiClientWrapper,
    gemini_types::{
        Content, ContentPart, DynamicRetrieval, DynamicRetrievalConfig, FunctionCall,
        FunctionDeclaration, GenerateContentResponse, Role, ToolConfig,
    },
    vcs,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use similar::{ChangeTag, TextDiff};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use tracing::{error, info, warn};

pub struct ImplAgent {
    gemini: GeminiClientWrapper,
    enrich_gemini: GeminiClientWrapper,
    max_retries: u32,
    auto_commit: bool,
    enrich_errors: bool,
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
    pub fn new(max_retries: u32, auto_commit: bool, enrich_errors: bool) -> Result<Self> {
        let gemini = GeminiClientWrapper::new_impl_agent()?;
        // The plan agent uses a faster model, which is good for summarization/enrichment.
        let enrich_gemini = GeminiClientWrapper::new_plan_agent()?;
        Ok(Self {
            gemini,
            enrich_gemini,
            max_retries,
            auto_commit,
            enrich_errors,
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

    fn create_user_prompt_with_context(
        &self,
        current_task_index: usize,
        plan: &ImplementationPlan,
        file_context: &str,
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

    fn create_user_prompt(
        &self,
        current_task_index: usize,
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

        self.create_user_prompt_with_context(
            current_task_index,
            plan,
            &file_context,
            error_context,
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

                        let old_content = std::fs::read_to_string(&path).unwrap_or_default();
                        std::fs::write(&path, &args.new_content)?;

                        println!("\n--- DIFF for {} ---", path.display());
                        let diff = TextDiff::from_lines(&old_content, &args.new_content);
                        for change in diff.iter_all_changes() {
                            let sign = match change.tag() {
                                ChangeTag::Delete => "-",
                                ChangeTag::Insert => "+",
                                ChangeTag::Equal => " ",
                            };
                            print!("{}{}", sign, change);
                        }
                        println!("--- END DIFF ---\n");

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

                        println!("\n--- NEW FILE {} ---", path.display());
                        for line in args.content.lines() {
                            println!("+{}", line);
                        }
                        println!("--- END NEW FILE ---\n");

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

    async fn enrich_error_context(&self, error: &str) -> Result<String> {
        info!("Enriching error context with Google Search.");
        let search_prompt = format!(
            "The following error occurred during a software development task:
\"\"\"
{error}
\"\"\"
Please use Google Search, prioritizing results from `docs.rs` and `crates.io`, to find relevant documentation, examples, or explanations that could help resolve this error. Provide a concise summary of your findings and include any relevant URLs."
        );

        let search_contents = vec![Content {
            role: Role::User,
            parts: vec![ContentPart::Text(search_prompt)],
        }];

        let search_tools = vec![self.create_google_search_tool()];

        let search_response = self
            .enrich_gemini
            .generate_content(search_contents, Some(search_tools))
            .await?;

        let search_result_text = self.process_search_response(&search_response)?;

        Ok(format!(
            "Original error:\n{}\n\nResearch results:\n{}",
            error, search_result_text
        ))
    }

    fn create_google_search_tool(&self) -> ToolConfig {
        ToolConfig::DynamicRetrieval {
            google_search: DynamicRetrieval {},
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
impl Agent for ImplAgent {
    type Input = PathBuf;
    type Output = ();

    async fn run(&self, plan_path: Self::Input) -> Result<Self::Output> {
        let plan_content = std::fs::read_to_string(&plan_path)?;
        let mut plan: ImplementationPlan = toml::from_str(&plan_content)?;

        let mut initial_error_context: Option<String> = None;
        info!("Running initial validation of the current project state...");
        if let Err(e) = self.run_validation_steps(&plan.original_prompt.validation_commands) {
            warn!("Initial validation failed. This error will be added to the first task's context.");
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

                    let file_names_for_log = file_contents
                        .iter()
                        .map(|(path, _)| path.to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join("\n");

                    let last_error_for_log = last_error.as_ref().map(|e| {
                        let mut lines = e.lines();
                        let first_line = lines.next().unwrap_or("").to_string();
                        if lines.next().is_some() {
                            format!("{}\n... (error truncated in log)", first_line)
                        } else {
                            first_line
                        }
                    });

                    let log_user_prompt = self.create_user_prompt_with_context(
                        i,
                        &plan,
                        &file_names_for_log,
                        &last_error_for_log,
                    );
                    let log_full_prompt = format!("{}\n\n{}", system_prompt, log_user_prompt);

                    info!(
                        prompt = %format!("\n---\n{}\n---", log_full_prompt),
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

                            let mut diff_context = String::new();
                            if !modified_files.is_empty() {
                                diff_context.push_str("\n\nChanges applied in the failed attempt:\n");
                                for path in &modified_files {
                                    if let Some((_, old_content)) =
                                        file_contents.iter().find(|(p, _)| p == path)
                                    {
                                        if let Ok(new_content) = std::fs::read_to_string(path) {
                                            let diff =
                                                TextDiff::from_lines(old_content, &new_content);
                                            diff_context.push_str(&format!(
                                                "\n--- DIFF for {} ---\n",
                                                path.display()
                                            ));
                                            for change in diff.iter_all_changes() {
                                                let sign = match change.tag() {
                                                    ChangeTag::Delete => "-",
                                                    ChangeTag::Insert => "+",
                                                    ChangeTag::Equal => " ",
                                                };
                                                diff_context.push_str(&format!("{}{}", sign, change));
                                            }
                                            diff_context.push_str("--- END DIFF ---\n");
                                        }
                                    }
                                }
                            }

                            let full_error = format!("{}{}", e, diff_context);

                            if self.enrich_errors {
                                match self.enrich_error_context(&full_error).await {
                                    Ok(enriched_error) => last_error = Some(enriched_error),
                                    Err(enrich_err) => {
                                        error!(error = %enrich_err, "Failed to enrich error context");
                                        last_error = Some(full_error); // Fallback to original error
                                    }
                                }
                            } else {
                                last_error = Some(full_error);
                            }
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
