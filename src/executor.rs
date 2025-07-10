use anyhow::{Context, Result};
use std::process::Stdio;
use std::time::Duration;
use std::io::Write;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command as TokioCommand};
use tokio::time::timeout;
use tempfile::NamedTempFile;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[derive(Debug)]
pub struct ExecutionResult {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

pub struct CommandExecutor {
    timeout_duration: Duration,
}

impl Default for CommandExecutor {
    fn default() -> Self {
        Self {
            timeout_duration: Duration::from_secs(300), // 5 minutes default
        }
    }
}

impl CommandExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self {
            timeout_duration: Duration::from_secs(timeout_secs),
        }
    }

    pub async fn execute(&self, command: &str) -> Result<ExecutionResult> {
        let start_time = std::time::Instant::now();
        
        log::debug!("Executing command: {}", command);
        
        // Check if this is a multi-line script
        if command.lines().count() > 1 {
            self.execute_script(command).await
        } else {
            self.execute_single_command(command).await
        }.map(|mut result| {
            result.duration = start_time.elapsed();
            result
        })
    }

    async fn execute_single_command(&self, command: &str) -> Result<ExecutionResult> {
        let mut cmd = TokioCommand::new("/bin/zsh");
        cmd.arg("-c")
           .arg(command)
           .stdout(Stdio::piped())
           .stderr(Stdio::piped())
           .stdin(Stdio::null());

        log::debug!("Spawning command: /bin/zsh -c '{}'", command);

        let child = cmd.spawn()
            .context("Failed to spawn command")?;

        self.wait_for_completion(child).await
    }

    async fn execute_script(&self, script: &str) -> Result<ExecutionResult> {
        // Create a temporary script file
        let mut temp_file = NamedTempFile::new()
            .context("Failed to create temporary script file")?;

        // Write the script content
        temp_file.write_all(b"#!/bin/zsh\nset -e\n\n")
            .context("Failed to write script header")?;
        temp_file.write_all(script.as_bytes())
            .context("Failed to write script content")?;
        temp_file.flush()
            .context("Failed to flush script file")?;

        // Make the script executable
        let path = temp_file.path();
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;

        // Execute the script
        let mut cmd = TokioCommand::new(path);
        cmd.stdout(Stdio::piped())
           .stderr(Stdio::piped())
           .stdin(Stdio::null());

        log::debug!("Executing script: {}", path.display());

        let child = cmd.spawn()
            .context("Failed to spawn script")?;

        self.wait_for_completion(child).await
    }

    async fn wait_for_completion(&self, mut child: Child) -> Result<ExecutionResult> {
        // Set up stdout and stderr capture
        let stdout = child.stdout.take()
            .context("Failed to capture stdout")?;
        let stderr = child.stderr.take()
            .context("Failed to capture stderr")?;

        // Run with timeout
        let result = timeout(self.timeout_duration, async {
            // Capture output streams
            let (stdout_output, stderr_output, exit_status) = tokio::try_join!(
                self.read_stream(stdout),
                self.read_stream(stderr),
                async { child.wait().await.map_err(|e| anyhow::anyhow!(e)) }
            )?;

            Ok::<(String, String, std::process::ExitStatus), anyhow::Error>((
                stdout_output,
                stderr_output, 
                exit_status
            ))
        }).await;

        match result {
            Ok(Ok((stdout_output, stderr_output, exit_status))) => {
                Ok(ExecutionResult {
                    success: exit_status.success(),
                    exit_code: exit_status.code(),
                    stdout: stdout_output,
                    stderr: stderr_output,
                    duration: Duration::default(), // Will be set by caller
                })
            }
            Ok(Err(e)) => Err(e),
            Err(_) => {
                // Timeout occurred, kill the process
                if let Err(kill_err) = child.kill().await {
                    log::warn!("Failed to kill timed-out process: {}", kill_err);
                }
                
                anyhow::bail!(
                    "Command timed out after {} seconds", 
                    self.timeout_duration.as_secs()
                );
            }
        }
    }

    async fn read_stream<R>(&self, mut reader: R) -> Result<String>
    where
        R: AsyncReadExt + Unpin,
    {
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).await
            .context("Failed to read from stream")?;
        
        Ok(String::from_utf8_lossy(&buffer).to_string())
    }

    pub async fn test_command_exists(&self, command: &str) -> bool {
        let result = self.execute(&format!("which {}", command)).await;
        match result {
            Ok(exec_result) => exec_result.success,
            Err(_) => false,
        }
    }

    pub async fn get_command_help(&self, command: &str) -> Result<String> {
        // Try different help options
        let help_commands = vec![
            format!("{} --help", command),
            format!("{} -h", command),
            format!("man {}", command),
        ];

        for help_cmd in help_commands {
            if let Ok(result) = self.execute(&help_cmd).await {
                if result.success && !result.stdout.is_empty() {
                    return Ok(result.stdout);
                }
            }
        }

        anyhow::bail!("No help available for command: {}", command)
    }

    pub async fn validate_syntax(&self, command: &str) -> Result<bool> {
        // Use shell's built-in syntax checking
        let check_cmd = format!("/bin/zsh -n -c '{}'", command.replace("'", "'\"'\"'"));
        
        match self.execute(&check_cmd).await {
            Ok(result) => Ok(result.success),
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simple_command() {
        let executor = CommandExecutor::new();
        let result = executor.execute("echo 'hello world'").await.unwrap();
        
        assert!(result.success);
        assert_eq!(result.stdout.trim(), "hello world");
        assert!(result.stderr.is_empty());
    }

    #[tokio::test]
    async fn test_command_with_error() {
        let executor = CommandExecutor::new();
        let result = executor.execute("ls /nonexistent/directory").await.unwrap();
        
        assert!(!result.success);
        assert!(!result.stderr.is_empty());
    }

    #[tokio::test]
    async fn test_multiline_script() {
        let executor = CommandExecutor::new();
        let script = r#"
echo "Line 1"
echo "Line 2"
echo "Done"
"#;
        
        let result = executor.execute(script).await.unwrap();
        
        assert!(result.success);
        assert!(result.stdout.contains("Line 1"));
        assert!(result.stdout.contains("Line 2"));
        assert!(result.stdout.contains("Done"));
    }

    #[tokio::test]
    async fn test_command_exists() {
        let executor = CommandExecutor::new();
        
        // Test existing command
        assert!(executor.test_command_exists("ls").await);
        
        // Test non-existing command
        assert!(!executor.test_command_exists("nonexistentcommand12345").await);
    }

    #[tokio::test]
    async fn test_syntax_validation() {
        let executor = CommandExecutor::new();
        
        // Valid syntax
        assert!(executor.validate_syntax("ls -la").await.unwrap());
        
        // Invalid syntax
        assert!(!executor.validate_syntax("ls -la |").await.unwrap());
    }

    #[tokio::test]
    async fn test_timeout() {
        let executor = CommandExecutor::with_timeout(1); // 1 second timeout
        let result = executor.execute("sleep 5").await;
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }
}
