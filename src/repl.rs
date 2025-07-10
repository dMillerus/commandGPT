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
        let results = history::search_history(query).await?;
        
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

    #[test]
    fn test_special_commands() {
        // This would test the special command parsing
        // Implementation would depend on refactoring handle_special_command
        // to be more testable
    }
}
