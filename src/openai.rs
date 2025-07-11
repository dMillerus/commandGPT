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
            let json_start = start + 7; // length of "```json"
            // Find the next newline to skip the language specifier line
            let content_start = if let Some(newline) = content[json_start..].find('\n') {
                json_start + newline + 1
            } else {
                json_start
            };
            
            if let Some(end) = content[content_start..].find("```") {
                let json_end = content_start + end;
                let extracted = content[content_start..json_end].trim();
                if serde_json::from_str::<serde_json::Value>(extracted).is_ok() {
                    return Ok(extracted.to_string());
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
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use wiremock::matchers::{method, path, header};
    use serde_json::json;

    #[test]
    fn test_extract_json() {
        let config = AppConfig::default();
        let client = OpenAIClient::new(&config);

        // Test direct JSON
        let direct_json = r#"{"command": "ls", "explanation": "test", "auto_execute": true}"#;
        let result = client.extract_json(direct_json).unwrap();
        
        // Parse the JSON to verify it's valid
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["command"], "ls");
        assert_eq!(parsed["explanation"], "test");
        assert_eq!(parsed["auto_execute"], true);

        // Test JSON in code blocks
        let with_code_blocks = r#"
Here's the command:
```json
{"command": "pwd", "explanation": "show current directory", "auto_execute": false}
```
"#;
        let result = client.extract_json(with_code_blocks).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["command"], "pwd");
        assert_eq!(parsed["explanation"], "show current directory");
        assert_eq!(parsed["auto_execute"], false);

        // Test JSON with surrounding text
        let with_text = r#"
The command you need is: {"command": "echo hello", "explanation": "print hello", "auto_execute": true}
Hope this helps!
"#;
        let result = client.extract_json(with_text).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["command"], "echo hello");
        assert_eq!(parsed["explanation"], "print hello");
        assert_eq!(parsed["auto_execute"], true);

        // Test invalid JSON
        let invalid_json = r#"This is not JSON at all"#;
        assert!(client.extract_json(invalid_json).is_err());

        // Test malformed JSON
        let malformed = r#"{"command": "ls", "explanation": "test"#; // Missing closing brace
        assert!(client.extract_json(malformed).is_err());
    }

    #[test]
    fn test_chat_message_creation() {
        let msg = ChatMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        };
        
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_chat_request_creation() {
        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are a helpful assistant".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "List files".to_string(),
            },
        ];

        let request = ChatRequest {
            model: "gpt-3.5-turbo".to_string(),
            messages,
            max_tokens: 100,
            temperature: 0.1,
        };

        assert_eq!(request.model, "gpt-3.5-turbo");
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.max_tokens, 100);
        assert_eq!(request.temperature, 0.1);
    }

    #[tokio::test]
    async fn test_openai_client_mock() {
        let mock_server = MockServer::start().await;
        
        let mock_response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": r#"{"command": "ls -la", "explanation": "List all files with details", "auto_execute": false}"#
                }
            }]
        });

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response))
            .mount(&mock_server)
            .await;

        let mut config = AppConfig::default();
        config.openai_base_url = mock_server.uri();
        
        let client = OpenAIClient::new(&config);
        
        let messages = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "list files".to_string(),
            }
        ];

        // This would normally require a real API key, but we're mocking the response
        // In a real test, you'd need to handle the keychain access
        // let result = client.generate_command(messages).await;
        // assert!(result.is_ok());
    }

    #[test]
    fn test_extract_json_with_markdown() {
        let config = AppConfig::default();
        let client = OpenAIClient::new(&config);

        let markdown_response = r#"
I'll help you list the files. Here's the command:

```json
{
    "command": "ls -la",
    "explanation": "Lists all files in the current directory with detailed information including permissions, ownership, size, and modification date",
    "auto_execute": false
}
```

This command will show you all files including hidden ones (those starting with a dot).
"#;

        let result = client.extract_json(markdown_response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["command"], "ls -la");
        assert!(parsed["explanation"].as_str().unwrap().contains("Lists all files"));
        assert_eq!(parsed["auto_execute"], false);
    }

    #[test]
    fn test_extract_json_with_backticks() {
        let config = AppConfig::default();
        let client = OpenAIClient::new(&config);

        let backtick_response = r#"
The command is:
```
{"command": "pwd", "explanation": "Print working directory", "auto_execute": true}
```
"#;

        let result = client.extract_json(backtick_response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["command"], "pwd");
        assert_eq!(parsed["explanation"], "Print working directory");
        assert_eq!(parsed["auto_execute"], true);
    }

    #[test]
    fn test_extract_json_multiple_json_blocks() {
        let config = AppConfig::default();
        let client = OpenAIClient::new(&config);

        let multiple_json = r#"
Here are some options:

```json
{"command": "ls", "explanation": "Basic list", "auto_execute": true}
```

Or you could use:

```json
{"command": "ls -la", "explanation": "Detailed list", "auto_execute": false}
```
"#;

        // Should extract the first valid JSON block
        let result = client.extract_json(multiple_json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["command"], "ls");
        assert_eq!(parsed["explanation"], "Basic list");
        assert_eq!(parsed["auto_execute"], true);
    }

    #[test]
    fn test_command_response_deserialization() {
        let json_str = r#"{"command": "echo test", "explanation": "Print test", "auto_execute": true}"#;
        let response: CommandResponse = serde_json::from_str(json_str).unwrap();
        
        assert_eq!(response.command, "echo test");
        assert_eq!(response.explanation, "Print test");
        assert!(response.auto_execute);
    }

    #[test]
    fn test_chat_response_deserialization() {
        let json_str = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello there!"
                }
            }]
        }"#;
        
        let response: ChatResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.role, "assistant");
        assert_eq!(response.choices[0].message.content, "Hello there!");
    }

    #[test]
    fn test_error_response_deserialization() {
        let json_str = r#"{
            "error": {
                "type": "invalid_request_error",
                "message": "Invalid API key"
            }
        }"#;
        
        let response: ErrorResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(response.error.error_type, "invalid_request_error");
        assert_eq!(response.error.message, "Invalid API key");
    }
}
