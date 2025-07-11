#[cfg(test)]
mod hook_tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::hook::{HookConfig, ShellHook, generate_hook_script};
    use tempfile::TempDir;

    #[test]
    fn test_hook_config_defaults() {
        let config = HookConfig::default();
        
        assert!(!config.enabled); // Disabled by default for safety
        assert_eq!(config.min_length, 3);
        assert_eq!(config.max_length, 200);
        assert!(config.always_confirm);
        assert_eq!(config.api_timeout, 10);
        assert!(config.excluded_patterns.contains(&"sudo".to_string()));
    }

    #[test]
    fn test_should_process_command() {
        let app_config = AppConfig::default();
        let hook_config = HookConfig::default();
        let hook = ShellHook::new(&app_config, hook_config);

        // Should process normal requests
        assert!(hook.should_process_command("show large files"));
        assert!(hook.should_process_command("install nodejs"));
        
        // Should not process short commands
        assert!(!hook.should_process_command("ls"));
        assert!(!hook.should_process_command("cd"));
        
        // Should not process excluded patterns
        assert!(!hook.should_process_command("sudo rm -rf /"));
        assert!(!hook.should_process_command("su root"));
        
        // Should not process URLs
        assert!(!hook.should_process_command("https://example.com"));
        assert!(!hook.should_process_command("http://test.org"));
        
        // Should not process very long commands
        let long_command = "a".repeat(250);
        assert!(!hook.should_process_command(&long_command));
    }

    #[test]
    fn test_edit_distance() {
        let app_config = AppConfig::default();
        let hook_config = HookConfig::default();
        let hook = ShellHook::new(&app_config, hook_config);

        assert_eq!(hook.edit_distance("cat", "cat"), 0);
        assert_eq!(hook.edit_distance("cat", "bat"), 1);
        assert_eq!(hook.edit_distance("ls", "lss"), 1);
        assert_eq!(hook.edit_distance("cd", "cdd"), 1);
        assert_eq!(hook.edit_distance("hello", "world"), 4);
    }

    #[test]
    fn test_is_likely_typo() {
        let app_config = AppConfig::default();
        let hook_config = HookConfig::default();
        let hook = ShellHook::new(&app_config, hook_config);

        // Common typos should be detected
        assert!(hook.is_likely_typo("lss"));   // ls -> lss
        assert!(hook.is_likely_typo("catt"));  // cat -> catt
        assert!(hook.is_likely_typo("cdd"));   // cd -> cdd
        
        // Real commands should not be considered typos
        assert!(!hook.is_likely_typo("list"));
        assert!(!hook.is_likely_typo("show"));
        assert!(!hook.is_likely_typo("find"));
        
        // Very short strings should not be typos
        assert!(!hook.is_likely_typo("ab"));
    }

    #[test]
    fn test_generate_hook_script() {
        let config = HookConfig {
            enabled: true,
            ..Default::default()
        };
        
        let script = generate_hook_script(&config);
        
        // Check essential components
        assert!(script.contains("command_not_found_handler"));
        assert!(script.contains("COMMANDGPT_HOOK_ENABLED=true"));
        assert!(script.contains("commandgpt hook"));
        assert!(script.contains("commandgpt-hook-on"));
        assert!(script.contains("commandgpt-hook-off"));
        assert!(script.contains("commandgpt-hook-status"));
    }

    #[test]
    fn test_generate_hook_script_disabled() {
        let config = HookConfig {
            enabled: false,
            ..Default::default()
        };
        
        let script = generate_hook_script(&config);
        
        // Should generate disabled script
        assert!(script.contains("COMMANDGPT_HOOK_ENABLED=false"));
    }

    #[tokio::test]
    async fn test_hook_creation() {
        let app_config = AppConfig::default();
        let hook_config = HookConfig::default();
        
        // Should not panic when creating hook
        let _hook = ShellHook::new(&app_config, hook_config);
    }
}
