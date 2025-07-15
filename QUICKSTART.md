# Quick Start Guide

Welcome to CommandGPT! This guide will get you up and running in just a few minutes.

## Prerequisites

1. **macOS** (Apple Silicon or Intel)
2. **Rust toolchain** (install if needed):
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source ~/.cargo/env
   ```
3. **OpenAI API key** (get one from [OpenAI Platform](https://platform.openai.com/))

## Installation

### Option 1: Build from Source
```bash
# Clone the repository
git clone https://github.com/dMillerus/commandGPT
cd commandGPT

# Build optimized version
make release

# Install system-wide
make install
```

### Option 2: Direct Cargo Install
```bash
cargo install --git https://github.com/dMillerus/commandGPT
```

## Initial Setup

1. **Configure your API key**:
   ```bash
   commandgpt config set-key
   ```
   Enter your OpenAI API key when prompted (it will be stored securely in Keychain).

2. **Verify installation**:
   ```bash
   commandgpt config show
   ```

## Basic Usage

### Interactive Mode
```bash
commandgpt
```

Try these example requests:
- `find all large files in my Downloads folder`
- `show me disk usage by directory`
- `list all running processes containing "node"`
- `compress this directory into a tar.gz file`

### One-Shot Mode
```bash
commandgpt "show me the 10 largest files"
commandgpt "what processes are using the most CPU"
```

## Safety Features

CommandGPT will:
- ✅ **Auto-execute** safe commands (ls, find, grep, etc.)
- ⚠️ **Ask for confirmation** on potentially harmful commands
- 🚫 **Block** dangerous commands (rm -rf /, fork bombs, etc.)

You can override with `--force` if needed:
```bash
commandgpt --force "sudo systemctl restart nginx"
```

## Customization

### Add Context
Create files in `~/.commandgpt/context/` to give the AI more information about your environment:

```bash
# Create a project context file
cat > ~/.commandgpt/context/current-project.md << EOF
# Current Project
Working on a React.js application with:
- TypeScript for type safety
- Docker for development
- PostgreSQL database
- Jest for testing
EOF
```

### Customize System Prompt
Edit `~/.commandgpt/system.md` to change how the AI behaves:

```markdown
# My Preferences
- Always use long-form flags (--verbose instead of -v)
- Prefer modern tools (fd, rg, bat) when available
- Include explanations for complex commands
```

## Useful Commands

| Command | Description |
|---------|-------------|
| `help` | Show available commands |
| `history` | Show recent command history |
| `history 50` | Show last 50 commands |
| `search git` | Search history for "git" commands |
| `clear` | Clear the screen |
| `exit` | Exit the program |

## Troubleshooting

### API Key Issues
```bash
# Check if key is configured
commandgpt config show

# Reset API key
commandgpt config delete-key
commandgpt config set-key
```

### Permission Errors
```bash
# Make sure binary is executable
chmod +x /usr/local/bin/commandgpt

# Or run from local build
./target/release/commandgpt
```

### Debug Mode

```bash
# Enable debug logging
commandgpt --debug
```

### Development Builds

```bash
# For development: clean system and rebuild from source
./dev-clean.sh

# Keep your configuration while rebuilding
./dev-clean.sh --keep-config

# Clean old installations without rebuilding
./dev-clean.sh --skip-build
```

The `dev-clean.sh` script ensures you're testing the latest version by removing old installations and rebuilding from source.

## Next Steps

- Explore the full documentation in [README.md](README.md)
- Check out [IMPLEMENTATION.md](IMPLEMENTATION.md) for technical details
- For development work, see the Development section in [README.md](README.md)
- Use `./dev-clean.sh` for clean development builds
- Customize your context files for better AI responses
- Try the various command-line options and flags

Happy commanding! 🚀
