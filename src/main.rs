mod config;
mod repl;
mod context;
mod openai;
mod safety;
mod executor;
mod history;
mod telemetry;

use anyhow::Result;
use clap::{Parser, Subcommand};

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

    // Initialize logging
    if cli.debug {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
            .init();
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .init();
    }

    // Load configuration
    let config = config::AppConfig::load()?;

    match &cli.command {
        Some(Commands::Config { action }) => {
            handle_config_command(action, &config).await?;
        }
        Some(Commands::History { count }) => {
            history::show_history(*count).await?;
        }
        Some(Commands::Clear) => {
            history::clear_history().await?;
            println!("Command history cleared.");
        }
        None => {
            if let Some(ref request) = cli.request {
                // One-shot mode
                handle_oneshot(&config, request, &cli).await?;
            } else {
                // Interactive REPL mode
                repl::run_interactive(&config, &cli).await?;
            }
        }
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

    let mut stdout = StandardStream::stdout(ColorChoice::Auto);
    
    // Build context and send to OpenAI
    let context_builder = context::ContextBuilder::new(config);
    let payload = context_builder.build_payload(request, None).await?;
    
    let openai_client = openai::OpenAIClient::new(config);
    let response = openai_client.send_chat(&payload).await?;
    
    // Safety check
    let safety_result = safety::validate_command(&response.command, cli.force)?;
    
    // Display command with explanation
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)).set_bold(true))?;
    writeln!(&mut stdout, "💡 Suggested command:")?;
    stdout.reset()?;
    
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
    writeln!(&mut stdout, "{}", response.command)?;
    stdout.reset()?;
    
    if !response.explanation.is_empty() {
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
        writeln!(&mut stdout, "\n📝 Explanation: {}", response.explanation)?;
        stdout.reset()?;
    }

    // Handle execution based on safety and auto_execute flag
    let should_execute = match safety_result {
        safety::SafetyResult::Safe => {
            if response.auto_execute && !cli.always_confirm {
                println!("\n🚀 Auto-executing...");
                true
            } else {
                print!("\nExecute this command? [y/N]: ");
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                input.trim().to_lowercase() == "y"
            }
        }
        safety::SafetyResult::NeedsConfirmation(warning) => {
            stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
            writeln!(&mut stdout, "\n⚠️  Warning: {}", warning)?;
            stdout.reset()?;
            
            print!("Are you sure you want to execute this? [y/N]: ");
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            input.trim().to_lowercase() == "y"
        }
        safety::SafetyResult::Blocked(reason) => {
            stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
            writeln!(&mut stdout, "\n🚫 Command blocked: {}", reason)?;
            stdout.reset()?;
            false
        }
    };

    if should_execute {
        let executor = executor::CommandExecutor::new();
        match executor.execute(&response.command).await {
            Ok(result) => {
                // Save to history
                history::record_command(&response.command, &result.stdout, &result.stderr).await?;
                
                if !result.stdout.is_empty() {
                    println!("{}", result.stdout);
                }
                if !result.stderr.is_empty() {
                    eprintln!("{}", result.stderr);
                }
                
                if !result.success {
                    std::process::exit(result.exit_code.unwrap_or(1));
                }
            }
            Err(e) => {
                eprintln!("❌ Execution failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
