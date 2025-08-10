use crate::error::{Error, Result};
use crate::gemini_types::{Content, GenerateContentRequest, GenerateContentResponse, Tool};
use crate::logging::{RequestLog, ResponseLog, RunLogger};
use dotenvy::dotenv;
use reqwest::Client;
use std::env;
use std::time::Instant;
use tracing::{error, info, trace};

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
        contents: &[Content],
        tools: Option<&Vec<Tool>>,
        model_override: Option<&str>,
    ) -> Result<GenerateContentResponse> {
        let start_time = Instant::now();
        let request_body = GenerateContentRequest {
            contents: contents.to_vec(),
            tools: tools.cloned(),
        };

        let model_to_use = model_override.unwrap_or(&self.model_name);

        self.logger.log_request(&RequestLog {
            model_name: model_to_use.to_string(),
            request: request_body.clone(),
        });

        info!(
            "Sending request to Gemini model '{}' at '{}'.",
            model_to_use, self.base_url
        );
        trace!(request = %serde_json::to_string_pretty(&request_body).unwrap_or_else(|_| "Failed to format request body".to_string()), "Gemini request body");

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url, model_to_use, self.api_key
        );

        trace!(url = %url, "Requesting URL");
        let response = self.client.post(&url).json(&request_body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            let err_msg = format!("API Error: {} - {}", status, text);
            return Err(Error::ApiError(err_msg));
        }

        let response_text = response.text().await?;
        trace!(response_text = %response_text, "Raw Gemini response body");

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
        trace!(response = %serde_json::to_string_pretty(&response).unwrap_or_else(|_| "Failed to format response body".to_string()), "Gemini response received");

        let time_taken = start_time.elapsed();
        self.logger.log_response(ResponseLog {
            model_name: model_to_use.to_string(),
            response: response.clone(),
            time_taken_ms: time_taken.as_millis(),
        });

        Ok(response)
    }
}
