use anyhow::{Context, Result, anyhow};
use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::cmd::Cmd;
use crate::config::{Config, PaneConfig, SplitDirection};

use super::{Multiplexer, PaneSetupOptions, PaneSetupResult};

/// Tmux multiplexer implementation
pub struct Tmux;

impl Tmux {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Tmux {
    fn default() -> Self {
        Self::new()
    }
}

impl Multiplexer for Tmux {
    fn is_running(&self) -> Result<bool> {
        Cmd::new("tmux").arg("has-session").run_as_check()
    }

    fn tab_exists(&self, prefix: &str, name: &str) -> Result<bool> {
        let prefixed_name = self.prefixed(prefix, name);
        let windows = Cmd::new("tmux")
            .args(&["list-windows", "-F", "#{window_name}"])
            .run_and_capture_stdout();

        match windows {
            Ok(output) => Ok(output.lines().any(|line| line == prefixed_name)),
            Err(_) => Ok(false), // If command fails, window doesn't exist
        }
    }

    fn current_tab_name(&self) -> Result<Option<String>> {
        match Cmd::new("tmux")
            .args(&["display-message", "-p", "#{window_name}"])
            .run_and_capture_stdout()
        {
            Ok(name) => Ok(Some(name.trim().to_string())),
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

        let mut cmd = Cmd::new("tmux").arg("new-window");
        if detached {
            cmd = cmd.arg("-d");
        }

        // Use -P to print pane info, -F to format output to just the pane ID
        let pane_id = cmd
            .args(&[
                "-n",
                &prefixed_name,
                "-c",
                working_dir_str,
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .run_and_capture_stdout()
            .context("Failed to create tmux window and get pane ID")?;

        Ok(pane_id.trim().to_string())
    }

    fn select_tab(&self, prefix: &str, name: &str) -> Result<()> {
        let prefixed_name = self.prefixed(prefix, name);
        let target = format!("={}", prefixed_name);

        Cmd::new("tmux")
            .args(&["select-window", "-t", &target])
            .run()
            .context("Failed to select window")?;

        Ok(())
    }

    fn kill_tab(&self, prefix: &str, name: &str) -> Result<()> {
        let prefixed_name = self.prefixed(prefix, name);
        let target = format!("={}", prefixed_name);

        Cmd::new("tmux")
            .args(&["kill-window", "-t", &target])
            .run()
            .context("Failed to kill tmux window")?;

        Ok(())
    }

    fn split_pane(
        &self,
        target_pane_id: &str,
        direction: &SplitDirection,
        working_dir: &Path,
        size: Option<u16>,
        percentage: Option<u8>,
        command: Option<&str>,
    ) -> Result<String> {
        let split_arg = match direction {
            SplitDirection::Horizontal => "-h",
            SplitDirection::Vertical => "-v",
        };

        let working_dir_str = working_dir
            .to_str()
            .ok_or_else(|| anyhow!("Working directory path contains non-UTF8 characters"))?;

        let mut cmd = Cmd::new("tmux").args(&[
            "split-window",
            split_arg,
            "-t",
            target_pane_id,
            "-c",
            working_dir_str,
            "-P", // Print new pane info
            "-F", // Format to get just the ID
            "#{pane_id}",
        ]);

        let size_arg;
        if let Some(p) = percentage {
            size_arg = format!("{}%", p);
            cmd = cmd.args(&["-l", &size_arg]);
        } else if let Some(s) = size {
            size_arg = s.to_string();
            cmd = cmd.args(&["-l", &size_arg]);
        }

        if let Some(shell_cmd) = command {
            cmd = cmd.arg(shell_cmd);
        }

        let new_pane_id = cmd
            .run_and_capture_stdout()
            .context("Failed to split pane")?;

        Ok(new_pane_id.trim().to_string())
    }

    fn respawn_pane(
        &self,
        pane_id: &str,
        working_dir: &Path,
        command: Option<&str>,
    ) -> Result<()> {
        let working_dir_str = working_dir
            .to_str()
            .ok_or_else(|| anyhow!("Working directory path contains non-UTF8 characters"))?;

        let mut cmd =
            Cmd::new("tmux").args(&["respawn-pane", "-t", pane_id, "-c", working_dir_str, "-k"]);

        if let Some(shell_cmd) = command {
            cmd = cmd.arg(shell_cmd);
        }

        cmd.run().context("Failed to respawn pane")?;

        Ok(())
    }

    fn send_keys(&self, pane_id: &str, command: &str) -> Result<()> {
        // Use -l for literal keys (avoids interpretation of special characters)
        // Then send Enter separately to execute the command
        Cmd::new("tmux")
            .args(&["send-keys", "-t", pane_id, "-l", command])
            .run()
            .context("Failed to send keys to pane")?;

        Cmd::new("tmux")
            .args(&["send-keys", "-t", pane_id, "Enter"])
            .run()
            .context("Failed to send Enter key to pane")?;

        Ok(())
    }

    fn select_pane(&self, pane_id: &str) -> Result<()> {
        Cmd::new("tmux")
            .args(&["select-pane", "-t", pane_id])
            .run()
            .context("Failed to select pane")?;

        Ok(())
    }

    fn get_all_tab_names(&self) -> Result<HashSet<String>> {
        // tmux list-windows may exit with error if no windows exist
        let windows = Cmd::new("tmux")
            .args(&["list-windows", "-F", "#{window_name}"])
            .run_and_capture_stdout()
            .unwrap_or_default(); // Return empty string if command fails

        Ok(windows.lines().map(String::from).collect())
    }

    fn schedule_tab_close(&self, prefix: &str, name: &str, delay: Duration) -> Result<()> {
        let prefixed_name = self.prefixed(prefix, name);
        let delay_secs = format!("{:.3}", delay.as_secs_f64());
        let script = format!(
            "sleep {delay}; tmux kill-window -t ={window} >/dev/null 2>&1",
            delay = delay_secs,
            window = prefixed_name
        );

        self.run_shell(&script)
    }

    fn run_shell(&self, script: &str) -> Result<()> {
        Cmd::new("tmux")
            .args(&["run-shell", script])
            .run()
            .context("Failed to run shell command via tmux")?;
        Ok(())
    }

    fn get_default_shell(&self) -> Result<String> {
        let output = Cmd::new("tmux")
            .args(&["show-option", "-gqv", "default-shell"])
            .run_and_capture_stdout()?;
        let shell = output.trim();
        if shell.is_empty() {
            Ok("/bin/bash".to_string())
        } else {
            Ok(shell.to_string())
        }
    }

    fn name(&self) -> &'static str {
        "tmux"
    }

    fn window_term(&self) -> &'static str {
        "window"
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
        let mut pane_ids: Vec<String> = vec![initial_pane_id.to_string()];
        let effective_agent = task_agent.or(config.agent.as_deref());

        // Handle the first pane (initial pane from window creation)
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
                // Use wait-for handshake to ensure shell is ready before sending keys
                let channel = init_wait_channel()?;
                let default_shell = self.get_default_shell()?;
                let wrapper = build_ready_wrapper(&channel, &default_shell);

                self.respawn_pane(initial_pane_id, working_dir, Some(&wrapper))?;
                wait_for_pane_ready(&channel)?;
                self.send_keys(initial_pane_id, cmd_str)?;
            }
            if pane_config.focus {
                focus_pane_id = Some(initial_pane_id.to_string());
            }
        }

        // Create additional panes by splitting
        for pane_config in panes.iter().skip(1) {
            if let Some(ref direction) = pane_config.split {
                // Determine which pane to split based on logical index, then get its ID
                let target_pane_idx = pane_config.target.unwrap_or(pane_ids.len() - 1);
                let target_pane_id = pane_ids
                    .get(target_pane_idx)
                    .ok_or_else(|| anyhow!("Invalid target pane index: {}", target_pane_idx))?;

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

                let new_pane_id = if let Some(cmd_str) = adjusted_command.as_ref().map(|c| c.as_ref()) {
                    // Use wait-for handshake to ensure shell is ready before sending keys
                    let channel = init_wait_channel()?;
                    let default_shell = self.get_default_shell()?;
                    let wrapper = build_ready_wrapper(&channel, &default_shell);

                    let pane_id = self.split_pane(
                        target_pane_id,
                        direction,
                        working_dir,
                        pane_config.size,
                        pane_config.percentage,
                        Some(&wrapper),
                    )?;

                    wait_for_pane_ready(&channel)?;
                    self.send_keys(&pane_id, cmd_str)?;
                    pane_id
                } else {
                    self.split_pane(
                        target_pane_id,
                        direction,
                        working_dir,
                        pane_config.size,
                        pane_config.percentage,
                        None,
                    )?
                };

                if pane_config.focus {
                    focus_pane_id = Some(new_pane_id.clone());
                }
                pane_ids.push(new_pane_id);
            }
        }

        Ok(PaneSetupResult {
            // Default to the first pane if no focus is specified
            focus_pane_id: focus_pane_id.unwrap_or_else(|| initial_pane_id.to_string()),
        })
    }
}

// --- Private helper functions for tmux-specific operations ---

/// Timeout for waiting for pane readiness (seconds)
const HANDSHAKE_TIMEOUT_SECS: u64 = 5;

/// Initialize a tmux wait-for channel by locking it.
/// Returns the channel name for use in the handshake.
fn init_wait_channel() -> Result<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let channel = format!("wm_ready_{}_{}", pid, nanos);

    // Handshake Part 1: Lock the channel
    // This ensures we don't miss the signal even if shell starts instantly
    Cmd::new("tmux")
        .args(&["wait-for", "-L", &channel])
        .run()
        .context("Failed to initialize wait channel")?;

    Ok(channel)
}

/// Wait for the pane to signal it is ready (unlock the channel),
/// then clean up the channel.
fn wait_for_pane_ready(channel: &str) -> Result<()> {
    // Handshake Part 3: Wait for shell to unlock (by attempting to lock again)
    // This blocks until the shell runs `tmux wait-for -U`
    // Use a polling loop with timeout to prevent indefinite hangs if pane fails to start

    let mut child = std::process::Command::new("tmux")
        .args(["wait-for", "-L", channel])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Failed to spawn tmux wait-for command")?;

    let start = Instant::now();
    let timeout = Duration::from_secs(HANDSHAKE_TIMEOUT_SECS);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    // Handshake Part 4: Cleanup - unlock the channel we just re-locked
                    Cmd::new("tmux")
                        .args(&["wait-for", "-U", channel])
                        .run()
                        .context("Failed to cleanup wait channel")?;
                    return Ok(());
                } else {
                    // Attempt cleanup even on failure
                    let _ = Cmd::new("tmux").args(&["wait-for", "-U", channel]).run();
                    return Err(anyhow!(
                        "Pane handshake failed - tmux wait-for returned error"
                    ));
                }
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait(); // Ensure process is reaped

                    // Attempt cleanup
                    let _ = Cmd::new("tmux").args(&["wait-for", "-U", channel]).run();

                    return Err(anyhow!(
                        "Pane handshake timed out after {}s - shell may have failed to start",
                        HANDSHAKE_TIMEOUT_SECS
                    ));
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = Cmd::new("tmux").args(&["wait-for", "-U", channel]).run();
                return Err(anyhow!("Error waiting for pane handshake: {}", e));
            }
        }
    }
}

/// Build the wrapper command that signals readiness before starting the shell.
/// Uses stty -echo to prevent double-echo, then signals via wait-for -U.
fn build_ready_wrapper(channel: &str, shell: &str) -> String {
    // Quote shell path in case it contains spaces
    // Silence stty errors in case it's not available in minimal environments
    format!(
        "stty -echo 2>/dev/null; tmux wait-for -U {}; exec '{}'",
        channel, shell
    )
}

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
///
/// When a prompt file is provided (via --prompt-file or --prompt-editor), this function
/// modifies the agent command to automatically pass the prompt content. For example,
/// "claude" becomes "claude \"$(cat PROMPT.md)\"".
///
/// Only rewrites commands that match the configured agent. For instance, if the config
/// specifies "gemini" as the agent, a "claude" command won't be rewritten.
///
/// Special handling:
/// - gemini: Adds `-i` flag for interactive mode after the prompt
/// - Other agents (claude, codex, etc.): Just passes the prompt as first argument
///
/// Returns None if the command shouldn't be rewritten (empty, doesn't match configured agent, etc.)
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

    // Build the command step-by-step to ensure correct order:
    // [agent_command] [agent_options] [user_args] [prompt_argument]
    let mut cmd = pane_token.to_string();

    // Add user-provided arguments from config (must come before the prompt)
    if !rest.is_empty() {
        cmd.push(' ');
        cmd.push_str(rest);
    }

    // Add the prompt argument (agent-specific handling)
    let pane_stem_str = pane_stem.and_then(|s| s.to_str());
    if pane_stem_str == Some("gemini") {
        // gemini uses -i flag with the prompt as its argument
        cmd.push_str(&format!(" -i \"$(cat {})\"", prompt_path));
    } else if pane_stem_str == Some("opencode") {
        // opencode uses -p flag for interactive TUI with initial prompt
        // (opencode run is non-interactive, similar to claude -p)
        cmd.push_str(&format!(" -p \"$(cat {})\"", prompt_path));
    } else {
        // Other agents use -- separator
        cmd.push_str(&format!(" -- \"$(cat {})\"", prompt_path));
    }

    Some(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_rewrite_claude_command() {
        let prompt_file = PathBuf::from("/tmp/worktree/PROMPT.md");
        let working_dir = PathBuf::from("/tmp/worktree");

        let result = rewrite_agent_command("claude", &prompt_file, &working_dir, Some("claude"));
        assert_eq!(result, Some("claude -- \"$(cat PROMPT.md)\"".to_string()));
    }

    #[test]
    fn test_rewrite_codex_command() {
        let prompt_file = PathBuf::from("/tmp/worktree/PROMPT.md");
        let working_dir = PathBuf::from("/tmp/worktree");

        let result = rewrite_agent_command("codex", &prompt_file, &working_dir, Some("codex"));
        assert_eq!(result, Some("codex -- \"$(cat PROMPT.md)\"".to_string()));
    }

    #[test]
    fn test_rewrite_gemini_command() {
        let prompt_file = PathBuf::from("/tmp/worktree/PROMPT.md");
        let working_dir = PathBuf::from("/tmp/worktree");

        let result = rewrite_agent_command("gemini", &prompt_file, &working_dir, Some("gemini"));
        assert_eq!(result, Some("gemini -i \"$(cat PROMPT.md)\"".to_string()));
    }

    #[test]
    fn test_rewrite_command_with_path() {
        let prompt_file = PathBuf::from("/tmp/worktree/PROMPT.md");
        let working_dir = PathBuf::from("/tmp/worktree");

        let result = rewrite_agent_command(
            "/usr/local/bin/claude",
            &prompt_file,
            &working_dir,
            Some("/usr/local/bin/claude"),
        );
        assert_eq!(
            result,
            Some("/usr/local/bin/claude -- \"$(cat PROMPT.md)\"".to_string())
        );
    }

    #[test]
    fn test_rewrite_command_with_args() {
        let prompt_file = PathBuf::from("/tmp/worktree/PROMPT.md");
        let working_dir = PathBuf::from("/tmp/worktree");

        let result = rewrite_agent_command(
            "claude --verbose",
            &prompt_file,
            &working_dir,
            Some("claude"),
        );
        assert_eq!(
            result,
            Some("claude --verbose -- \"$(cat PROMPT.md)\"".to_string())
        );
    }

    #[test]
    fn test_rewrite_mismatched_agent() {
        let prompt_file = PathBuf::from("/tmp/worktree/PROMPT.md");
        let working_dir = PathBuf::from("/tmp/worktree");

        // Command is for claude
        let result = rewrite_agent_command("claude", &prompt_file, &working_dir, Some("gemini"));
        assert_eq!(result, None);
    }

    #[test]
    fn test_rewrite_unknown_agent() {
        let prompt_file = PathBuf::from("/tmp/worktree/PROMPT.md");
        let working_dir = PathBuf::from("/tmp/worktree");

        let result = rewrite_agent_command(
            "unknown-agent",
            &prompt_file,
            &working_dir,
            Some("unknown-agent"),
        );
        assert_eq!(
            result,
            Some("unknown-agent -- \"$(cat PROMPT.md)\"".to_string())
        );
    }

    #[test]
    fn test_rewrite_empty_command() {
        let prompt_file = PathBuf::from("/tmp/worktree/PROMPT.md");
        let working_dir = PathBuf::from("/tmp/worktree");

        let result = rewrite_agent_command("", &prompt_file, &working_dir, Some("claude"));
        assert_eq!(result, None);
    }

    #[test]
    fn test_rewrite_opencode_command_basic() {
        let prompt_file = PathBuf::from("/tmp/worktree/PROMPT.md");
        let working_dir = PathBuf::from("/tmp/worktree");

        let result =
            rewrite_agent_command("opencode", &prompt_file, &working_dir, Some("opencode"));
        assert_eq!(result, Some("opencode -p \"$(cat PROMPT.md)\"".to_string()));
    }
}
