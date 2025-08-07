use crate::error::{Error, Result};
use dotenvy::dotenv;
use gemini_client_rs::{
    types::{
        Content, FunctionDeclaration, GenerateContentRequest, GenerateContentResponse, ToolConfig,
        ToolConfigFunctionDeclaration,
    },
    GeminiClient,
};
use std::env;
use tracing::info;

pub struct GeminiClientWrapper {
    client: GeminiClient,
    model_name: String,
}

impl GeminiClientWrapper {
    // Using gemini-1.5-flash for planning as it's fast and capable for generation.
    pub fn new_plan_agent() -> Result<Self> {
        Self::new("gemini-1.5-flash".to_string())
    }

    // Using gemini-1.5-pro for implementation as it's more powerful for complex reasoning.
    pub fn new_impl_agent() -> Result<Self> {
        Self::new("gemini-1.5-pro".to_string())
    }

    fn new(model_name: String) -> Result<Self> {
        dotenv().ok();
        let api_key = env::var("GEMINI_API_KEY")
            .map_err(|_| Error::Config("GEMINI_API_KEY must be set".to_string()))?;

        let client = GeminiClient::new(api_key);

        Ok(Self { client, model_name })
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

        info!("Sending request to Gemini model '{}'.", self.model_name);

        let response = self
            .client
            .generate_content(&self.model_name, &request)
            .await?;

        Ok(response)
    }
}
