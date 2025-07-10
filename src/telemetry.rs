use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub properties: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandExecutionEvent {
    pub command_hash: String, // SHA256 hash for privacy
    pub success: bool,
    pub duration_ms: u64,
    pub command_length: usize,
    pub has_pipes: bool,
    pub has_redirects: bool,
    pub is_sudo: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionEvent {
    pub session_id: String,
    pub session_duration_ms: u64,
    pub commands_executed: u32,
    pub successful_commands: u32,
}

pub struct TelemetryCollector {
    enabled: bool,
    session_id: String,
    session_start: DateTime<Utc>,
    events: Vec<TelemetryEvent>,
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetryCollector {
    pub fn new() -> Self {
        Self {
            enabled: false, // Opt-in by default
            session_id: uuid::Uuid::new_v4().to_string(),
            session_start: Utc::now(),
            events: Vec::new(),
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
        log::info!("Telemetry enabled for session {}", self.session_id);
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        log::info!("Telemetry disabled");
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn record_command_execution(
        &mut self,
        command: &str,
        success: bool,
        duration: Duration,
    ) {
        if !self.enabled {
            return;
        }

        let command_hash = self.hash_command(command);
        
        let event = CommandExecutionEvent {
            command_hash,
            success,
            duration_ms: duration.as_millis() as u64,
            command_length: command.len(),
            has_pipes: command.contains('|'),
            has_redirects: command.contains('>') || command.contains('<'),
            is_sudo: command.trim_start().starts_with("sudo"),
        };

        self.add_event("command_execution", serde_json::to_value(event).unwrap());
    }

    pub fn record_api_call(&mut self, model: &str, tokens_used: u32, duration: Duration) {
        if !self.enabled {
            return;
        }

        let properties = serde_json::json!({
            "model": model,
            "tokens_used": tokens_used,
            "duration_ms": duration.as_millis(),
        });

        self.add_event("api_call", properties);
    }

    pub fn record_error(&mut self, error_type: &str, error_message: &str) {
        if !self.enabled {
            return;
        }

        let properties = serde_json::json!({
            "error_type": error_type,
            "error_message_hash": self.hash_string(error_message),
        });

        self.add_event("error", properties);
    }

    pub fn record_safety_action(&mut self, command_hash: &str, action: &str, reason: &str) {
        if !self.enabled {
            return;
        }

        let properties = serde_json::json!({
            "command_hash": command_hash,
            "action": action, // "blocked", "confirmed", "auto_executed"
            "reason_hash": self.hash_string(reason),
        });

        self.add_event("safety_action", properties);
    }

    pub fn end_session(&mut self, commands_executed: u32, successful_commands: u32) {
        if !self.enabled {
            return;
        }

        let session_duration = Utc::now().signed_duration_since(self.session_start);
        
        let event = SessionEvent {
            session_id: self.session_id.clone(),
            session_duration_ms: session_duration.num_milliseconds() as u64,
            commands_executed,
            successful_commands,
        };

        self.add_event("session_end", serde_json::to_value(event).unwrap());
        
        // In a real implementation, this would send the events to a telemetry service
        self.flush_events();
    }

    fn add_event(&mut self, event_type: &str, properties: serde_json::Value) {
        let event = TelemetryEvent {
            event_type: event_type.to_string(),
            timestamp: Utc::now(),
            properties,
        };

        self.events.push(event);
        
        // Auto-flush if we have too many events
        if self.events.len() >= 100 {
            self.flush_events();
        }
    }

    fn flush_events(&mut self) {
        if self.events.is_empty() {
            return;
        }

        log::debug!("Flushing {} telemetry events", self.events.len());
        
        // In a real implementation, this would send events to a telemetry service
        // For now, we just log them in debug mode
        for event in &self.events {
            log::debug!("Telemetry event: {} - {:?}", event.event_type, event.properties);
        }

        self.events.clear();
    }

    fn hash_command(&self, command: &str) -> String {
        // Remove potentially sensitive information before hashing
        let cleaned = self.clean_command_for_telemetry(command);
        self.hash_string(&cleaned)
    }

    fn clean_command_for_telemetry(&self, command: &str) -> String {
        let mut cleaned = command.to_string();
        
        // Remove potential secrets (API keys, passwords, etc.)
        // This is a simple heuristic - in practice, you'd want more sophisticated detection
        let secret_patterns = vec![
            regex::Regex::new(r"[A-Za-z0-9_\-]{20,}").unwrap(), // Long alphanumeric strings
            regex::Regex::new(r"sk-[A-Za-z0-9]{48}").unwrap(),   // OpenAI API keys
            regex::Regex::new(r"[A-Za-z0-9+/]{40,}={0,2}").unwrap(), // Base64 strings
        ];

        for pattern in secret_patterns {
            cleaned = pattern.replace_all(&cleaned, "[REDACTED]").to_string();
        }

        // Remove file paths that might contain usernames
        if let Some(home_dir) = dirs_next::home_dir() {
            let home_str = home_dir.to_string_lossy();
            cleaned = cleaned.replace(home_str.as_ref(), "~");
        }

        cleaned
    }

    fn hash_string(&self, input: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        input.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

// Global telemetry instance
static mut TELEMETRY: Option<TelemetryCollector> = None;
static INIT: std::sync::Once = std::sync::Once::new();

fn get_telemetry() -> &'static mut TelemetryCollector {
    unsafe {
        INIT.call_once(|| {
            TELEMETRY = Some(TelemetryCollector::new());
        });
        TELEMETRY.as_mut().unwrap()
    }
}

// Public API functions
pub fn enable_telemetry() {
    get_telemetry().enable();
}

pub fn disable_telemetry() {
    get_telemetry().disable();
}

pub fn is_telemetry_enabled() -> bool {
    get_telemetry().is_enabled()
}

pub async fn record_command_execution(command: &str, success: bool, duration: Duration) {
    get_telemetry().record_command_execution(command, success, duration);
}

pub async fn record_api_call(model: &str, tokens_used: u32, duration: Duration) {
    get_telemetry().record_api_call(model, tokens_used, duration);
}

pub async fn record_error(error_type: &str, error_message: &str) {
    get_telemetry().record_error(error_type, error_message);
}

pub async fn record_safety_action(command_hash: &str, action: &str, reason: &str) {
    get_telemetry().record_safety_action(command_hash, action, reason);
}

pub async fn end_session(commands_executed: u32, successful_commands: u32) {
    get_telemetry().end_session(commands_executed, successful_commands);
}

// Configuration management for telemetry
pub fn load_telemetry_preferences() -> Result<bool> {
    // Check if user has opted in/out
    let config_dir = dirs_next::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?
        .join(".commandgpt");

    let telemetry_file = config_dir.join("telemetry.txt");
    
    if telemetry_file.exists() {
        let content = std::fs::read_to_string(telemetry_file)?;
        Ok(content.trim() == "enabled")
    } else {
        // Default to disabled
        Ok(false)
    }
}

pub fn save_telemetry_preference(enabled: bool) -> Result<()> {
    let config_dir = dirs_next::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?
        .join(".commandgpt");

    std::fs::create_dir_all(&config_dir)?;
    
    let telemetry_file = config_dir.join("telemetry.txt");
    let content = if enabled { "enabled" } else { "disabled" };
    
    std::fs::write(telemetry_file, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_cleaning() {
        let collector = TelemetryCollector::new();
        
        // Test API key redaction
        let cmd_with_key = "curl -H 'Authorization: Bearer sk-1234567890abcdef1234567890abcdef12345678' https://api.openai.com";
        let cleaned = collector.clean_command_for_telemetry(cmd_with_key);
        assert!(cleaned.contains("[REDACTED]"));
        assert!(!cleaned.contains("sk-1234567890abcdef1234567890abcdef12345678"));

        // Test normal command unchanged
        let normal_cmd = "ls -la";
        let cleaned_normal = collector.clean_command_for_telemetry(normal_cmd);
        assert_eq!(cleaned_normal, normal_cmd);
    }

    #[test]
    fn test_hash_consistency() {
        let collector = TelemetryCollector::new();
        let input = "test command";
        
        let hash1 = collector.hash_string(input);
        let hash2 = collector.hash_string(input);
        
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, collector.hash_string("different command"));
    }

    #[test]
    fn test_event_recording() {
        let mut collector = TelemetryCollector::new();
        collector.enable();
        
        collector.record_command_execution("ls -la", true, Duration::from_millis(100));
        assert_eq!(collector.events.len(), 1);
        assert_eq!(collector.events[0].event_type, "command_execution");
    }
}
