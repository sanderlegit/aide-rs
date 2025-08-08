use crate::error::{Error, Result};
use crate::gemini_types::{Content, GenerateContentRequest, GenerateContentResponse, Tool};
use crate::logging::{ResponseLog, RunLogger};
use dotenvy::dotenv;
use reqwest::Client;
use std::env;
use std::time::Instant;
use tracing::{debug, error, info};

pub struct GeminiClientWrapper {
    client: Client,
    api_key: String,
    model_name: String,
    base_url: String,
    logger: RunLogger,
}

impl GeminiClientWrapper {
    pub fn new(model_name: String, logger: RunLogger) -> Result<Self> {
        dotenv().ok();
        let api_key = env::var("GEMINI_API_KEY")
            .map_err(|_| Error::Config("GEMINI_API_KEY must be set".to_string()))?;

        let base_url = env::var("GEMINI_BASE_URL")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com".to_string());

        let client = Client::new();

        Ok(Self {
            client,
            api_key,
            model_name,
            base_url,
            logger,
        })
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    pub async fn generate_content(
        &self,
        contents: Vec<Content>,
        tools: Option<Vec<Tool>>,
    ) -> Result<GenerateContentResponse> {
        let start_time = Instant::now();
        let request_body = GenerateContentRequest {
            contents,
            tools,
        };

        info!(
            "Sending request to Gemini model '{}' at '{}'.",
            self.model_name, self.base_url
        );
        debug!(request = %serde_json::to_string_pretty(&request_body).unwrap_or_else(|_| "Failed to format request body".to_string()), "Gemini request body");

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url, self.model_name, self.api_key
        );

        let response = self.client.post(&url).json(&request_body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            let err_msg = format!("API Error: {} - {}", status, text);
            return Err(Error::ApiError(err_msg));
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

        let time_taken = start_time.elapsed();
        self.logger.log_response(ResponseLog {
            model_name: self.model_name.clone(),
            response: response.clone(),
            time_taken_ms: time_taken.as_millis(),
        });

        // Log text parts of response at info level for visibility
        if let Some(candidates) = &response.candidates {
            for candidate in candidates {
                for part in &candidate.content.parts {
                    if let Some(text) = &part.text {
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
