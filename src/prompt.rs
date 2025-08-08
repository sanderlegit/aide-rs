use crate::error::{Error, Result};
use crate::files;
use crate::flows::types::{FileScope, Prompt, PromptPart};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use toml::Value;

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
    ) -> Result<String> {
        let mut parts = Vec::new();
        for part in &prompt_def.composition {
            let content = self.process_part(part, prompt_path, block_outputs).await?;
            parts.push(content);
        }
        Ok(parts.join("\n"))
    }

    async fn process_part(
        &mut self,
        part: &PromptPart,
        prompt_path: &Path,
        block_outputs: &HashMap<String, serde_json::Value>,
    ) -> Result<String> {
        match part {
            PromptPart::StaticText { content } => Ok(content.clone()),
            PromptPart::PromptFileField { field, prefix } => {
                let toml_value = self.get_prompt_toml(prompt_path).await?;
                let field_value = toml_value
                    .get(field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Ok(format!("{}{}", prefix, field_value))
            }
            PromptPart::PreviousOutput { block_id, prefix } => {
                let output = block_outputs
                    .get(block_id)
                    .and_then(|v| serde_json::to_string_pretty(v).ok())
                    .unwrap_or_else(|| format!("Error: Output for block '{}' not found.", block_id));
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

                for file_path in files {
                    let file_content = match tokio::fs::read_to_string(&file_path).await {
                        Ok(c) => c,
                        Err(e) => {
                            // It's possible a file is deleted during a run, so we just warn.
                            eprintln!("Warning: could not read file {}: {}", file_path.display(), e);
                            continue;
                        }
                    };
                    content.push_str(&format!(
                        "--- FILE: {} ---\n{}\n\n",
                        file_path.display(),
                        file_content
                    ));
                }
                Ok(content)
            }
        }
    }

    async fn get_prompt_toml(&mut self, path: &Path) -> Result<&Value> {
        if !self.prompt_file_cache.contains_key(path) {
            let content = tokio::fs::read_to_string(path).await?;
            let toml_value: Value = toml::from_str(&content)?;
            self.prompt_file_cache
                .insert(path.to_path_buf(), toml_value);
        }
        Ok(self.prompt_file_cache.get(path).unwrap())
    }

    async fn get_prompt_file_scope(&mut self, path: &Path) -> Result<FileScope> {
        let toml_value = self.get_prompt_toml(path).await?;
        if let Some(scope_value) = toml_value.get("file_scoping") {
            let scope: FileScope = scope_value.clone().try_deserialize()?;
            Ok(scope)
        } else {
            Ok(FileScope::default())
        }
    }
}
