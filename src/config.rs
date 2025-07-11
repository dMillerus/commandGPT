use anyhow::{Context, Result};
use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

const KEYCHAIN_SERVICE: &str = "commandgpt";
const KEYCHAIN_ACCOUNT: &str = "openai";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub openai_model: String,
    pub openai_base_url: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub timeout_seconds: u64,
    pub max_retries: u32,
    pub config_dir: PathBuf,
    pub context_dir: PathBuf,
    pub history_path: PathBuf,
    pub system_prompt_path: PathBuf,
}

impl Default for AppConfig {
    fn default() -> Self {
        let config_dir = dirs_next::home_dir()
            .expect("Could not find home directory")
            .join(".commandgpt");
        
        Self {
            openai_model: "gpt-3.5-turbo".to_string(),
            openai_base_url: "https://api.openai.com/v1".to_string(),
            max_tokens: 500,
            temperature: 0.1,
            timeout_seconds: 30,
            max_retries: 3,
            context_dir: config_dir.join("context"),
            history_path: config_dir.join("history.db"),
            system_prompt_path: config_dir.join("system.md"),
            config_dir,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let mut config = Self::default();
        
        // Allow environment variable override for model
        if let Ok(model) = env::var("OPENAI_MODEL") {
            config.openai_model = model;
        } else if let Ok(model) = env::var("COMMANDGPT_MODEL") {
            config.openai_model = model;
        }
        
        // Ensure config directory exists
        fs::create_dir_all(&config.config_dir)
            .context("Failed to create config directory")?;
        fs::create_dir_all(&config.context_dir)
            .context("Failed to create context directory")?;

        // Create default system prompt if it doesn't exist
        if !config.system_prompt_path.exists() {
            fs::write(&config.system_prompt_path, Self::default_system_prompt())
                .context("Failed to create default system prompt")?;
        }

        Ok(config)
    }

    pub fn get_api_key(&self) -> Result<String> {
        // Check environment variable first
        if let Ok(key) = env::var("OPENAI_API_KEY") {
            return Ok(key);
        }

        // Fall back to Keychain
        match get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
            Ok(password) => {
                String::from_utf8(password)
                    .context("Invalid UTF-8 in stored API key")
            }
            Err(_) => {
                anyhow::bail!(
                    "No API key found. Set it with 'commandgpt config set-key' or set OPENAI_API_KEY environment variable"
                );
            }
        }
    }

    fn default_system_prompt() -> &'static str {
        r#"You are a helpful command-line assistant that generates shell commands for macOS/zsh.

## Rules:
1. Always respond with valid JSON in this exact format:
   {
     "command": "string",
     "explanation": "string", 
     "auto_execute": boolean
   }

2. Generate commands that are:
   - Safe and non-destructive by default
   - Compatible with zsh on macOS
   - Use commonly available tools (prefer built-in commands)

3. Set auto_execute to true only for:
   - Read-only operations (ls, find, cat, grep, etc.)
   - Safe informational commands
   - Commands that don't modify system state

4. Set auto_execute to false for:
   - Any write operations
   - System modifications
   - Network operations
   - Package installations

5. Keep explanations concise but helpful.

6. If the request is unclear or potentially dangerous, ask for clarification in the explanation and provide a safe alternative command.

7. Use absolute paths when possible to avoid ambiguity.

## Examples:
User: "show me large files"
Response: {"command": "find ~ -type f -size +100M -exec ls -lh {} +", "explanation": "Find files larger than 100MB in home directory", "auto_execute": true}

User: "install node"
Response: {"command": "brew install node", "explanation": "Install Node.js using Homebrew (requires confirmation)", "auto_execute": false}
"#
    }

    pub fn ensure_directories(&self) -> Result<()> {
        // Ensure config directory exists
        fs::create_dir_all(&self.config_dir)
            .context("Failed to create config directory")?;
        // Ensure context directory exists
        fs::create_dir_all(&self.context_dir)
            .context("Failed to create context directory")?;
        Ok(())
    }

    pub fn is_valid_api_key(key: &str) -> bool {
        // Basic validation - OpenAI keys start with 'sk-' and are at least 40 characters long
        key.starts_with("sk-") && key.len() >= 40 && key.len() <= 60
    }

    pub fn create_default_system_prompt(&self) -> Result<()> {
        if !self.system_prompt_path.exists() {
            fs::write(&self.system_prompt_path, Self::default_system_prompt())
                .context("Failed to create default system prompt")?;
        }
        Ok(())
    }
}

pub async fn set_api_key() -> Result<()> {
    use std::io::{self, Write};

    print!("Enter your OpenAI API key: ");
    io::stdout().flush()?;
    
    let api_key = rpassword::read_password()
        .context("Failed to read API key. Make sure you're running in a proper terminal.")?;
    
    let trimmed_key = api_key.trim();
    
    if trimmed_key.is_empty() {
        anyhow::bail!("API key cannot be empty. Please try again.");
    }
    
    // Basic validation - OpenAI keys typically start with 'sk-'
    if !trimmed_key.starts_with("sk-") {
        eprintln!("⚠️  Warning: OpenAI API keys typically start with 'sk-'. Please verify your key is correct.");
    }
    
    // Validate key length (OpenAI keys are usually around 51 characters)
    if trimmed_key.len() < 20 {
        anyhow::bail!("API key appears to be too short. OpenAI keys are typically 51+ characters.");
    }

    set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, trimmed_key.as_bytes())
        .context("Failed to store API key in Keychain. You may need to grant permission when prompted.")?;

    Ok(())
}

pub async fn delete_api_key() -> Result<()> {
    delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .context("Failed to delete API key from Keychain")?;
    Ok(())
}

pub async fn show_config(config: &AppConfig) -> Result<()> {
    println!("📋 Configuration:");
    println!("  OpenAI Model: {}", config.openai_model);
    println!("  Base URL: {}", config.openai_base_url);
    println!("  Max Tokens: {}", config.max_tokens);
    println!("  Temperature: {}", config.temperature);
    println!("  Timeout: {}s", config.timeout_seconds);
    println!("  Config Dir: {}", config.config_dir.display());
    
    // Check API key status
    match config.get_api_key() {
        Ok(_) => println!("  API Key: ✅ Configured"),
        Err(_) => println!("  API Key: ❌ Not configured"),
    }
    
    // Check system prompt
    if config.system_prompt_path.exists() {
        println!("  System Prompt: ✅ {}", config.system_prompt_path.display());
    } else {
        println!("  System Prompt: ❌ Not found");
    }
    
    // Check context files
    if config.context_dir.exists() {
        let context_files = fs::read_dir(&config.context_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.path().extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext == "md")
                    .unwrap_or(false)
            })
            .count();
        println!("  Context Files: {} files in {}", context_files, config.context_dir.display());
    } else {
        println!("  Context Files: ❌ Directory not found");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        
        assert_eq!(config.openai_model, "gpt-3.5-turbo");
        assert_eq!(config.openai_base_url, "https://api.openai.com/v1");
        assert_eq!(config.max_tokens, 500);
        assert_eq!(config.temperature, 0.1);
        assert_eq!(config.timeout_seconds, 30);
        assert_eq!(config.max_retries, 3);
        
        // Check that paths are correctly set
        assert!(config.config_dir.ends_with(".commandgpt"));
        assert!(config.context_dir.ends_with("context"));
        assert!(config.history_path.ends_with("history.db"));
        assert!(config.system_prompt_path.ends_with("system.md"));
    }

    #[test]
    fn test_config_load() {
        // Ensure no environment override affects this test
        std::env::remove_var("COMMANDGPT_MODEL");
        
        // Test loading default config when no config file exists
        let config = AppConfig::load().unwrap();
        assert_eq!(config.openai_model, "gpt-3.5-turbo");
    }

    #[test]
    fn test_config_directory_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().join(".commandgpt");
        
        let mut config = AppConfig::default();
        config.config_dir = config_dir.clone();
        config.context_dir = config_dir.join("context");
        config.history_path = config_dir.join("history.db");
        config.system_prompt_path = config_dir.join("system.md");
        
        // Ensure directories are created
        assert!(config.ensure_directories().is_ok());
        assert!(config.config_dir.exists());
        assert!(config.context_dir.exists());
    }

    #[test]
    fn test_environment_variable_override() {
        // Clean up first to ensure test isolation
        std::env::remove_var("COMMANDGPT_MODEL");
        
        // Set environment variable
        std::env::set_var("COMMANDGPT_MODEL", "gpt-4");
        
        let config = AppConfig::load().unwrap();
        assert_eq!(config.openai_model, "gpt-4");
        
        // Clean up
        std::env::remove_var("COMMANDGPT_MODEL");
    }

    #[test]
    fn test_api_key_validation() {
        // Test invalid API key formats
        assert!(!AppConfig::is_valid_api_key(""));
        assert!(!AppConfig::is_valid_api_key("invalid"));
        assert!(!AppConfig::is_valid_api_key("sk-short"));
        
        // Test valid API key format
        assert!(AppConfig::is_valid_api_key("sk-1234567890abcdef1234567890abcdef12345678"));
    }

    #[tokio::test]
    async fn test_keychain_operations() {
        // Note: These tests might fail in CI environments without keychain access
        // They are primarily for local development testing
        
        // Create a config instance to test instance methods
        let config = AppConfig::default();
        
        // Test that we can attempt to get an API key (may fail, which is expected)
        let _ = config.get_api_key(); // This may error, which is fine for testing
        
        // Note: set_api_key and delete_api_key are async functions that require user interaction
        // We can't easily test them in unit tests without mocking the keychain
    }

    #[test]
    fn test_config_paths() {
        let config = AppConfig::default();
        
        // All paths should be absolute
        assert!(config.config_dir.is_absolute());
        assert!(config.context_dir.is_absolute());
        assert!(config.history_path.is_absolute());
        assert!(config.system_prompt_path.is_absolute());
        
        // Context dir should be a subdirectory of config dir
        assert!(config.context_dir.starts_with(&config.config_dir));
        
        // Files should be in config dir
        assert!(config.history_path.starts_with(&config.config_dir));
        assert!(config.system_prompt_path.starts_with(&config.config_dir));
    }

    #[test]
    fn test_config_validation() {
        let mut config = AppConfig::default();
        
        // Test invalid timeout
        config.timeout_seconds = 0;
        // Should not panic or error, but might be handled in validation logic
        
        // Test invalid temperature
        config.temperature = -1.0;
        // Should not panic or error, but might be handled in validation logic
        
        // Test invalid max_tokens
        config.max_tokens = 0;
        // Should not panic or error, but might be handled in validation logic
    }

    #[test]
    fn test_system_prompt_creation() {
        let temp_dir = TempDir::new().unwrap();
        let system_prompt_path = temp_dir.path().join("system.md");
        
        let mut config = AppConfig::default();
        config.system_prompt_path = system_prompt_path.clone();
        
        // Create default system prompt
        assert!(config.create_default_system_prompt().is_ok());
        assert!(system_prompt_path.exists());
        
        let content = std::fs::read_to_string(&system_prompt_path).unwrap();
        assert!(content.contains("You are a helpful command-line assistant"));
    }

    #[test]
    fn test_config_serialization() {
        let config = AppConfig::default();
        
        // Test that config can be serialized to JSON
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("gpt-3.5-turbo"));
        
        // Test deserialization
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.openai_model, config.openai_model);
        assert_eq!(deserialized.max_tokens, config.max_tokens);
    }
}
