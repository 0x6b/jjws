mod herdr;
mod ignored;
mod jj;
mod names;

use std::{
    env::current_dir,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use dirs::data_dir;
use herdr::open_tab;
use ignored::symlink_ignored_paths;
use jj::{
    ForgetDeletion, LoadedWorkspace, create_workspace, forget_workspaces, list_workspaces,
    load_workspace, locate_workspace, repo_root_from_repo_path, repo_workspace_dir,
};
use jj_lib::ref_name::WorkspaceNameBuf;
use names::generate;

pub struct NewOptions {
    pub name: Option<String>,
    pub command: Option<String>,
    pub no_tab: bool,
}

pub struct ListOptions {
    pub porcelain: bool,
    pub path_only: Option<String>,
}

fn open_tab_or_warn(path: &Path, repo_root: &Path, command: Option<&str>) -> bool {
    match open_tab(path, repo_root, command) {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(err) => {
            eprintln!("Warning: failed to open Herdr tab: {err:#}");
            false
        }
    }
}

pub async fn new_workspace(options: NewOptions, workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root).await?;
    let name = options.name.unwrap_or_else(|| {
        let repo_view = ctx.current.repo.view();
        generate(|candidate| {
            repo_view
                .get_wc_commit_id(&WorkspaceNameBuf::from(candidate))
                .is_some()
        })
    });
    let destination = repo_workspace_dir(&ctx.repo_root, &ctx.workspace_root).join(&name);
    let workspace_name = WorkspaceNameBuf::from(name.as_str());

    create_workspace(&ctx.current, &destination, workspace_name).await?;

    let symlinked = symlink_ignored_paths(
        ctx.current.workspace.workspace_root(),
        &destination,
        &ctx.current.repo,
        ctx.current.workspace.workspace_name(),
    )?;

    let tab_opened = !options.no_tab
        && open_tab_or_warn(&destination, &ctx.repo_root, options.command.as_deref());

    println!("Created workspace at {}", destination.display());
    let noun = if symlinked == 1 { "path" } else { "paths" };
    println!("Symlinked {symlinked} jj-ignored {noun}");
    if !options.no_tab {
        println!(
            "{}",
            if tab_opened { "Opened and focused a Herdr tab" } else { "Herdr tab was not opened" }
        );
    }

    Ok(())
}

pub async fn forget(workspaces: Vec<String>, workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root).await?;
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
    )
    .await?;

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

pub async fn tab(workspace: String, workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root).await?;
    let workspace_name = WorkspaceNameBuf::from(workspace.as_str());
    let path =
        locate_workspace(&ctx.current, &workspace_name, &ctx.repo_root, &ctx.workspace_root).await?;

    match open_tab(&path, &ctx.repo_root, None)? {
        Some(_) => println!("Opened Herdr tab at {}", path.display()),
        None => bail!("Herdr is not available"),
    }
    Ok(())
}

pub async fn list(options: ListOptions, workspace_root: Option<&Path>) -> Result<()> {
    let ctx = CommandContext::load(workspace_root).await?;
    if let Some(workspace) = options.path_only {
        let workspace_name = WorkspaceNameBuf::from(workspace.as_str());
        let path =
            locate_workspace(&ctx.current, &workspace_name, &ctx.repo_root, &ctx.workspace_root)
                .await?;
        println!("{}", path.display());
        return Ok(());
    }

    let include_commits = !options.porcelain;

    for ws in
        list_workspaces(&ctx.current, &ctx.repo_root, &ctx.workspace_root, include_commits).await
    {
        if options.porcelain {
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
    async fn load(workspace_root: Option<&Path>) -> Result<Self> {
        let cwd = current_dir().context("failed to determine current directory")?;
        let current = load_workspace(&cwd).await?;
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
