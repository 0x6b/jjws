use std::{
    fs::{create_dir_all, read, read_dir},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Error, Result, bail};
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
    workspace::{
        Workspace, WorkspaceLoadError, default_working_copy_factories, default_working_copy_factory,
    },
};
use pollster::FutureExt as _;

pub(crate) struct LoadedWorkspace {
    pub(crate) workspace: Workspace,
    pub(crate) repo: Arc<ReadonlyRepo>,
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
    .map_err(map_workspace_load_error)?;
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

fn prepare_destination(destination: &Path) -> Result<()> {
    if !destination.exists() {
        create_dir_all(destination)
            .with_context(|| format!("failed to create {}", destination.display()))?;
        return Ok(());
    }

    if !destination.is_dir() {
        bail!("destination path exists and is not a directory");
    }

    if read_dir(destination)
        .with_context(|| format!("failed to read {}", destination.display()))?
        .next()
        .transpose()
        .with_context(|| format!("failed to read {}", destination.display()))?
        .is_some()
    {
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
    let candidates: Vec<PathBuf> = [
        home.as_ref().map(|home| home.join(".jjconfig.toml")),
        home.as_ref().map(|home| home.join(".config/jj/config.toml")),
        config_dir().map(|config_dir| config_dir.join("jj/config.toml")),
    ]
    .into_iter()
    .flatten()
    .collect();

    for path in candidates {
        if path.exists() {
            let layer = ConfigLayer::load_from_file(ConfigSource::User, path)?;
            config.add_layer(layer);
        }
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

fn map_workspace_load_error(err: WorkspaceLoadError) -> Error {
    err.into()
}

#[cfg(test)]
mod tests {

    #[cfg(test)]
    use std::fs::create_dir_all;

    use jj_lib::config::StackedConfig;
    #[cfg(test)]
    use jj_lib::ref_name;
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
}
