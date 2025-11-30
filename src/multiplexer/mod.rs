mod tmux;
mod zellij;

use anyhow::{Result, anyhow};
use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::time::Duration;

pub use tmux::Tmux;
pub use zellij::Zellij;

use crate::config::{Config, PaneConfig, SplitDirection};

/// Result of setting up panes
pub struct PaneSetupResult {
    /// The ID of the pane that should receive focus.
    pub focus_pane_id: String,
}

/// Options for pane setup
pub struct PaneSetupOptions<'a> {
    pub run_commands: bool,
    pub prompt_file_path: Option<&'a Path>,
}

/// Trait abstracting terminal multiplexer operations.
///
/// This allows workmux to support both tmux and Zellij with the same codebase.
pub trait Multiplexer: Send + Sync {
    /// Check if the multiplexer server/session is running
    fn is_running(&self) -> Result<bool>;

    /// Check if a tab/window with the given name exists
    fn tab_exists(&self, prefix: &str, name: &str) -> Result<bool>;

    /// Get the name of the current tab/window
    fn current_tab_name(&self) -> Result<Option<String>>;

    /// Create a new tab/window and return the initial pane ID
    fn create_tab(
        &self,
        prefix: &str,
        name: &str,
        working_dir: &Path,
        detached: bool,
    ) -> Result<String>;

    /// Select/focus a tab/window by name
    fn select_tab(&self, prefix: &str, name: &str) -> Result<()>;

    /// Kill/close a tab/window by name
    fn kill_tab(&self, prefix: &str, name: &str) -> Result<()>;

    /// Split a pane and return the new pane's ID
    fn split_pane(
        &self,
        target_pane_id: &str,
        direction: &SplitDirection,
        working_dir: &Path,
        size: Option<u16>,
        percentage: Option<u8>,
        command: Option<&str>,
    ) -> Result<String>;

    /// Respawn a pane with a new command
    fn respawn_pane(
        &self,
        pane_id: &str,
        working_dir: &Path,
        command: Option<&str>,
    ) -> Result<()>;

    /// Send keys (type text) into a pane
    fn send_keys(&self, pane_id: &str, command: &str) -> Result<()>;

    /// Select/focus a specific pane
    fn select_pane(&self, pane_id: &str) -> Result<()>;

    /// Get all tab/window names
    fn get_all_tab_names(&self) -> Result<HashSet<String>>;

    /// Schedule a tab/window to be closed after a delay
    fn schedule_tab_close(&self, prefix: &str, name: &str, delay: Duration) -> Result<()>;

    /// Run a shell script via the multiplexer
    fn run_shell(&self, script: &str) -> Result<()>;

    /// Get the default shell configured in the multiplexer
    fn get_default_shell(&self) -> Result<String>;

    /// Create a prefixed name for tabs/windows
    fn prefixed(&self, prefix: &str, name: &str) -> String {
        format!("{}{}", prefix, name)
    }

    /// Get the multiplexer name for error messages
    fn name(&self) -> &'static str;

    /// Get the term for "window" in this multiplexer (window/tab)
    fn window_term(&self) -> &'static str;

    /// Setup panes in a tab/window according to configuration.
    /// Default implementation that can be overridden if needed.
    fn setup_panes(
        &self,
        initial_pane_id: &str,
        panes: &[PaneConfig],
        working_dir: &Path,
        options: PaneSetupOptions<'_>,
        config: &Config,
        task_agent: Option<&str>,
    ) -> Result<PaneSetupResult>;
}

/// Detected multiplexer type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum MultiplexerType {
    Tmux,
    Zellij,
}

/// Get a multiplexer instance based on configuration and environment.
///
/// Priority:
/// 1. CLI override (if provided)
/// 2. Config file setting
/// 3. Auto-detection from environment variables
/// 4. Default to tmux if ambiguous
pub fn get_multiplexer(
    config: &Config,
    cli_override: Option<&str>,
) -> Result<Box<dyn Multiplexer>> {
    let backend = cli_override
        .or(config.multiplexer.as_deref())
        .unwrap_or("auto");

    match backend {
        "auto" => detect_multiplexer(),
        "tmux" => Ok(Box::new(Tmux::new())),
        "zellij" => Ok(Box::new(Zellij::new())),
        other => Err(anyhow!(
            "Unknown multiplexer '{}'. Valid options: auto, tmux, zellij",
            other
        )),
    }
}

/// Detect which multiplexer we're running inside based on environment variables.
fn detect_multiplexer() -> Result<Box<dyn Multiplexer>> {
    // Check for Zellij first (more specific)
    if env::var("ZELLIJ").is_ok() {
        return Ok(Box::new(Zellij::new()));
    }

    // Check for tmux
    if env::var("TMUX").is_ok() {
        return Ok(Box::new(Tmux::new()));
    }

    // Default to tmux for backwards compatibility
    // This allows users outside a multiplexer to still use workmux
    // (tmux will report "not running" when they try to use it)
    Ok(Box::new(Tmux::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explicit_tmux_override() {
        let config = Config::default();
        let mux = get_multiplexer(&config, Some("tmux")).unwrap();
        assert_eq!(mux.name(), "tmux");
    }

    #[test]
    fn test_explicit_zellij_override() {
        let config = Config::default();
        let mux = get_multiplexer(&config, Some("zellij")).unwrap();
        assert_eq!(mux.name(), "zellij");
    }

    #[test]
    fn test_invalid_multiplexer() {
        let config = Config::default();
        let result = get_multiplexer(&config, Some("invalid"));
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("Unknown multiplexer"));
    }

    #[test]
    fn test_config_multiplexer_used_when_no_override() {
        let mut config = Config::default();
        config.multiplexer = Some("zellij".to_string());
        let mux = get_multiplexer(&config, None).unwrap();
        assert_eq!(mux.name(), "zellij");
    }
}
