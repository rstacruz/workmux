use anyhow::{Context, Result, anyhow};

use crate::{git, tmux};
use tracing::info;

use super::context::WorkflowContext;
use super::setup;
use super::types::{CreateResult, SetupOptions};

/// Open a tmux window for an existing worktree.
/// Accepts either a branch name or worktree directory name (basename).
pub fn open(name: &str, context: &WorkflowContext, options: SetupOptions) -> Result<CreateResult> {
    info!(
        name = name,
        run_hooks = options.run_hooks,
        run_file_ops = options.run_file_ops,
        "open:start"
    );

    // Validate pane config before any other operations
    if let Some(panes) = &context.config.panes {
        crate::config::validate_panes_config(panes)?;
    }

    // Pre-flight checks
    context.ensure_tmux_running()?;

    // Look up worktree by name (branch or directory basename)
    let (worktree_path, branch_name) = git::get_worktree_by_name(name).with_context(|| {
        format!(
            "No worktree found for '{}'. Use 'workmux add {}' to create it.",
            name, name
        )
    })?;

    // Extract handle from the existing worktree path
    // This is robust even if config changed since creation
    let handle = worktree_path
        .file_name()
        .ok_or_else(|| anyhow!("Invalid worktree path: no directory name"))?
        .to_string_lossy()
        .to_string();

    // Check if tmux window exists using handle (the directory name)
    if tmux::window_exists(&context.prefix, &handle)? {
        return Err(anyhow!(
            "A tmux window named '{}{}' already exists. To switch to it, run: tmux select-window -t '{}'",
            context.prefix,
            handle,
            tmux::prefixed(&context.prefix, &handle)
        ));
    }

    // Setup the environment
    let result = setup::setup_environment(
        &branch_name,
        &handle,
        &worktree_path,
        &context.config,
        &options,
        None,
    )?;
    info!(
        branch = branch_name,
        path = %result.worktree_path.display(),
        hooks_run = result.post_create_hooks_run,
        "open:completed"
    );
    Ok(result)
}
