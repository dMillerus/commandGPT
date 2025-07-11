use assert_cmd::Command;
use predicates::prelude::*;
use serial_test::serial;
use std::env;
use tempfile::TempDir;

/// Integration tests for the CommandGPT application
/// These tests run the actual binary and test end-to-end functionality

#[test]
#[serial]
fn test_help_command() {
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.arg("--help");
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("CommandGPT"))
        .stdout(predicate::str::contains("Usage"));
}

#[test]
#[serial]
fn test_version_command() {
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.arg("--version");
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
#[serial]
fn test_config_show() {
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["config", "show"]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Configuration"));
}

#[test]
#[serial]
fn test_history_empty() {
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["history"])
        .env("HOME", temp_dir.path());
    
    cmd.assert()
        .success();
}

#[test]
#[serial]
fn test_clear_history() {
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["clear"])
        .env("HOME", temp_dir.path());
    
    cmd.assert()
        .success();
}

#[test]
#[serial]
fn test_invalid_subcommand() {
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.arg("invalid-command");
    
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
#[serial]
fn test_debug_flag() {
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["--debug", "config", "show"])
        .env("HOME", temp_dir.path());
    
    cmd.assert()
        .success();
}

#[test]
#[serial]
fn test_one_shot_mode_without_api_key() {
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.arg("list files")
        .env("HOME", temp_dir.path());
    
    // This should fail without an API key
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("API key"));
}

#[test]
#[serial]
fn test_force_flag() {
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["--force", "config", "show"])
        .env("HOME", temp_dir.path());
    
    cmd.assert()
        .success();
}

#[test]
#[serial]
fn test_no_context_flag() {
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["--no-context", "config", "show"])
        .env("HOME", temp_dir.path());
    
    cmd.assert()
        .success();
}

#[test]
#[serial]
fn test_config_directory_creation() {
    let temp_dir = TempDir::new().unwrap();
    let home_dir = temp_dir.path();
    
    // Run any command that would trigger config directory creation
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["config", "show"])
        .env("HOME", home_dir);
    
    cmd.assert()
        .success();
    
    // Verify config directory was created
    let config_dir = home_dir.join(".commandgpt");
    assert!(config_dir.exists());
}

#[test]
#[serial]
fn test_environment_variable_override() {
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["config", "show"])
        .env("HOME", temp_dir.path())
        .env("COMMANDGPT_MODEL", "gpt-4");
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("gpt-4"));
}

#[test]
#[serial]
fn test_config_delete_key_without_key() {
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["config", "delete-key"])
        .env("HOME", temp_dir.path());
    
    // Should handle gracefully even if no key exists
    cmd.assert()
        .success();
}

#[test]
#[serial]
fn test_multiple_flags_combination() {
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["--debug", "--force", "--no-context", "config", "show"])
        .env("HOME", temp_dir.path());
    
    cmd.assert()
        .success();
}

#[test]
#[serial]
fn test_history_with_count() {
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["history", "--count", "5"])
        .env("HOME", temp_dir.path());
    
    cmd.assert()
        .success();
}

#[test]
#[serial]
fn test_binary_size() {
    let binary_path = assert_cmd::cargo::cargo_bin("commandgpt");
    let metadata = std::fs::metadata(binary_path).unwrap();
    
    // Binary should be reasonably sized (less than 50MB)
    assert!(metadata.len() < 50 * 1024 * 1024);
}

#[test]
#[serial]
fn test_concurrent_access() {
    let temp_dir = TempDir::new().unwrap();
    
    // Run multiple instances simultaneously
    let mut handles = vec![];
    
    for _ in 0..3 {
        let temp_path = temp_dir.path().to_owned();
        let handle = std::thread::spawn(move || {
            let mut cmd = Command::cargo_bin("commandgpt").unwrap();
            cmd.args(&["config", "show"])
                .env("HOME", temp_path);
            
            cmd.assert().success();
        });
        handles.push(handle);
    }
    
    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }
}

#[cfg(target_os = "macos")]
#[test]
#[serial]
fn test_macos_specific_features() {
    let temp_dir = TempDir::new().unwrap();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.args(&["config", "show"])
        .env("HOME", temp_dir.path());
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("macOS"));
}

// Performance test - ensure the binary starts quickly
#[test]
#[serial]
fn test_startup_performance() {
    let start = std::time::Instant::now();
    
    let mut cmd = Command::cargo_bin("commandgpt").unwrap();
    cmd.arg("--help");
    
    cmd.assert().success();
    
    let duration = start.elapsed();
    // Should start within 5 seconds
    assert!(duration.as_secs() < 5);
}

// Test error handling for invalid arguments
#[test]
#[serial]
fn test_invalid_argument_combinations() {
    let test_cases = vec![
        vec!["--invalid-flag"],
        vec!["config", "invalid-action"],
        vec!["history", "--count", "abc"], // Non-numeric count
        vec!["history", "--count", "-1"],  // Negative count
    ];
    
    for args in test_cases {
        let mut cmd = Command::cargo_bin("commandgpt").unwrap();
        cmd.args(&args);
        
        cmd.assert().failure();
    }
}
