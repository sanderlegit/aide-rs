use crate::error::{Error, Result};
use dotenvy::dotenv;
use gemini_client_rs::types::{Content, GenerateContentResponse, PartResponse};
use reqwest::Client;
use std::env;
use tracing::{debug, error, info};

pub struct GeminiClientWrapper {
    client: Client,
    api_key: String,
    model_name: String,
    base_url: String,
}

impl GeminiClientWrapper {
    // Using gemini-2.5-flash for planning as it's fast and capable for generation.
    pub fn new_plan_agent() -> Result<Self> {
        Self::new("gemini-2.5-pro".to_string())
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

        let base_url = env::var("GEMINI_BASE_URL").unwrap_or_else(|_| {
            "https://generativelanguage.googleapis.com/v1beta/models".to_string()
        });

        let client = Client::new();

        Ok(Self {
            client,
            api_key,
            model_name,
            base_url,
        })
    }

    pub async fn generate_content<T: serde::Serialize>(
        &self,
        contents: Vec<Content>,
        tools: Option<T>,
    ) -> Result<GenerateContentResponse> {
        let request_body = serde_json::json!({
            "contents": contents,
            "tools": tools,
        });

        info!(
            "Sending request to Gemini model '{}' at '{}'.",
            self.model_name, self.base_url
        );
        debug!(request = %serde_json::to_string_pretty(&request_body).unwrap_or_else(|_| "Failed to format request body".to_string()), "Gemini request body");

        let url = format!(
            "{}/{}:generateContent?key={}",
            self.base_url, self.model_name, self.api_key
        );

        let response = self.client.post(&url).json(&request_body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            let err_msg = format!("API Error: {} - {}", status, text);
            // We'll wrap this in the existing GeminiError type for consistency,
            // even though we are not using the client directly.
            return Err(Error::Gemini(gemini_client_rs::GeminiError::Api(err_msg)));
        }

        let response_text = response.text().await?;
        debug!(response_text = %response_text, "Raw Gemini response body");

        let response: GenerateContentResponse = match serde_json::from_str(&response_text) {
            Ok(resp) => resp,
            Err(e) => {
                error!(
                    error = %e,
                    response_text = %response_text,
                    "Failed to deserialize Gemini response body."
                );
                return Err(e.into());
            }
        };
        debug!(response = %serde_json::to_string_pretty(&response).unwrap_or_else(|_| "Failed to format response body".to_string()), "Gemini response received");

        // Log text parts of response at info level for visibility
        if let Some(candidates) = &response.candidates {
            for candidate in candidates {
                for part in &candidate.content.parts {
                    if let PartResponse::Text(text) = part {
                        if !text.trim().is_empty() {
                            info!(
                                response_text = %format!("\n---\n{}\n---", text.trim()),
                                "Gemini response text part"
                            );
                        }
                    }
                }
            }
        }

        Ok(response)
    }
}
