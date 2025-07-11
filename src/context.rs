use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
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

        let files = self.collect_context_files(&self.config.context_dir).await?;

        // Sort files for consistent ordering
        let mut sorted_files = files;
        sorted_files.sort();

        for file_path in sorted_files {
            if let Ok(content) = async_fs::read_to_string(&file_path).await {
                let filename = file_path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                
                context_content.push_str(&format!("### {} content:\n", filename));
                // Truncate large files to prevent context overflow
                let truncated_content = self.truncate_output(&content, 2048);
                context_content.push_str(&truncated_content);
                context_content.push_str("\n\n");
            }
        }

        Ok(context_content)
    }

    async fn collect_context_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        
        let mut entries = async_fs::read_dir(dir).await
            .context("Failed to read context directory")?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                // Recursively collect files from subdirectories using Box::pin
                let subfiles_future = Box::pin(self.collect_context_files(&path));
                let mut subfiles = subfiles_future.await?;
                files.append(&mut subfiles);
            } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                // Include both .md and .markdown files
                if ext == "md" || ext == "markdown" {
                    files.push(path);
                }
            }
        }

        Ok(files)
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
        let sample_context = r#"# CommandGPT Context

This is a sample context file. You can add information about:

- Current project details
- Preferred tools and workflows  
- Custom aliases or functions
- Environment-specific configurations
- Common tasks and procedures

The assistant will include this context when generating commands.

## System Information
- Operating System: macOS
- Shell: zsh
- Package Manager: Homebrew

## Preferences
- Editor: VS Code
- Version Control: Git
- Containerization: Docker

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
    use std::fs;

    #[tokio::test]
    async fn test_build_environment_context() {
        let config = AppConfig::default();
        let builder = ContextBuilder::new(&config);
        
        let context = builder.build_environment_context();
        assert!(context.contains("Current Environment"));
        assert!(context.contains("macOS"));
        assert!(context.contains("Apple Silicon"));
        assert!(context.contains("Shell:"));
        assert!(context.contains("Working Directory:"));
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
        
        // Test empty output
        assert_eq!(builder.truncate_output("", 100), "");
        
        // Test exact limit
        let exact_limit = "a".repeat(50);
        assert_eq!(builder.truncate_output(&exact_limit, 50), exact_limit);
    }

    #[tokio::test]
    async fn test_build_context() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = AppConfig::default();
        config.context_dir = temp_dir.path().join("context");
        fs::create_dir_all(&config.context_dir).unwrap();
        
        // Create test context files
        let test_file1 = config.context_dir.join("test1.md");
        fs::write(&test_file1, "# Test Context 1\nThis is test content 1").unwrap();
        
        let test_file2 = config.context_dir.join("test2.md");
        fs::write(&test_file2, "# Test Context 2\nThis is test content 2").unwrap();
        
        // Create non-markdown file (should be ignored)
        let non_md_file = config.context_dir.join("test.txt");
        fs::write(&non_md_file, "This should be ignored").unwrap();
        
        let builder = ContextBuilder::new(&config);
        let context = builder.build_system_message().await.unwrap();
        
        assert!(context.contains("Current Environment"));
        assert!(context.contains("Test Context 1"));
        assert!(context.contains("Test Context 2"));
        assert!(context.contains("test content 1"));
        assert!(context.contains("test content 2"));
        assert!(!context.contains("This should be ignored"));
    }

    #[tokio::test]
    async fn test_load_context_files() {
        let temp_dir = TempDir::new().unwrap();
        let context_dir = temp_dir.path().join("context");
        fs::create_dir_all(&context_dir).unwrap();
        
        // Create test files
        let file1 = context_dir.join("file1.md");
        fs::write(&file1, "Content 1").unwrap();
        
        let file2 = context_dir.join("file2.md");
        fs::write(&file2, "Content 2").unwrap();
        
        // Create subdirectory with file
        let subdir = context_dir.join("subdir");
        fs::create_dir_all(&subdir).unwrap();
        let subfile = subdir.join("subfile.md");
        fs::write(&subfile, "Sub content").unwrap();
        
        let mut config = AppConfig::default();
        config.context_dir = context_dir;
        let builder = ContextBuilder::new(&config);
        
        let content = builder.load_context_files().await.unwrap();
        assert!(content.contains("Content 1"));
        assert!(content.contains("Content 2"));
        assert!(content.contains("Sub content"));
    }

    #[tokio::test]
    async fn test_load_context_files_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let context_dir = temp_dir.path().join("context");
        fs::create_dir_all(&context_dir).unwrap();
        
        let mut config = AppConfig::default();
        config.context_dir = context_dir;
        let builder = ContextBuilder::new(&config);
        
        let content = builder.load_context_files().await.unwrap();
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn test_load_context_files_nonexistent_directory() {
        let temp_dir = TempDir::new().unwrap();
        let context_dir = temp_dir.path().join("nonexistent");
        
        let mut config = AppConfig::default();
        config.context_dir = context_dir;
        let builder = ContextBuilder::new(&config);
        
        let content = builder.load_context_files().await.unwrap();
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn test_create_default_context() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = AppConfig::default();
        config.context_dir = temp_dir.path().join("context");
        
        let builder = ContextBuilder::new(&config);
        assert!(builder.create_default_context_files().await.is_ok());
        
        assert!(config.context_dir.exists());
        let sample_file = config.context_dir.join("sample.md");
        assert!(sample_file.exists());
        
        let content = fs::read_to_string(&sample_file).unwrap();
        assert!(content.contains("# CommandGPT Context"));
        assert!(content.contains("## System Information"));
        assert!(content.contains("## Preferences"));
    }

    #[tokio::test]
    async fn test_file_filtering() {
        let temp_dir = TempDir::new().unwrap();
        let context_dir = temp_dir.path().join("context");
        fs::create_dir_all(&context_dir).unwrap();
        
        // Create various file types
        fs::write(context_dir.join("valid.md"), "Valid markdown").unwrap();
        fs::write(context_dir.join("also_valid.markdown"), "Also valid").unwrap();
        fs::write(context_dir.join("invalid.txt"), "Invalid text").unwrap();
        fs::write(context_dir.join("invalid.json"), "{}").unwrap();
        fs::write(context_dir.join(".hidden.md"), "Hidden markdown").unwrap();
        
        let mut config = AppConfig::default();
        config.context_dir = context_dir;
        let builder = ContextBuilder::new(&config);
        
        let content = builder.load_context_files().await.unwrap();
        assert!(content.contains("Valid markdown"));
        assert!(content.contains("Also valid"));
        assert!(content.contains("Hidden markdown")); // Hidden files should be included
        assert!(!content.contains("Invalid text"));
        assert!(!content.contains("{}"));
    }

    #[tokio::test]
    async fn test_large_file_handling() {
        let temp_dir = TempDir::new().unwrap();
        let context_dir = temp_dir.path().join("context");
        fs::create_dir_all(&context_dir).unwrap();
        
        // Create a large file
        let large_content = "Large content line\n".repeat(1000);
        let large_file = context_dir.join("large.md");
        fs::write(&large_file, &large_content).unwrap();
        
        let mut config = AppConfig::default();
        config.context_dir = context_dir;
        let builder = ContextBuilder::new(&config);
        
        let content = builder.load_context_files().await.unwrap();
        // Should be truncated
        assert!(content.len() < large_content.len());
        assert!(content.contains("Large content line"));
    }

    #[test]
    fn test_context_builder_new() {
        let config = AppConfig::default();
        let builder = ContextBuilder::new(&config);
        
        // Verify the builder is created with the correct config
        assert_eq!(builder.config.context_dir, config.context_dir);
    }
}
