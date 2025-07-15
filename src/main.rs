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
        /// Exit code of the failed command
        #[arg(long)]
        exit_code: Option<i32>,
        /// Error message from the shell
        #[arg(long)]
        error_context: Option<String>,
        /// Standard error output from the command
        #[arg(long)]
        stderr_output: Option<String>,
        /// Standard output from the command
        #[arg(long)]
        stdout_output: Option<String>,
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
        /// Command execution duration in milliseconds
        #[arg(long)]
        command_duration: Option<u64>,
        /// Environment variables context
        #[arg(long)]
        environment_vars: Option<String>,
        /// Proactive mode (before execution)
        #[arg(long)]
        preexec_mode: bool,
        /// Type of error (command_not_found, permission_denied, etc.)
        #[arg(long)]
        error_type: Option<String>,
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
    /// Test comprehensive error handling
    Test,
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
            exit_code,
            error_context, 
            stderr_output,
            stdout_output,
            pwd, 
            user, 
            last_command, 
            recent_similar, 
            command_duration,
            environment_vars,
            preexec_mode,
            error_type,
        }) => {
            handle_hook_command_enhanced(&config, hook::HookArgs {
                command: command.clone(),
                args: args.clone(),
                exit_code: *exit_code,
                error_context: error_context.clone(),
                stderr_output: stderr_output.clone(),
                stdout_output: stdout_output.clone(),
                pwd: pwd.clone(),
                user: user.clone(),
                last_command: last_command.clone(),
                recent_similar: recent_similar.clone(),
                command_duration: *command_duration,
                environment_vars: environment_vars.clone(),
                preexec_mode: *preexec_mode,
                error_type: error_type.clone(),
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
    
    // Build comprehensive error context from parsed arguments
    let error_context = hook::ErrorContext {
        error_message: hook_args.error_context,
        exit_code: hook_args.exit_code,
        stderr_output: hook_args.stderr_output,
        stdout_output: hook_args.stdout_output,
        current_directory: hook_args.pwd,
        user_context: hook_args.user,
        last_command: hook_args.last_command,
        recent_similar: hook_args.recent_similar,
        command_duration: hook_args.command_duration,
        environment_vars: hook_args.environment_vars,
        preexec_mode: hook_args.preexec_mode,
        error_type: hook_args.error_type,
    };
    
    // Create shell hook processor
    let shell_hook = hook::ShellHook::new(config, hook_config);
    
    // Build the full command with arguments
    let mut full_args = vec![hook_args.command];
    full_args.extend(hook_args.args);
    
    // Determine which processing method to use based on context
    let result = if error_context.exit_code.is_some() || error_context.stderr_output.is_some() {
        // Use comprehensive exit processing for commands with exit codes or stderr
        shell_hook.process_command_exit(&full_args, error_context).await
    } else {
        // Fall back to enhanced context processing for command not found scenarios
        shell_hook.process_unknown_command_with_context(&full_args, error_context).await
    };
    
    if let Err(e) = result {
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
        HookAction::Test => {
            test_comprehensive_hook(config).await
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

async fn test_comprehensive_hook(config: &config::AppConfig) -> Result<()> {
    use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
    
    let mut stdout = StandardStream::stdout(ColorChoice::Auto);
    
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Blue)).set_bold(true))?;
    println!("🧪 Testing CommandGPT Comprehensive Hook System");
    stdout.reset()?;
    println!();
    
    // Test different error scenarios
    let test_scenarios = vec![
        (
            "unknown_command_test",
            vec![],
            Some(127),
            Some("command not found: unknown_command_test".to_string()),
            Some("command_not_found".to_string()),
            "Testing unknown command handling"
        ),
        (
            "ls",
            vec!["/nonexistent/path".to_string()],
            Some(2),
            Some("ls: cannot access '/nonexistent/path': No such file or directory".to_string()),
            Some("file_not_found".to_string()),
            "Testing file not found error"
        ),
        (
            "cat",
            vec!["/etc/shadow".to_string()],
            Some(1),
            Some("cat: /etc/shadow: Permission denied".to_string()),
            Some("permission_denied".to_string()),
            "Testing permission denied error"
        ),
        (
            "git",
            vec!["invalid-subcommand".to_string()],
            Some(1),
            Some("git: 'invalid-subcommand' is not a git command".to_string()),
            Some("syntax_error".to_string()),
            "Testing invalid subcommand error"
        ),
        (
            "curl",
            vec!["http://nonexistent.invalid".to_string()],
            Some(6),
            Some("curl: (6) Could not resolve host: nonexistent.invalid".to_string()),
            Some("network_error".to_string()),
            "Testing network error"
        ),
    ];
    
    println!("Running {} test scenarios...\n", test_scenarios.len());
    
    for (i, (command, args, exit_code, stderr, error_type, description)) in test_scenarios.iter().enumerate() {
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)))?;
        println!("Test {}: {}", i + 1, description);
        stdout.reset()?;
        
        // Create comprehensive error context
        let error_context = hook::ErrorContext {
            error_message: stderr.clone(),
            exit_code: *exit_code,
            stderr_output: stderr.clone(),
            stdout_output: None,
            current_directory: Some(std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()),
            user_context: Some(format!("{}@{}", 
                std::env::var("USER").unwrap_or_else(|_| "testuser".to_string()),
                std::env::var("HOSTNAME").unwrap_or_else(|_| "testhost".to_string())
            )),
            last_command: if i > 0 { 
                Some(format!("{} {}", test_scenarios[i-1].0, test_scenarios[i-1].1.join(" "))) 
            } else { 
                None 
            },
            recent_similar: None,
            command_duration: Some(150 + (i as u64 * 50)), // Simulate different durations
            environment_vars: Some("PATH=/usr/bin:/bin;SHELL=/bin/zsh".to_string()),
            preexec_mode: false,
            error_type: error_type.clone(),
        };
        
        // Create hook processor
        let hook_config = hook::HookConfig::enabled();
        let shell_hook = hook::ShellHook::new(config, hook_config);
        
        // Build command arguments
        let mut full_args = vec![command.to_string()];
        full_args.extend(args.clone());
        
        println!("  Command: {} {}", command, args.join(" "));
        if let Some(code) = exit_code {
            println!("  Exit Code: {}", code);
        }
        if let Some(err) = stderr {
            println!("  Error: {}", err);
        }
        
        // Test the hook processing
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
        println!("  🤖 Triggering AI analysis...");
        stdout.reset()?;
        
        match shell_hook.process_command_exit(&full_args, error_context).await {
            Ok(()) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
                println!("  ✅ Test completed successfully");
                stdout.reset()?;
            }
            Err(e) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
                println!("  ❌ Test failed: {}", e);
                stdout.reset()?;
            }
        }
        
        println!();
        
        // Small delay between tests for better UX
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    
    stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)).set_bold(true))?;
    println!("🎉 Comprehensive hook testing completed!");
    stdout.reset()?;
    
    println!("\n📋 Test Summary:");
    println!("  • {} scenarios tested", test_scenarios.len());
    println!("  • Command not found handling");
    println!("  • File system errors");
    println!("  • Permission errors");
    println!("  • Syntax/usage errors");
    println!("  • Network connectivity issues");
    
    println!("\n💡 Next Steps:");
    println!("  1. Enable the hook: commandgpt shell-hook enable");
    println!("  2. Restart your terminal or: source ~/.zshrc");
    println!("  3. Try commands that fail to see AI assistance in action");
    
    Ok(())
}
