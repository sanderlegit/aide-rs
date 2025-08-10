use crate::error::{Error, Result};
use crate::gemini_types::{FunctionCall, FunctionDeclaration};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

/// A trait for any tool that can be executed by the agent.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Returns the name of the tool.
    fn name(&self) -> String;
    /// Returns the function declaration schema for the tool.
    fn schema(&self) -> FunctionDeclaration;
    /// Executes the tool with the given arguments.
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value>;
}

/// Manages the collection of available tools and executes them.
pub struct ToolExecutor {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolExecutor {
    /// Creates a new ToolExecutor with a specific set of enabled tools.
    pub fn new(enabled_tools: &[String]) -> Self {
        let mut tools: HashMap<String, Arc<dyn Tool>> = HashMap::new();
        for tool_name in enabled_tools {
            // This allows for toolsets, e.g. "file_system" expands to multiple tools
            let concrete_tools = Self::get_tools_by_name(tool_name);
            for tool in concrete_tools {
                tools.insert(tool.name(), tool);
            }
        }
        Self { tools }
    }

    /// Expands a toolset name into a vector of concrete tool implementations.
    fn get_tools_by_name(name: &str) -> Vec<Arc<dyn Tool>> {
        match name {
            "doc_retriever" => vec![Arc::new(DocRetrieverTool)],
            _ => vec![],
        }
    }

    /// Returns the list of `FunctionDeclaration` schemas for all enabled tools.
    pub fn schemas(&self) -> Vec<FunctionDeclaration> {
        self.tools.values().map(|t| t.schema()).collect()
    }

    /// Executes a function call by finding the corresponding tool.
    pub async fn execute(&self, call: &FunctionCall) -> Result<serde_json::Value> {
        if let Some(tool) = self.tools.get(&call.name) {
            tool.execute(call.arguments.clone()).await
        } else {
            Err(Error::Config(format!(
                "Tool '{}' not found or not enabled for this block.",
                call.name
            )))
        }
    }
}

// --- Tool Definitions ---

// --- Doc Retriever Tool ---

#[derive(Deserialize)]
struct DocRetrieverArgs {
    crate_name: String,
    path: Option<String>,
}

pub struct DocRetrieverTool;
#[async_trait]
impl Tool for DocRetrieverTool {
    fn name(&self) -> String {
        "doc_retriever".to_string()
    }
    fn schema(&self) -> FunctionDeclaration {
        FunctionDeclaration {
            name: self.name(),
            description: "Retrieves Rust documentation for a crate, module, or type.".to_string(),
            parameters: serde_json::from_str(r#"{
                "type": "object",
                "properties": {
                    "crate_name": { "type": "string", "description": "The name of the crate." },
                    "path": { "type": "string", "description": "Optional. The full path to the item (e.g., 'my_crate::my_module::MyStruct'). If omitted, returns crate-level docs." }
                },
                "required": ["crate_name"]
            }"#).unwrap(),
        }
    }
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let doc_args: DocRetrieverArgs = serde_json::from_value(args)?;
        let base_dir = std::env::current_dir()?;
        // In the test environment, the crate we want to document is in a subdirectory
        // named after the crate itself. We'll check for that directory and use it as
        // the context for `rustdoc-json`, falling back to the current directory.
        let crate_dir = base_dir.join(&doc_args.crate_name);
        let doc_path = if crate_dir.is_dir() {
            crate_dir
        } else {
            base_dir
        };
        debug!(path = %doc_path.display(), "Using path for doc retrieval");

        if let Some(path) = doc_args.path {
            crate::doc_retriever::get_item_docs(&doc_args.crate_name, &path, Some(&doc_path))
        } else {
            crate::doc_retriever::get_crate_docs(&doc_args.crate_name, Some(&doc_path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use serde_json::json;

    #[tokio::test]
    async fn test_tool_executor_new_and_schemas() {
        let executor = ToolExecutor::new(&["doc_retriever".to_string()]);
        let schemas = executor.schemas();
        assert_eq!(schemas.len(), 1);
        assert!(schemas.iter().any(|s| s.name == "doc_retriever"));
    }

    #[tokio::test]
    async fn test_tool_executor_execute_not_found() {
        let executor = ToolExecutor::new(&[]);
        let call = FunctionCall {
            name: "non_existent_tool".to_string(),
            arguments: json!({}),
        };

        let result = executor.execute(&call).await;
        assert!(result.is_err());
        if let Err(Error::Config(msg)) = result {
            assert!(msg.contains("Tool 'non_existent_tool' not found"));
        } else {
            panic!("Expected Config error");
        }
    }
}
