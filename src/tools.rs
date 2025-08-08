use crate::error::Result;
use crate::gemini_types::{FunctionCall, FunctionDeclaration};

/// A trait for any tool that can be executed by the agent.
pub trait Tool {
    /// Returns the name of the tool.
    fn name(&self) -> String;
    /// Returns the function declaration schema for the tool.
    fn schema(&self) -> FunctionDeclaration;
    /// Executes the tool with the given arguments.
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value>;
}

/// Manages the collection of available tools and executes them.
pub struct ToolExecutor {}

impl ToolExecutor {
    /// Creates a new ToolExecutor with a specific set of enabled tools.
    pub fn new(_enabled_tools: &[String]) -> Self {
        // TODO: Based on enabled_tools, instantiate and store the concrete tool structs.
        Self {}
    }

    /// Returns the list of `FunctionDeclaration` schemas for all enabled tools.
    pub fn schemas(&self) -> Vec<FunctionDeclaration> {
        // TODO: Collect and return schemas from all stored tool instances.
        vec![]
    }

    /// Executes a function call by finding the corresponding tool.
    pub async fn execute(&self, _call: &FunctionCall) -> Result<serde_json::Value> {
        // TODO: Find the tool by name and call its execute method.
        Ok(serde_json::json!({ "status": "success", "message": "Tool executed (stub)." }))
    }
}
