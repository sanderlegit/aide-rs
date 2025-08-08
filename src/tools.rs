use crate::error::{Error, Result};
use crate::flows::types::TaskList;
use crate::gemini_types::{FunctionCall, FunctionDeclaration};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

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
            "task_creator" => vec![Arc::new(TaskCreatorTool)],
            "file_system" => vec![
                Arc::new(CreateFileTool),
                Arc::new(EditFileTool),
                Arc::new(ReadFileTool),
            ],
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

// --- Task Creator Tool ---
pub struct TaskCreatorTool;

#[async_trait]
impl Tool for TaskCreatorTool {
    fn name(&self) -> String {
        "create_task_list".to_string()
    }

    fn schema(&self) -> FunctionDeclaration {
        FunctionDeclaration {
            name: self.name(),
            description: "Creates a structured list of tasks from a high-level plan. This should be the final step of planning.".to_string(),
            parameters: serde_json::from_str(r#"{
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "description": "The list of tasks.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {
                                    "type": "string",
                                    "description": "A short, unique identifier for the task, e.g., 'impl-rocket-route'."
                                },
                                "description": {
                                    "type": "string",
                                    "description": "A detailed description of what needs to be done for this task."
                                }
                            },
                            "required": ["id", "description"]
                        }
                    }
                },
                "required": ["tasks"]
            }"#).unwrap(),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        // The tool's job is just to validate and return the arguments,
        // which the runner will then use as the structured output.
        let task_list: TaskList = serde_json::from_value(args)?;
        Ok(serde_json::to_value(task_list)?)
    }
}

// --- File System Tools ---

#[derive(Deserialize)]
struct FilePathArgs {
    path: String,
}

#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
}

pub struct CreateFileTool;
#[async_trait]
impl Tool for CreateFileTool {
    fn name(&self) -> String {
        "create_file".to_string()
    }
    fn schema(&self) -> FunctionDeclaration {
        FunctionDeclaration {
            name: self.name(),
            description: "Creates a new file with the given content. Fails if the file already exists.".to_string(),
            parameters: serde_json::from_str(r#"{
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The path to the file to create." },
                    "content": { "type": "string", "description": "The initial content of the file." }
                },
                "required": ["path", "content"]
            }"#).unwrap(),
        }
    }
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let write_args: WriteFileArgs = serde_json::from_value(args)?;
        let path = PathBuf::from(&write_args.path);
        if path.exists() {
            return Err(Error::Config(format!(
                "File '{}' already exists. Use edit_file to modify it.",
                write_args.path
            )));
        }
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, &write_args.content).await?;
        Ok(json!({ "status": "success", "path": write_args.path }))
    }
}

pub struct EditFileTool;
#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> String {
        "edit_file".to_string()
    }
    fn schema(&self) -> FunctionDeclaration {
        FunctionDeclaration {
            name: self.name(),
            description: "Edits an existing file by replacing its entire content.".to_string(),
            parameters: serde_json::from_str(r#"{
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The path to the file to edit." },
                    "content": { "type": "string", "description": "The new content of the file." }
                },
                "required": ["path", "content"]
            }"#).unwrap(),
        }
    }
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let write_args: WriteFileArgs = serde_json::from_value(args)?;
        tokio::fs::write(&write_args.path, &write_args.content).await?;
        Ok(json!({ "status": "success", "path": write_args.path }))
    }
}

pub struct ReadFileTool;
#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> String {
        "read_file".to_string()
    }
    fn schema(&self) -> FunctionDeclaration {
        FunctionDeclaration {
            name: self.name(),
            description: "Reads the entire content of a file.".to_string(),
            parameters: serde_json::from_str(r#"{
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The path to the file to read." }
                },
                "required": ["path"]
            }"#).unwrap(),
        }
    }
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let read_args: FilePathArgs = serde_json::from_value(args)?;
        let content = tokio::fs::read_to_string(&read_args.path).await?;
        Ok(json!({ "path": read_args.path, "content": content }))
    }
}

// --- Doc Retriever Tool ---

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
                    "path": { "type": "string", "description": "Optional. The full path to the item (e.g., 'my_crate::my_module::MyStruct')." }
                },
                "required": ["crate_name"]
            }"#).unwrap(),
        }
    }
    async fn execute(&self, _args: serde_json::Value) -> Result<serde_json::Value> {
        Err(Error::Config(
            "doc_retriever tool not fully implemented yet".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_tool_executor_new_and_schemas() {
        let executor = ToolExecutor::new(&["file_system".to_string(), "task_creator".to_string()]);
        let schemas = executor.schemas();
        assert_eq!(schemas.len(), 4); // create, edit, read, create_task_list
        assert!(schemas.iter().any(|s| s.name == "create_file"));
        assert!(schemas.iter().any(|s| s.name == "edit_file"));
        assert!(schemas.iter().any(|s| s.name == "read_file"));
        assert!(schemas.iter().any(|s| s.name == "create_task_list"));
    }

    #[tokio::test]
    async fn test_tool_executor_execute_dispatch() {
        let executor = ToolExecutor::new(&["task_creator".to_string()]);
        let args = json!({
            "tasks": [
                { "id": "task1", "description": "First task" }
            ]
        });
        let call = FunctionCall {
            name: "create_task_list".to_string(),
            arguments: args.clone(),
        };

        let result = executor.execute(&call).await.unwrap();
        assert_eq!(result, args);
    }

    #[tokio::test]
    async fn test_tool_executor_execute_not_found() {
        let executor = ToolExecutor::new(&["file_system".to_string()]);
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

    #[tokio::test]
    async fn test_create_file_tool() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        let tool = CreateFileTool;
        let args = json!({
            "path": file_path.to_str().unwrap(),
            "content": "hello world"
        });

        let result = tool.execute(args).await.unwrap();
        assert_eq!(result["status"], "success");
        assert_eq!(result["path"], file_path.to_str().unwrap());

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "hello world");

        // Test failure on existing file
        let args_fail = json!({
            "path": file_path.to_str().unwrap(),
            "content": "fail"
        });
        let result_fail = tool.execute(args_fail).await;
        assert!(result_fail.is_err());
    }

    #[tokio::test]
    async fn test_edit_file_tool() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        tokio::fs::write(&file_path, "initial content")
            .await
            .unwrap();

        let tool = EditFileTool;
        let args = json!({
            "path": file_path.to_str().unwrap(),
            "content": "new content"
        });

        let result = tool.execute(args).await.unwrap();
        assert_eq!(result["status"], "success");

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn test_read_file_tool() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        tokio::fs::write(&file_path, "read me").await.unwrap();

        let tool = ReadFileTool;
        let args = json!({ "path": file_path.to_str().unwrap() });

        let result = tool.execute(args).await.unwrap();
        assert_eq!(result["content"], "read me");

        // Test failure on non-existent file
        let args_fail = json!({ "path": "nonexistent.txt" });
        let result_fail = tool.execute(args_fail).await;
        assert!(result_fail.is_err());
    }
}
