pub mod add;
pub mod args;
pub mod list;
pub mod merge;
pub mod open;
pub mod path;
pub mod remove;
pub mod set_window_status;

use crate::{config::Config, git, workflow::SetupOptions};
use anyhow::{Context, Result};

/// Represents the different phases where hooks can be executed
pub enum HookPhase {
    PostCreate,
    PreDelete,
}

/// Announce that hooks are about to run, if applicable.
/// Returns true if the announcement was printed (hooks will run).
pub fn announce_hooks(config: &Config, options: Option<&SetupOptions>, phase: HookPhase) -> bool {
    match phase {
        HookPhase::PostCreate => {
            let should_run = options.is_some_and(|opts| opts.run_hooks)
                && config.post_create.as_ref().is_some_and(|v| !v.is_empty());

            if should_run {
                println!("Running setup commands...");
            }
            should_run
        }
        HookPhase::PreDelete => {
            let should_run = config.pre_delete.as_ref().is_some_and(|v| !v.is_empty());

            if should_run {
                println!("Running pre-delete commands...");
            }
            should_run
        }
    }
}

/// Resolve a worktree name (branch or directory basename) to a branch name.
/// Falls back to current branch if no argument provided.
/// Note: Must be called BEFORE workflow operations that change CWD (like merge/remove).
pub fn resolve_worktree_name(arg: Option<&str>, operation: &str) -> Result<String> {
    match arg {
        Some(name) => resolve_name_to_branch(name),
        None => git::get_current_branch()
            .with_context(|| format!("Failed to get current branch for {} operation", operation)),
    }
}

/// Resolve a worktree name (branch or directory basename) to a branch name.
/// This helper is used by commands that take a required name parameter.
pub fn resolve_name_to_branch(name: &str) -> Result<String> {
    let (_path, branch) = git::get_worktree_by_name(name).with_context(|| {
        format!(
            "No worktree found for '{}'. Use 'workmux list' to see available worktrees.",
            name
        )
    })?;
    Ok(branch)
}
