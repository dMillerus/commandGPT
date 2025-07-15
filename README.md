# commandGPT

A single-binary, Rust-based CLI that turns natural-language requests into vetted zsh commands, purpose-built for Apple-silicon MacBook Air M4 laptops.

## Features

- 🚀 **Sub-50ms cold start** with <25MB memory footprint
- 🔐 **Native Keychain integration** for secure API key storage  
- 🛡️ **Built-in safety layer** that refuses or double-checks destructive operations
- 💬 **Interactive chat-like interface** with command history
- 🎯 **Optimized for macOS** with Apple distribution compliance
- 📝 **Context-aware** - remembers previous commands and custom context
- 🔍 **Command history** with search and statistics
- ⚡ **Fast execution** with async I/O and HTTP/2 keep-alive
- 🪝 **Shell hooks** - automatic command suggestions for typos and unknown commands

## Documentation

For complete documentation, see:

- **📚 [Documentation Index](DOCS_INDEX.md)** - Navigation guide to all docs
- **🚀 [Quick Start Guide](QUICKSTART.md)** - Fast setup and basic usage  
- **🪝 [Shell Hook Guide](SHELL_HOOK.md)** - Advanced shell integration
- **🔧 [Technical Implementation](IMPLEMENTATION.md)** - Architecture and development details

Or view the manual page:

```bash
man commandgpt
```

## Installation

### From Source

```bash
git clone https://github.com/yourorg/commandgpt
cd commandgpt
make release
make install
```

### Using Cargo

```bash
cargo install --git https://github.com/yourorg/commandgpt
```

### Homebrew (Coming Soon)

```bash
brew install commandgpt
```

## Quick Start

1. **Set your OpenAI API key:**

```bash
commandgpt config set-key
```

2. **Start the interactive mode:**

```bash
commandgpt
```

3. **Ask for commands in natural language:**

```text
🤖 > find all large files in my home directory
💡 Suggested command:
find ~ -type f -size +100M -exec ls -lh {} +

📝 Find files larger than 100MB in home directory
Execute this command? [y/N]: y
```

## Usage

### Interactive Mode

```bash
commandgpt
```

Special commands in interactive mode:
- `help` - Show available commands
- `history [N]` - Show last N commands (default: 20)  
- `search <query>` - Search command history
- `clear` - Clear screen
- `exit` - Exit the program

### One-shot Mode

```bash
commandgpt "compress all images in this folder"
commandgpt "show me disk usage for each directory"
commandgpt "kill all processes containing 'node'"
```

### Configuration Management

```bash
# Set API key securely in Keychain
commandgpt config set-key

# Delete stored API key
commandgpt config delete-key

# Show current configuration
commandgpt config show
```

### Shell Hook - Intelligent Auto-Fallback

CommandGPT includes an advanced shell hook system that provides AI assistance for all command failures:

```bash
# Install and enable shell hook
commandgpt shell-hook install
commandgpt shell-hook enable
```

**Comprehensive Error Handling** - Not just "command not found":

- ✅ Unknown commands and typos
- ✅ Permission denied errors  
- ✅ File/directory not found
- ✅ Syntax errors and invalid flags
- ✅ Network connectivity issues
- ✅ Missing dependencies and packages

Example usage:

```bash
$ lss
🤖 Command 'lss' not found. Getting AI assistance...
💡 Suggested command: ls -la

$ curl invalid-domain.test
🤖 Command failed (exit 6). Getting AI assistance...  
💡 Suggested fix: curl -I google.com  # Test connectivity first
```

**Safety Features:**

- 🔒 Disabled by default for security
- 🛡️ Multi-layer safety validation
- ⏱️ 30-second timeout protection
- ✋ Always requires user confirmation

For complete shell hook documentation, see [SHELL_HOOK.md](SHELL_HOOK.md).

### Command Line Options

```bash
commandgpt [OPTIONS] [REQUEST]

Options:
  -d, --debug           Enable debug logging
      --force           Force execution without safety checks  
      --always-confirm  Always confirm commands even if auto_execute is true
      --no-context      Disable context inclusion
  -h, --help           Print help
  -V, --version        Print version
```

## Safety Features

commandGPT includes multiple layers of safety protection:

### Automatic Detection
- **Destructive commands**: `rm -rf`, `dd`, `mkfs`, etc.
- **System modifications**: `sudo` operations, service management
- **Network risks**: Piping remote scripts to shell
- **Privilege escalation**: `chmod`, `chown` on system directories

### Safety Actions
- 🚫 **Blocked**: Extremely dangerous commands are refused
- ⚠️ **Confirmation**: Potentially harmful commands require explicit approval
- ✅ **Auto-execute**: Safe read-only commands can run automatically

### Override Options
- Use `--force` flag to convert blocked commands to confirmation-required
- Commands are validated even with force flag enabled

## Configuration

Configuration files are stored in `~/.commandgpt/`:

```text
~/.commandgpt/
├── system.md          # Custom system prompt additions
├── context/           # Additional context files
│   └── development.md # Example context file
├── history.db         # Command history database
└── telemetry.txt      # Telemetry preference (optional)
```

### Custom System Prompt

Edit `~/.commandgpt/system.md` to customize the AI's behavior:

```markdown
# Custom Instructions

## Preferences
- Always use long-form flags for clarity
- Prefer `fd` over `find` when available
- Include explanations for complex commands

## Environment
- Current project: React.js web application
- Using Docker for development
- Database: PostgreSQL
```

### Context Files

Add `.md` files to `~/.commandgpt/context/` to provide additional context:

```markdown
# Project Context

## Current Task
Working on user authentication system

## Technology Stack  
- Frontend: React with TypeScript
- Backend: Node.js with Express
- Database: PostgreSQL with Prisma ORM
```

## Architecture

commandGPT is built with a modular architecture optimized for performance:

### Core Modules
- **Config**: Keychain integration, environment detection
- **OpenAI**: HTTP/2 client with retry logic and response parsing
- **Safety**: Multi-layer command validation and risk assessment
- **Executor**: Async command execution with timeout handling
- **History**: Embedded database for command persistence
- **Context**: Dynamic context building from files and environment
- **REPL**: Interactive terminal interface with colored output

### Performance Optimizations
- **Static linking** with LTO for minimal binary size
- **HTTP/2 keep-alive** for reduced API latency
- **Async I/O** throughout for non-blocking operations
- **Embedded database** (sled) for fast local storage
- **Zero-copy** string processing where possible

## Development

### Building

```bash
# Debug build
make build

# Optimized release build  
make release

# Run tests
make test

# Code quality checks
make check

# Format code
make format
```

### Project Structure

```text
src/
├── main.rs          # CLI entry point and argument parsing
├── config.rs        # Configuration and Keychain integration
├── openai.rs        # OpenAI API client with retry logic
├── safety.rs        # Command safety validation
├── executor.rs      # Async command execution
├── history.rs       # Command history management
├── context.rs       # Context building and file management
├── repl.rs          # Interactive REPL interface
└── telemetry.rs     # Optional usage analytics
```

### Testing

```bash
# Run all tests
cargo test

# Run specific module tests
cargo test safety
cargo test executor

# Run with output
cargo test -- --nocapture
```

## Security & Privacy

### API Key Storage
- Keys stored securely in macOS Keychain
- Environment variable override for CI/headless use
- No plaintext storage on disk

### Command Safety
- Static pattern matching for dangerous operations
- AST parsing to detect obfuscated commands
- User confirmation for system-altering operations

### Data Privacy
- Minimal data sent to OpenAI (user text + basic context)
- Optional telemetry with anonymized data only
- Local command history with automatic secret redaction

### Supply Chain Security
- Dependencies audited with `cargo deny`
- Reproducible builds with locked dependencies
- Code signing and notarization for distribution

## Performance Benchmarks

Target performance on M4 MacBook Air:

| Metric | Target | Actual |
|--------|--------|--------|
| Cold start | ≤50ms | ~35ms |
| Peak RSS | ≤25MB | ~18MB |
| API call | ≤2s | ~800ms |
| Command execution | ≤1s | ~200ms |

## Roadmap

### v1.1
- [ ] Plugin system for custom safety rules
- [ ] Shell auto-detection (bash, fish support)
- [ ] Command templates and snippets

### v1.2  
- [ ] Native SwiftUI menubar app
- [ ] Local model fallback (llama.cpp)
- [ ] Enhanced context from git/project files

### v2.0
- [ ] Multi-language support
- [ ] Team collaboration features
- [ ] Advanced telemetry dashboard

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality  
4. Ensure `make check` passes
5. Submit a pull request

### Development Guidelines

- Follow Rust best practices and idioms
- Add comprehensive tests for new features
- Update documentation for user-facing changes
- Maintain performance benchmarks
- Respect the 50ms cold start requirement

## Acknowledgments

- OpenAI for the ChatGPT API
- The Rust community for excellent crates
- Apple for macOS development tools
- All contributors and testers
