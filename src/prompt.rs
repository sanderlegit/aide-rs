use crate::error::Result;
use crate::files;
use crate::flows::types::{FileScope, Prompt, PromptPart};
use serde_yaml::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    ) -> Result<String> {
        let mut parts = Vec::new();
        for part in &prompt_def.composition {
            let content = self
                .process_part(
                    part,
                    prompt_path,
                    block_outputs,
                    current_block_id,
                    verification_output,
                )
                .await?;
            parts.push(content);
        }
        Ok(parts.join("\n"))
    }

    async fn process_part(
        &mut self,
        part: &PromptPart,
        prompt_path: &Path,
        block_outputs: &HashMap<String, serde_json::Value>,
        current_block_id: &str,
        verification_output: &Option<serde_json::Value>,
    ) -> Result<String> {
        match part {
            PromptPart::StaticText { content } => Ok(content.clone()),
            PromptPart::PromptFileField { field, prefix } => {
                let yaml_value = self.get_prompt_yaml(prompt_path).await?;
                let field_value = yaml_value
                    .get(field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Ok(format!("{}{}", prefix, field_value))
            }
            PromptPart::PreviousOutput { block_id, prefix } => {
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
                        .and_then(|v| serde_json::to_string_pretty(v).ok())
                        .unwrap_or_else(|| {
                            format!("Error: Output for block '{}' not found.", block_id)
                        })
                };
                Ok(format!("{}{}", prefix, output))
            }
            PromptPart::FileContents { scopes, prefix } => {
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
                Ok(content)
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
                },
                PromptPart::StaticText {
                    content: "World".to_string(),
                },
            ],
        };
        let prompt_path = Path::new("dummy.yml");
        let block_outputs = HashMap::new();

        let result = builder
            .build(&prompt_def, prompt_path, &block_outputs, "test_block", &None)
            .await
            .unwrap();

        assert_eq!(result, "Hello\nWorld");
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
                },
                PromptPart::PreviousOutput {
                    block_id: "prev_block".to_string(),
                    prefix: "Previous: ".to_string(),
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
            result,
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
            result,
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
        assert_eq!(result, expected);
    }
}
