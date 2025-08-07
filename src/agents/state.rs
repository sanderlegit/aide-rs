use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Input for the PlanAgent, deserialized from a TOML file.
#[derive(Serialize, Deserialize, Debug)]
pub struct PlanPrompt {
    pub objective: String,
    pub file_scoping: FileScope,
    pub coding_conventions: String,
    pub formatter_command: Option<String>,
    pub validation_commands: Vec<ValidationStep>,
}

// Defines file include/exclude rules.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct FileScope {
    pub include: Vec<String>, // Glob patterns
    pub exclude: Vec<String>, // Glob patterns
}

// The primary state file for a workflow.
#[derive(Serialize, Deserialize, Debug)]
pub struct ImplementationPlan {
    pub original_prompt: PlanPrompt,
    pub tasks: Vec<Task>,
}

// A single, executable task within a plan.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Task {
    pub description: String,
    pub file_scoping: FileScope,
    pub validation_steps: Vec<ValidationStep>,
    pub status: TaskStatus,
    pub attempts: u32,
    pub result: Option<TaskResult>,
}

// The outcome of a successfully completed task.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TaskResult {
    pub success: bool,
    pub agent_tips: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Pending,
    Success,
    Failed,
}

// A command to be run for validation.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ValidationStep {
    pub command: String,
    pub expected_exit_code: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_prompt_toml_serialization() {
        let prompt = PlanPrompt {
            objective: "Test objective".to_string(),
            file_scoping: FileScope {
                include: vec!["src/**/*.rs".to_string()],
                exclude: vec!["src/main.rs".to_string()],
            },
            coding_conventions: "Use snake_case".to_string(),
            formatter_command: Some("cargo fmt".to_string()),
            validation_commands: vec![ValidationStep {
                command: "cargo check".to_string(),
                expected_exit_code: 0,
            }],
        };

        let toml_string = toml::to_string(&prompt).unwrap();
        let deserialized: PlanPrompt = toml::from_str(&toml_string).unwrap();

        assert_eq!(deserialized.objective, prompt.objective);
        assert_eq!(
            deserialized.file_scoping.include,
            prompt.file_scoping.include
        );
    }

    #[test]
    fn test_implementation_plan_json_serialization() {
        let plan = ImplementationPlan {
            original_prompt: PlanPrompt {
                objective: "Test objective".to_string(),
                file_scoping: FileScope::default(),
                coding_conventions: "None".to_string(),
                formatter_command: None,
                validation_commands: vec![],
            },
            tasks: vec![Task {
                description: "Do a thing".to_string(),
                file_scoping: FileScope {
                    include: vec!["src/lib.rs".to_string()],
                    exclude: vec![],
                },
                validation_steps: vec![ValidationStep {
                    command: "cargo test".to_string(),
                    expected_exit_code: 0,
                }],
                status: TaskStatus::Pending,
                attempts: 0,
                result: None,
            }],
        };

        let json_string = serde_json::to_string_pretty(&plan).unwrap();
        let deserialized: ImplementationPlan = serde_json::from_str(&json_string).unwrap();

        assert_eq!(deserialized.tasks.len(), 1);
        assert_eq!(deserialized.tasks[0].description, "Do a thing");
        assert_eq!(deserialized.tasks[0].status, TaskStatus::Pending);
    }
}
