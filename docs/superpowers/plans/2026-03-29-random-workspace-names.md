# Random Workspace Names Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto-generate `adjective-animal` workspace names and rename `add` to `new`.

**Architecture:** A new `names` module provides a `generate()` function that picks an adjective-animal pair using system time as a seed. The `add` subcommand is renamed to `new` with `name` becoming an optional `--name` flag. Fish completions and README are updated to match.

**Tech Stack:** Rust, clap, std::time::SystemTime

---

### Task 1: Create the name generator module

**Files:**
- Create: `src/names.rs`

- [ ] **Step 1: Write the failing test**

In `src/names.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_returns_adjective_hyphen_animal() {
        let name = generate(|_| false);
        let parts: Vec<&str> = name.splitn(2, '-').collect();
        assert_eq!(parts.len(), 2, "expected adjective-animal, got: {name}");
        assert!(ADJECTIVES.contains(&parts[0]), "unknown adjective: {}", parts[0]);
        assert!(ANIMALS.contains(&parts[1]), "unknown animal: {}", parts[1]);
    }

    #[test]
    fn generate_appends_number_on_collision() {
        let first = generate(|_| false);
        // Simulate: the base name and name+2 both exist
        let mut call = 0;
        let name = generate(|candidate| {
            call += 1;
            candidate == first || candidate == format!("{first}2")
        });
        assert_eq!(name, format!("{first}3"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib names`
Expected: compilation error — `generate`, `ADJECTIVES`, `ANIMALS` not defined.

- [ ] **Step 3: Write the implementation**

In `src/names.rs`:

```rust
use std::time::SystemTime;

static ADJECTIVES: &[&str] = &[
    "bold", "brave", "bright", "calm", "clever",
    "cool", "daring", "eager", "fair", "fierce",
    "fleet", "free", "gentle", "glad", "golden",
    "grand", "happy", "hardy", "keen", "kind",
    "lively", "lucky", "mellow", "merry", "mighty",
    "noble", "plucky", "proud", "quick", "quiet",
    "rapid", "ready", "sharp", "sleek", "sleepy",
    "smooth", "snappy", "snowy", "spry", "steady",
    "stoic", "sunny", "swift", "tender", "tidy",
    "vivid", "warm", "wild", "witty", "zesty",
];

static ANIMALS: &[&str] = &[
    "alpaca", "badger", "bear", "bison", "bobcat",
    "bunny", "caribou", "cat", "cobra", "condor",
    "corgi", "crane", "crow", "deer", "dingo",
    "eagle", "falcon", "ferret", "finch", "fox",
    "gecko", "goose", "hawk", "heron", "horse",
    "husky", "ibis", "impala", "jackal", "jaguar",
    "koala", "lemur", "lion", "llama", "lynx",
    "moose", "newt", "okapi", "otter", "owl",
    "panda", "parrot", "puma", "quail", "raven",
    "robin", "salmon", "seal", "stork", "swan",
    "tiger", "toad", "viper", "whale", "wolf",
];

pub fn generate(exists: impl Fn(&str) -> bool) -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize;

    let adj = ADJECTIVES[nanos % ADJECTIVES.len()];
    let animal = ANIMALS[(nanos / ADJECTIVES.len()) % ANIMALS.len()];
    let base = format!("{adj}-{animal}");

    if !exists(&base) {
        return base;
    }

    let mut suffix = 2;
    loop {
        let candidate = format!("{base}{suffix}");
        if !exists(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

#[cfg(test)]
mod tests {
    // ... tests from step 1 ...
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib names`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/names.rs
git commit -m "feat: add adjective-animal name generator"
```

---

### Task 2: Rename `add` to `new` and make name optional

**Files:**
- Modify: `src/main.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Update `src/main.rs`**

Replace the `Command` enum and `main()`:

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use jjws::{NewOptions, cd, forget, list, new_workspace};

#[derive(Parser, Debug)]
#[command(about, version)]
struct Cli {
    /// Root directory where workspaces are created as <DIR>/<repo>/<name>.
    /// Defaults to <data-dir>/jjws (e.g. ~/Library/Application Support/jjws)
    #[arg(long, global = true, value_name = "DIR")]
    workspace_root: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create a new workspace and open it in Ghostty
    New {
        /// Workspace name (auto-generated if omitted)
        #[arg(long)]
        name: Option<String>,

        /// Command to run in the new tab after cd-ing into the workspace
        command: Option<String>,

        /// Skip opening a Ghostty tab
        #[arg(long)]
        no_tab: bool,
    },
    /// Open a Ghostty tab at a workspace (defaults to repo-host)
    Cd {
        /// Workspace name (defaults to repo-host workspace)
        name: Option<String>,
    },
    /// List workspaces associated with the repo
    List,
    /// Forget workspaces, then remove their directories when safe.
    /// Must be run from the repo-host workspace.
    Forget {
        /// Workspace names to forget
        #[arg(required = true)]
        workspaces: Vec<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let ws_root = cli.workspace_root.as_deref();

    match cli.command {
        Command::New { name, command, no_tab } => {
            new_workspace(NewOptions { name, command, no_tab }, ws_root)
        }
        Command::Forget { workspaces } => forget(workspaces, ws_root),
        Command::List => list(ws_root),
        Command::Cd { name } => cd(name.as_deref(), ws_root),
    }
}
```

- [ ] **Step 2: Update `src/lib.rs`**

1. Add `mod names;` to the module declarations at the top (line 1-3).
2. Rename `AddOptions` to `NewOptions`, change `name` to `Option<String>`.
3. Rename `pub fn add` to `pub fn new_workspace`.
4. When `name` is `None`, call `names::generate()` with a closure that checks existing workspace names.

The updated `NewOptions` and `new_workspace()`:

```rust
pub struct NewOptions {
    pub name: Option<String>,
    pub command: Option<String>,
    pub no_tab: bool,
}

pub fn new_workspace(options: NewOptions, workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root)?;

    let name = match options.name {
        Some(name) => name,
        None => {
            let repo_view = ctx.current.repo.view();
            names::generate(|candidate| {
                repo_view
                    .get_wc_commit_id(&WorkspaceNameBuf::from(candidate))
                    .is_some()
            })
        }
    };

    let repo_dir_name = ctx
        .repo_root
        .file_name()
        .context("repo root has no directory name")?;
    let destination = ctx.workspace_root.join(repo_dir_name).join(&name);
    let workspace_name = WorkspaceNameBuf::from(name.as_str());

    create_workspace(&ctx.current, &destination, workspace_name)?;

    let symlinked = symlink_ignored_paths(
        ctx.current.workspace.workspace_root(),
        &destination,
        &ctx.current.repo,
        ctx.current.workspace.workspace_name(),
    )?;

    let tab_opened = !options.no_tab
        && match open_tab(&destination, options.command.as_deref()) {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(err) => {
                eprintln!("Warning: failed to open Ghostty tab: {err:#}");
                false
            }
        };

    println!("Created workspace at {}", destination.display());
    let noun = if symlinked == 1 { "path" } else { "paths" };
    println!("Symlinked {symlinked} jj-ignored {noun}");
    match (tab_opened, options.no_tab) {
        (true, _) => println!("Opened and focused a Ghostty tab"),
        (false, false) => println!("Ghostty tab was not opened"),
        _ => {}
    }

    Ok(())
}
```

- [ ] **Step 3: Build and run tests**

Run: `cargo test`
Expected: all tests pass (existing tests plus the 2 new names tests).

- [ ] **Step 4: Commit**

```bash
git add src/main.rs src/lib.rs
git commit -m "feat: rename add to new, auto-generate workspace names"
```

---

### Task 3: Update fish completions

**Files:**
- Modify: `completions/jjws.fish`

- [ ] **Step 1: Update the completions file**

```fish
# Fish completions for jjws

# Helper: extract workspace names from `jjws list` output
function __jjws_workspaces
    jjws list 2>/dev/null | string replace -r '^[* ] ([^\t]+)\t.*' '$1'
end

# Disable file completions by default
complete -c jjws -f

# Global options
complete -c jjws -l workspace-root -r -F -d 'Root directory for workspaces'
complete -c jjws -l help -s h -d 'Print help'
complete -c jjws -l version -s V -d 'Print version'

# Subcommands (only when no subcommand given yet)
complete -c jjws -n __fish_use_subcommand -a new -d 'Create a new workspace and open it in Ghostty'
complete -c jjws -n __fish_use_subcommand -a cd -d 'Open a Ghostty tab at a workspace'
complete -c jjws -n __fish_use_subcommand -a list -d 'List workspaces associated with the repo'
complete -c jjws -n __fish_use_subcommand -a forget -d 'Forget workspaces and remove directories'
complete -c jjws -n __fish_use_subcommand -a help -d 'Print help for a subcommand'

# new: optional --name flag and --no-tab
complete -c jjws -n '__fish_seen_subcommand_from new' -l name -r -d 'Workspace name (auto-generated if omitted)'
complete -c jjws -n '__fish_seen_subcommand_from new' -l no-tab -d 'Skip opening a Ghostty tab'

# cd: complete workspace names
complete -c jjws -n '__fish_seen_subcommand_from cd' -a '(__jjws_workspaces)' -d 'Workspace name'

# forget: complete workspace names
complete -c jjws -n '__fish_seen_subcommand_from forget' -a '(__jjws_workspaces)' -d 'Workspace name'

# list: no additional arguments

# help: complete subcommand names
complete -c jjws -n '__fish_seen_subcommand_from help' -a 'new cd list forget' -d 'Subcommand'
```

- [ ] **Step 2: Commit**

```bash
git add completions/jjws.fish
git commit -m "chore: update fish completions for new subcommand"
```

---

### Task 4: Update README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update feature list and usage block**

Feature list — replace the first bullet and add auto-naming mention:

```markdown
- **Creates workspaces** outside the repo tree (under `<data-dir>/jjws` by default), with auto-generated animal names
- **Symlinks jj-ignored paths** (e.g. `node_modules/`, `build/`) from the source workspace so tools just work
- **Opens a Ghostty tab** in the new workspace (macOS, opt-out with `--no-tab`), optionally running a command
- **Jumps to workspaces** with `cd` — opens a Ghostty tab at any workspace (defaults to repo-host)
- **Cleans up** forgotten workspaces by removing their directories when safe
```

Update the `--help` block to match the new `cargo run -- --help` output (run it and paste).

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: update README for new subcommand and auto-naming"
```
