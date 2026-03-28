mod ghostty;
mod ignored;
mod jj;

use std::{
    env::current_dir,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use ghostty::open_tab;
use ignored::symlink_ignored_paths;
use jj::{create_workspace, load_workspace};
use jj_lib::ref_name::WorkspaceNameBuf;

pub struct RunOptions {
    pub name: String,
    pub parent_dir: Option<PathBuf>,
    pub no_tab: bool,
}

pub fn run(options: RunOptions) -> Result<()> {
    let cwd = current_dir().context("failed to determine current directory")?;
    let parent_dir = resolve_parent_dir(&cwd, options.parent_dir.as_deref())?;
    let destination = parent_dir.join(&options.name);

    let current = load_workspace(&cwd)?;
    let workspace_name = WorkspaceNameBuf::from(options.name.as_str());
    create_workspace(&current, &destination, workspace_name)?;

    let symlinked = symlink_ignored_paths(
        current.workspace.workspace_root(),
        &destination,
        &current.repo,
        current.workspace.workspace_name(),
    )?;

    let tab_opened = if options.no_tab {
        false
    } else {
        match open_tab(&destination) {
            Ok(Some(_terminal_id)) => true,
            Ok(None) => false,
            Err(err) => {
                eprintln!("Warning: failed to open Ghostty tab: {err:#}");
                false
            }
        }
    };

    println!("Created workspace at {}", destination.display());
    println!("Symlinked {symlinked} jj-ignored {}", if symlinked == 1 { "path" } else { "paths" });
    if tab_opened {
        println!("Opened and focused a Ghostty tab");
    } else if !options.no_tab {
        println!("Ghostty tab was not opened");
    }

    Ok(())
}

fn resolve_parent_dir(cwd: &Path, configured_parent: Option<&Path>) -> Result<PathBuf> {
    if let Some(parent) = configured_parent {
        return Ok(if parent.is_absolute() { parent.to_path_buf() } else { cwd.join(parent) });
    }

    let current_dir_name = cwd.file_name().context("current directory has no basename")?;
    let cwd_parent = cwd.parent().context("current directory has no parent")?;
    Ok(cwd_parent.join("workspaces").join(current_dir_name))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn default_parent_dir_uses_current_directory_name() {
        let cwd = Path::new("/tmp/example-repo");
        let parent = resolve_parent_dir(cwd, None).unwrap();
        assert_eq!(parent, PathBuf::from("/tmp/workspaces/example-repo"));
    }

    #[test]
    fn relative_parent_dir_is_resolved_from_cwd() {
        let cwd = Path::new("/tmp/example-repo");
        let parent = resolve_parent_dir(cwd, Some(Path::new("../custom"))).unwrap();
        assert_eq!(parent, PathBuf::from("/tmp/example-repo/../custom"));
    }
}
