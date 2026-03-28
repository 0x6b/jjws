mod ghostty;
mod ignored;
mod jj;

use std::{
    env::current_dir,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use ghostty::open_tab;
use ignored::symlink_ignored_paths;
use jj::{
    ForgetDeletion, LoadedWorkspace, create_workspace, forget_workspaces, list_workspaces,
    load_workspace, repo_root_from_repo_path,
};
use jj_lib::ref_name::WorkspaceNameBuf;

pub struct AddOptions {
    pub name: String,
    pub command: Option<String>,
    pub no_tab: bool,
}

pub fn add(options: AddOptions, workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root)?;
    let repo_dir_name = ctx
        .repo_root
        .file_name()
        .context("repo root has no directory name")?;
    let destination = ctx.workspace_root.join(repo_dir_name).join(&options.name);
    let workspace_name = WorkspaceNameBuf::from(options.name.as_str());

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

pub fn forget(workspaces: Vec<String>, workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root)?;
    if ctx.current.workspace.workspace_root() != ctx.repo_root {
        bail!(
            "forget must be run from the repo-host workspace ({})",
            ctx.repo_root.display()
        );
    }
    let target_names: Vec<WorkspaceNameBuf> = workspaces
        .iter()
        .map(|name| WorkspaceNameBuf::from(name.as_str()))
        .collect();
    let results =
        forget_workspaces(&ctx.current, &target_names, &ctx.cwd, &ctx.repo_root, &ctx.workspace_root)?;

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

pub fn list(workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root)?;

    for ws in list_workspaces(&ctx.current, &ctx.repo_root, &ctx.workspace_root) {
        println!("{ws}");
    }

    Ok(())
}

struct CommandContext {
    cwd: PathBuf,
    current: LoadedWorkspace,
    repo_root: PathBuf,
    workspace_root: PathBuf,
}

impl CommandContext {
    fn load(workspace_root: Option<&Path>) -> Result<Self> {
        let cwd = current_dir().context("failed to determine current directory")?;
        let current = load_workspace(&cwd)?;
        let repo_root = repo_root_from_repo_path(current.workspace.repo_path())?;
        let workspace_root = resolve_workspace_root(&cwd, workspace_root)?;
        Ok(Self { cwd, current, repo_root, workspace_root })
    }

}

fn resolve_workspace_root(cwd: &Path, configured: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = configured {
        return Ok(if root.is_absolute() { root.to_path_buf() } else { cwd.join(root) });
    }

    dirs::data_dir()
        .map(|d| d.join("jjws"))
        .context("failed to determine data directory")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn default_workspace_root_uses_data_dir() {
        let cwd = Path::new("/tmp/example-repo");
        let root = resolve_workspace_root(cwd, None).unwrap();
        assert_eq!(root, dirs::data_dir().unwrap().join("jjws"));
    }

    #[test]
    fn relative_workspace_root_is_resolved_from_cwd() {
        let cwd = Path::new("/tmp/example-repo");
        let root = resolve_workspace_root(cwd, Some(Path::new("../custom"))).unwrap();
        assert_eq!(root, PathBuf::from("/tmp/example-repo/../custom"));
    }

    #[test]
    fn absolute_workspace_root_is_used_as_is() {
        let cwd = Path::new("/tmp/example-repo");
        let root = resolve_workspace_root(cwd, Some(Path::new("/my/workspaces"))).unwrap();
        assert_eq!(root, PathBuf::from("/my/workspaces"));
    }
}
