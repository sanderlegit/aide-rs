use crate::error::{Error, Result};
use crate::gemini_types::{FunctionCall, FunctionDeclaration};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
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
            "file_system" => vec![Arc::new(FileSystemTool)],
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

// --- File System Tool ---

#[derive(Deserialize)]
struct FileSystemArgs {
    operation: String,
    path: PathBuf,
    content: Option<String>,
}

pub struct FileSystemTool;
#[async_trait]
impl Tool for FileSystemTool {
    fn name(&self) -> String {
        "file_system".to_string()
    }
    fn schema(&self) -> FunctionDeclaration {
        FunctionDeclaration {
            name: self.name(),
            description: "Performs file system operations: read, write, or list files in a directory.".to_string(),
            parameters: serde_json::from_str(r#"{
                "type": "object",
                "properties": {
                    "operation": {
                        "type": "string",
                        "description": "The operation to perform.",
                        "enum": ["read", "write", "list"]
                    },
                    "path": { "type": "string", "description": "The path to the file or directory." },
                    "content": { "type": "string", "description": "The content to write to the file (only for 'write' operation)." }
                },
                "required": ["operation", "path"]
            }"#).unwrap(),
        }
    }
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value> {
        let fs_args: FileSystemArgs = serde_json::from_value(args)?;

        // Basic security check: prevent path traversal.
        if fs_args.path.is_absolute()
            || fs_args
                .path
                .components()
                .any(|c| c == std::path::Component::ParentDir)
        {
            return Err(Error::Config(
                "Path must be relative and within the project directory.".to_string(),
            ));
        }

        match fs_args.operation.as_str() {
            "read" => {
                let content = tokio::fs::read_to_string(&fs_args.path).await?;
                Ok(json!({ "success": true, "content": content }))
            }
            "write" => {
                let content = fs_args.content.ok_or_else(|| {
                    Error::Config("Content is required for write operation".to_string())
                })?;
                if let Some(parent) = fs_args.path.parent() {
                    if !parent.as_os_str().is_empty() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                }
                tokio::fs::write(&fs_args.path, content).await?;
                Ok(json!({ "success": true, "path": fs_args.path.to_str() }))
            }
            "list" => {
                let mut files = vec![];
                let mut read_dir = tokio::fs::read_dir(&fs_args.path).await?;
                while let Some(entry) = read_dir.next_entry().await? {
                    files.push(entry.path().to_string_lossy().to_string());
                }
                Ok(json!({ "success": true, "files": files }))
            }
            _ => Err(Error::Config(format!(
                "Unknown file system operation: {}",
                fs_args.operation
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_tool_executor_new_and_schemas() {
        let executor = ToolExecutor::new(&["doc_retriever".to_string(), "file_system".to_string()]);
        let schemas = executor.schemas();
        assert_eq!(schemas.len(), 2);
        assert!(schemas.iter().any(|s| s.name == "doc_retriever"));
        assert!(schemas.iter().any(|s| s.name == "file_system"));
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

    #[tokio::test]
    async fn test_fs_tool_write_and_read() {
        let dir = tempdir().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let tool = FileSystemTool;
        let path = "test.txt";
        let content = "hello world";

        // Write
        let write_args = json!({
            "operation": "write",
            "path": path,
            "content": content
        });
        let result = tool.execute(write_args).await.unwrap();
        assert_eq!(result, json!({"success": true, "path": path}));
        assert_eq!(fs::read_to_string(path).unwrap(), content);

        // Read
        let read_args = json!({
            "operation": "read",
            "path": path
        });
        let result = tool.execute(read_args).await.unwrap();
        assert_eq!(result, json!({"success": true, "content": content}));

        // Write to subdir
        let path_subdir = "subdir/test.txt";
        let write_args_subdir = json!({
            "operation": "write",
            "path": path_subdir,
            "content": content
        });
        tool.execute(write_args_subdir).await.unwrap();
        assert_eq!(fs::read_to_string(path_subdir).unwrap(), content);

        std::env::set_current_dir(original_dir).unwrap();
    }

    #[tokio::test]
    async fn test_fs_tool_list() {
        let dir = tempdir().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        fs::write("file1.txt", "a").unwrap();
        fs::create_dir("subdir").unwrap();
        fs::write("subdir/file2.txt", "b").unwrap();

        let tool = FileSystemTool;
        let list_args = json!({
            "operation": "list",
            "path": "."
        });
        let result = tool.execute(list_args).await.unwrap();
        let mut files = result["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        files.sort();
        // Use Path::join to create platform-agnostic paths for comparison
        let expected_file1 = std::path::Path::new(".").join("file1.txt");
        let expected_subdir = std::path::Path::new(".").join("subdir");
        let mut expected = vec![
            expected_file1.to_str().unwrap().to_string(),
            expected_subdir.to_str().unwrap().to_string(),
        ];
        expected.sort();
        assert_eq!(files, expected);

        let list_args_subdir = json!({
            "operation": "list",
            "path": "subdir"
        });
        let result_subdir = tool.execute(list_args_subdir).await.unwrap();
        let files_subdir = result_subdir["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        let expected_subdir_file = std::path::Path::new("subdir").join("file2.txt");
        assert_eq!(
            files_subdir,
            vec![expected_subdir_file.to_str().unwrap().to_string()]
        );

        std::env::set_current_dir(original_dir).unwrap();
    }

    #[tokio::test]
    async fn test_fs_tool_security() {
        let tool = FileSystemTool;

        // Absolute path
        let args = json!({"operation": "read", "path": "/etc/passwd"});
        let result = tool.execute(args).await;
        assert!(result.is_err());
        if let Err(Error::Config(msg)) = result {
            assert!(msg.contains("Path must be relative"));
        } else {
            panic!("Expected Config error for absolute path");
        }

        // Path traversal
        let args = json!({"operation": "read", "path": "../some_file"});
        let result = tool.execute(args).await;
        assert!(result.is_err());
        if let Err(Error::Config(msg)) = result {
            assert!(msg.contains("Path must be relative"));
        } else {
            panic!("Expected Config error for path traversal");
        }
    }
}
