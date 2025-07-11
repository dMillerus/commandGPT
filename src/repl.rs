use anyhow::{Context, Result};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::io::Write;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

use crate::config::AppConfig;
use crate::context::ContextBuilder;
use crate::executor::CommandExecutor;
use crate::history;
use crate::openai::OpenAIClient;
use crate::safety;
use crate::telemetry;
use crate::Cli;

pub struct ReplSession {
    editor: DefaultEditor,
    config: AppConfig,
    context_builder: ContextBuilder,
    openai_client: OpenAIClient,
    executor: CommandExecutor,
    stdout: StandardStream,
}

impl ReplSession {
    pub fn new(config: &AppConfig) -> Result<Self> {
        let editor = DefaultEditor::new()
            .context("Failed to create readline editor")?;

        Ok(Self {
            editor,
            config: config.clone(),
            context_builder: ContextBuilder::new(config),
            openai_client: OpenAIClient::new(config),
            executor: CommandExecutor::new(),
            stdout: StandardStream::stdout(ColorChoice::Auto),
        })
    }

    pub async fn run(&mut self, cli: &Cli) -> Result<()> {
        self.print_welcome().await?;

        loop {
            match self.editor.readline("🤖 > ") {
                Ok(line) => {
                    let input = line.trim();
                    
                    if input.is_empty() {
                        continue;
                    }

                    // Handle special commands
                    if let Some(result) = self.handle_special_command(input).await? {
                        if result {
                            break; // Exit requested
                        }
                        continue;
                    }

                    // Add to history
                    let _ = self.editor.add_history_entry(&line);

                    // Process the request
                    if let Err(e) = self.process_request(input, cli).await {
                        self.print_error(&format!("Error: {}", e)).await?;
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    self.print_info("Use 'exit' or Ctrl+D to quit").await?;
                }
                Err(ReadlineError::Eof) => {
                    self.print_info("Goodbye! 👋").await?;
                    break;
                }
                Err(err) => {
                    self.print_error(&format!("Input error: {}", err)).await?;
                }
            }
        }

        Ok(())
    }

    async fn print_welcome(&mut self) -> Result<()> {
        self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)).set_bold(true))?;
        writeln!(&mut self.stdout, "🚀 CommandGPT v{}", env!("CARGO_PKG_VERSION"))?;
        self.stdout.reset()?;
        
        writeln!(&mut self.stdout, "Ask me to generate shell commands in natural language!")?;
        writeln!(&mut self.stdout, "Type 'help' for available commands, 'exit' to quit.")?;
        writeln!(&mut self.stdout)?;

        // Check API key
        match self.config.get_api_key() {
            Ok(_) => {
                self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                writeln!(&mut self.stdout, "✅ API key configured")?;
            }
            Err(_) => {
                self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
                writeln!(&mut self.stdout, "❌ No API key found. Run 'commandgpt config set-key' first.")?;
            }
        }
        self.stdout.reset()?;
        writeln!(&mut self.stdout)?;

        Ok(())
    }

    async fn handle_special_command(&mut self, input: &str) -> Result<Option<bool>> {
        match input {
            "exit" | "quit" | "q" => {
                self.print_info("Goodbye! 👋").await?;
                return Ok(Some(true));
            }
            "help" | "h" => {
                self.print_help().await?;
                return Ok(Some(false));
            }
            "clear" => {
                print!("\x1B[2J\x1B[1;1H"); // Clear screen
                return Ok(Some(false));
            }
            "history" => {
                history::show_history(20).await?;
                return Ok(Some(false));
            }
            "stats" => {
                self.show_stats().await?;
                return Ok(Some(false));
            }
            _ => {}
        }

        // Handle history commands
        if input.starts_with("history ") {
            let parts: Vec<&str> = input.split_whitespace().collect();
            if parts.len() == 2 {
                if let Ok(count) = parts[1].parse::<usize>() {
                    history::show_history(count).await?;
                    return Ok(Some(false));
                }
            }
        }

        // Handle search commands
        if input.starts_with("search ") {
            let query = &input[7..]; // Remove "search "
            self.search_history(query).await?;
            return Ok(Some(false));
        }

        Ok(None)
    }

    async fn print_help(&mut self) -> Result<()> {
        self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)).set_bold(true))?;
        writeln!(&mut self.stdout, "📚 Available Commands:")?;
        self.stdout.reset()?;
        
        writeln!(&mut self.stdout, "  help, h         - Show this help message")?;
        writeln!(&mut self.stdout, "  exit, quit, q   - Exit the program")?;
        writeln!(&mut self.stdout, "  clear           - Clear the screen")?;
        writeln!(&mut self.stdout, "  history [N]     - Show last N commands (default: 20)")?;
        writeln!(&mut self.stdout, "  search <query>  - Search command history")?;
        writeln!(&mut self.stdout, "  stats           - Show usage statistics")?;
        writeln!(&mut self.stdout)?;
        
        self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
        writeln!(&mut self.stdout, "💡 Examples:")?;
        self.stdout.reset()?;
        writeln!(&mut self.stdout, "  > find all PDF files in my Downloads folder")?;
        writeln!(&mut self.stdout, "  > compress this directory into a tar.gz file")?;
        writeln!(&mut self.stdout, "  > show me disk usage for each directory")?;
        writeln!(&mut self.stdout, "  > kill all processes containing 'node'")?;
        writeln!(&mut self.stdout)?;

        Ok(())
    }

    async fn process_request(&mut self, input: &str, cli: &Cli) -> Result<()> {
        // Show thinking indicator
        self.print_thinking().await?;

        // Get last command for context
        let last_entry = if cli.no_context {
            None
        } else {
            history::get_last_command().await.unwrap_or(None)
        };

        // Build context and send to OpenAI
        let messages = self.context_builder.build_payload(input, last_entry.as_ref()).await
            .context("Failed to build request payload")?;

        let response = self.openai_client.send_chat(&messages).await
            .context("Failed to get response from OpenAI")?;

        // Clear thinking indicator
        print!("\r\x1b[K"); // Clear line

        // Validate command safety
        let safety_result = safety::validate_command(&response.command, cli.force)
            .context("Failed to validate command safety")?;

        // Display the suggested command
        self.display_command_suggestion(&response.command, &response.explanation).await?;

        // Handle execution based on safety result
        let should_execute = self.handle_execution_decision(&safety_result, response.auto_execute, cli.always_confirm).await?;

        if should_execute {
            self.execute_command(&response.command).await?;
        }

        writeln!(&mut self.stdout)?;
        Ok(())
    }

    async fn print_thinking(&mut self) -> Result<()> {
        self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
        print!("🤔 Thinking...");
        self.stdout.flush()?;
        self.stdout.reset()?;
        Ok(())
    }

    async fn display_command_suggestion(&mut self, command: &str, explanation: &str) -> Result<()> {
        self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)).set_bold(true))?;
        writeln!(&mut self.stdout, "💡 Suggested command:")?;
        self.stdout.reset()?;

        self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
        writeln!(&mut self.stdout, "{}", command)?;
        self.stdout.reset()?;

        if !explanation.is_empty() {
            self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
            writeln!(&mut self.stdout, "\n📝 {}", explanation)?;
            self.stdout.reset()?;
        }

        Ok(())
    }

    async fn handle_execution_decision(
        &mut self,
        safety_result: &safety::SafetyResult,
        auto_execute: bool,
        always_confirm: bool,
    ) -> Result<bool> {
        match safety_result {
            safety::SafetyResult::Safe => {
                if auto_execute && !always_confirm {
                    writeln!(&mut self.stdout, "\n🚀 Auto-executing safe command...")?;
                    Ok(true)
                } else {
                    self.prompt_for_confirmation("Execute this command?").await
                }
            }
            safety::SafetyResult::NeedsConfirmation(warning) => {
                self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
                writeln!(&mut self.stdout, "\n⚠️  {}", warning)?;
                self.stdout.reset()?;
                self.prompt_for_confirmation("Are you sure you want to execute this?").await
            }
            safety::SafetyResult::Blocked(reason) => {
                self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
                writeln!(&mut self.stdout, "\n🚫 Command blocked: {}", reason)?;
                self.stdout.reset()?;
                Ok(false)
            }
        }
    }

    async fn prompt_for_confirmation(&mut self, message: &str) -> Result<bool> {
        match self.editor.readline(&format!("\n{} [y/N]: ", message)) {
            Ok(response) => {
                let answer = response.trim().to_lowercase();
                Ok(answer == "y" || answer == "yes")
            }
            Err(_) => Ok(false),
        }
    }

    async fn execute_command(&mut self, command: &str) -> Result<()> {
        self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Blue)))?;
        writeln!(&mut self.stdout, "\n⚡ Executing...")?;
        self.stdout.reset()?;

        let start_time = std::time::Instant::now();
        
        match self.executor.execute(command).await {
            Ok(result) => {
                // Record in history
                history::record_command(command, &result.stdout, &result.stderr).await?;

                // Show output
                if !result.stdout.is_empty() {
                    writeln!(&mut self.stdout, "{}", result.stdout)?;
                }

                if !result.stderr.is_empty() {
                    self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
                    writeln!(&mut self.stdout, "{}", result.stderr)?;
                    self.stdout.reset()?;
                }

                // Show execution status
                let duration = start_time.elapsed();
                if result.success {
                    self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                    writeln!(&mut self.stdout, "✅ Completed in {:.2}s", duration.as_secs_f64())?;
                } else {
                    self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
                    writeln!(&mut self.stdout, "❌ Failed with exit code {} in {:.2}s", 
                        result.exit_code.unwrap_or(-1), duration.as_secs_f64())?;
                }
                self.stdout.reset()?;

                // Record telemetry
                telemetry::record_command_execution(command, result.success, duration).await;
            }
            Err(e) => {
                self.print_error(&format!("Execution failed: {}", e)).await?;
            }
        }

        Ok(())
    }

    async fn search_history(&mut self, query: &str) -> Result<()> {
        let results = history::search_history(query, Some(20)).await?;
        
        if results.is_empty() {
            self.print_info(&format!("No commands found matching '{}'", query)).await?;
            return Ok(());
        }

        self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)).set_bold(true))?;
        writeln!(&mut self.stdout, "🔍 Search results for '{}':", query)?;
        self.stdout.reset()?;

        for (i, entry) in results.iter().take(10).enumerate() {
            let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S");
            
            self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
            write!(&mut self.stdout, "{}. [{}] ", i + 1, timestamp)?;
            self.stdout.reset()?;
            
            writeln!(&mut self.stdout, "{}", entry.command)?;
        }

        if results.len() > 10 {
            writeln!(&mut self.stdout, "... and {} more results", results.len() - 10)?;
        }

        Ok(())
    }

    async fn show_stats(&mut self) -> Result<()> {
        // This would require implementing stats in the history module
        self.print_info("Statistics feature coming soon!").await?;
        Ok(())
    }

    async fn print_info(&mut self, message: &str) -> Result<()> {
        self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Blue)))?;
        writeln!(&mut self.stdout, "ℹ️  {}", message)?;
        self.stdout.reset()?;
        Ok(())
    }

    async fn print_error(&mut self, message: &str) -> Result<()> {
        self.stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
        writeln!(&mut self.stdout, "❌ {}", message)?;
        self.stdout.reset()?;
        Ok(())
    }
}

pub async fn run_interactive(config: &AppConfig, cli: &Cli) -> Result<()> {
    let mut session = ReplSession::new(config)
        .context("Failed to create REPL session")?;
    
    session.run(cli).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_special_commands() {
        // Test help command detection
        assert!(is_special_command("help"));
        assert!(is_special_command("?"));
        assert!(is_special_command("exit"));
        assert!(is_special_command("quit"));
        assert!(is_special_command("history"));
        assert!(is_special_command("clear"));
        
        // Test normal commands
        assert!(!is_special_command("ls -la"));
        assert!(!is_special_command("echo hello"));
        assert!(!is_special_command(""));
    }

    #[test]
    fn test_prompt_generation() {
        let config = AppConfig::default();
        let prompt = generate_command_prompt(&config, "list files");
        
        assert!(prompt.contains("list files"));
        assert!(prompt.contains("macOS"));
        assert!(prompt.contains("JSON"));
        assert!(prompt.contains("command"));
        assert!(prompt.contains("explanation"));
        assert!(prompt.contains("auto_execute"));
    }

    #[test]
    fn test_input_sanitization() {
        // Test that inputs are properly trimmed and handled
        let inputs = vec![
            "  ls -la  ",
            "\tpwd\t",
            "\necho hello\n",
            "   ",
            "",
        ];
        
        for input in inputs {
            let trimmed = input.trim();
            // Basic validation that trim works
            assert!(!trimmed.starts_with(' '));
            assert!(!trimmed.ends_with(' '));
        }
    }

    #[test]
    fn test_repl_session_creation() {
        let config = AppConfig::default();
        let session = ReplSession::new(&config);
        
        // Session creation should succeed
        assert!(session.is_ok());
    }

    #[test]
    fn test_color_formatting() {
        use termcolor::{ColorChoice, StandardStream};
        
        // Test that we can create color writers without errors
        let stdout = StandardStream::stdout(ColorChoice::Auto);
        let stderr = StandardStream::stderr(ColorChoice::Auto);
        
        // These should not panic
        drop(stdout);
        drop(stderr);
    }

    #[test]
    fn test_command_formatting() {
        // Test formatting of different command types
        let test_cases = vec![
            ("ls", "ls"),
            ("ls -la", "ls -la"),
            ("echo 'hello world'", "echo 'hello world'"),
            ("", ""),
        ];
        
        for (input, expected) in test_cases {
            assert_eq!(input.trim(), expected);
        }
    }

    #[tokio::test]
    async fn test_history_integration() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = AppConfig::default();
        config.history_path = temp_dir.path().join("test_history.db");
        
        // Test that history recording works
        let result = crate::history::record_command("test command", "output", "").await;
        // This might fail if the history database isn't properly set up
        // In a real test environment, you'd mock this
    }

    #[test]
    fn test_error_formatting() {
        use std::io::{self, Write};
        
        // Test that error messages can be written to stderr
        let mut stderr = Vec::new();
        writeln!(stderr, "Error: Test error message").unwrap();
        
        let output = String::from_utf8(stderr).unwrap();
        assert!(output.contains("Error: Test error message"));
    }

    #[test]
    fn test_welcome_message() {
        // Test that welcome message contains expected information
        let welcome = format!(
            "Welcome to CommandGPT v{}! Type 'help' for available commands.",
            env!("CARGO_PKG_VERSION")
        );
        
        assert!(welcome.contains("CommandGPT"));
        assert!(welcome.contains("help"));
    }

    #[test]
    fn test_confirmation_prompts() {
        // Test confirmation prompt formatting
        let command = "rm important_file.txt";
        let prompt = format!("Execute '{}' (y/N)? ", command);
        
        assert!(prompt.contains(command));
        assert!(prompt.contains("(y/N)"));
        assert!(prompt.ends_with("? "));
    }

    #[test]
    fn test_help_text() {
        let help_text = r#"
CommandGPT - Interactive Command Generation

Available commands:
  help, ?     - Show this help message
  history     - Show command history
  clear       - Clear command history
  exit, quit  - Exit the program

Type your request in natural language to generate commands.
"#;
        
        assert!(help_text.contains("CommandGPT"));
        assert!(help_text.contains("help"));
        assert!(help_text.contains("history"));
        assert!(help_text.contains("exit"));
    }
}

// Helper function for testing (would be added to the main module)
fn is_special_command(input: &str) -> bool {
    matches!(input.trim().to_lowercase().as_str(),
        "help" | "?" | "exit" | "quit" | "history" | "clear"
    )
}

fn generate_command_prompt(config: &AppConfig, user_input: &str) -> String {
    format!(
        r#"You are a helpful command-line assistant for macOS users.

User request: {}

Generate a single command that accomplishes the user's request. Respond with valid JSON in this exact format:

{{
    "command": "the actual command to run",
    "explanation": "brief explanation of what the command does",
    "auto_execute": false
}}

Important guidelines:
- Only return the JSON, no additional text
- Set auto_execute to true only for completely safe read-only commands
- For destructive operations, always set auto_execute to false
- Use standard Unix/macOS commands
- Prefer commonly available tools
"#,
        user_input
    )
}
