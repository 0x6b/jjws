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
use jj::{
    ForgetDeletion, LoadedWorkspace, create_workspace, forget_workspaces, list_workspaces,
    load_workspace, repo_root_from_repo_path,
};
use jj_lib::ref_name::WorkspaceNameBuf;

pub struct AddOptions {
    pub name: String,
    pub parent_dir: Option<PathBuf>,
    pub no_tab: bool,
}

pub struct ForgetOptions {
    pub workspaces: Vec<String>,
    pub parent_dir: Option<PathBuf>,
}

pub struct ListOptions {
    pub parent_dir: Option<PathBuf>,
}

pub fn add(options: AddOptions) -> Result<()> {
    let ctx = CommandContext::load(options.parent_dir.as_deref())?;
    let destination = ctx.parent_dir.join(&options.name);
    let workspace_name = WorkspaceNameBuf::from(options.name.as_str());

    create_workspace(&ctx.current, &destination, workspace_name)?;

    let symlinked = symlink_ignored_paths(
        ctx.current.workspace.workspace_root(),
        &destination,
        &ctx.current.repo,
        ctx.current.workspace.workspace_name(),
    )?;

    let tab_opened = !options.no_tab
        && match open_tab(&destination) {
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

pub fn forget(options: ForgetOptions) -> Result<()> {
    let ctx = CommandContext::load(options.parent_dir.as_deref())?;
    let target_names = ctx.workspace_names_or_current(&options.workspaces);
    let results =
        forget_workspaces(&ctx.current, &target_names, &ctx.cwd, &ctx.repo_root, &ctx.parent_dir)?;

    if results.is_empty() {
        println!("Nothing changed.");
        return Ok(());
    }

    for r in &results {
        println!("{r}");
    }
    if results.iter().any(|r| r.deletion == ForgetDeletion::KeptRepoHost) {
        println!("The repo still lives under {}", ctx.repo_root.display());
    }

    Ok(())
}

pub fn list(options: ListOptions) -> Result<()> {
    let ctx = CommandContext::load(options.parent_dir.as_deref())?;

    for ws in list_workspaces(&ctx.current, &ctx.repo_root, &ctx.parent_dir) {
        println!("{ws}");
    }

    Ok(())
}

struct CommandContext {
    cwd: PathBuf,
    current: LoadedWorkspace,
    repo_root: PathBuf,
    parent_dir: PathBuf,
}

impl CommandContext {
    fn load(configured_parent: Option<&Path>) -> Result<Self> {
        let cwd = current_dir().context("failed to determine current directory")?;
        let current = load_workspace(&cwd)?;
        let repo_root = repo_root_from_repo_path(current.workspace.repo_path())?;
        let parent_dir = resolve_parent_dir(&cwd, &repo_root, configured_parent)?;
        Ok(Self { cwd, current, repo_root, parent_dir })
    }

    fn workspace_names_or_current(&self, names: &[String]) -> Vec<WorkspaceNameBuf> {
        if names.is_empty() {
            vec![self.current.workspace.workspace_name().to_owned()]
        } else {
            names
                .iter()
                .map(|name| WorkspaceNameBuf::from(name.as_str()))
                .collect()
        }
    }
}

fn resolve_parent_dir(
    cwd: &Path,
    repo_root: &Path,
    configured_parent: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(parent) = configured_parent {
        return Ok(if parent.is_absolute() { parent.to_path_buf() } else { cwd.join(parent) });
    }

    let repo_dir_name = repo_root.file_name().context("repo root has no basename")?;
    let repo_parent = repo_root.parent().context("repo root has no parent")?;
    Ok(repo_parent.join("workspaces").join(repo_dir_name))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn default_parent_dir_uses_repo_root_name() {
        let cwd = Path::new("/tmp/workspaces/example-repo/feature");
        let repo_root = Path::new("/tmp/example-repo");
        let parent = resolve_parent_dir(cwd, repo_root, None).unwrap();
        assert_eq!(parent, PathBuf::from("/tmp/workspaces/example-repo"));
    }

    #[test]
    fn relative_parent_dir_is_resolved_from_cwd() {
        let cwd = Path::new("/tmp/example-repo");
        let repo_root = Path::new("/tmp/example-repo");
        let parent = resolve_parent_dir(cwd, repo_root, Some(Path::new("../custom"))).unwrap();
        assert_eq!(parent, PathBuf::from("/tmp/example-repo/../custom"));
    }
}
