from pathlib import Path

from .conftest import (
    TmuxEnvironment,
    get_window_name,
    get_worktree_path,
    run_workmux_add,
    run_workmux_command,
    run_workmux_open,
    slugify,
    write_workmux_config,
)


def _kill_window(env: TmuxEnvironment, branch_name: str) -> None:
    """Helper to close the tmux window for a branch if it exists."""
    window_name = get_window_name(branch_name)
    env.tmux(["has-session", "-t", window_name], check=False)
    env.tmux(["kill-window", "-t", window_name], check=False)


def test_open_recreates_tmux_window_for_existing_worktree(
    isolated_tmux_server: TmuxEnvironment, workmux_exe_path: Path, repo_path: Path
):
    """Verifies `workmux open` recreates a tmux window for an existing worktree."""
    env = isolated_tmux_server
    branch_name = "feature-open-success"
    window_name = get_window_name(branch_name)

    write_workmux_config(repo_path)
    run_workmux_add(env, workmux_exe_path, repo_path, branch_name)

    # Close the original window to simulate a detached worktree
    env.tmux(["kill-window", "-t", window_name])

    run_workmux_open(env, workmux_exe_path, repo_path, branch_name)

    list_windows = env.tmux(
        ["list-windows", "-F", "#{window_name}"]
    ).stdout.splitlines()
    assert window_name in list_windows


def test_open_fails_when_window_already_exists(
    isolated_tmux_server: TmuxEnvironment, workmux_exe_path: Path, repo_path: Path
):
    """Verifies `workmux open` fails when the tmux window already exists."""
    env = isolated_tmux_server
    branch_name = "feature-open-window-exists"

    write_workmux_config(repo_path)
    run_workmux_add(env, workmux_exe_path, repo_path, branch_name)

    result = run_workmux_open(
        env,
        workmux_exe_path,
        repo_path,
        branch_name,
        expect_fail=True,
    )

    assert "window named" in result.stderr


def test_open_fails_when_worktree_missing(
    isolated_tmux_server: TmuxEnvironment, workmux_exe_path: Path, repo_path: Path
):
    """Verifies `workmux open` fails if the worktree does not exist."""
    env = isolated_tmux_server
    branch_name = "missing-worktree"

    write_workmux_config(repo_path)

    result = run_workmux_open(
        env,
        workmux_exe_path,
        repo_path,
        branch_name,
        expect_fail=True,
    )

    assert "No worktree found for" in result.stderr


def test_open_with_run_hooks_reexecutes_post_create_commands(
    isolated_tmux_server: TmuxEnvironment, workmux_exe_path: Path, repo_path: Path
):
    """Verifies `workmux open --run-hooks` re-runs post_create hooks."""
    env = isolated_tmux_server
    branch_name = "feature-open-hooks"
    hook_file = "open_hook.txt"

    write_workmux_config(repo_path, post_create=[f"touch {hook_file}"])
    run_workmux_add(env, workmux_exe_path, repo_path, branch_name)

    worktree_path = get_worktree_path(repo_path, branch_name)
    hook_path = worktree_path / hook_file
    hook_path.unlink()

    _kill_window(env, branch_name)

    run_workmux_open(
        env,
        workmux_exe_path,
        repo_path,
        branch_name,
        run_hooks=True,
    )

    assert hook_path.exists()


def test_open_with_force_files_reapplies_file_operations(
    isolated_tmux_server: TmuxEnvironment, workmux_exe_path: Path, repo_path: Path
):
    """Verifies `workmux open --force-files` reapplies copy operations."""
    env = isolated_tmux_server
    branch_name = "feature-open-files"
    shared_file = repo_path / "shared.env"
    shared_file.write_text("KEY=value")

    write_workmux_config(repo_path, files={"copy": ["shared.env"]})
    run_workmux_add(env, workmux_exe_path, repo_path, branch_name)

    worktree_path = get_worktree_path(repo_path, branch_name)
    worktree_file = worktree_path / "shared.env"
    worktree_file.unlink()

    _kill_window(env, branch_name)

    run_workmux_open(
        env,
        workmux_exe_path,
        repo_path,
        branch_name,
        force_files=True,
    )

    assert worktree_file.exists()
    assert worktree_file.read_text() == "KEY=value"


def test_open_by_worktree_directory_name(
    isolated_tmux_server: TmuxEnvironment, workmux_exe_path: Path, repo_path: Path
):
    """Verifies `workmux open` works with worktree directory name (basename) instead of branch name."""
    env = isolated_tmux_server
    branch_name = "feature/my-long-branch-name"
    custom_name = "short-name"

    write_workmux_config(repo_path)

    # Create worktree with custom name (handle differs from branch)
    run_workmux_command(
        env,
        workmux_exe_path,
        repo_path,
        f"add {branch_name} --name {custom_name}",
    )

    expected_handle = slugify(custom_name)
    window_name = f"wm-{expected_handle}"

    # Close the original window
    env.tmux(["kill-window", "-t", window_name])

    # Open using the directory name (handle), not branch name
    run_workmux_open(env, workmux_exe_path, repo_path, custom_name)

    # Verify window was recreated
    list_windows = env.tmux(
        ["list-windows", "-F", "#{window_name}"]
    ).stdout.splitlines()
    assert window_name in list_windows


def test_open_prefers_branch_match_over_directory_name(
    isolated_tmux_server: TmuxEnvironment, workmux_exe_path: Path, repo_path: Path
):
    """Verifies branch name match takes precedence over directory name match."""
    env = isolated_tmux_server
    # Create a branch whose name matches another worktree's directory name
    branch_a = "feature-a"
    branch_b = "feature-b"

    write_workmux_config(repo_path)

    # Create first worktree with custom name "feature-b"
    run_workmux_command(
        env,
        workmux_exe_path,
        repo_path,
        f"add {branch_a} --name {branch_b}",
    )

    # Create second worktree with branch name "feature-b"
    run_workmux_add(env, workmux_exe_path, repo_path, branch_b)

    # Kill both windows
    env.tmux(["kill-window", "-t", f"wm-{branch_b}"], check=False)
    # The first worktree's window name is wm-feature-b (from --name)
    # The second worktree's window name is also wm-feature-b (from branch)
    # They can't both exist, so one was already killed above

    # When opening "feature-b", it should match the branch (second worktree)
    # not the directory name (first worktree)
    run_workmux_open(env, workmux_exe_path, repo_path, branch_b)

    # Verify the window exists
    list_windows = env.tmux(
        ["list-windows", "-F", "#{window_name}"]
    ).stdout.splitlines()
    assert f"wm-{branch_b}" in list_windows

    # Verify the opened worktree is for branch_b, not branch_a
    worktree_path = get_worktree_path(repo_path, branch_b)
    result = env.run_command(
        ["git", "-C", str(worktree_path), "rev-parse", "--abbrev-ref", "HEAD"]
    )
    assert result.stdout.strip() == branch_b
