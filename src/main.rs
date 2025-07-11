mod config;
mod repl;
mod context;
mod openai;
mod safety;
mod executor;
mod history;
mod telemetry;
mod error;

use error::{Result, CommandGPTError};
use clap::{Parser, Subcommand};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use std::io::Write;

#[derive(Parser)]
#[command(name = "commandgpt")]
#[command(about = "ChatGPT-powered zsh command generator for macOS")]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Force execution without safety checks
    #[arg(long)]
    force: bool,

    /// Always confirm commands even if auto_execute is true
    #[arg(long)]
    always_confirm: bool,

    /// Disable context inclusion
    #[arg(long)]
    no_context: bool,

    /// One-shot mode: provide command as argument
    #[arg(value_name = "REQUEST")]
    request: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Show command history
    History {
        /// Number of entries to show
        #[arg(short, long, default_value = "10")]
        count: usize,
    },
    /// Clear command history
    Clear,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set OpenAI API key
    SetKey,
    /// Delete stored API key
    DeleteKey,
    /// Show current configuration
    Show,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up panic and signal handlers
    setup_error_handlers();

    // Initialize logging with error handling
    if let Err(e) = init_logging(cli.debug) {
        eprintln!("Warning: Failed to initialize logging: {}", e);
    }

    // Load configuration with enhanced error handling
    let config = match config::AppConfig::load() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("❌ Configuration error: {}", e);
            std::process::exit(1);
        }
    };

    let result = match &cli.command {
        Some(Commands::Config { action }) => {
            handle_config_command(action, &config).await
        }
        Some(Commands::History { count }) => {
            history::show_history(*count).await.map_err(|e| CommandGPTError::HistoryError {
                message: format!("Failed to show history: {}", e),
                source: None,
            })
        }
        Some(Commands::Clear) => {
            history::clear_history().await.map_err(|e| CommandGPTError::HistoryError {
                message: format!("Failed to clear history: {}", e),
                source: None,
            }).map(|_| {
                println!("Command history cleared.");
            })
        }
        None => {
            if let Some(ref request) = cli.request {
                // One-shot mode
                handle_oneshot(&config, request, &cli).await
            } else {
                // Interactive REPL mode
                repl::run_interactive(&config, &cli).await.map_err(|e| CommandGPTError::SystemError {
                    message: format!("REPL error: {}", e),
                    source: None,
                })
            }
        }
    };

    if let Err(e) = result {
        eprintln!("❌ {}", e.user_message());
        std::process::exit(e.exit_code());
    }

    Ok(())
}

fn setup_error_handlers() {
    // Handle panics gracefully
    std::panic::set_hook(Box::new(|panic_info| {
        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s
        } else {
            "Unknown panic occurred"
        };

        eprintln!("❌ Internal error: {}", msg);
        
        if let Some(location) = panic_info.location() {
            eprintln!("   at {}:{}:{}", location.file(), location.line(), location.column());
        }
        
        eprintln!("Please report this issue at: https://github.com/dMillerus/commandGPT/issues");
        std::process::exit(1);
    }));

    // Handle Ctrl+C gracefully
    ctrlc::set_handler(move || {
        println!("\n👋 Goodbye!");
        std::process::exit(130); // 128 + SIGINT
    }).expect("Error setting Ctrl+C handler");
}

fn init_logging(debug: bool) -> std::result::Result<(), Box<dyn std::error::Error>> {
    if debug {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
            .try_init()?;
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .try_init()?;
    }
    Ok(())
}

async fn handle_config_command(action: &ConfigAction, config: &config::AppConfig) -> Result<()> {
    match action {
        ConfigAction::SetKey => {
            config::set_api_key().await?;
            println!("✅ API key stored securely in Keychain");
        }
        ConfigAction::DeleteKey => {
            config::delete_api_key().await?;
            println!("✅ API key deleted from Keychain");
        }
        ConfigAction::Show => {
            config::show_config(config).await?;
        }
    }
    Ok(())
}

async fn handle_oneshot(
    config: &config::AppConfig,
    request: &str,
    cli: &Cli,
) -> Result<()> {
    use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
    use std::io::Write;

    if request.trim().is_empty() {
        return Err(CommandGPTError::InputError {
            message: "Request cannot be empty".to_string(),
            source: None,
        });
    }

    let mut stdout = StandardStream::stdout(ColorChoice::Auto);
    
    // Build context and send to OpenAI with enhanced error handling
    let context_builder = context::ContextBuilder::new(config);
    let payload = context_builder.build_payload(request, None).await
        .map_err(|e| CommandGPTError::Unknown {
            message: format!("Failed to build request payload: {}", e),
            source: None,
        })?;
    
    let openai_client = openai::OpenAIClient::new(config);
    let response = openai_client.send_chat(&payload).await?;
    
    // Safety check with enhanced error handling
    let safety_result = safety::validate_command(&response.command, cli.force)?;
    
    // Display command with explanation
    if let Err(e) = write_colored_output(&mut stdout, &response) {
        return Err(CommandGPTError::OutputError {
            message: format!("Failed to display command output: {}", e),
            source: Some(Box::new(e)),
        });
    }

    // Handle execution based on safety and auto_execute flag
    let should_execute = match safety_result {
        safety::SafetyResult::Safe => {
            if response.auto_execute && !cli.always_confirm {
                println!("\n🚀 Auto-executing...");
                true
            } else {
                get_user_confirmation("Execute this command? [y/N]: ")?
            }
        }
        safety::SafetyResult::NeedsConfirmation(warning) => {
            if let Err(e) = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red))) {
                log::warn!("Failed to set terminal color: {}", e);
            }
            if let Err(e) = writeln!(&mut stdout, "\n⚠️  Warning: {}", warning) {
                log::warn!("Failed to write warning: {}", e);
            }
            let _ = stdout.reset();
            
            get_user_confirmation("Are you sure you want to execute this? [y/N]: ")?
        }
        safety::SafetyResult::Blocked(reason) => {
            if let Err(e) = stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true)) {
                log::warn!("Failed to set terminal color: {}", e);
            }
            if let Err(e) = writeln!(&mut stdout, "\n🚫 Command blocked: {}", reason) {
                log::warn!("Failed to write blocked message: {}", e);
            }
            let _ = stdout.reset();
            return Ok(());
        }
    };

    if should_execute {
        execute_command_safely(&response.command).await?;
    }

    Ok(())
}

fn write_colored_output(
    stdout: &mut StandardStream, 
    response: &openai::CommandResponse
) -> std::result::Result<(), std::io::Error> {
    stdout.set_color(ColorSpec::new().set_fg(Some(termcolor::Color::Cyan)).set_bold(true))?;
    writeln!(stdout, "💡 Suggested command:")?;
    stdout.reset()?;
    
    stdout.set_color(ColorSpec::new().set_fg(Some(termcolor::Color::Green)))?;
    writeln!(stdout, "{}", response.command)?;
    stdout.reset()?;
    
    if !response.explanation.is_empty() {
        stdout.set_color(ColorSpec::new().set_fg(Some(termcolor::Color::Yellow)))?;
        writeln!(stdout, "\n� Explanation: {}", response.explanation)?;
        stdout.reset()?;
    }
    
    Ok(())
}

fn get_user_confirmation(prompt: &str) -> Result<bool> {
    use std::io::{self, Write};
    
    print!("{}", prompt);
    io::stdout().flush().map_err(|e| CommandGPTError::OutputError {
        message: format!("Failed to flush stdout: {}", e),
        source: Some(Box::new(e)),
    })?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(|e| CommandGPTError::InputError {
        message: format!("Failed to read user input: {}", e),
        source: Some(Box::new(e)),
    })?;
    
    Ok(input.trim().to_lowercase() == "y")
}

async fn execute_command_safely(command: &str) -> Result<()> {
    let executor = executor::CommandExecutor::new();
    match executor.execute(command).await {
        Ok(result) => {
            // Save to history
            if let Err(e) = history::record_command(command, &result.stdout, &result.stderr).await {
                log::warn!("Failed to record command in history: {}", e);
            }
            
            if !result.stdout.is_empty() {
                println!("{}", result.stdout);
            }
            if !result.stderr.is_empty() {
                eprintln!("{}", result.stderr);
            }
            
            if !result.success {
                return Err(CommandGPTError::ExecutionError {
                    message: format!("Command '{}' failed with exit code {:?}: {}", 
                                   command, result.exit_code, result.stderr),
                    source: None,
                });
            }
        }
        Err(e) => {
            return Err(CommandGPTError::ExecutionError {
                message: format!("Failed to execute command '{}': {}", command, e),
                source: None,
            });
        }
    }
    
    Ok(())
}
