use anyhow::{Context, Result};
use std::fs;
use tokio::fs as async_fs;

use crate::config::AppConfig;
use crate::history::HistoryEntry;
use crate::openai::ChatMessage;

pub struct ContextBuilder {
    config: AppConfig,
}

impl ContextBuilder {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    pub async fn build_payload(
        &self,
        user_message: &str,
        last_entry: Option<&HistoryEntry>,
    ) -> Result<Vec<ChatMessage>> {
        let mut messages = Vec::new();

        // Start with system message
        let system_content = self.build_system_message().await?;
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: system_content,
        });

        // Add last command context if available
        if let Some(entry) = last_entry {
            let context_message = self.format_last_command_context(entry);
            messages.push(ChatMessage {
                role: "user".to_string(),
                content: context_message,
            });
        }

        // Add current user message
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: user_message.to_string(),
        });

        Ok(messages)
    }

    async fn build_system_message(&self) -> Result<String> {
        let mut content = String::new();

        // Load system prompt
        if self.config.system_prompt_path.exists() {
            let system_prompt = async_fs::read_to_string(&self.config.system_prompt_path).await
                .context("Failed to read system prompt")?;
            content.push_str(&system_prompt);
            content.push_str("\n\n");
        }

        // Load context files
        if self.config.context_dir.exists() {
            let context_content = self.load_context_files().await?;
            if !context_content.is_empty() {
                content.push_str("## Additional Context:\n");
                content.push_str(&context_content);
                content.push_str("\n\n");
            }
        }

        // Add current environment info
        content.push_str(&self.build_environment_context());

        Ok(content)
    }

    async fn load_context_files(&self) -> Result<String> {
        let mut context_content = String::new();

        if !self.config.context_dir.exists() {
            return Ok(context_content);
        }

        let mut entries = async_fs::read_dir(&self.config.context_dir).await
            .context("Failed to read context directory")?;

        let mut files = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                files.push(path);
            }
        }

        // Sort files for consistent ordering
        files.sort();

        for file_path in files {
            if let Ok(content) = async_fs::read_to_string(&file_path).await {
                let filename = file_path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                
                context_content.push_str(&format!("### {} content:\n", filename));
                context_content.push_str(&content);
                context_content.push_str("\n\n");
            }
        }

        Ok(context_content)
    }

    fn build_environment_context(&self) -> String {
        let mut context = String::new();
        
        context.push_str("## Current Environment:\n");
        
        // Current working directory
        if let Ok(cwd) = std::env::current_dir() {
            context.push_str(&format!("- Working Directory: {}\n", cwd.display()));
        }

        // Current user
        if let Ok(user) = std::env::var("USER") {
            context.push_str(&format!("- User: {}\n", user));
        }

        // Shell info
        if let Ok(shell) = std::env::var("SHELL") {
            context.push_str(&format!("- Shell: {}\n", shell));
        }

        // Home directory
        if let Some(home) = dirs_next::home_dir() {
            context.push_str(&format!("- Home: {}\n", home.display()));
        }

        // System info
        context.push_str("- OS: macOS\n");
        context.push_str("- Architecture: Apple Silicon (ARM64)\n");

        context.push_str("\n");
        context
    }

    fn format_last_command_context(&self, entry: &HistoryEntry) -> String {
        let mut context = String::new();
        
        context.push_str("## Previous Command Context:\n");
        context.push_str(&format!("Last command executed: `{}`\n", entry.command));
        
        if !entry.stdout.is_empty() {
            context.push_str(&format!("Output:\n```\n{}\n```\n", 
                self.truncate_output(&entry.stdout, 512)));
        }
        
        if !entry.stderr.is_empty() {
            context.push_str(&format!("Errors:\n```\n{}\n```\n", 
                self.truncate_output(&entry.stderr, 256)));
        }
        
        context.push_str(&format!("Exit code: {}\n", entry.exit_code));
        
        context
    }

    fn truncate_output(&self, output: &str, max_chars: usize) -> String {
        if output.len() <= max_chars {
            output.to_string()
        } else {
            let truncated = &output[..max_chars];
            format!("{}... (truncated)", truncated)
        }
    }

    pub async fn create_default_context_files(&self) -> Result<()> {
        fs::create_dir_all(&self.config.context_dir)
            .context("Failed to create context directory")?;

        // Create a sample context file
        let sample_context = r#"# Development Environment

This is a sample context file. You can add information about:

- Current project details
- Preferred tools and workflows  
- Custom aliases or functions
- Environment-specific configurations
- Common tasks and procedures

The assistant will include this context when generating commands.

## Common Tools
- Homebrew for package management
- Git for version control
- VS Code as primary editor
- Docker for containerization

## Preferences
- Prefer single-line commands when possible
- Use long-form flags for clarity
- Include safety checks for destructive operations
"#;

        let sample_path = self.config.context_dir.join("sample.md");
        if !sample_path.exists() {
            fs::write(sample_path, sample_context)
                .context("Failed to create sample context file")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_build_environment_context() {
        let config = AppConfig::default();
        let builder = ContextBuilder::new(&config);
        
        let context = builder.build_environment_context();
        assert!(context.contains("Current Environment"));
        assert!(context.contains("macOS"));
        assert!(context.contains("Apple Silicon"));
    }

    #[tokio::test]
    async fn test_truncate_output() {
        let config = AppConfig::default();
        let builder = ContextBuilder::new(&config);
        
        let short_output = "short output";
        assert_eq!(builder.truncate_output(short_output, 100), short_output);
        
        let long_output = "a".repeat(1000);
        let truncated = builder.truncate_output(&long_output, 50);
        assert!(truncated.len() <= 50 + 15); // 15 for "... (truncated)"
        assert!(truncated.ends_with("... (truncated)"));
    }
}
