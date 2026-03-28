use std::{
    env::set_current_dir,
    fmt,
    fmt::{Display, Formatter},
    fs::{create_dir_all, read, read_dir, remove_dir_all},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use dirs::{config_dir, home_dir};
use dunce::canonicalize;
use gethostname::gethostname;
use jj_lib::{
    commit::Commit,
    config::{ConfigLayer, ConfigResolutionContext, ConfigSource, StackedConfig, resolve},
    file_util::path_from_bytes,
    ref_name::{WorkspaceName, WorkspaceNameBuf},
    repo::{ReadonlyRepo, Repo as _, StoreFactories},
    rewrite::merge_commit_trees,
    settings::UserSettings,
    workspace::{Workspace, default_working_copy_factories, default_working_copy_factory},
};
use pollster::FutureExt as _;

pub(crate) struct LoadedWorkspace {
    pub(crate) workspace: Workspace,
    pub(crate) repo: Arc<ReadonlyRepo>,
}

pub(crate) struct WorkspaceListEntry {
    pub(crate) name: WorkspaceNameBuf,
    pub(crate) path: PathBuf,
    pub(crate) exists_on_disk: bool,
    pub(crate) is_current: bool,
    pub(crate) is_repo_host: bool,
}

pub(crate) struct ForgetResult {
    pub(crate) name: WorkspaceNameBuf,
    pub(crate) path: PathBuf,
    pub(crate) deletion: ForgetDeletion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ForgetDeletion {
    Removed,
    NotFoundAtInferredPath,
    KeptRepoHost,
}

impl ForgetDeletion {
    fn plan(path: &Path) -> Self {
        if path.join(".jj").join("repo").is_dir() { Self::KeptRepoHost } else { Self::Removed }
    }
}

impl Display for ForgetResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let (name, path) = (self.name.as_symbol(), self.path.display());
        match self.deletion {
            ForgetDeletion::Removed => write!(f, "Forgot workspace {name} and removed {path}"),
            ForgetDeletion::NotFoundAtInferredPath => write!(
                f,
                "Forgot workspace {name} but the inferred directory was not found at {path}"
            ),
            ForgetDeletion::KeptRepoHost => {
                write!(f, "Forgot workspace {name} but kept {path} because it hosts the repo")
            }
        }
    }
}

impl Display for WorkspaceListEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let marker = if self.is_current { "*" } else { " " };
        let suffix = if self.is_repo_host {
            " [repo-host]"
        } else if !self.exists_on_disk {
            " [out-of-control]"
        } else {
            ""
        };
        write!(f, "{marker} {}\t{}{suffix}", self.name.as_symbol(), self.path.display())
    }
}

pub(crate) fn load_workspace(start_dir: &Path) -> Result<LoadedWorkspace> {
    let workspace_root = find_workspace_root(start_dir)?;
    let settings = load_settings(workspace_root)?;
    let workspace = Workspace::load(
        &settings,
        workspace_root,
        &StoreFactories::default(),
        &default_working_copy_factories(),
    )
    .map_err(anyhow::Error::from)?;
    let repo = workspace.repo_loader().load_at_head()?;
    Ok(LoadedWorkspace { workspace, repo })
}

pub(crate) fn create_workspace(
    current: &LoadedWorkspace,
    destination: &Path,
    workspace_name: WorkspaceNameBuf,
) -> Result<()> {
    if current.repo.view().get_wc_commit_id(&workspace_name).is_some() {
        bail!("workspace named '{}' already exists", workspace_name.as_symbol());
    }

    prepare_destination(destination)?;

    let (mut new_workspace, repo_after_add) = Workspace::init_workspace_with_existing_repo(
        destination,
        current.workspace.repo_path(),
        &current.repo,
        &*default_working_copy_factory(),
        workspace_name.clone(),
    )?;

    copy_sparse_patterns(&current.workspace, &mut new_workspace)?;

    let (new_repo, new_wc_commit) = create_initial_workspace_commit(
        &repo_after_add,
        current.workspace.workspace_name(),
        workspace_name,
    )?;
    let new_wc_commit = new_repo.store().get_commit(new_wc_commit.id())?;
    new_workspace.check_out(new_repo.op_id().clone(), None, &new_wc_commit)?;

    Ok(())
}

pub(crate) fn forget_workspaces(
    current: &LoadedWorkspace,
    target_names: &[WorkspaceNameBuf],
    cwd: &Path,
    repo_root: &Path,
    workspace_root: &Path,
) -> Result<Vec<ForgetResult>> {
    let known_targets: Vec<_> = target_names
        .iter()
        .filter(|name| {
            let exists = current.repo.view().get_wc_commit_id(name).is_some();
            if !exists {
                eprintln!("Warning: no such workspace: {}", name.as_symbol());
            }
            exists
        })
        .cloned()
        .collect();

    if known_targets.is_empty() {
        return Ok(vec![]);
    }

    let locator = WorkspaceLocator::new(current, repo_root, workspace_root);
    let planned: Vec<_> = known_targets
        .iter()
        .map(|name| {
            let path = locator.path(name);
            let deletion = ForgetDeletion::plan(&path);
            (name.clone(), path, deletion)
        })
        .collect();

    let mut tx = current.repo.start_transaction();
    for name in &known_targets {
        tx.repo_mut().remove_wc_commit(name)?;
    }
    tx.repo_mut().rebase_descendants()?;

    let description = match known_targets.as_slice() {
        [name] => format!("forget workspace {}", name.as_symbol()),
        names => format!(
            "forget workspaces {}",
            names.iter().map(|n| n.as_str()).collect::<Vec<_>>().join(", ")
        ),
    };
    tx.commit(description)?;

    // Move cwd out before deleting
    for (_, path, deletion) in &planned {
        if *deletion == ForgetDeletion::Removed && cwd.starts_with(path) {
            let parent = path.parent().context("workspace to delete has no parent directory")?;
            set_current_dir(parent)
                .with_context(|| format!("failed to switch to {}", parent.display()))?;
            break;
        }
    }

    planned
        .into_iter()
        .map(|(name, path, deletion)| {
            let deletion = match deletion {
                ForgetDeletion::Removed if path.exists() => {
                    remove_dir_all(&path)
                        .with_context(|| format!("failed to remove {}", path.display()))?;
                    ForgetDeletion::Removed
                }
                ForgetDeletion::Removed => ForgetDeletion::NotFoundAtInferredPath,
                other => other,
            };
            Ok(ForgetResult { name, path, deletion })
        })
        .collect()
}

pub(crate) fn list_workspaces(
    current: &LoadedWorkspace,
    repo_root: &Path,
    workspace_root: &Path,
) -> Vec<WorkspaceListEntry> {
    let locator = WorkspaceLocator::new(current, repo_root, workspace_root);

    current
        .repo
        .view()
        .wc_commit_ids()
        .keys()
        .map(|name| {
            let path = locator.path(name);
            WorkspaceListEntry {
                name: name.clone(),
                exists_on_disk: path.exists(),
                is_current: name == current.workspace.workspace_name(),
                is_repo_host: locator.is_repo_host(name),
                path,
            }
        })
        .collect()
}

pub(crate) fn repo_root_from_repo_path(repo_path: &Path) -> Result<PathBuf> {
    repo_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("repo path is missing its workspace root")
}

fn repo_host_workspace_name(
    current: &LoadedWorkspace,
    repo_root: &Path,
) -> Option<WorkspaceNameBuf> {
    if current.workspace.workspace_root() == repo_root {
        return Some(current.workspace.workspace_name().to_owned());
    }

    load_workspace(repo_root)
        .ok()
        .map(|workspace| workspace.workspace.workspace_name().to_owned())
}

struct WorkspaceLocator<'a> {
    current: &'a LoadedWorkspace,
    repo_root: &'a Path,
    workspace_root: &'a Path,
    repo_host_name: Option<WorkspaceNameBuf>,
}

impl<'a> WorkspaceLocator<'a> {
    fn new(current: &'a LoadedWorkspace, repo_root: &'a Path, workspace_root: &'a Path) -> Self {
        Self {
            current,
            repo_root,
            workspace_root,
            repo_host_name: repo_host_workspace_name(current, repo_root),
        }
    }

    fn path(&self, workspace_name: &WorkspaceName) -> PathBuf {
        if workspace_name == self.current.workspace.workspace_name() {
            return self.current.workspace.workspace_root().to_path_buf();
        }
        if self.is_repo_host(workspace_name) {
            return self.repo_root.to_path_buf();
        }
        self.workspace_root.join(workspace_name.as_str())
    }

    fn is_repo_host(&self, workspace_name: &WorkspaceName) -> bool {
        self.repo_host_name
            .as_ref()
            .is_some_and(|repo_host_name| workspace_name == repo_host_name)
    }
}

fn prepare_destination(destination: &Path) -> Result<()> {
    if !destination.exists() {
        return create_dir_all(destination)
            .with_context(|| format!("failed to create {}", destination.display()));
    }
    if !destination.is_dir() {
        bail!("destination path exists and is not a directory");
    }
    if read_dir(destination)?.next().is_some() {
        bail!("destination path exists and is not an empty directory");
    }
    Ok(())
}

fn copy_sparse_patterns(current: &Workspace, new_workspace: &mut Workspace) -> Result<()> {
    let sparse_patterns = current.working_copy().sparse_patterns()?.to_vec();
    let mut locked_workspace = new_workspace.start_working_copy_mutation()?;
    locked_workspace
        .locked_wc()
        .set_sparse_patterns(sparse_patterns)
        .block_on()?;
    let operation_id = locked_workspace.locked_wc().old_operation_id().clone();
    locked_workspace.finish(operation_id)?;
    Ok(())
}

fn create_initial_workspace_commit(
    repo: &Arc<ReadonlyRepo>,
    current_workspace_name: &WorkspaceName,
    new_workspace_name: WorkspaceNameBuf,
) -> Result<(Arc<ReadonlyRepo>, Commit)> {
    let mut tx = repo.start_transaction();
    let parents = current_workspace_parents(tx.base_repo(), current_workspace_name)?;
    let tree = merge_commit_trees(tx.repo(), &parents).block_on()?;
    let parent_ids = parents.iter().map(|commit| commit.id().clone()).collect();
    let new_wc_commit = tx.repo_mut().new_commit(parent_ids, tree).write()?;
    let operation_description = format!(
        "create initial working-copy commit in workspace {}",
        new_workspace_name.as_symbol()
    );
    tx.repo_mut().edit(new_workspace_name, &new_wc_commit)?;
    tx.repo_mut().rebase_descendants()?;
    let new_repo = tx.commit(operation_description)?;
    Ok((new_repo, new_wc_commit))
}

fn current_workspace_parents(
    repo: &Arc<ReadonlyRepo>,
    workspace_name: &WorkspaceName,
) -> Result<Vec<Commit>> {
    let Some(wc_commit_id) = repo.view().get_wc_commit_id(workspace_name) else {
        return Ok(vec![repo.store().root_commit()]);
    };

    let wc_commit = repo.store().get_commit(wc_commit_id)?;
    if wc_commit.parent_ids().is_empty() {
        return Ok(vec![repo.store().root_commit()]);
    }

    wc_commit
        .parent_ids()
        .iter()
        .map(|parent_id| repo.store().get_commit(parent_id).map_err(Into::into))
        .collect()
}

fn find_workspace_root(start_dir: &Path) -> Result<&Path> {
    let mut current_dir = start_dir;
    loop {
        if current_dir.join(".jj").is_dir() {
            return Ok(current_dir);
        }
        current_dir = current_dir.parent().context(format!(
            "no Jujutsu workspace found in '{}' or any parent directory",
            start_dir.display()
        ))?;
    }
}

fn load_settings(workspace_root: &Path) -> Result<UserSettings> {
    let mut config = StackedConfig::with_defaults();
    load_user_config(&mut config)?;

    let repo_path = resolve_repo_path(workspace_root)?;
    let repo_config_path = repo_path.join("config.toml");
    if repo_config_path.exists() {
        let layer = ConfigLayer::load_from_file(ConfigSource::Repo, repo_config_path)?;
        config.add_layer(layer);
    }

    let hostname = gethostname()
        .into_string()
        .unwrap_or_else(|hostname| hostname.to_string_lossy().into_owned());
    let home_dir = home_dir();
    let context = ConfigResolutionContext {
        home_dir: home_dir.as_deref(),
        repo_path: Some(workspace_root),
        workspace_path: Some(workspace_root),
        command: None,
        hostname: &hostname,
    };
    let resolved = resolve(&config, &context)?;
    Ok(UserSettings::from_config(resolved)?)
}

fn load_user_config(config: &mut StackedConfig) -> Result<()> {
    let home = home_dir();
    let candidates = [
        home.as_ref().map(|h| h.join(".jjconfig.toml")),
        home.as_ref().map(|h| h.join(".config/jj/config.toml")),
        config_dir().map(|d| d.join("jj/config.toml")),
    ];

    for path in candidates.into_iter().flatten().filter(|p| p.exists()) {
        config.add_layer(ConfigLayer::load_from_file(ConfigSource::User, path)?);
    }

    Ok(())
}

fn resolve_repo_path(workspace_root: &Path) -> Result<PathBuf> {
    let jj_dir = workspace_root.join(".jj");
    let repo_path = jj_dir.join("repo");
    if repo_path.is_dir() {
        return Ok(repo_path);
    }
    if repo_path.is_file() {
        let bytes =
            read(&repo_path).with_context(|| format!("failed to read {}", repo_path.display()))?;
        let linked_repo_path = path_from_bytes(&bytes)?;
        return canonicalize(jj_dir.join(linked_repo_path))
            .with_context(|| format!("failed to resolve {}", repo_path.display()));
    }
    bail!("workspace metadata is missing .jj/repo");
}

#[cfg(test)]
mod tests {
    use jj_lib::{config::StackedConfig, ref_name};
    use tempfile::TempDir;

    use super::*;

    fn test_settings() -> UserSettings {
        UserSettings::from_config(StackedConfig::with_defaults()).unwrap()
    }

    #[test]
    fn create_workspace_reuses_current_workspace_parents() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let source_root = temp_dir.path().join("source");
        create_dir_all(&source_root)?;

        let settings = test_settings();
        let (mut workspace, repo) = Workspace::init_simple(&settings, &source_root)?;

        let mut tx = repo.start_transaction();
        let parent_commit = tx
            .repo_mut()
            .new_commit(
                vec![repo.store().root_commit_id().clone()],
                repo.store().root_commit().tree(),
            )
            .set_description("base")
            .write()?;
        let current_wc_commit = tx
            .repo_mut()
            .check_out(workspace.workspace_name().to_owned(), &parent_commit)?;
        tx.repo_mut().rebase_descendants()?;
        let repo = tx.commit("set up workspace")?;
        let current_wc_commit = repo.store().get_commit(current_wc_commit.id())?;
        workspace.check_out(repo.op_id().clone(), None, &current_wc_commit)?;

        let loaded = LoadedWorkspace { workspace, repo };
        let destination = temp_dir.path().join("secondary");
        create_workspace(&loaded, &destination, ref_name::WorkspaceNameBuf::from("secondary"))?;

        let secondary = load_workspace(&destination)?;
        let wc_commit_id = secondary
            .repo
            .view()
            .get_wc_commit_id(secondary.workspace.workspace_name())
            .context("missing working-copy commit for secondary workspace")?;
        let wc_commit = secondary.repo.store().get_commit(wc_commit_id)?;
        assert_eq!(wc_commit.parent_ids(), vec![parent_commit.id().clone()]);
        Ok(())
    }

    #[test]
    fn forget_workspace_removes_linked_workspace_directory() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let source_root = temp_dir.path().join("source");
        let parent_dir = temp_dir.path().join("workspaces");
        let secondary_root = parent_dir.join("secondary");
        create_dir_all(&source_root)?;
        create_dir_all(&parent_dir)?;

        let settings = test_settings();
        let (workspace, repo) = Workspace::init_simple(&settings, &source_root)?;
        let loaded = LoadedWorkspace { workspace, repo };
        create_workspace(&loaded, &secondary_root, WorkspaceNameBuf::from("secondary"))?;

        let secondary = load_workspace(&secondary_root)?;
        let results = forget_workspaces(
            &secondary,
            &[WorkspaceNameBuf::from("secondary")],
            temp_dir.path(),
            &source_root,
            &parent_dir,
        )?;
        assert_eq!(results.len(), 1);
        assert!(!secondary_root.exists());
        assert!(
            secondary
                .repo
                .view()
                .get_wc_commit_id(&WorkspaceNameBuf::from("secondary"))
                .is_some()
        );

        let default_loaded = load_workspace(&source_root)?;
        assert!(
            default_loaded
                .repo
                .view()
                .get_wc_commit_id(&WorkspaceNameBuf::from("secondary"))
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn forget_repo_host_keeps_directory() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let source_root = temp_dir.path().join("source");
        let parent_dir = temp_dir.path().join("workspaces");
        create_dir_all(&source_root)?;
        create_dir_all(&parent_dir)?;

        let settings = test_settings();
        let (workspace, repo) = Workspace::init_simple(&settings, &source_root)?;
        let loaded = LoadedWorkspace { workspace, repo };

        let results = forget_workspaces(
            &loaded,
            &[WorkspaceNameBuf::from("default")],
            temp_dir.path(),
            &source_root,
            &parent_dir,
        )?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].deletion, ForgetDeletion::KeptRepoHost);
        assert!(source_root.exists());
        Ok(())
    }
}
