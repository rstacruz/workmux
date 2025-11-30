use anyhow::{Context, Result, anyhow};
use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::cmd::Cmd;
use crate::config::{Config, PaneConfig, SplitDirection};

use super::{Multiplexer, PaneSetupOptions, PaneSetupResult};

/// Zellij multiplexer implementation
pub struct Zellij;

impl Zellij {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Zellij {
    fn default() -> Self {
        Self::new()
    }
}

impl Multiplexer for Zellij {
    fn is_running(&self) -> Result<bool> {
        // If ZELLIJ env var is set, we're inside a Zellij session
        if std::env::var("ZELLIJ").is_ok() {
            return Ok(true);
        }
        
        // Otherwise check if there are any sessions
        let result = Cmd::new("zellij")
            .args(&["list-sessions"])
            .run_and_capture_stdout();
        
        match result {
            Ok(output) => Ok(!output.trim().is_empty()),
            Err(_) => Ok(false),
        }
    }

    fn tab_exists(&self, prefix: &str, name: &str) -> Result<bool> {
        let prefixed_name = self.prefixed(prefix, name);
        let tabs = self.get_all_tab_names()?;
        Ok(tabs.contains(&prefixed_name))
    }

    fn current_tab_name(&self) -> Result<Option<String>> {
        // Zellij doesn't have a direct way to get current tab name via CLI
        // We need to parse the output of query-tab-names or use dump-layout
        // For now, use a workaround with dump-layout
        let output = Cmd::new("zellij")
            .args(&["action", "dump-layout"])
            .run_and_capture_stdout();

        match output {
            Ok(layout) => {
                // Parse the KDL layout to find the focused tab name
                // This is a simplified parser - may need improvement
                for line in layout.lines() {
                    if line.contains("focus=true") && line.contains("tab name=") {
                        if let Some(start) = line.find("name=\"") {
                            let rest = &line[start + 6..];
                            if let Some(end) = rest.find('"') {
                                return Ok(Some(rest[..end].to_string()));
                            }
                        }
                    }
                }
                // If no focused tab found, return None
                Ok(None)
            }
            Err(_) => Ok(None),
        }
    }

    fn create_tab(
        &self,
        prefix: &str,
        name: &str,
        working_dir: &Path,
        detached: bool,
    ) -> Result<String> {
        let prefixed_name = self.prefixed(prefix, name);
        let working_dir_str = working_dir
            .to_str()
            .ok_or_else(|| anyhow!("Working directory path contains non-UTF8 characters"))?;

        // Create the new tab
        Cmd::new("zellij")
            .args(&["action", "new-tab", "--name", &prefixed_name, "--cwd", working_dir_str])
            .run()
            .context("Failed to create Zellij tab")?;

        // If detached, switch back to previous tab
        // Zellij creates tabs focused by default
        if detached {
            // Go back to previous tab
            Cmd::new("zellij")
                .args(&["action", "go-to-previous-tab"])
                .run()
                .context("Failed to switch back to previous tab")?;
        }

        // Zellij doesn't return pane IDs like tmux does
        // We'll use a synthetic ID based on tab name for tracking
        // The actual pane operations will work on the focused pane
        Ok(format!("{}:0", prefixed_name))
    }

    fn select_tab(&self, prefix: &str, name: &str) -> Result<()> {
        let prefixed_name = self.prefixed(prefix, name);

        Cmd::new("zellij")
            .args(&["action", "go-to-tab-name", &prefixed_name])
            .run()
            .context("Failed to select Zellij tab")?;

        Ok(())
    }

    fn kill_tab(&self, prefix: &str, name: &str) -> Result<()> {
        let prefixed_name = self.prefixed(prefix, name);

        // First go to the tab, then close it
        Cmd::new("zellij")
            .args(&["action", "go-to-tab-name", &prefixed_name])
            .run()
            .context("Failed to select tab for closing")?;

        Cmd::new("zellij")
            .args(&["action", "close-tab"])
            .run()
            .context("Failed to close Zellij tab")?;

        Ok(())
    }

    fn split_pane(
        &self,
        _target_pane_id: &str,
        direction: &SplitDirection,
        working_dir: &Path,
        _size: Option<u16>,
        _percentage: Option<u8>,
        command: Option<&str>,
    ) -> Result<String> {
        let working_dir_str = working_dir
            .to_str()
            .ok_or_else(|| anyhow!("Working directory path contains non-UTF8 characters"))?;

        // Map workmux split directions to Zellij directions
        // In tmux: horizontal splits side-by-side, vertical splits top-bottom
        // In Zellij: we use right/down to match this behavior
        let zellij_direction = match direction {
            SplitDirection::Horizontal => "right",
            SplitDirection::Vertical => "down",
        };

        let mut cmd = Cmd::new("zellij");
        cmd = cmd.args(&["action", "new-pane", "--direction", zellij_direction, "--cwd", working_dir_str]);

        // Note: Zellij doesn't support size/percentage in the same way as tmux
        // The pane will be created with default sizing
        // TODO: Consider using resize-pane after creation if precise sizing is needed

        cmd.run().context("Failed to split Zellij pane")?;

        // If a command was specified, it's a shell wrapper - we need to handle this differently
        // For Zellij, we'll run the command in the new pane
        if let Some(shell_cmd) = command {
            // Small delay to ensure pane is ready
            thread::sleep(Duration::from_millis(100));
            
            // Run the command in the new pane
            Cmd::new("zellij")
                .args(&["action", "write-chars", shell_cmd])
                .run()
                .context("Failed to write command to pane")?;
            
            Cmd::new("zellij")
                .args(&["action", "write", "13"]) // Enter key
                .run()
                .context("Failed to send Enter to pane")?;
        }

        // Return a synthetic pane ID
        // Zellij pane IDs aren't easily accessible via CLI
        Ok(format!("pane-{}", std::process::id()))
    }

    fn respawn_pane(
        &self,
        _pane_id: &str,
        working_dir: &Path,
        command: Option<&str>,
    ) -> Result<()> {
        // Zellij doesn't have a direct respawn-pane equivalent
        // We'll close the current pane and create a new one
        // But this would change the layout, so instead we'll just run the command
        
        let working_dir_str = working_dir
            .to_str()
            .ok_or_else(|| anyhow!("Working directory path contains non-UTF8 characters"))?;

        // Change directory first
        let cd_cmd = format!("cd {}", working_dir_str);
        Cmd::new("zellij")
            .args(&["action", "write-chars", &cd_cmd])
            .run()
            .context("Failed to change directory in pane")?;
        
        Cmd::new("zellij")
            .args(&["action", "write", "13"]) // Enter
            .run()?;

        // Then run the command if specified
        if let Some(shell_cmd) = command {
            thread::sleep(Duration::from_millis(50));
            Cmd::new("zellij")
                .args(&["action", "write-chars", shell_cmd])
                .run()
                .context("Failed to write command to pane")?;
            
            Cmd::new("zellij")
                .args(&["action", "write", "13"]) // Enter
                .run()
                .context("Failed to send Enter to pane")?;
        }

        Ok(())
    }

    fn send_keys(&self, _pane_id: &str, command: &str) -> Result<()> {
        // Zellij write-chars sends text to the focused pane
        Cmd::new("zellij")
            .args(&["action", "write-chars", command])
            .run()
            .context("Failed to send keys to Zellij pane")?;

        // Send Enter key (keycode 13)
        Cmd::new("zellij")
            .args(&["action", "write", "13"])
            .run()
            .context("Failed to send Enter key to Zellij pane")?;

        Ok(())
    }

    fn select_pane(&self, _pane_id: &str) -> Result<()> {
        // Zellij doesn't have pane IDs accessible via CLI in the same way
        // For now, we rely on focus being set correctly during creation
        // In future, we could use move-focus commands
        Ok(())
    }

    fn get_all_tab_names(&self) -> Result<HashSet<String>> {
        // Use dump-layout to get tab names
        let output = Cmd::new("zellij")
            .args(&["action", "dump-layout"])
            .run_and_capture_stdout();

        match output {
            Ok(layout) => {
                let mut tabs = HashSet::new();
                // Parse KDL format for tab names
                for line in layout.lines() {
                    if line.trim().starts_with("tab name=") {
                        if let Some(start) = line.find("name=\"") {
                            let rest = &line[start + 6..];
                            if let Some(end) = rest.find('"') {
                                tabs.insert(rest[..end].to_string());
                            }
                        }
                    }
                }
                Ok(tabs)
            }
            Err(_) => Ok(HashSet::new()),
        }
    }

    fn schedule_tab_close(&self, prefix: &str, name: &str, delay: Duration) -> Result<()> {
        let prefixed_name = self.prefixed(prefix, name);
        let delay_secs = delay.as_secs_f64();

        // Use nohup to run in background
        let script = format!(
            "sleep {:.3} && zellij action go-to-tab-name '{}' && zellij action close-tab",
            delay_secs, prefixed_name
        );

        // Spawn background process
        std::process::Command::new("sh")
            .args(["-c", &format!("nohup sh -c '{}' >/dev/null 2>&1 &", script)])
            .spawn()
            .context("Failed to schedule tab close")?;

        Ok(())
    }

    fn run_shell(&self, script: &str) -> Result<()> {
        // Run a shell command via Zellij
        // Use zellij run for non-interactive commands
        Cmd::new("zellij")
            .args(&["run", "--", "sh", "-c", script])
            .run()
            .context("Failed to run shell command via Zellij")?;

        Ok(())
    }

    fn get_default_shell(&self) -> Result<String> {
        // Zellij uses the user's default shell from SHELL env var
        std::env::var("SHELL")
            .or_else(|_| Ok("/bin/bash".to_string()))
    }

    fn name(&self) -> &'static str {
        "zellij"
    }

    fn window_term(&self) -> &'static str {
        "tab"
    }

    fn setup_panes(
        &self,
        initial_pane_id: &str,
        panes: &[PaneConfig],
        working_dir: &Path,
        options: PaneSetupOptions<'_>,
        config: &Config,
        task_agent: Option<&str>,
    ) -> Result<PaneSetupResult> {
        if panes.is_empty() {
            return Ok(PaneSetupResult {
                focus_pane_id: initial_pane_id.to_string(),
            });
        }

        let mut focus_pane_id: Option<String> = None;
        let effective_agent = task_agent.or(config.agent.as_deref());

        // Handle the first pane (initial pane from tab creation)
        if let Some(pane_config) = panes.first() {
            let command_to_run = if pane_config.command.as_deref() == Some("<agent>") {
                effective_agent.map(|agent_cmd| agent_cmd.to_string())
            } else {
                pane_config.command.clone()
            };

            let adjusted_command = if options.run_commands {
                command_to_run.as_ref().map(|cmd| {
                    adjust_command(
                        cmd,
                        options.prompt_file_path,
                        working_dir,
                        effective_agent,
                    )
                })
            } else {
                None
            };

            if let Some(cmd_str) = adjusted_command.as_ref().map(|c| c.as_ref()) {
                // Small delay to ensure shell is ready
                thread::sleep(Duration::from_millis(200));
                self.send_keys(initial_pane_id, cmd_str)?;
            }

            if pane_config.focus {
                focus_pane_id = Some(initial_pane_id.to_string());
            }
        }

        // Create additional panes by splitting
        for pane_config in panes.iter().skip(1) {
            if let Some(ref direction) = pane_config.split {
                let command_to_run = if pane_config.command.as_deref() == Some("<agent>") {
                    effective_agent.map(|agent_cmd| agent_cmd.to_string())
                } else {
                    pane_config.command.clone()
                };

                let adjusted_command = if options.run_commands {
                    command_to_run.as_ref().map(|cmd| {
                        adjust_command(
                            cmd,
                            options.prompt_file_path,
                            working_dir,
                            effective_agent,
                        )
                    })
                } else {
                    None
                };

                // Split the pane
                let new_pane_id = self.split_pane(
                    "", // Zellij doesn't use target pane IDs
                    direction,
                    working_dir,
                    pane_config.size,
                    pane_config.percentage,
                    None, // Don't pass command here, we'll send it separately
                )?;

                // Send command if specified
                if let Some(cmd_str) = adjusted_command.as_ref().map(|c| c.as_ref()) {
                    thread::sleep(Duration::from_millis(100));
                    self.send_keys(&new_pane_id, cmd_str)?;
                }

                if pane_config.focus {
                    focus_pane_id = Some(new_pane_id.clone());
                }
            }
        }

        // If a specific pane should be focused and it's not the current one,
        // we need to navigate to it. For Zellij, this is tricky without pane IDs.
        // For now, we rely on the last created pane being focused.

        Ok(PaneSetupResult {
            focus_pane_id: focus_pane_id.unwrap_or_else(|| initial_pane_id.to_string()),
        })
    }
}

// --- Private helper functions ---

fn adjust_command<'a>(
    command: &'a str,
    prompt_file_path: Option<&Path>,
    working_dir: &Path,
    effective_agent: Option<&str>,
) -> Cow<'a, str> {
    if let Some(prompt_path) = prompt_file_path
        && let Some(rewritten) =
            rewrite_agent_command(command, prompt_path, working_dir, effective_agent)
    {
        return Cow::Owned(rewritten);
    }
    Cow::Borrowed(command)
}

/// Rewrites an agent command to inject a prompt file's contents.
/// Same logic as tmux implementation.
fn rewrite_agent_command(
    command: &str,
    prompt_file: &Path,
    working_dir: &Path,
    effective_agent: Option<&str>,
) -> Option<String> {
    let agent_command = effective_agent?;
    let trimmed_command = command.trim();
    if trimmed_command.is_empty() {
        return None;
    }

    let (pane_token, pane_rest) = crate::config::split_first_token(trimmed_command)?;
    let (config_token, _) = crate::config::split_first_token(agent_command)?;

    let resolved_pane_path = crate::config::resolve_executable_path(pane_token)
        .unwrap_or_else(|| pane_token.to_string());
    let resolved_config_path = crate::config::resolve_executable_path(config_token)
        .unwrap_or_else(|| config_token.to_string());

    let pane_stem = Path::new(&resolved_pane_path).file_stem();
    let config_stem = Path::new(&resolved_config_path).file_stem();

    if pane_stem != config_stem {
        return None;
    }

    let relative = prompt_file.strip_prefix(working_dir).unwrap_or(prompt_file);
    let prompt_path = relative.to_string_lossy();
    let rest = pane_rest.trim_start();

    let mut cmd = pane_token.to_string();

    if !rest.is_empty() {
        cmd.push(' ');
        cmd.push_str(rest);
    }

    let pane_stem_str = pane_stem.and_then(|s| s.to_str());
    if pane_stem_str == Some("gemini") {
        cmd.push_str(&format!(" -i \"$(cat {})\"", prompt_path));
    } else if pane_stem_str == Some("opencode") {
        cmd.push_str(&format!(" -p \"$(cat {})\"", prompt_path));
    } else {
        cmd.push_str(&format!(" -- \"$(cat {})\"", prompt_path));
    }

    Some(cmd)
}
