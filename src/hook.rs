use crate::config::AppConfig;
use crate::context::ContextBuilder;
use crate::openai::OpenAIClient;
use crate::safety::{self, SafetyResult};
use crate::executor::CommandExecutor;
use crate::history;
use crate::error::{Result, CommandGPTError};
use std::io::{self, Write};
use std::time::Duration;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

/// Enhanced error context for better command suggestions
#[derive(Debug, Clone, Default)]
pub struct ErrorContext {
    pub error_message: Option<String>,
    pub current_directory: Option<String>,
    pub user_context: Option<String>,
    pub last_command: Option<String>,
    pub recent_similar: Option<String>,
    pub preexec_mode: bool,
}

/// Arguments for hook command processing
#[derive(Debug, Clone)]
pub struct HookArgs {
    pub command: String,
    pub args: Vec<String>,
    pub error_context: Option<String>,
    pub pwd: Option<String>,
    pub user: Option<String>,
    pub last_command: Option<String>,
    pub recent_similar: Option<String>,
    pub preexec_mode: bool,
}

/// Analysis of the error context for intelligent processing
#[derive(Debug, Default)]
struct ErrorAnalysis {
    error_type: ErrorType,
    similarity_score: f64,
    context_relevance: f64,
    likely_intended_command: Option<String>,
}

#[derive(Debug)]
enum ErrorType {
    LikelyTypo,
    UnknownCommand,
    Permission,
    FileNotFound,
    MissingPackage,
    Other,
}

impl Default for ErrorType {
    fn default() -> Self {
        ErrorType::UnknownCommand
    }
}

/// Configuration for the shell hook system
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HookConfig {
    /// Whether to enable the hook system
    pub enabled: bool,
    /// Minimum command length to trigger hook (prevents typos)
    pub min_length: usize,
    /// Maximum command length to process (security limit)
    pub max_length: usize,
    /// Whether to always ask for confirmation
    pub always_confirm: bool,
    /// Timeout for API calls in seconds
    pub api_timeout: u64,
    /// List of patterns to never hook (security)
    pub excluded_patterns: Vec<String>,
}

impl Default for HookConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Disabled by default for safety
            min_length: 3,
            max_length: 200,
            always_confirm: true,
            api_timeout: 30, // Increased timeout for debugging
            excluded_patterns: vec![
                "sudo".to_string(),
                "su".to_string(),
                "rm".to_string(),
                "chmod".to_string(),
                "chown".to_string(),
            ],
        }
    }
}

impl HookConfig {
    /// Create an enabled hook config suitable for command processing
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            min_length: 3,
            max_length: 200,
            always_confirm: false,  // Don't always confirm in hook mode
            api_timeout: 30,  // Longer timeout for hook processing
            excluded_patterns: vec![
                "sudo".to_string(),
                "su".to_string(),
                "rm".to_string(),
                "chmod".to_string(),
                "chown".to_string(),
            ],
        }
    }
}

/// Main shell hook processor
pub struct ShellHook {
    config: AppConfig,
    hook_config: HookConfig,
    context_builder: ContextBuilder,
    openai_client: OpenAIClient,
    executor: CommandExecutor,
}

impl ShellHook {
    pub fn new(config: &AppConfig, hook_config: HookConfig) -> Self {
        Self {
            config: config.clone(),
            hook_config,
            context_builder: ContextBuilder::new(config),
            openai_client: OpenAIClient::new(config),
            executor: CommandExecutor::new(),
        }
    }

    /// Main entry point for command hook processing with enhanced error context
    pub async fn process_unknown_command_with_context(&self, args: &[String], error_context: ErrorContext) -> Result<()> {
        // Prevent processing if disabled or in recursive call
        if !self.hook_config.enabled || std::env::var("COMMANDGPT_HOOK_ACTIVE").unwrap_or_default() == "true" {
            return Ok(());
        }
        
        if args.is_empty() {
            return Ok(());
        }

        let command = args[0].clone();
        let command_args = if args.len() > 1 { &args[1..] } else { &[] };
        
        // Apply filters with enhanced context
        let should_process = self.should_process_command_with_context(&command, &error_context);
        
        if !should_process {
            // For dangerous commands, just show error without AI suggestions
            return self.show_command_not_found_with_context(&command, &error_context);
        }

        // Analyze error type for better context
        let error_analysis = self.analyze_error_context(&command, &error_context);

        // Show hook activation with context
        if error_context.preexec_mode {
            self.show_preexec_hook_activation(&command)?;
        } else {
            self.show_hook_activation(&command)?;
        }

        // Build enhanced prompt with error context
        let enhanced_prompt = self.build_enhanced_prompt(&command, command_args, &error_context, &error_analysis);
        
        // Get AI suggestion with timeout
        let timeout_duration = Duration::from_secs(self.hook_config.api_timeout);
        
        match tokio::time::timeout(timeout_duration, self.get_ai_suggestion_with_enhanced_context(&enhanced_prompt)).await {
            Ok(Ok(suggestion)) => {
                self.handle_suggestion_with_context(suggestion, &command, &error_context).await
            }
            Ok(Err(e)) => {
                log::debug!("CommandGPT hook error: {}", e);
                if error_context.preexec_mode {
                    println!("❌ CommandGPT couldn't provide suggestions. Proceeding with original command.");
                }
                self.show_command_not_found_with_context(&command, &error_context)
            }
            Err(_) => {
                log::debug!("CommandGPT hook timeout for command: {}", command);
                if error_context.preexec_mode {
                    println!("⏱️  CommandGPT timeout. Proceeding with original command.");
                }
                self.show_command_not_found_with_context(&command, &error_context)
            }
        }
    }

    /// Main entry point for command hook processing
    pub async fn process_unknown_command(&self, args: &[String]) -> Result<()> {
        // Use the enhanced version with default context
        let error_context = ErrorContext::default();
        self.process_unknown_command_with_context(args, error_context).await
    }

    /// Check if a command should be processed by the hook
    pub fn should_process_command(&self, command: &str) -> bool {
        let command = command.trim();
        
        // Length checks
        if command.len() < self.hook_config.min_length || command.len() > self.hook_config.max_length {
            return false;
        }

        // Check excluded patterns
        for pattern in &self.hook_config.excluded_patterns {
            if command.starts_with(pattern) {
                return false;
            }
        }

        // Don't process if it's clearly not a command intention
        if command.contains("http://") || command.contains("https://") {
            return false;
        }

        true
    }

    /// Check if a command should be processed by the hook with enhanced context
    fn should_process_command_with_context(&self, command: &str, context: &ErrorContext) -> bool {
        // Standard filtering
        let basic_check = self.should_process_command(command);
        
        if !basic_check {
            return false;
        }
        
        // Enhanced filtering based on context
        if let Some(error_msg) = &context.error_message {
            // If it's a permission error, don't process
            if error_msg.contains("permission denied") || error_msg.contains("access denied") {
                return false;
            }
        }
        
        // If we have a recent similar command, we're more likely to process
        if context.recent_similar.is_some() {
            return true;
        }
        
        // In preexec mode, be more selective
        if context.preexec_mode {
            // Only process if it looks like a typo or known pattern
            let is_typo = self.is_likely_typo(command);
            let is_pattern = self.is_known_pattern(command);
            return is_typo || is_pattern;
        }
        
        true
    }

    /// Analyze error context for intelligent processing
    fn analyze_error_context(&self, command: &str, context: &ErrorContext) -> ErrorAnalysis {
        let mut analysis = ErrorAnalysis::default();
        
        // Analyze error message
        if let Some(error_msg) = &context.error_message {
            analysis.error_type = if error_msg.contains("command not found") {
                if self.is_likely_typo(command) {
                    ErrorType::LikelyTypo
                } else if self.is_known_pattern(command) {
                    ErrorType::MissingPackage
                } else {
                    ErrorType::UnknownCommand
                }
            } else if error_msg.contains("permission denied") {
                ErrorType::Permission
            } else if error_msg.contains("no such file") {
                ErrorType::FileNotFound
            } else {
                ErrorType::Other
            };
        }
        
        // Analyze command similarity to recent commands
        if let Some(recent) = &context.recent_similar {
            analysis.similarity_score = self.calculate_similarity(command, recent);
            analysis.likely_intended_command = if analysis.similarity_score > 0.7 {
                Some(recent.clone())
            } else {
                None
            };
        }
        
        // Analyze based on last command context
        if let Some(last_cmd) = &context.last_command {
            analysis.context_relevance = self.calculate_context_relevance(command, last_cmd);
        }
        
        analysis
    }

    /// Build enhanced prompt with error context
    fn build_enhanced_prompt(&self, command: &str, args: &[String], context: &ErrorContext, analysis: &ErrorAnalysis) -> String {
        let mut prompt_parts = vec![
            format!("User attempted to run command: {}", command),
        ];
        
        if !args.is_empty() {
            prompt_parts.push(format!("With arguments: {}", args.join(" ")));
        }
        
        // Add error context
        if let Some(error_msg) = &context.error_message {
            prompt_parts.push(format!("Shell error: {}", error_msg));
        }
        
        // Add directory context
        if let Some(pwd) = &context.current_directory {
            prompt_parts.push(format!("Current directory: {}", pwd));
        }
        
        // Add user context
        if let Some(user) = &context.user_context {
            prompt_parts.push(format!("User context: {}", user));
        }
        
        // Add last command context
        if let Some(last_cmd) = &context.last_command {
            prompt_parts.push(format!("Previous command: {}", last_cmd));
        }
        
        // Add similarity analysis
        if let Some(similar) = &context.recent_similar {
            prompt_parts.push(format!("Recent similar command: {}", similar));
        }
        
        // Add error analysis
        prompt_parts.push(format!("Error analysis: {:?}", analysis.error_type));
        if let Some(intended) = &analysis.likely_intended_command {
            prompt_parts.push(format!("Likely intended: {}", intended));
        }
        
        // Add mode context
        if context.preexec_mode {
            prompt_parts.push("Mode: Proactive suggestion (before execution)".to_string());
        } else {
            prompt_parts.push("Mode: Reactive suggestion (after command not found)".to_string());
        }
        
        // Build the final prompt
        format!(
            "{}\n\nBased on this context, please suggest the most appropriate command(s) that the user likely intended to run. Consider:\n1. Possible typos or misspellings\n2. Missing package installations\n3. Alternative commands that accomplish the same goal\n4. Context from previous commands\n5. Current directory relevance\n\nYou must respond with a JSON object in this exact format:\n{{\n  \"command\": \"the suggested command\",\n  \"explanation\": \"brief explanation of why this command is suggested\",\n  \"auto_execute\": false\n}}\n\nDo not include any other text or formatting - just the JSON object.",
            prompt_parts.join("\n")
        )
    }

    /// Check if command matches known patterns
    fn is_known_pattern(&self, command: &str) -> bool {
        // Common command patterns that users might type
        let patterns = [
            "install", "update", "upgrade", "remove", "search", "find", "list", "show",
            "get", "set", "start", "stop", "restart", "status", "check", "test",
            "create", "delete", "copy", "move", "rename", "chmod", "chown", "mount",
            "compress", "extract", "backup", "restore", "sync", "download", "upload"
        ];
        
        patterns.iter().any(|&pattern| command.contains(pattern))
    }

    /// Calculate similarity between two commands
    fn calculate_similarity(&self, cmd1: &str, cmd2: &str) -> f64 {
        let distance = self.edit_distance(cmd1, cmd2);
        let max_len = cmd1.len().max(cmd2.len()) as f64;
        if max_len == 0.0 {
            1.0
        } else {
            1.0 - (distance as f64 / max_len)
        }
    }

    /// Calculate context relevance between current and last command
    fn calculate_context_relevance(&self, current_cmd: &str, last_cmd: &str) -> f64 {
        // Simple heuristic: commands that share common prefixes or patterns
        let current_parts: Vec<&str> = current_cmd.split_whitespace().collect();
        let last_parts: Vec<&str> = last_cmd.split_whitespace().collect();
        
        if current_parts.is_empty() || last_parts.is_empty() {
            return 0.0;
        }
        
        // Check if they share similar prefixes (git, docker, etc.)
        let current_base = current_parts[0];
        let last_base = last_parts[0];
        
        if current_base.starts_with(&last_base[..last_base.len().min(3)]) {
            0.8
        } else if current_base.len() >= 3 && last_base.len() >= 3 && 
                  current_base[..3] == last_base[..3] {
            0.6
        } else {
            0.2
        }
    }

    /// Get AI suggestion with enhanced context
    async fn get_ai_suggestion_with_enhanced_context(&self, enhanced_prompt: &str) -> Result<crate::openai::CommandResponse> {
        let messages = vec![
            crate::openai::ChatMessage {
                role: "system".to_string(),
                content: "You are CommandGPT, an AI assistant that helps users with shell commands. Analyze the provided context and suggest the most appropriate command(s).".to_string(),
            },
            crate::openai::ChatMessage {
                role: "user".to_string(),
                content: enhanced_prompt.to_string(),
            },
        ];
        
        self.openai_client.send_chat(&messages).await
            .map_err(|e| {
                log::debug!("OpenAI API error: {}", e);
                crate::error::CommandGPTError::ApiError {
                    message: format!("OpenAI API error: {}", e),
                    source: None,
                }
            })
    }

    /// Show command not found with context
    fn show_command_not_found_with_context(&self, command: &str, context: &ErrorContext) -> Result<()> {
        if context.preexec_mode {
            println!("⚠️  Command '{}' not found. Continuing with execution...", command);
        } else {
            eprintln!("zsh: command not found: {}", command);
        }
        Ok(())
    }

    /// Show preexec hook activation
    fn show_preexec_hook_activation(&self, command: &str) -> Result<()> {
        let mut stdout = StandardStream::stdout(ColorChoice::Auto);
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
        write!(&mut stdout, "⚠️  ")?;
        stdout.reset()?;
        println!("Command '{}' not found. Getting suggestions...", command);
        Ok(())
    }

    /// Handle suggestion with context
    async fn handle_suggestion_with_context(&self, suggestion: crate::openai::CommandResponse, original_command: &str, context: &ErrorContext) -> Result<()> {
        let suggested_command = &suggestion.command;
        
        if context.preexec_mode {
            println!("💡 Suggestion: {}", suggested_command);
            println!("📝 {}", suggestion.explanation);
            
            print!("Execute this command instead? [y/N]: ");
            io::stdout().flush()?;
            
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            
            if input.trim().to_lowercase() == "y" {
                let result = self.executor.execute(suggested_command).await?;
                if !result.success {
                    println!("❌ Command failed: {}", result.stderr);
                }
            }
            Ok(())
        } else {
            // Standard reactive mode
            self.handle_suggestion(suggestion, original_command).await
        }
    }

    /// Check if command is likely a typo of a common command
    pub fn is_likely_typo(&self, command: &str) -> bool {
        let common_commands = vec![
            "ls", "cd", "pwd", "cat", "echo", "grep", "find", "git", "vim", "nano",
            "cp", "mv", "mkdir", "rmdir", "touch", "head", "tail", "sort", "uniq",
        ];

        // Simple edit distance check for typos
        for common in common_commands {
            if self.edit_distance(command, common) == 1 && command.len() > 2 {
                return true;
            }
        }

        false
    }

    /// Calculate simple edit distance
    pub fn edit_distance(&self, a: &str, b: &str) -> usize {
        let a_chars: Vec<char> = a.chars().collect();
        let b_chars: Vec<char> = b.chars().collect();
        let a_len = a_chars.len();
        let b_len = b_chars.len();

        if a_len == 0 { return b_len; }
        if b_len == 0 { return a_len; }

        let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];

        for i in 0..=a_len { matrix[i][0] = i; }
        for j in 0..=b_len { matrix[0][j] = j; }

        for i in 1..=a_len {
            for j in 1..=b_len {
                let cost = if a_chars[i-1] == b_chars[j-1] { 0 } else { 1 };
                matrix[i][j] = std::cmp::min(
                    std::cmp::min(
                        matrix[i-1][j] + 1,      // deletion
                        matrix[i][j-1] + 1       // insertion
                    ),
                    matrix[i-1][j-1] + cost      // substitution
                );
            }
        }

        matrix[a_len][b_len]
    }

    /// Show hook activation message
    fn show_hook_activation(&self, command: &str) -> Result<()> {
        let mut stderr = StandardStream::stderr(ColorChoice::Auto);
        stderr.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
        eprintln!("🤖 Command '{}' not found. Asking CommandGPT for help...", command);
        stderr.reset()?;
        Ok(())
    }

    /// Get command suggestion from OpenAI
    async fn get_command_suggestion(&self, original_command: &str) -> Result<crate::openai::CommandResponse> {
        // Build enhanced context with the original attempted command
        let enhanced_request = format!(
            "I tried to run '{}' but it wasn't found. Please suggest the correct command or alternative.",
            original_command
        );

        let last_entry = history::get_last_command().await.unwrap_or(None);
        let payload = self.context_builder.build_payload(&enhanced_request, last_entry.as_ref()).await
            .map_err(|e| CommandGPTError::Unknown {
                message: format!("Failed to build request payload: {}", e),
                source: None,
            })?;

        // Set shorter timeout for hook usage
        let timeout_duration = std::time::Duration::from_secs(self.hook_config.api_timeout);
        let response = tokio::time::timeout(timeout_duration, self.openai_client.send_chat(&payload)).await
            .map_err(|_| CommandGPTError::NetworkError {
                message: "API request timed out".to_string(),
                source: None,
            })?;

        response.map_err(|e| CommandGPTError::Unknown {
            message: format!("Failed to get response from OpenAI: {}", e),
            source: None,
        })
    }

    /// Handle the AI suggestion
    async fn handle_suggestion(&self, suggestion: crate::openai::CommandResponse, _original_command: &str) -> Result<()> {
        let mut stdout = StandardStream::stdout(ColorChoice::Auto);
        
        // Display suggestion
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)).set_bold(true))?;
        println!("💡 Suggested command:");
        stdout.reset()?;
        
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)))?;
        println!("  {}", suggestion.command);
        stdout.reset()?;
        
        if !suggestion.explanation.is_empty() {
            stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
            println!("📝 Explanation: {}", suggestion.explanation);
            stdout.reset()?;
        }

        // Safety validation
        let safety_result = safety::validate_command(&suggestion.command, false)?;
        
        // Determine if we should execute
        let should_execute = match safety_result {
            SafetyResult::Safe => {
                if suggestion.auto_execute && !self.hook_config.always_confirm {
                    println!("\n🚀 Auto-executing safe command...");
                    true
                } else {
                    self.get_user_confirmation("Execute this command? [y/N]: ")?
                }
            }
            SafetyResult::NeedsConfirmation(warning) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
                println!("\n⚠️  Warning: {}", warning);
                stdout.reset()?;
                self.get_user_confirmation("Are you sure you want to execute this? [y/N]: ")?
            }
            SafetyResult::Blocked(reason) => {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
                println!("\n🚫 Command blocked: {}", reason);
                stdout.reset()?;
                return Ok(());
            }
        };

        if should_execute {
            self.execute_command(&suggestion.command).await?;
        }

        Ok(())
    }

    /// Get user confirmation
    fn get_user_confirmation(&self, prompt: &str) -> Result<bool> {
        print!("{}", prompt);
        io::stdout().flush()?;
        
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        
        Ok(input.trim().to_lowercase() == "y")
    }

    /// Execute the suggested command
    async fn execute_command(&self, command: &str) -> Result<()> {
        match self.executor.execute(command).await {
            Ok(result) => {
                // Record in history
                if let Err(e) = history::record_command(command, &result.stdout, &result.stderr).await {
                    log::warn!("Failed to record command in history: {}", e);
                }
                
                // Display output
                if !result.stdout.is_empty() {
                    println!("{}", result.stdout);
                }
                if !result.stderr.is_empty() {
                    eprintln!("{}", result.stderr);
                }
                
                if !result.success {
                    eprintln!("❌ Command failed with exit code {:?}", result.exit_code);
                }
                
                Ok(())
            }
            Err(e) => {
                Err(CommandGPTError::ExecutionError {
                    message: format!("Failed to execute command: {}", e),
                    source: None,
                })
            }
        }
    }

    /// Show standard command not found message
    fn show_command_not_found(&self, command: &str) -> Result<()> {
        eprintln!("zsh: command not found: {}", command);
        Ok(())
    }
}

/// Generate enhanced shell hook installation script with error context
pub fn generate_hook_script(config: &HookConfig) -> String {
    format!(r#"# CommandGPT Enhanced Shell Hook for zsh
# Generated by CommandGPT v{}
# Add this to your ~/.zshrc to enable automatic CommandGPT fallback with enhanced error context

# Set hook enabled state
export COMMANDGPT_HOOK_ENABLED={}

# Enhanced command_not_found_handler with error context
command_not_found_handler() {{
    local cmd="$1"
    shift || true
    
    # Prevent infinite loops
    if [[ "$COMMANDGPT_HOOK_ACTIVE" == "true" ]]; then
        echo "zsh: command not found: $cmd" >&2
        return 127
    fi
    
    # Check if hook is enabled
    if [[ "$COMMANDGPT_HOOK_ENABLED" != "true" ]]; then
        echo "zsh: command not found: $cmd" >&2
        return 127
    fi
    
    # Verify commandgpt exists
    if ! command -v commandgpt >/dev/null 2>&1; then
        echo "zsh: command not found: $cmd" >&2
        return 127
    fi
    
    # Gather enhanced context
    local pwd_context="$(pwd)"
    local user_context="$(whoami)@$(hostname)"
    local shell_level="$SHLVL"
    
    # Get last command from history if available
    local last_command=""
    if [[ -n "$ZSH_VERSION" && "$HISTCMD" -gt 1 ]]; then
        last_command="$(fc -ln $(($HISTCMD-1)) $(($HISTCMD-1)) 2>/dev/null | sed 's/^[[:space:]]*//' | head -1)"
    fi
    
    # Check for recent similar commands in history
    local recent_similar=""
    if command -v fc >/dev/null 2>&1; then
        recent_similar="$(fc -ln -10 2>/dev/null | grep -E "^[[:space:]]*${{cmd:0:3}}" | tail -1 | sed 's/^[[:space:]]*//' 2>/dev/null || true)"
    fi
    
    # Build error context arguments
    local error_msg="zsh: command not found: $cmd"
    local context_args=(
        "--error-context" "$error_msg"
        "--pwd" "$pwd_context"
        "--user" "$user_context"
    )
    
    # Add last command if available and different
    if [[ -n "$last_command" && "$last_command" != "$cmd"* ]]; then
        context_args+=("--last-command" "$last_command")
    fi
    
    # Add similar recent command if found and different
    if [[ -n "$recent_similar" && "$recent_similar" != "$cmd"* ]]; then
        context_args+=("--recent-similar" "$recent_similar")
    fi
    
    # Set flag and call commandgpt with enhanced context
    export COMMANDGPT_HOOK_ACTIVE=true
    if commandgpt hook "$cmd" "$@" "${{context_args[@]}}" 2>/dev/null; then
        local exit_code=$?
        unset COMMANDGPT_HOOK_ACTIVE
        return $exit_code
    else
        unset COMMANDGPT_HOOK_ACTIVE
        echo "$error_msg" >&2
        return 127
    fi
}}

# Alternative method: Preexec hook for proactive command validation (optional)
preexec_commandgpt_hook() {{
    local cmd="$1"
    
    # Skip if hook disabled or we're in a hook already
    if [[ "$COMMANDGPT_HOOK_ENABLED" != "true" || "$COMMANDGPT_HOOK_ACTIVE" == "true" ]]; then
        return
    fi
    
    # Check if this command might fail and offer proactive help
    local base_cmd="${{cmd%% *}}"
    if ! command -v "$base_cmd" >/dev/null 2>&1; then
        echo "⚠️  Command '$base_cmd' not found. CommandGPT can suggest alternatives."
        read -q "REPLY?Get suggestions? [y/N]: " && echo
        if [[ "$REPLY" =~ ^[Yy]$ ]]; then
            export COMMANDGPT_HOOK_ACTIVE=true
            local args=("${{(@s/ /)cmd}}")
            commandgpt hook "${{args[1]}}" "${{args[@]:2}}" --preexec-mode --pwd "$(pwd)" --user "$(whoami)@$(hostname)" 2>/dev/null
            unset COMMANDGPT_HOOK_ACTIVE
            echo "Press Enter to continue with original command or Ctrl+C to cancel..."
            read
        fi
    fi
}}

# Hook into zsh's preexec if available (runs before command execution)
if [[ -n "$ZSH_VERSION" ]]; then
    autoload -U add-zsh-hook 2>/dev/null && add-zsh-hook preexec preexec_commandgpt_hook || true
fi

# Convenience aliases with enhanced status
alias commandgpt-hook-on='export COMMANDGPT_HOOK_ENABLED=true && echo "✅ CommandGPT enhanced hook enabled"'
alias commandgpt-hook-off='export COMMANDGPT_HOOK_ENABLED=false && echo "❌ CommandGPT hook disabled"'
alias commandgpt-hook-status='echo "CommandGPT Hook Status: $([[ "$COMMANDGPT_HOOK_ENABLED" == "true" ]] && echo "✅ Enabled (Enhanced)" || echo "❌ Disabled")"'

# Show hook features on first load
if [[ "$COMMANDGPT_HOOK_ENABLED" == "true" ]]; then
    echo "🤖 CommandGPT enhanced shell hook loaded with error context analysis"
fi

# CommandGPT Enhanced Shell Hook - End
"#, 
        env!("CARGO_PKG_VERSION"),
        if config.enabled { "true" } else { "false" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_process_command() {
        let config = HookConfig::default();
        let app_config = AppConfig::default();
        let hook = ShellHook::new(&app_config, config);

        // Should process
        assert!(hook.should_process_command("list files"));
        assert!(hook.should_process_command("show directory"));
        
        // Should not process - too short
        assert!(!hook.should_process_command("ls"));
        assert!(!hook.should_process_command("cd"));
        
        // Should not process - excluded patterns
        assert!(!hook.should_process_command("sudo rm -rf /"));
        
        // Should not process - URLs
        assert!(!hook.should_process_command("https://example.com"));
    }

    #[test]
    fn test_edit_distance() {
        let config = HookConfig::default();
        let app_config = AppConfig::default();
        let hook = ShellHook::new(&app_config, config);

        assert_eq!(hook.edit_distance("cat", "cat"), 0);
        assert_eq!(hook.edit_distance("cat", "bat"), 1);
        assert_eq!(hook.edit_distance("ls", "lss"), 1);
        assert_eq!(hook.edit_distance("hello", "world"), 4);
    }

    #[test]
    fn test_is_likely_typo() {
        let config = HookConfig::default();
        let app_config = AppConfig::default();
        let hook = ShellHook::new(&app_config, config);

        // Likely typos
        assert!(hook.is_likely_typo("lss"));  // ls -> lss
        assert!(hook.is_likely_typo("catt")); // cat -> catt
        
        // Not typos
        assert!(!hook.is_likely_typo("list"));
        assert!(!hook.is_likely_typo("show"));
    }

    #[test]
    fn test_generate_hook_script() {
        let config = HookConfig { enabled: true, ..Default::default() };
        let script = generate_hook_script(&config);
        
        assert!(script.contains("command_not_found_handler"));
        assert!(script.contains("COMMANDGPT_HOOK_ENABLED=true"));
        assert!(script.contains("commandgpt --hook"));
    }
}
