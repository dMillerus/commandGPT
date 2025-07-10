use anyhow::{Context, Result};
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

use crate::config::AppConfig;

#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessage,
}

#[derive(Debug, Deserialize)]
pub struct CommandResponse {
    pub command: String,
    pub explanation: String,
    pub auto_execute: bool,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ErrorDetail {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

pub struct OpenAIClient {
    client: Client,
    config: AppConfig,
}

impl OpenAIClient {
    pub fn new(config: &AppConfig) -> Self {
        let client = ClientBuilder::new()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .http2_prior_knowledge()
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            config: config.clone(),
        }
    }

    pub async fn send_chat(&self, messages: &[ChatMessage]) -> Result<CommandResponse> {
        let api_key = self.config.get_api_key()
            .context("Failed to get API key")?;

        let request = ChatRequest {
            model: self.config.openai_model.clone(),
            messages: messages.to_vec(),
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
        };

        let mut last_error = None;
        
        for attempt in 0..self.config.max_retries {
            if attempt > 0 {
                let delay = Duration::from_secs(2_u64.pow(attempt));
                log::debug!("Retrying request in {}s (attempt {})", delay.as_secs(), attempt + 1);
                sleep(delay).await;
            }

            match self.make_request(&api_key, &request).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    log::warn!("Request failed (attempt {}): {}", attempt + 1, e);
                    last_error = Some(e);
                    
                    // Don't retry for certain error types
                    if let Some(ref err_str) = last_error.as_ref().map(|e| e.to_string()) {
                        if err_str.contains("401") || err_str.contains("403") {
                            break; // Don't retry auth errors
                        }
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All retry attempts failed")))
    }

    async fn make_request(&self, api_key: &str, request: &ChatRequest) -> Result<CommandResponse> {
        let url = format!("{}/chat/completions", self.config.openai_base_url);
        
        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await
            .context("Failed to send request to OpenAI")?;

        let status = response.status();
        let response_text = response.text().await
            .context("Failed to read response body")?;

        if !status.is_success() {
            // Try to parse error response
            if let Ok(error_response) = serde_json::from_str::<ErrorResponse>(&response_text) {
                anyhow::bail!(
                    "OpenAI API error ({}): {} - {}", 
                    status, 
                    error_response.error.error_type,
                    error_response.error.message
                );
            } else {
                anyhow::bail!("HTTP error {}: {}", status, response_text);
            }
        }

        // Parse successful response
        let chat_response: ChatResponse = serde_json::from_str(&response_text)
            .context("Failed to parse OpenAI response")?;

        if chat_response.choices.is_empty() {
            anyhow::bail!("No choices returned from OpenAI");
        }

        let content = &chat_response.choices[0].message.content;
        log::debug!("Raw OpenAI response: {}", content);

        // Try to extract JSON from the response
        let json_content = self.extract_json(content)
            .context("Failed to extract JSON from response")?;

        let command_response: CommandResponse = serde_json::from_str(&json_content)
            .context("Failed to parse command response JSON")?;

        Ok(command_response)
    }

    fn extract_json(&self, content: &str) -> Result<String> {
        // First try parsing the content directly as JSON
        if let Ok(_) = serde_json::from_str::<serde_json::Value>(content) {
            return Ok(content.to_string());
        }

        // Look for JSON between code blocks
        if let Some(start) = content.find("```json") {
            if let Some(end) = content[start..].find("```") {
                let json_start = start + 7; // length of "```json"
                let json_end = start + end;
                if json_start < json_end {
                    let extracted = content[json_start..json_end].trim();
                    if serde_json::from_str::<serde_json::Value>(extracted).is_ok() {
                        return Ok(extracted.to_string());
                    }
                }
            }
        }

        // Look for JSON between plain code blocks
        if let Some(start) = content.find("```") {
            if let Some(end) = content[start + 3..].find("```") {
                let json_start = start + 3;
                let json_end = start + 3 + end;
                let extracted = content[json_start..json_end].trim();
                if serde_json::from_str::<serde_json::Value>(extracted).is_ok() {
                    return Ok(extracted.to_string());
                }
            }
        }

        // Look for JSON between braces
        if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                if start < end {
                    let extracted = &content[start..=end];
                    if serde_json::from_str::<serde_json::Value>(extracted).is_ok() {
                        return Ok(extracted.to_string());
                    }
                }
            }
        }

        anyhow::bail!("Could not extract valid JSON from response: {}", content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json() {
        let config = AppConfig::default();
        let client = OpenAIClient::new(&config);

        // Test direct JSON
        let direct_json = r#"{"command": "ls", "explanation": "test", "auto_execute": true}"#;
        assert!(client.extract_json(direct_json).is_ok());

        // Test JSON in code blocks
        let with_code_blocks = r#"
Here's the command:
```json
{"command": "ls", "explanation": "test", "auto_execute": true}
```
"#;
        assert!(client.extract_json(with_code_blocks).is_ok());

        // Test JSON with surrounding text
        let with_text = r#"
The command you need is: {"command": "ls", "explanation": "test", "auto_execute": true}
Hope this helps!
"#;
        assert!(client.extract_json(with_text).is_ok());
    }
}
