# Shell Hook Feature Documentation

## Overview

The CommandGPT Shell Hook is a powerful feature that automatically intercepts unknown commands in your terminal and suggests alternatives using AI. Instead of seeing "command not found", you'll get intelligent suggestions for what you might have meant.

## Features

### 🔒 **Safety First**
- **Disabled by default** - must be explicitly enabled
- **Smart filtering** - excludes dangerous commands like `sudo`, `rm`
- **Length limits** - prevents processing of very short or very long inputs
- **Typo detection** - skips likely typos of common commands
- **Confirmation required** - always asks before executing suggestions

### 🧠 **Intelligent Processing**
- **Context-aware** - uses your command history and project context
- **Natural language** - handles free-text requests like "show large files"
- **Command suggestions** - provides alternatives for unknown commands
- **Explanations** - explains what the suggested command does

### ⚡ **Performance Optimized**
- **Fast timeout** - 10-second API timeout to avoid hanging
- **Minimal overhead** - only activates for unknown commands
- **Local filtering** - pre-filters commands locally before API calls

## Installation

### 1. Install the Shell Hook

```bash
commandgpt shell-hook install
```

This adds the hook function to your `~/.zshrc` file.

### 2. Reload Your Shell

```bash
source ~/.zshrc
# or restart your terminal
```

### 3. Enable the Hook

```bash
commandgpt shell-hook enable
```

## Usage Examples

### Unknown Command Suggestions

```bash
$ lss
🤖 Command 'lss' not found. Asking CommandGPT for help...
💡 Suggested command:
  ls -la
📝 Explanation: List all files in long format (including hidden files)
Execute this command? [y/N]: y
```

### Natural Language Requests

```bash
$ find large files
🤖 Command 'find large files' not found. Asking CommandGPT for help...
💡 Suggested command:
  find . -type f -size +100M -exec ls -lh {} +
📝 Explanation: Find files larger than 100MB in current directory and subdirectories
Execute this command? [y/N]: y
```

### Package Installation

```bash
$ install node
🤖 Command 'install node' not found. Asking CommandGPT for help...
💡 Suggested command:
  brew install node
📝 Explanation: Install Node.js using Homebrew package manager
⚠️  Warning: This command modifies your system
Are you sure you want to execute this? [y/N]: y
```

## Management Commands

### Check Status
```bash
commandgpt shell-hook status
```

### Enable/Disable
```bash
commandgpt shell-hook enable
commandgpt shell-hook disable
```

### Quick Toggle (if hook is installed)
```bash
commandgpt-hook-on   # Enable hook
commandgpt-hook-off  # Disable hook
commandgpt-hook-status  # Show status
```

### Generate Script
```bash
commandgpt shell-hook generate
```

### Uninstall
```bash
commandgpt shell-hook uninstall
```

## Configuration

The hook system uses safe defaults:

- **Minimum length**: 3 characters
- **Maximum length**: 200 characters  
- **API timeout**: 10 seconds
- **Always confirm**: Yes (safety feature)
- **Excluded patterns**: `sudo`, `su`, `rm`, `chmod`, `chown`

## Safety Features

### Automatic Exclusions

The hook will **never** process commands that:
- Start with dangerous patterns (`sudo`, `rm`, etc.)
- Are too short (< 3 characters) 
- Are too long (> 200 characters)
- Look like URLs (`http://`, `https://`)
- Are likely typos of common commands

### Safety Validation

All suggestions go through CommandGPT's safety system:
- 🟢 **Safe commands** (read-only) can auto-execute
- 🟡 **Moderate risk** commands require confirmation
- 🔴 **Dangerous commands** are blocked entirely

### User Control

- Hook is **disabled by default**
- All commands require confirmation (configurable)
- Easy enable/disable without uninstalling
- Full uninstall removes all traces

## Troubleshooting

### Hook Not Working

1. Check if it's enabled:
   ```bash
   commandgpt shell-hook status
   ```

2. Verify installation:
   ```bash
   grep -n "CommandGPT Shell Hook" ~/.zshrc
   ```

3. Check environment variable:
   ```bash
   echo $COMMANDGPT_HOOK_ENABLED
   ```

### API Timeouts

If you experience frequent timeouts:

1. Check your internet connection
2. Verify your OpenAI API key is valid:
   ```bash
   commandgpt config show
   ```

### Unwanted Activations

If the hook triggers for commands you don't want:

1. Temporarily disable: `commandgpt-hook-off`
2. Add patterns to exclusion list (future feature)
3. Uninstall if not needed: `commandgpt shell-hook uninstall`

## Technical Details

### How It Works

1. **Shell Integration**: Uses zsh's `command_not_found_handler` function
2. **Smart Filtering**: Pre-processes commands locally for safety
3. **AI Processing**: Sends enhanced requests to OpenAI API
4. **Safety Validation**: All suggestions go through safety checks
5. **User Confirmation**: Requires explicit approval before execution

### File Locations

- Hook script: Added to `~/.zshrc`
- Configuration: In-memory (future: config file)
- History: Shared with main CommandGPT history

### Dependencies

- zsh shell (macOS default)
- CommandGPT binary in PATH
- Valid OpenAI API key
- Internet connection

## Best Practices

### When to Use

✅ **Good for:**
- Learning new commands
- Finding alternatives to GUI tools
- Complex file operations
- System administration tasks
- Package management

❌ **Avoid for:**
- Production scripts
- Automated systems
- Critical operations without review
- When you need exact command syntax

### Security Tips

1. **Always review** suggestions before executing
2. **Start with disable** then enable when comfortable
3. **Use in development** environments first
4. **Disable for scripts** and automation
5. **Monitor usage** in shared environments

## Future Enhancements

- Configurable exclusion patterns
- Local command caching
- Bash and Fish shell support
- Team-shared configurations
- Offline mode with local models
- Integration with package managers
