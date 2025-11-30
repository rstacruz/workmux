# PRD: Zellij support for workmux

## Problem statement

workmux currently only supports tmux as its terminal multiplexer. Users who prefer or use [Zellij](https://zellij.dev/) cannot use workmux, limiting the tool's adoption among developers who have moved to this modern alternative.

Zellij is gaining popularity due to its:
- Built-in layout system with persistent configurations
- Modern default keybindings and discoverability
- WebAssembly plugin system
- Simpler configuration compared to tmux

Supporting Zellij would expand workmux's user base and align with its philosophy of being a workflow tool that integrates with the user's existing terminal environment.

## Solution overview

Introduce an abstraction layer for terminal multiplexer operations, allowing workmux to support both tmux and Zellij while maintaining identical user-facing commands and configuration.

### Key changes

1. **Multiplexer trait**: Abstract common operations (create tab/window, split panes, send keys, etc.) into a trait
2. **Zellij implementation**: Implement the trait for Zellij using its CLI (`zellij action`)
3. **Auto-detection**: Detect the current multiplexer environment and use the appropriate backend
4. **Configuration option**: Allow users to explicitly specify their preferred multiplexer

## Functional requirements

### F1: Multiplexer abstraction

Create a trait that encapsulates terminal multiplexer operations:

```rust
pub trait Multiplexer {
    fn is_running(&self) -> Result<bool>;
    fn tab_exists(&self, prefix: &str, name: &str) -> Result<bool>;
    fn current_tab_name(&self) -> Result<Option<String>>;
    fn create_tab(&self, prefix: &str, name: &str, working_dir: &Path, detached: bool) -> Result<String>;
    fn select_tab(&self, prefix: &str, name: &str) -> Result<()>;
    fn kill_tab(&self, prefix: &str, name: &str) -> Result<()>;
    fn split_pane(&self, target: &str, direction: &SplitDirection, working_dir: &Path, size: Option<u16>, percentage: Option<u8>, command: Option<&str>) -> Result<String>;
    fn send_keys(&self, pane_id: &str, command: &str) -> Result<()>;
    fn select_pane(&self, pane_id: &str) -> Result<()>;
    fn get_all_tab_names(&self) -> Result<HashSet<String>>;
    fn schedule_tab_close(&self, prefix: &str, name: &str, delay: Duration) -> Result<()>;
}
```

### F2: Tmux implementation

Refactor existing `tmux.rs` into a struct implementing the `Multiplexer` trait. All current functionality must be preserved.

### F3: Zellij implementation

Implement the `Multiplexer` trait for Zellij using its CLI:

| Operation | Zellij command |
|-----------|----------------|
| Check running | `zellij list-sessions` |
| Create tab | `zellij action new-tab --name <name> --cwd <dir>` |
| Close tab | `zellij action close-tab` |
| Split pane | `zellij action new-pane --direction <dir> --cwd <dir>` |
| Send keys | `zellij action write-chars <text>` + `zellij action write 13` (Enter) |
| Focus pane | `zellij action focus-pane --id <id>` or `zellij action move-focus <direction>` |
| Go to tab | `zellij action go-to-tab-name <name>` |

**Zellij-specific considerations:**

- Zellij uses "tabs" instead of "windows" (terminology mapping)
- Pane IDs work differently; may need to track pane positions
- Split directions in Zellij: `up`, `down`, `left`, `right` vs tmux's `horizontal`/`vertical`
- Zellij has a different approach to detached operations

### F4: Auto-detection

Detect the current multiplexer environment:

1. Check `ZELLIJ` environment variable (set when inside Zellij)
2. Check `TMUX` environment variable (set when inside tmux)
3. Fall back to configuration or error if neither detected

### F5: Configuration

Add a new configuration option:

```yaml
# ~/.config/workmux/config.yaml or .workmux.yaml
multiplexer: auto  # Options: auto (default), tmux, zellij
```

The `auto` option uses environment detection.

### F6: Command-line override

Add a global flag to override the multiplexer:

```bash
workmux --multiplexer zellij add feature-branch
workmux -m tmux add feature-branch
```

### F7: Terminology mapping

Map tmux terminology to Zellij in user-facing messages:

| tmux | Zellij |
|------|--------|
| window | tab |
| pane | pane |
| session | session |

When running in Zellij mode, error messages and help text should use Zellij terminology.

### F8: Feature parity

The following features must work identically in both backends:

- `workmux add` - Create worktree with tab/panes
- `workmux open` - Open tab for existing worktree
- `workmux merge` - Merge and close tab
- `workmux remove` - Remove worktree and close tab
- `workmux list` - Show worktrees with tab status
- Pane layouts (horizontal/vertical splits)
- Pane commands and agent injection
- Prompt file handling
- Background creation (`--background`)

## Non-functional requirements

### N1: Backwards compatibility

- Existing tmux users must experience no breaking changes
- Default behaviour remains tmux when `auto` detection is ambiguous (e.g., neither env var set but both installed)
- Configuration files remain compatible

### N2: Performance

- Multiplexer detection should be cached for the duration of a command
- No additional overhead for tmux users

### N3: Error handling

- Clear error messages when:
  - Running outside any multiplexer without explicit config
  - Zellij version is too old (minimum version TBD based on required features)
  - Mixed environments (e.g., tmux nested in Zellij)

### N4: Testing

- Unit tests for multiplexer trait implementations
- Integration tests parameterised to run against both backends
- CI should test both tmux and Zellij

## Technical constraints

### Zellij CLI limitations

1. **Pane IDs**: Zellij's CLI doesn't expose pane IDs in the same way as tmux. May need workarounds:
   - Use `zellij action dump-layout` to parse pane structure
   - Track panes by creation order and position

2. **Detached operations**: Zellij doesn't support creating tabs in background sessions as easily as tmux. May need to:
   - Use `zellij run` for certain operations
   - Implement workarounds for `--background` flag

3. **Command execution**: Zellij's `write-chars` is the equivalent of tmux's `send-keys`, but handles special characters differently.

### Minimum Zellij version

Target Zellij 0.40.0+ which includes:
- `new-tab` with `--name` and `--cwd` options
- `go-to-tab-name` command
- `new-pane` with direction options

## Design considerations

### Architecture

```
src/
  multiplexer/
    mod.rs           # Multiplexer trait + factory function
    tmux.rs          # Tmux implementation (refactored from existing)
    zellij.rs        # Zellij implementation
  tmux.rs            # DEPRECATED: redirect to multiplexer/tmux.rs (or remove)
```

### Factory pattern

```rust
pub fn get_multiplexer(config: &Config, cli_override: Option<&str>) -> Result<Box<dyn Multiplexer>> {
    let backend = cli_override
        .or(config.multiplexer.as_deref())
        .unwrap_or("auto");
    
    match backend {
        "auto" => detect_multiplexer(),
        "tmux" => Ok(Box::new(Tmux::new())),
        "zellij" => Ok(Box::new(Zellij::new())),
        _ => Err(anyhow!("Unknown multiplexer: {}", backend)),
    }
}
```

### Split direction mapping

```rust
impl From<SplitDirection> for ZellijDirection {
    fn from(dir: SplitDirection) -> Self {
        match dir {
            SplitDirection::Horizontal => ZellijDirection::Right,
            SplitDirection::Vertical => ZellijDirection::Down,
        }
    }
}
```

## Open questions

1. **Pane sizing**: Zellij uses pixel-based or percentage sizing differently from tmux. How should we handle the `size` config option?

   a. Map to Zellij percentage equivalents where possible *(recommended)*
   b. Ignore `size` option in Zellij mode and log a warning
   c. Add Zellij-specific size options

2. **Layout persistence**: Zellij has built-in layout files (KDL format). Should workmux integrate with these?

   a. No, keep workmux's YAML config as the source of truth *(recommended for initial implementation)*
   b. Allow exporting workmux pane config to Zellij layout files
   c. Support importing Zellij layouts into workmux config

3. **Tab naming**: Zellij tab names have different constraints than tmux window names. How should we handle the `window_prefix` option?

   a. Rename config option to `tab_prefix` with `window_prefix` as deprecated alias
   b. Keep `window_prefix` name but document it applies to tabs in Zellij *(recommended)*
   c. Add separate `zellij_tab_prefix` option

4. **Session handling**: workmux currently assumes a single tmux session. Zellij also has sessions. Should we support multi-session workflows?

   a. No, target current session only (consistent with current tmux behaviour) *(recommended)*
   b. Add session management commands
   c. Add `session` config option

5. **Minimum Zellij version**: What's the minimum version we should support?

   a. 0.40.0+ (latest stable features) *(recommended)*
   b. 0.38.0+ (wider compatibility)
   c. Latest stable only

6. **Terminology in config**: Should pane-related config keys remain tmux-centric or become generic?

   a. Keep current names (`panes`, `window_prefix`) for backwards compatibility *(recommended)*
   b. Add aliases (`tabs`, `tab_prefix`) that work in both modes
   c. Rename to generic terms (`splits`, `container_prefix`)
