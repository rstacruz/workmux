use crate::git;
use anyhow::Result;

pub fn run(name: &str) -> Result<()> {
    // Resolve name (supports both branch name and worktree directory name) to branch
    let branch = super::resolve_name_to_branch(name)?;
    
    // Get the worktree path for this branch
    let path = git::get_worktree_path(&branch)?;
    println!("{}", path.display());
    Ok(())
}
