mod ghostty;
mod ignored;
mod jj;
mod names;

use std::{
    env::current_dir,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use dirs::data_dir;
use ghostty::open_tab;
use ignored::symlink_ignored_paths;
use jj::{
    ForgetDeletion, LoadedWorkspace, create_workspace, forget_workspaces, list_workspaces,
    load_workspace, locate_workspace, repo_root_from_repo_path,
};
use jj_lib::ref_name::WorkspaceNameBuf;
use names::generate;

pub struct NewOptions {
    pub name: Option<String>,
    pub command: Option<String>,
    pub no_tab: bool,
}

fn open_tab_or_warn(path: &Path, command: Option<&str>) -> bool {
    match open_tab(path, command) {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(err) => {
            eprintln!("Warning: failed to open Ghostty tab: {err:#}");
            false
        }
    }
}

pub fn new_workspace(options: NewOptions, workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root)?;
    let name = options.name.unwrap_or_else(|| {
        let repo_view = ctx.current.repo.view();
        generate(|candidate| {
            repo_view
                .get_wc_commit_id(&WorkspaceNameBuf::from(candidate))
                .is_some()
        })
    });
    let repo_dir_name = ctx.repo_root.file_name().context("repo root has no directory name")?;
    let destination = ctx.workspace_root.join(repo_dir_name).join(&name);
    let workspace_name = WorkspaceNameBuf::from(name.as_str());

    create_workspace(&ctx.current, &destination, workspace_name)?;

    let symlinked = symlink_ignored_paths(
        ctx.current.workspace.workspace_root(),
        &destination,
        &ctx.current.repo,
        ctx.current.workspace.workspace_name(),
    )?;

    let tab_opened = !options.no_tab && open_tab_or_warn(&destination, options.command.as_deref());

    println!("Created workspace at {}", destination.display());
    let noun = if symlinked == 1 { "path" } else { "paths" };
    println!("Symlinked {symlinked} jj-ignored {noun}");
    if !options.no_tab {
        println!(
            "{}",
            if tab_opened {
                "Opened and focused a Ghostty tab"
            } else {
                "Ghostty tab was not opened"
            }
        );
    }

    Ok(())
}

pub fn forget(workspaces: Vec<String>, workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root)?;
    if ctx.current.workspace.workspace_root() != ctx.repo_root {
        bail!("forget must be run from the repo-host workspace ({})", ctx.repo_root.display());
    }
    let target_names: Vec<WorkspaceNameBuf> = workspaces
        .iter()
        .map(|name| WorkspaceNameBuf::from(name.as_str()))
        .collect();
    let results = forget_workspaces(
        &ctx.current,
        &target_names,
        &ctx.cwd,
        &ctx.repo_root,
        &ctx.workspace_root,
    )?;

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

pub fn cd(name: Option<&str>, workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root)?;
    let path = match name {
        Some(name) => {
            let workspace_name = WorkspaceNameBuf::from(name);
            locate_workspace(&ctx.current, &workspace_name, &ctx.repo_root, &ctx.workspace_root)?
        }
        None => ctx.repo_root.clone(),
    };

    if open_tab_or_warn(&path, None) {
        println!("Opened Ghostty tab at {}", path.display());
    } else {
        println!("{}", path.display());
    }
    Ok(())
}

pub fn list(porcelain: bool, workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root)?;
    let include_commits = !porcelain;

    for ws in list_workspaces(&ctx.current, &ctx.repo_root, &ctx.workspace_root, include_commits) {
        if porcelain {
            println!("{ws}");
        } else {
            ws.print_colored();
        }
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

    data_dir()
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
