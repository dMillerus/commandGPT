use thiserror::Error;

pub type Result<T> = std::result::Result<T, CommandGPTError>;

#[derive(Error, Debug)]
pub enum CommandGPTError {
    #[error("Configuration error: {message}")]
    ConfigError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Configuration directory error: {message}")]
    ConfigDirectoryError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("API error: {message}")]
    ApiError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Network error: {message}")]
    NetworkError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("History error: {message}")]
    HistoryError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Safety error: {message}")]
    SafetyError {
        message: String,
        reason: String,
    },

    #[error("Execution error: {message}")]
    ExecutionError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Input error: {message}")]
    InputError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Output error: {message}")]
    OutputError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Parse error: {message}")]
    ParseError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("System error: {message}")]
    SystemError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Keychain error: {message}")]
    KeychainError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Unknown error: {message}")]
    Unknown {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

impl CommandGPTError {
    /// Get a user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            Self::ConfigError { message, .. } => {
                format!("Configuration error: {}", message)
            }
            Self::ConfigDirectoryError { message, .. } => {
                format!("Could not create configuration directory: {}", message)
            }
            Self::ApiError { message, .. } => {
                format!("OpenAI API error: {}", message)
            }
            Self::NetworkError { message, .. } => {
                format!("Network error: {}", message)
            }
            Self::HistoryError { message, .. } => {
                format!("History database error: {}", message)
            }
            Self::SafetyError { message, reason } => {
                format!("Command blocked for safety: {} ({})", message, reason)
            }
            Self::ExecutionError { message, .. } => {
                format!("Command execution failed: {}", message)
            }
            Self::InputError { message, .. } => {
                format!("Input error: {}", message)
            }
            Self::OutputError { message, .. } => {
                format!("Output error: {}", message)
            }
            Self::ParseError { message, .. } => {
                format!("Parse error: {}", message)
            }
            Self::SystemError { message, .. } => {
                format!("System error: {}", message)
            }
            Self::KeychainError { message, .. } => {
                format!("Keychain error: {}", message)
            }
            Self::Unknown { message, .. } => {
                format!("Unknown error: {}", message)
            }
        }
    }

    /// Get an appropriate exit code for the error
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::ConfigError { .. } => 1,
            Self::ConfigDirectoryError { .. } => 1,
            Self::ApiError { .. } => 2,
            Self::NetworkError { .. } => 3,
            Self::HistoryError { .. } => 4,
            Self::SafetyError { .. } => 5,
            Self::ExecutionError { .. } => 6,
            Self::InputError { .. } => 7,
            Self::OutputError { .. } => 8,
            Self::ParseError { .. } => 11,
            Self::SystemError { .. } => 9,
            Self::KeychainError { .. } => 10,
            Self::Unknown { .. } => 99,
        }
    }

    /// Check if the error is recoverable
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::NetworkError { .. } => true,
            Self::ApiError { .. } => true,
            Self::InputError { .. } => true,
            Self::OutputError { .. } => true,
            Self::ParseError { .. } => true,
            _ => false,
        }
    }
}

impl From<std::io::Error> for CommandGPTError {
    fn from(error: std::io::Error) -> Self {
        CommandGPTError::SystemError {
            message: format!("I/O error: {}", error),
            source: Some(Box::new(error)),
        }
    }
}

impl From<serde_json::Error> for CommandGPTError {
    fn from(error: serde_json::Error) -> Self {
        CommandGPTError::ParseError {
            message: format!("JSON parse error: {}", error),
            source: Some(Box::new(error)),
        }
    }
}

impl From<anyhow::Error> for CommandGPTError {
    fn from(error: anyhow::Error) -> Self {
        CommandGPTError::SystemError {
            message: format!("System error: {}", error),
            source: None, // Can't box anyhow::Error as it doesn't implement the right trait
        }
    }
}

impl From<sled::Error> for CommandGPTError {
    fn from(error: sled::Error) -> Self {
        CommandGPTError::HistoryError {
            message: format!("Database error: {}", error),
            source: Some(Box::new(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn test_error_creation() {
        let error = CommandGPTError::ConfigError {
            message: "Test config error".to_string(),
            source: None,
        };

        assert_eq!(error.to_string(), "Configuration error: Test config error");
        assert_eq!(error.exit_code(), 1);
        assert!(!error.is_recoverable());
    }

    #[test]
    fn test_user_message() {
        let error = CommandGPTError::ApiError {
            message: "Invalid API key".to_string(),
            source: None,
        };

        let user_msg = error.user_message();
        assert!(user_msg.contains("OpenAI API error"));
        assert!(user_msg.contains("Invalid API key"));
    }

    #[test]
    fn test_exit_codes() {
        let test_cases = vec![
            (CommandGPTError::ConfigError { message: "test".to_string(), source: None }, 1),
            (CommandGPTError::ApiError { message: "test".to_string(), source: None }, 2),
            (CommandGPTError::NetworkError { message: "test".to_string(), source: None }, 3),
            (CommandGPTError::HistoryError { message: "test".to_string(), source: None }, 4),
            (CommandGPTError::SafetyError { message: "test".to_string(), reason: "test".to_string() }, 5),
            (CommandGPTError::ExecutionError { message: "test".to_string(), source: None }, 6),
            (CommandGPTError::Unknown { message: "test".to_string(), source: None }, 99),
        ];

        for (error, expected_code) in test_cases {
            assert_eq!(error.exit_code(), expected_code);
        }
    }

    #[test]
    fn test_recoverable_errors() {
        let recoverable_errors = vec![
            CommandGPTError::NetworkError { message: "test".to_string(), source: None },
            CommandGPTError::ApiError { message: "test".to_string(), source: None },
            CommandGPTError::InputError { message: "test".to_string(), source: None },
            CommandGPTError::OutputError { message: "test".to_string(), source: None },
        ];

        for error in recoverable_errors {
            assert!(error.is_recoverable());
        }

        let non_recoverable_errors = vec![
            CommandGPTError::ConfigError { message: "test".to_string(), source: None },
            CommandGPTError::SafetyError { message: "test".to_string(), reason: "test".to_string() },
            CommandGPTError::SystemError { message: "test".to_string(), source: None },
        ];

        for error in non_recoverable_errors {
            assert!(!error.is_recoverable());
        }
    }

    #[test]
    fn test_safety_error() {
        let error = CommandGPTError::SafetyError {
            message: "Dangerous command".to_string(),
            reason: "rm -rf detected".to_string(),
        };

        let user_msg = error.user_message();
        assert!(user_msg.contains("Dangerous command"));
        assert!(user_msg.contains("rm -rf detected"));
        assert_eq!(error.exit_code(), 5);
    }

    #[test]
    fn test_error_with_source() {
        use std::io;

        let io_error = io::Error::new(io::ErrorKind::NotFound, "File not found");
        let error = CommandGPTError::SystemError {
            message: "Could not read file".to_string(),
            source: Some(Box::new(io_error)),
        };

        assert!(error.to_string().contains("Could not read file"));
        assert!(error.source().is_some());
    }

    #[test]
    fn test_error_display() {
        let errors = vec![
            CommandGPTError::ConfigError { message: "config".to_string(), source: None },
            CommandGPTError::ApiError { message: "api".to_string(), source: None },
            CommandGPTError::NetworkError { message: "network".to_string(), source: None },
            CommandGPTError::HistoryError { message: "history".to_string(), source: None },
            CommandGPTError::SafetyError { message: "safety".to_string(), reason: "reason".to_string() },
            CommandGPTError::ExecutionError { message: "execution".to_string(), source: None },
            CommandGPTError::InputError { message: "input".to_string(), source: None },
            CommandGPTError::OutputError { message: "output".to_string(), source: None },
            CommandGPTError::SystemError { message: "system".to_string(), source: None },
            CommandGPTError::KeychainError { message: "keychain".to_string(), source: None },
            CommandGPTError::Unknown { message: "unknown".to_string(), source: None },
        ];

        for error in errors {
            let display = error.to_string();
            assert!(!display.is_empty());
            assert!(display.len() > 5); // Should have meaningful content
        }
    }
}