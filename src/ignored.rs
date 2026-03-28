#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(windows)]
use std::os::windows::fs::symlink_dir;
#[cfg(windows)]
use std::os::windows::fs::symlink_file;
use std::{
    collections::HashSet,
    env::var,
    ffi::OsStr,
    fs::{DirEntry, create_dir_all, read_dir, symlink_metadata},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use dirs::home_dir;
use jj_lib::{
    git::get_git_backend,
    gitignore::GitIgnoreFile,
    ref_name::WorkspaceName,
    repo::{ReadonlyRepo, Repo as _},
};

pub fn symlink_ignored_paths(
    source_root: &Path,
    destination_root: &Path,
    repo: &Arc<ReadonlyRepo>,
    workspace_name: &WorkspaceName,
) -> Result<usize> {
    let tracked_paths = collect_tracked_paths(repo, workspace_name)?;
    let base_ignores = load_base_ignores(repo)?;
    let ignored_paths = collect_ignored_paths(source_root, &tracked_paths, &base_ignores)?;

    let mut created = 0;
    for relative_path in ignored_paths {
        let source_path = source_root.join(&relative_path);
        let destination_path = destination_root.join(&relative_path);
        if destination_path.symlink_metadata().is_ok() {
            continue;
        }
        if let Some(parent) = destination_path.parent() {
            create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        create_symlink(&source_path, &destination_path)?;
        created += 1;
    }

    Ok(created)
}

fn collect_tracked_paths(
    repo: &Arc<ReadonlyRepo>,
    workspace_name: &WorkspaceName,
) -> Result<TrackedPaths> {
    let Some(wc_commit_id) = repo.view().get_wc_commit_id(workspace_name) else {
        return Ok(TrackedPaths::default());
    };

    let commit = repo.store().get_commit(wc_commit_id)?;
    let mut tracked_paths = HashSet::new();
    let mut tracked_dirs = HashSet::new();
    for (path, value) in commit.tree().entries() {
        let value = value?;
        if !value.is_present() || value.is_tree() {
            continue;
        }
        let path = path.as_internal_file_string().to_string();
        add_parent_directories(&path, &mut tracked_dirs);
        tracked_paths.insert(path);
    }

    Ok(TrackedPaths { tracked_paths, tracked_dirs })
}

fn add_parent_directories(path: &str, tracked_dirs: &mut HashSet<String>) {
    let mut end = 0;
    while let Some(next) = path[end..].find('/') {
        end += next;
        tracked_dirs.insert(path[..end].to_string());
        end += 1;
    }
}

fn load_base_ignores(repo: &Arc<ReadonlyRepo>) -> Result<Arc<GitIgnoreFile>> {
    let mut git_ignores = GitIgnoreFile::empty();

    if let Some(global_excludes) = default_global_git_ignore() {
        git_ignores = git_ignores.chain_with_file("", global_excludes)?;
    }

    if let Ok(git_backend) = get_git_backend(repo.store()) {
        git_ignores = git_ignores
            .chain_with_file("", git_backend.git_repo_path().join("info").join("exclude"))?;
    }

    Ok(git_ignores)
}

fn default_global_git_ignore() -> Option<PathBuf> {
    if let Ok(xdg_config_home) = var("XDG_CONFIG_HOME")
        && !xdg_config_home.is_empty()
    {
        let path = PathBuf::from(xdg_config_home).join("git").join("ignore");
        if path.is_file() {
            return Some(path);
        }
    }

    let home = home_dir()?;
    let path = home.join(".config").join("git").join("ignore");
    path.is_file().then_some(path)
}

fn collect_ignored_paths(
    source_root: &Path,
    tracked_paths: &TrackedPaths,
    base_ignores: &Arc<GitIgnoreFile>,
) -> Result<Vec<PathBuf>> {
    let mut ignored_paths = Vec::new();
    walk_ignored_paths(
        source_root,
        source_root,
        "",
        tracked_paths,
        base_ignores.clone(),
        &mut ignored_paths,
    )?;
    Ok(ignored_paths)
}

fn walk_ignored_paths(
    source_root: &Path,
    current_dir: &Path,
    relative_dir: &str,
    tracked_paths: &TrackedPaths,
    inherited_ignores: Arc<GitIgnoreFile>,
    ignored_paths: &mut Vec<PathBuf>,
) -> Result<()> {
    let current_ignores = load_directory_gitignore(current_dir, relative_dir, inherited_ignores)?;
    let mut entries: Vec<DirEntry> = read_dir(current_dir)
        .with_context(|| format!("failed to read {}", current_dir.display()))?
        .collect::<io::Result<_>>()
        .with_context(|| format!("failed to read {}", current_dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let file_name = entry.file_name();
        if should_skip_root_entry(source_root, current_dir, &file_name) {
            continue;
        }

        let file_name = file_name
            .to_str()
            .context("encountered a non-UTF-8 path while scanning ignored files")?;
        let source_path = entry.path();
        let metadata = symlink_metadata(&source_path)
            .with_context(|| format!("failed to read {}", source_path.display()))?;
        let is_dir = metadata.file_type().is_dir();
        let relative_path = if relative_dir.is_empty() {
            file_name.to_string()
        } else {
            format!("{relative_dir}/{file_name}")
        };

        let ignore_key = if is_dir { format!("{relative_path}/") } else { relative_path.clone() };
        let is_ignored = current_ignores.matches(&ignore_key);

        if is_dir {
            if is_ignored && !tracked_paths.has_tracked_descendants(&relative_path) {
                ignored_paths.push(PathBuf::from(&relative_path));
                continue;
            }
            walk_ignored_paths(
                source_root,
                &source_path,
                &relative_path,
                tracked_paths,
                current_ignores.clone(),
                ignored_paths,
            )?;
        } else if is_ignored && !tracked_paths.contains(&relative_path) {
            ignored_paths.push(PathBuf::from(relative_path));
        }
    }

    Ok(())
}

fn load_directory_gitignore(
    current_dir: &Path,
    relative_dir: &str,
    inherited_ignores: Arc<GitIgnoreFile>,
) -> Result<Arc<GitIgnoreFile>> {
    let prefix = if relative_dir.is_empty() { String::new() } else { format!("{relative_dir}/") };
    inherited_ignores
        .chain_with_file(&prefix, current_dir.join(".gitignore"))
        .map_err(Into::into)
}

fn should_skip_root_entry(source_root: &Path, current_dir: &Path, file_name: &OsStr) -> bool {
    current_dir == source_root && (file_name == ".jj" || file_name == ".git")
}

#[cfg(unix)]
fn create_symlink(source: &Path, destination: &Path) -> Result<()> {
    symlink(source, destination)
        .with_context(|| format!("failed to create {}", destination.display()))
}

#[cfg(windows)]
fn create_symlink(source: &Path, destination: &Path) -> Result<()> {
    if source.is_dir() {
        symlink_dir(source, destination)
            .with_context(|| format!("failed to create {}", destination.display()))
    } else {
        symlink_file(source, destination)
            .with_context(|| format!("failed to create {}", destination.display()))
    }
}

#[derive(Debug, Default)]
struct TrackedPaths {
    tracked_paths: HashSet<String>,
    tracked_dirs: HashSet<String>,
}

impl TrackedPaths {
    fn contains(&self, path: &str) -> bool {
        self.tracked_paths.contains(path)
    }

    fn has_tracked_descendants(&self, path: &str) -> bool {
        self.tracked_dirs.contains(path)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections,
        fs::{create_dir_all, write},
    };

    use jj_lib::gitignore;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn collect_ignored_paths_symlinks_whole_untracked_directory() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let root = temp_dir.path();
        write(root.join(".gitignore"), "node_modules/\n")?;
        create_dir_all(root.join("node_modules").join("pkg"))?;
        write(root.join("node_modules").join("pkg").join("file"), "contents")?;

        let ignored_paths = collect_ignored_paths(
            root,
            &TrackedPaths::default(),
            &gitignore::GitIgnoreFile::empty(),
        )?;
        assert_eq!(ignored_paths, vec![PathBuf::from("node_modules")]);
        Ok(())
    }

    #[test]
    fn collect_ignored_paths_recurses_when_directory_has_tracked_descendants() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let root = temp_dir.path();
        write(root.join(".gitignore"), "build/\n")?;
        create_dir_all(root.join("build"))?;
        write(root.join("build").join("tracked.txt"), "tracked")?;
        write(root.join("build").join("cache.bin"), "ignored")?;

        let tracked_paths = TrackedPaths {
            tracked_paths: collections::HashSet::from([String::from("build/tracked.txt")]),
            tracked_dirs: collections::HashSet::from([String::from("build")]),
        };
        let ignored_paths =
            collect_ignored_paths(root, &tracked_paths, &gitignore::GitIgnoreFile::empty())?;
        assert_eq!(ignored_paths, vec![PathBuf::from("build/cache.bin")]);
        Ok(())
    }
}
