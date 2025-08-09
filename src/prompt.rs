use crate::error::Result;
use crate::files;
use crate::flows::types::{FileScope, Prompt, PromptPart};
use serde_yaml::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Represents a fully constructed prompt, with a version for display.
pub struct BuiltPrompt {
    pub full_prompt: String,
    pub display_prompt: String,
}

/// Constructs a final prompt string from a `Prompt` definition.
#[derive(Default)]
pub struct PromptBuilder {
    // Caching prompt file content to avoid re-reading.
    prompt_file_cache: HashMap<PathBuf, Value>,
}

impl PromptBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the prompt, resolving all composition parts.
    pub async fn build(
        &mut self,
        prompt_def: &Prompt,
        prompt_path: &Path,
        block_outputs: &HashMap<String, serde_json::Value>,
        current_block_id: &str,
        verification_output: &Option<serde_json::Value>,
    ) -> Result<BuiltPrompt> {
        let is_debug = std::env::var("RUST_LOG").map_or(false, |v| v.contains("debug"));
        let mut full_parts = Vec::new();
        let mut display_parts = Vec::new();
        for part in &prompt_def.composition {
            let (content, hide) = self
                .process_part(
                    part,
                    prompt_path,
                    block_outputs,
                    current_block_id,
                    verification_output,
                )
                .await?;
            full_parts.push(content.clone());
            if !hide || is_debug {
                display_parts.push(content);
            } else if !content.trim().is_empty() {
                let line_count = content.lines().count();
                display_parts.push(format!(
                    "... ({} lines of hidden content in complete.log.jsonl) ...",
                    line_count
                ));
            }
        }
        Ok(BuiltPrompt {
            full_prompt: full_parts.join("\n"),
            display_prompt: display_parts.join("\n"),
        })
    }

    async fn process_part(
        &mut self,
        part: &PromptPart,
        prompt_path: &Path,
        block_outputs: &HashMap<String, serde_json::Value>,
        current_block_id: &str,
        verification_output: &Option<serde_json::Value>,
    ) -> Result<(String, bool)> {
        match part {
            PromptPart::StaticText {
                content,
                hide_in_stdout,
            } => Ok((content.clone(), *hide_in_stdout)),
            PromptPart::PromptFileField {
                field,
                prefix,
                hide_in_stdout,
            } => {
                let yaml_value = self.get_prompt_yaml(prompt_path).await?;
                let field_value = yaml_value
                    .get(field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Ok((format!("{}{}", prefix, field_value), *hide_in_stdout))
            }
            PromptPart::PreviousOutput {
                block_id,
                prefix,
                hide_in_stdout,
            } => {
                let output = if block_id == current_block_id {
                    // Special case: reference to current block's verification output
                    verification_output
                        .as_ref()
                        .and_then(|v| serde_json::to_string_pretty(v).ok())
                        .unwrap_or_else(|| {
                            format!(
                                "Error: Verification output for block '{}' not available.",
                                block_id
                            )
                        })
                } else {
                    block_outputs
                        .get(block_id)
                        .map(|v| {
                            // If the value is a JSON object that looks like a Task from a loop,
                            // just extract the description field to keep the prompt clean.
                            if v.is_object() {
                                if let Some(desc) = v.get("description").and_then(|d| d.as_str()) {
                                    return desc.to_string();
                                }
                            }
                            if let Some(s) = v.as_str() {
                                s.to_string()
                            } else {
                                serde_json::to_string_pretty(v).unwrap_or_default()
                            }
                        })
                        .unwrap_or_else(|| {
                            format!("Error: Output for block '{}' not found.", block_id)
                        })
                };
                Ok((format!("{}{}", prefix, output), *hide_in_stdout))
            }
            PromptPart::FileList {
                scopes,
                prefix,
                hide_in_stdout,
            } => {
                let mut final_scope = FileScope::default();
                let base_dir = PathBuf::from(".");

                for scope_name in scopes {
                    let scope = if scope_name == "prompt" {
                        self.get_prompt_file_scope(prompt_path).await?
                    } else {
                        let scope_path = PathBuf::from(format!("ctx/{}.yaml", scope_name));
                        FileScope::from_yaml_file(&scope_path)?
                    };
                    final_scope.merge(scope);
                }

                let files = files::get_filtered_files(&base_dir, &final_scope)?;
                let mut content = String::new();
                if !prefix.is_empty() {
                    content.push_str(prefix);
                    content.push('\n');
                }

                let canonical_base = base_dir.canonicalize()?;
                for file_path in files {
                    let display_path = file_path
                        .strip_prefix(&canonical_base)
                        .unwrap_or(&file_path);
                    content.push_str(&format!("- ./{}", display_path.display()));
                    content.push('\n');
                }
                Ok((content, *hide_in_stdout))
            }
            PromptPart::FileContents {
                scopes,
                prefix,
                hide_in_stdout,
            } => {
                let mut final_scope = FileScope::default();
                let base_dir = PathBuf::from(".");

                for scope_name in scopes {
                    let scope = if scope_name == "prompt" {
                        self.get_prompt_file_scope(prompt_path).await?
                    } else {
                        let scope_path = PathBuf::from(format!("ctx/{}.yaml", scope_name));
                        FileScope::from_yaml_file(&scope_path)?
                    };
                    final_scope.merge(scope);
                }

                let files = files::get_filtered_files(&base_dir, &final_scope)?;
                let mut content = String::new();
                if !prefix.is_empty() {
                    content.push_str(prefix);
                    content.push('\n');
                }

                let canonical_base = base_dir.canonicalize()?;
                for file_path in files {
                    let file_content = match tokio::fs::read_to_string(&file_path).await {
                        Ok(c) => c,
                        Err(e) => {
                            // It's possible a file is deleted during a run, so we just warn.
                            eprintln!("Warning: could not read file {}: {}", file_path.display(), e);
                            continue;
                        }
                    };
                    let display_path = file_path.strip_prefix(&canonical_base).unwrap_or(&file_path);
                    content.push_str(&format!(
                        "--- FILE: ./{} ---\n{}\n\n",
                        display_path.display(),
                        file_content
                    ));
                }
                Ok((content, *hide_in_stdout))
            }
        }
    }

    async fn get_prompt_yaml(&mut self, path: &Path) -> Result<&Value> {
        if !self.prompt_file_cache.contains_key(path) {
            let content = tokio::fs::read_to_string(path).await?;
            let yaml_value: Value = serde_yaml::from_str(&content)?;
            self.prompt_file_cache
                .insert(path.to_path_buf(), yaml_value);
        }
        Ok(self.prompt_file_cache.get(path).unwrap())
    }

    async fn get_prompt_file_scope(&mut self, path: &Path) -> Result<FileScope> {
        let yaml_value = self.get_prompt_yaml(path).await?;
        if let Some(scope_value) = yaml_value.get("file_scoping") {
            let scope: FileScope = serde_yaml::from_value(scope_value.clone())?;
            Ok(scope)
        } else {
            Ok(FileScope::default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_build_prompt_simple() {
        let mut builder = PromptBuilder::new();
        let prompt_def = Prompt {
            composition: vec![
                PromptPart::StaticText {
                    content: "Hello".to_string(),
                    hide_in_stdout: false,
                },
                PromptPart::StaticText {
                    content: "World".to_string(),
                    hide_in_stdout: false,
                },
            ],
        };
        let prompt_path = Path::new("dummy.yml");
        let block_outputs = HashMap::new();

        let result = builder
            .build(&prompt_def, prompt_path, &block_outputs, "test_block", &None)
            .await
            .unwrap();

        assert_eq!(result.full_prompt, "Hello\nWorld");
        assert_eq!(result.display_prompt, "Hello\nWorld");
    }

    #[tokio::test]
    async fn test_build_prompt_with_fields_and_outputs() {
        let dir = tempdir().unwrap();
        let prompt_file_path = dir.path().join("prompt.yml");
        let mut file = File::create(&prompt_file_path).unwrap();
        writeln!(file, "objective: Test Objective").unwrap();

        let mut builder = PromptBuilder::new();
        let prompt_def = Prompt {
            composition: vec![
                PromptPart::PromptFileField {
                    field: "objective".to_string(),
                    prefix: "Objective: ".to_string(),
                    hide_in_stdout: false,
                },
                PromptPart::PreviousOutput {
                    block_id: "prev_block".to_string(),
                    prefix: "Previous: ".to_string(),
                    hide_in_stdout: false,
                },
            ],
        };

        let mut block_outputs = HashMap::new();
        block_outputs.insert(
            "prev_block".to_string(),
            json!({ "result": "some output" }),
        );

        let result = builder
            .build(
                &prompt_def,
                &prompt_file_path,
                &block_outputs,
                "current_block",
                &None,
            )
            .await
            .unwrap();

        let expected_output = json!({ "result": "some output" });
        let expected_json_string = serde_json::to_string_pretty(&expected_output).unwrap();
        assert_eq!(
            result.full_prompt,
            format!(
                "Objective: Test Objective\nPrevious: {}",
                expected_json_string
            )
        );
    }

    #[tokio::test]
    async fn test_build_prompt_with_verification_output() {
        let mut builder = PromptBuilder::new();
        let prompt_def = Prompt {
            composition: vec![PromptPart::PreviousOutput {
                block_id: "current_block".to_string(),
                prefix: "Verification Failed: ".to_string(),
                hide_in_stdout: false,
            }],
        };
        let prompt_path = Path::new("dummy.yml");
        let block_outputs = HashMap::new();
        let verification_output = Some(json!({ "error": "cargo check failed" }));

        let result = builder
            .build(
                &prompt_def,
                prompt_path,
                &block_outputs,
                "current_block",
                &verification_output,
            )
            .await
            .unwrap();

        let expected_output = json!({ "error": "cargo check failed" });
        let expected_json_string = serde_json::to_string_pretty(&expected_output).unwrap();
        assert_eq!(
            result.full_prompt,
            format!("Verification Failed: {}", expected_json_string)
        );
    }

    #[tokio::test]
    async fn test_build_prompt_with_file_contents() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Create project files
        File::create(base.join("main.rs"))
            .unwrap()
            .write_all(b"fn main() {}")
            .unwrap();
        File::create(base.join("lib.rs"))
            .unwrap()
            .write_all(b"// lib")
            .unwrap();

        // Create context scope file
        let ctx_dir = base.join("ctx");
        std::fs::create_dir(&ctx_dir).unwrap();
        let mut scope_file = File::create(ctx_dir.join("test_scope.yaml")).unwrap();
        writeln!(scope_file, "include:\n  - \"**/*.rs\"").unwrap();

        // Change current directory to the temp dir so file paths are resolved correctly
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(base).unwrap();

        let mut builder = PromptBuilder::new();
        let prompt_def = Prompt {
            composition: vec![PromptPart::FileContents {
                scopes: vec!["test_scope".to_string()],
                prefix: "Code:\n".to_string(),
                hide_in_stdout: false,
            }],
        };
        let prompt_path = Path::new("dummy.yml");
        let block_outputs = HashMap::new();

        let result = builder
            .build(&prompt_def, prompt_path, &block_outputs, "test_block", &None)
            .await
            .unwrap();

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();

        // The order of files is sorted by `get_filtered_files`.
        let expected =
            "Code:\n\n--- FILE: ./lib.rs ---\n// lib\n\n--- FILE: ./main.rs ---\nfn main() {}\n\n";
        assert_eq!(result.full_prompt, expected);
    }
}
