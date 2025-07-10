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
            openai_model: "gpt-4".to_string(),
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
        let config = Self::default();
        
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
        r#"You are a helpful assistant that generates shell commands for macOS/zsh.

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
}

pub async fn set_api_key() -> Result<()> {
    use std::io::{self, Write};

    print!("Enter your OpenAI API key: ");
    io::stdout().flush()?;
    
    let api_key = rpassword::read_password()
        .context("Failed to read API key")?;
    
    if api_key.trim().is_empty() {
        anyhow::bail!("API key cannot be empty");
    }

    set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, api_key.trim().as_bytes())
        .context("Failed to store API key in Keychain")?;

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
