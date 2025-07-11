mod config;
mod repl;
mod context;
mod openai;
mod safety;
mod executor;
mod history;
mod telemetry;
mod error;
mod hook;

use error::{Result, CommandGPTError};
use clap::{Parser, Subcommand};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use std::io::Write;

#[derive(Parser, Debug)]
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

#[derive(Subcommand, Debug)]
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
    /// Hook mode - process unknown command (internal use)
    #[command(hide = true)]
    Hook {
        /// The unknown command that triggered the hook
        command: String,
        /// Additional arguments passed to the command
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
        /// Error message from the shell
        #[arg(long)]
        error_context: Option<String>,
        /// Current working directory
        #[arg(long)]
        pwd: Option<String>,
        /// User and hostname context
        #[arg(long)]
        user: Option<String>,
        /// Last executed command
        #[arg(long)]
        last_command: Option<String>,
        /// Recent similar command from history
        #[arg(long)]
        recent_similar: Option<String>,
        /// Proactive mode (before execution)
        #[arg(long)]
        preexec_mode: bool,
    },
    /// Shell hook management
    ShellHook {
        #[command(subcommand)]
        action: HookAction,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Set OpenAI API key
    SetKey,
    /// Delete stored API key
    DeleteKey,
    /// Show current configuration
    Show,
}

#[derive(Subcommand, Debug)]
enum HookAction {
    /// Install shell hook
    Install,
    /// Uninstall shell hook
    Uninstall,
    /// Show hook status
    Status,
    /// Enable hook
    Enable,
    /// Disable hook
    Disable,
    /// Generate hook script
    Generate,
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

    // Initialize history manager
    if let Err(e) = history::init_history(&config.history_path).await {
        eprintln!("Warning: Failed to initialize history: {}", e);
    }

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
        Some(Commands::Hook { 
            command, 
            args, 
            error_context, 
            pwd, 
            user, 
            last_command, 
            recent_similar, 
            preexec_mode 
        }) => {
            handle_hook_command_enhanced(&config, hook::HookArgs {
                command: command.clone(),
                args: args.clone(),
                error_context: error_context.clone(),
                pwd: pwd.clone(),
                user: user.clone(),
                last_command: last_command.clone(),
                recent_similar: recent_similar.clone(),
                preexec_mode: *preexec_mode,
            }).await
        }
        Some(Commands::ShellHook { action }) => {
            handle_shell_hook_command(action, &config).await
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

async fn handle_hook_command(config: &config::AppConfig, command: &str, args: &[String]) -> Result<()> {
    // Prevent recursive calls at the application level
    if std::env::var("COMMANDGPT_HOOK_ACTIVE").unwrap_or_default() == "true" {
        return Ok(());
    }
    
    // Load hook configuration with error handling
    let hook_config = match load_hook_config().await {
        Ok(config) => config,
        Err(_) => {
            // If config loading fails, just exit silently
            return Ok(());
        }
    };
    
    // Early exit if disabled
    if !hook_config.enabled {
        return Ok(());
    }
    
    // Create shell hook processor
    let shell_hook = hook::ShellHook::new(config, hook_config);
    
    // Build the full command with arguments
    let mut full_args = vec![command.to_string()];
    full_args.extend(args.iter().cloned());
    
    // Process the unknown command with error handling
    if let Err(e) = shell_hook.process_unknown_command(&full_args).await {
        // Log error but don't propagate to avoid shell errors
        log::debug!("Hook processing error: {}", e);
    }
    
    Ok(())
}

async fn handle_hook_command_enhanced(config: &config::AppConfig, hook_args: hook::HookArgs) -> Result<()> {
    // Prevent recursive calls
    if std::env::var("COMMANDGPT_HOOK_ACTIVE").unwrap_or_default() == "true" {
        return Ok(());
    }
    
    // Use enabled hook configuration for hook processing
    let hook_config = hook::HookConfig::enabled();
    
    // Build error context from parsed arguments
    let error_context = hook::ErrorContext {
        error_message: hook_args.error_context,
        current_directory: hook_args.pwd,
        user_context: hook_args.user,
        last_command: hook_args.last_command,
        recent_similar: hook_args.recent_similar,
        preexec_mode: hook_args.preexec_mode,
    };
    
    // Create shell hook processor
    let shell_hook = hook::ShellHook::new(config, hook_config);
    
    // Build the full command with arguments
    let mut full_args = vec![hook_args.command];
    full_args.extend(hook_args.args);
    
    // Process with enhanced context
    if let Err(e) = shell_hook.process_unknown_command_with_context(&full_args, error_context).await {
        log::debug!("Hook processing error: {}", e);
    }
    
    Ok(())
}

async fn handle_shell_hook_command(action: &HookAction, config: &config::AppConfig) -> Result<()> {
    match action {
        HookAction::Install => {
            install_shell_hook(config).await
        }
        HookAction::Uninstall => {
            uninstall_shell_hook().await
        }
        HookAction::Status => {
            show_hook_status().await
        }
        HookAction::Enable => {
            set_hook_enabled(true).await
        }
        HookAction::Disable => {
            set_hook_enabled(false).await
        }
        HookAction::Generate => {
            generate_hook_script_output().await
        }
    }
}

async fn install_shell_hook(_config: &config::AppConfig) -> Result<()> {
    use std::fs;
    
    let home_dir = dirs_next::home_dir().ok_or_else(|| CommandGPTError::SystemError {
        message: "Could not determine home directory".to_string(),
        source: None,
    })?;
    
    let zshrc_path = home_dir.join(".zshrc");
    let hook_config = load_hook_config().await?;
    let hook_script = hook::generate_hook_script(&hook_config);
    
    // Check if hook is already installed
    if zshrc_path.exists() {
        let content = fs::read_to_string(&zshrc_path).map_err(|e| CommandGPTError::SystemError {
            message: format!("Failed to read .zshrc: {}", e),
            source: Some(Box::new(e)),
        })?;
        
        if content.contains("CommandGPT Shell Hook") {
            println!("✅ CommandGPT shell hook is already installed in ~/.zshrc");
            return Ok(());
        }
    }
    
    // Append hook to .zshrc
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&zshrc_path)
        .map_err(|e| CommandGPTError::SystemError {
            message: format!("Failed to open .zshrc for writing: {}", e),
            source: Some(Box::new(e)),
        })?;
    
    use std::io::Write;
    writeln!(file, "\n{}", hook_script).map_err(|e| CommandGPTError::SystemError {
        message: format!("Failed to write hook script to .zshrc: {}", e),
        source: Some(Box::new(e)),
    })?;
    
    println!("✅ CommandGPT shell hook installed successfully!");
    println!("📝 Please restart your terminal or run: source ~/.zshrc");
    println!("🔧 The hook is disabled by default. Use 'commandgpt shell-hook enable' to activate it.");
    
    Ok(())
}

async fn uninstall_shell_hook() -> Result<()> {
    use std::fs;
    
    let home_dir = dirs_next::home_dir().ok_or_else(|| CommandGPTError::SystemError {
        message: "Could not determine home directory".to_string(),
        source: None,
    })?;
    
    let zshrc_path = home_dir.join(".zshrc");
    
    if !zshrc_path.exists() {
        println!("ℹ️  No .zshrc file found");
        return Ok(());
    }
    
    let content = fs::read_to_string(&zshrc_path).map_err(|e| CommandGPTError::SystemError {
        message: format!("Failed to read .zshrc: {}", e),
        source: Some(Box::new(e)),
    })?;
    
    // Remove hook section
    let lines: Vec<&str> = content.lines().collect();
    let mut filtered_lines = Vec::new();
    let mut in_hook_section = false;
    
    for line in lines {
        if line.contains("# CommandGPT Shell Hook") {
            in_hook_section = true;
            continue;
        }
        
        if in_hook_section {
            // Skip empty lines and comments within the hook section
            if line.trim().is_empty() || line.trim().starts_with('#') || 
               line.contains("command_not_found_handler") ||
               line.contains("COMMANDGPT_HOOK_ENABLED") ||
               line.contains("commandgpt-hook-") {
                continue;
            } else {
                in_hook_section = false;
            }
        }
        
        if !in_hook_section {
            filtered_lines.push(line);
        }
    }
    
    let new_content = filtered_lines.join("\n");
    fs::write(&zshrc_path, new_content).map_err(|e| CommandGPTError::SystemError {
        message: format!("Failed to write updated .zshrc: {}", e),
        source: Some(Box::new(e)),
    })?;
    
    println!("✅ CommandGPT shell hook uninstalled successfully!");
    println!("📝 Please restart your terminal or run: source ~/.zshrc");
    
    Ok(())
}

async fn show_hook_status() -> Result<()> {
    let hook_config = load_hook_config().await?;
    
    println!("🤖 CommandGPT Shell Hook Status:");
    println!("  Enabled: {}", if hook_config.enabled { "✅ Yes" } else { "❌ No" });
    println!("  Min length: {} characters", hook_config.min_length);
    println!("  Max length: {} characters", hook_config.max_length);
    println!("  Always confirm: {}", if hook_config.always_confirm { "Yes" } else { "No" });
    println!("  API timeout: {} seconds", hook_config.api_timeout);
    println!("  Excluded patterns: {:?}", hook_config.excluded_patterns);
    
    // Check if shell hook is installed
    let home_dir = dirs_next::home_dir().ok_or_else(|| CommandGPTError::SystemError {
        message: "Could not determine home directory".to_string(),
        source: None,
    })?;
    
    let zshrc_path = home_dir.join(".zshrc");
    if zshrc_path.exists() {
        let content = std::fs::read_to_string(&zshrc_path).map_err(|e| CommandGPTError::SystemError {
            message: format!("Failed to read .zshrc: {}", e),
            source: Some(Box::new(e)),
        })?;
        
        if content.contains("CommandGPT Shell Hook") {
            println!("  Installation: ✅ Installed in ~/.zshrc");
        } else {
            println!("  Installation: ❌ Not installed");
        }
    } else {
        println!("  Installation: ❌ ~/.zshrc not found");
    }
    
    Ok(())
}

async fn set_hook_enabled(enabled: bool) -> Result<()> {
    let mut hook_config = load_hook_config().await?;
    hook_config.enabled = enabled;
    save_hook_config(&hook_config).await?;
    
    if enabled {
        println!("✅ CommandGPT shell hook enabled");
        println!("💡 Now when you type an unknown command, CommandGPT will suggest alternatives");
    } else {
        println!("❌ CommandGPT shell hook disabled");
    }
    
    Ok(())
}

async fn generate_hook_script_output() -> Result<()> {
    let hook_config = load_hook_config().await?;
    let script = hook::generate_hook_script(&hook_config);
    
    println!("# CommandGPT Shell Hook Script");
    println!("# Copy and paste this into your ~/.zshrc file:\n");
    println!("{}", script);
    
    Ok(())
}

async fn load_hook_config() -> Result<hook::HookConfig> {
    let home_dir = dirs_next::home_dir().ok_or_else(|| CommandGPTError::SystemError {
        message: "Could not determine home directory".to_string(),
        source: None,
    })?;
    
    let config_path = home_dir.join(".config").join("commandgpt").join("hook.toml");
    
    if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path).await.map_err(|e| CommandGPTError::SystemError {
            message: format!("Failed to read hook config: {}", e),
            source: Some(Box::new(e)),
        })?;
        
        let config: hook::HookConfig = toml::from_str(&content).map_err(|e| CommandGPTError::SystemError {
            message: format!("Failed to parse hook config: {}", e),
            source: Some(Box::new(e)),
        })?;
        
        Ok(config)
    } else {
        Ok(hook::HookConfig::default())
    }
}

async fn save_hook_config(config: &hook::HookConfig) -> Result<()> {
    let home_dir = dirs_next::home_dir().ok_or_else(|| CommandGPTError::SystemError {
        message: "Could not determine home directory".to_string(),
        source: None,
    })?;
    
    let config_dir = home_dir.join(".config").join("commandgpt");
    let config_path = config_dir.join("hook.toml");
    
    // Create config directory if it doesn't exist
    tokio::fs::create_dir_all(&config_dir).await.map_err(|e| CommandGPTError::SystemError {
        message: format!("Failed to create config directory: {}", e),
        source: Some(Box::new(e)),
    })?;
    
    let content = toml::to_string_pretty(config).map_err(|e| CommandGPTError::SystemError {
        message: format!("Failed to serialize hook config: {}", e),
        source: Some(Box::new(e)),
    })?;
    
    tokio::fs::write(&config_path, content).await.map_err(|e| CommandGPTError::SystemError {
        message: format!("Failed to write hook config: {}", e),
        source: Some(Box::new(e)),
    })?;
    
    Ok(())
}
