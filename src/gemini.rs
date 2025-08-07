use crate::error::{Error, Result};
use dotenvy::dotenv;
use gemini_client_rs::types::{
    Content, FunctionDeclaration, GenerateContentRequest, GenerateContentResponse, ToolConfig,
    ToolConfigFunctionDeclaration,
};
use reqwest::Client;
use std::env;
use tracing::info;

pub struct GeminiClientWrapper {
    client: Client,
    api_key: String,
    model_name: String,
    base_url: String,
}

impl GeminiClientWrapper {
    // Using gemini-2.5-flash for planning as it's fast and capable for generation.
    pub fn new_plan_agent() -> Result<Self> {
        Self::new("gemini-2.5-flash".to_string())
    }

    // Using gemini-2.5-flash for summarization as it's fast and cheap.
    pub fn new_summarize_agent() -> Result<Self> {
        Self::new("gemini-2.5-flash".to_string())
    }

    // Using gemini-2.5-pro for implementation as it's more powerful for complex reasoning.
    pub fn new_impl_agent() -> Result<Self> {
        Self::new("gemini-2.5-pro".to_string())
    }

    fn new(model_name: String) -> Result<Self> {
        dotenv().ok();
        let api_key = env::var("GEMINI_API_KEY")
            .map_err(|_| Error::Config("GEMINI_API_KEY must be set".to_string()))?;

        let base_url = env::var("GEMINI_BASE_URL")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta/models".to_string());

        let client = Client::new();

        Ok(Self {
            client,
            api_key,
            model_name,
            base_url,
        })
    }

    pub async fn generate_content(
        &self,
        contents: Vec<Content>,
        tools: Option<Vec<FunctionDeclaration>>,
    ) -> Result<GenerateContentResponse> {
        let tool_config = tools.map(|function_declarations| {
            vec![ToolConfig::FunctionDeclaration(
                ToolConfigFunctionDeclaration {
                    function_declarations,
                },
            )]
        });

        let request = GenerateContentRequest {
            contents,
            tools: tool_config,
        };

        info!(
            "Sending request to Gemini model '{}' at '{}'.",
            self.model_name, self.base_url
        );

        let url = format!(
            "{}/{}:generateContent?key={}",
            self.base_url, self.model_name, self.api_key
        );

        let response = self.client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            let err_msg = format!("API Error: {} - {}", status, text);
            // We'll wrap this in the existing GeminiError type for consistency,
            // even though we are not using the client directly.
            return Err(Error::Gemini(gemini_client_rs::GeminiError::Api(
                err_msg,
            )));
        }

        Ok(response.json().await?)
    }
}
