#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(windows)]
use std::os::windows::fs::{symlink_dir, symlink_file};
use std::{
    collections::HashSet,
    env::var,
    ffi::OsStr,
    fs::{DirEntry, copy, create_dir_all, hard_link, read_dir, read_link},
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
    repo_path::RepoPath,
};

/// What `link_ignored_paths` produced, so the caller can report it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LinkSummary {
    pub symlinked: usize,
    pub hardlink_trees: usize,
    pub hardlinked_files: usize,
    pub copied_files: usize,
}

pub fn copy_worktree_included_files(
    source_root: &Path,
    destination_root: &Path,
    repo: &Arc<ReadonlyRepo>,
    workspace_name: &WorkspaceName,
) -> Result<usize> {
    let include_file = source_root.join(".worktreeinclude");
    if !include_file.is_file() {
        return Ok(0);
    }

    let tracked_paths = collect_tracked_paths(repo, workspace_name)?;
    let base_ignores = load_base_ignores(repo)?;
    let includes = GitIgnoreFile::empty().chain_with_file(RepoPath::root(), include_file)?;
    let paths = collect_worktree_included_paths(
        source_root,
        &tracked_paths,
        &base_ignores,
        &includes,
    )?;

    copy_worktree_included_paths(source_root, destination_root, &paths)
}

fn copy_worktree_included_paths(
    source_root: &Path,
    destination_root: &Path,
    paths: &[PathBuf],
) -> Result<usize> {
    paths.iter().try_fold(0, |copied, relative_path| {
        let destination_path = destination_root.join(relative_path);
        // A tracked checkout or an earlier setup step owns an existing path.
        // Never replace it with local contents from another workspace.
        if destination_path.symlink_metadata().is_ok() {
            return Ok(copied);
        }
        if let Some(parent) = destination_path.parent() {
            create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let source_path = source_root.join(relative_path);
        if source_path.symlink_metadata()?.file_type().is_symlink() {
            let target = read_link(&source_path)
                .with_context(|| format!("failed to read {}", source_path.display()))?;
            create_symlink(&target, &destination_path, source_path.is_dir())?;
        } else {
            copy(&source_path, &destination_path)
                .with_context(|| format!("failed to create {}", destination_path.display()))?;
        }
        Ok(copied + 1)
    })
}

pub fn link_ignored_paths(
    source_root: &Path,
    destination_root: &Path,
    repo: &Arc<ReadonlyRepo>,
    workspace_name: &WorkspaceName,
) -> Result<LinkSummary> {
    let tracked_paths = collect_tracked_paths(repo, workspace_name)?;
    let base_ignores = load_base_ignores(repo)?;
    let ignored_paths = collect_ignored_paths(source_root, &tracked_paths, &base_ignores)?;

    ignored_paths
        .iter()
        .try_fold(LinkSummary::default(), |mut summary, rel| {
            let destination_path = destination_root.join(rel);
            if destination_path.symlink_metadata().is_ok() {
                return Ok(summary);
            }
            if let Some(parent) = destination_path.parent() {
                create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            let source_path = source_root.join(rel);
            if wants_hardlink_tree(rel, &source_path) {
                let counts = hardlink_tree(&source_path, &destination_path)?;
                summary.hardlink_trees += 1;
                summary.hardlinked_files += counts.hardlinked;
                summary.copied_files += counts.copied;
            } else {
                create_symlink(&source_path, &destination_path, source_path.is_dir())?;
                summary.symlinked += 1;
            }
            Ok(summary)
        })
}

fn collect_tracked_paths(
    repo: &Arc<ReadonlyRepo>,
    workspace_name: &WorkspaceName,
) -> Result<TrackedPaths> {
    let Some(wc_commit_id) = repo.view().get_wc_commit_id(workspace_name) else {
        return Ok(TrackedPaths::default());
    };

    let commit = repo.store().get_commit(wc_commit_id)?;
    commit
        .tree()
        .entries()
        .try_fold(TrackedPaths::default(), |mut acc, (path, value)| {
            let value = value?;
            if value.is_present() && !value.is_tree() {
                let path = path.as_internal_file_string().to_string();
                add_parent_directories(&path, &mut acc.tracked_dirs);
                acc.tracked_paths.insert(path);
            }
            Ok(acc)
        })
}

fn add_parent_directories(path: &str, tracked_dirs: &mut HashSet<String>) {
    path.match_indices('/').for_each(|(i, _)| {
        tracked_dirs.insert(path[..i].to_string());
    });
}

fn load_base_ignores(repo: &Arc<ReadonlyRepo>) -> Result<Arc<GitIgnoreFile>> {
    let mut git_ignores = GitIgnoreFile::empty();

    if let Some(global_excludes) = default_global_git_ignore() {
        git_ignores = git_ignores.chain_with_file(RepoPath::root(), global_excludes)?;
    }

    if let Ok(git_backend) = get_git_backend(repo.store()) {
        git_ignores = git_ignores.chain_with_file(
            RepoPath::root(),
            git_backend.git_repo_path().join("info").join("exclude"),
        )?;
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
        false,
        tracked_paths,
        &base_ignores.clone(),
        &mut ignored_paths,
    )?;
    Ok(ignored_paths)
}

fn collect_worktree_included_paths(
    source_root: &Path,
    tracked_paths: &TrackedPaths,
    base_ignores: &Arc<GitIgnoreFile>,
    includes: &Arc<GitIgnoreFile>,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    walk_worktree_included_paths(
        source_root,
        source_root,
        "",
        false,
        false,
        tracked_paths,
        base_ignores,
        includes,
        &mut paths,
    )?;
    Ok(paths)
}

#[allow(clippy::too_many_arguments)]
fn walk_worktree_included_paths(
    source_root: &Path,
    current_dir: &Path,
    relative_dir: &str,
    parent_ignored: bool,
    parent_included: bool,
    tracked_paths: &TrackedPaths,
    inherited_ignores: &Arc<GitIgnoreFile>,
    includes: &Arc<GitIgnoreFile>,
    paths: &mut Vec<PathBuf>,
) -> Result<()> {
    let current_ignores =
        load_directory_gitignore(current_dir, relative_dir, &inherited_ignores.clone())?;
    let mut entries: Vec<DirEntry> = read_dir(current_dir)
        .with_context(|| format!("failed to read {}", current_dir.display()))?
        .collect::<io::Result<_>>()
        .with_context(|| format!("failed to read {}", current_dir.display()))?;
    entries.sort_by_key(DirEntry::file_name);

    for entry in entries {
        let file_name = entry.file_name();
        if should_skip_root_entry(source_root, current_dir, &file_name) {
            continue;
        }
        let file_name = file_name
            .to_str()
            .context("encountered a non-UTF-8 path while scanning .worktreeinclude files")?;
        let source_path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to read {}", source_path.display()))?;
        let is_dir = file_type.is_dir();
        let mut relative_path = String::with_capacity(relative_dir.len() + file_name.len() + 1);
        if !relative_dir.is_empty() {
            relative_path.push_str(relative_dir);
            relative_path.push('/');
        }
        relative_path.push_str(file_name);

        let repo_path = RepoPath::from_internal_string(&relative_path)?;
        let is_ignored = parent_ignored
            || if is_dir {
                current_ignores.matches_dir(repo_path)
            } else {
                current_ignores.matches_file(repo_path)
            };
        let is_included = parent_included
            || if is_dir {
                includes.matches_dir(repo_path)
            } else {
                includes.matches_file(repo_path)
            };

        if is_dir {
            walk_worktree_included_paths(
                source_root,
                &source_path,
                &relative_path,
                is_ignored,
                is_included,
                tracked_paths,
                &current_ignores,
                includes,
                paths,
            )?;
        } else if (file_type.is_file() || file_type.is_symlink())
            && is_ignored
            && is_included
            && !tracked_paths.contains(&relative_path)
        {
            paths.push(PathBuf::from(relative_path));
        }
    }

    Ok(())
}

fn walk_ignored_paths(
    source_root: &Path,
    current_dir: &Path,
    relative_dir: &str,
    parent_ignored: bool,
    tracked_paths: &TrackedPaths,
    inherited_ignores: &Arc<GitIgnoreFile>,
    ignored_paths: &mut Vec<PathBuf>,
) -> Result<()> {
    let current_ignores =
        load_directory_gitignore(current_dir, relative_dir, &inherited_ignores.clone())?;
    let mut entries: Vec<DirEntry> = read_dir(current_dir)
        .with_context(|| format!("failed to read {}", current_dir.display()))?
        .collect::<io::Result<_>>()
        .with_context(|| format!("failed to read {}", current_dir.display()))?;
    entries.sort_by_key(DirEntry::file_name);

    for entry in entries {
        let file_name = entry.file_name();
        if should_skip_root_entry(source_root, current_dir, &file_name) {
            continue;
        }

        let file_name = file_name
            .to_str()
            .context("encountered a non-UTF-8 path while scanning ignored files")?;
        let source_path = entry.path();
        let is_dir = entry
            .file_type()
            .with_context(|| format!("failed to read {}", source_path.display()))?
            .is_dir();
        let mut relative_path = String::with_capacity(relative_dir.len() + file_name.len() + 1);
        if !relative_dir.is_empty() {
            relative_path.push_str(relative_dir);
            relative_path.push('/');
        }
        relative_path.push_str(file_name);

        let repo_path = RepoPath::from_internal_string(&relative_path)?;
        // matches_dir/matches_file only match the exact path, so a path inside an
        // already-ignored directory is ignored by inheritance, not by matching.
        let is_ignored = parent_ignored
            || if is_dir {
                current_ignores.matches_dir(repo_path)
            } else {
                current_ignores.matches_file(repo_path)
            };

        if is_unconditional_symlink(file_name) {
            ignored_paths.push(PathBuf::from(&relative_path));
            continue;
        }

        if is_dir {
            if is_ignored && !tracked_paths.has_tracked_descendants(&relative_path) {
                ignored_paths.push(PathBuf::from(&relative_path));
                continue;
            }
            walk_ignored_paths(
                source_root,
                &source_path,
                &relative_path,
                is_ignored,
                tracked_paths,
                &current_ignores.clone(),
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
    inherited_ignores: &Arc<GitIgnoreFile>,
) -> Result<Arc<GitIgnoreFile>> {
    let prefix = RepoPath::from_internal_string(relative_dir)?;
    inherited_ignores
        .chain_with_file(prefix, current_dir.join(".gitignore"))
        .map_err(Into::into)
}

fn should_skip_root_entry(source_root: &Path, current_dir: &Path, file_name: &OsStr) -> bool {
    current_dir == source_root && (file_name == ".jj" || file_name == ".git")
}

const UNCONDITIONAL_SYMLINKS: &[&str] = &[
    ".claude",
    ".env",
    ".env.development",
    ".env.local",
    ".mcp.json",
    ".pi",
    "AGENTS.md",
    "CLAUDE.local.md",
    "CLAUDE.md",
    "scratch",
];

fn is_unconditional_symlink(file_name: &str) -> bool {
    UNCONDITIONAL_SYMLINKS.contains(&file_name)
}

/// Directories that get a real directory full of hard links instead of a
/// symlink. npm refuses to install into a symlinked `node_modules`, so the
/// workspace needs a directory it can treat as its own.
const HARDLINK_TREES: &[&str] = &["node_modules"];

fn wants_hardlink_tree(relative_path: &Path, source: &Path) -> bool {
    relative_path
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| HARDLINK_TREES.contains(&name))
        && source.symlink_metadata().is_ok_and(|m| m.is_dir())
}

#[derive(Debug, Default)]
struct TreeCounts {
    hardlinked: usize,
    copied: usize,
}

/// Mirrors `source` into `destination` as real directories whose files are hard
/// links back to the originals. Sharing inodes costs no disk space, and npm
/// replaces packages by unlinking rather than writing in place, so an install in
/// one workspace does not reach into another.
fn hardlink_tree(source: &Path, destination: &Path) -> Result<TreeCounts> {
    create_dir_all(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;

    let mut counts = TreeCounts::default();
    for entry in read_dir(source).with_context(|| format!("failed to read {}", source.display()))? {
        let entry = entry.with_context(|| format!("failed to read {}", source.display()))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to read {}", source_path.display()))?;

        if file_type.is_symlink() {
            // A hard link cannot stand in for a symlink, and the target is often
            // relative (`.bin` entries), so recreate the link itself.
            let target = read_link(&source_path)
                .with_context(|| format!("failed to read {}", source_path.display()))?;
            create_symlink(&target, &destination_path, source_path.is_dir())?;
        } else if file_type.is_dir() {
            let nested = hardlink_tree(&source_path, &destination_path)?;
            counts.hardlinked += nested.hardlinked;
            counts.copied += nested.copied;
        } else if hard_link(&source_path, &destination_path).is_ok() {
            counts.hardlinked += 1;
        } else {
            // Hard links fail across filesystems, so fall back to a real copy.
            copy(&source_path, &destination_path)
                .with_context(|| format!("failed to create {}", destination_path.display()))?;
            counts.copied += 1;
        }
    }

    Ok(counts)
}

#[cfg(unix)]
fn create_symlink(target: &Path, destination: &Path, _target_is_dir: bool) -> Result<()> {
    symlink(target, destination)
        .with_context(|| format!("failed to create {}", destination.display()))
}

#[cfg(windows)]
fn create_symlink(target: &Path, destination: &Path, target_is_dir: bool) -> Result<()> {
    if target_is_dir {
        symlink_dir(target, destination)
            .with_context(|| format!("failed to create {}", destination.display()))
    } else {
        symlink_file(target, destination)
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
    use std::fs::{create_dir_all, read_to_string, write};
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt as _;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn hardlink_tree_links_files_and_recreates_symlinks() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let source = temp_dir.path().join("node_modules");
        let destination = temp_dir.path().join("workspace").join("node_modules");
        create_dir_all(source.join("pkg"))?;
        write(source.join("pkg").join("index.js"), "contents")?;
        create_dir_all(source.join(".bin"))?;
        #[cfg(unix)]
        symlink("../pkg/index.js", source.join(".bin").join("tool"))?;

        let counts = hardlink_tree(&source, &destination)?;

        assert_eq!(counts.hardlinked, 1);
        assert_eq!(counts.copied, 0);
        assert_eq!(read_to_string(destination.join("pkg").join("index.js"))?, "contents");
        #[cfg(unix)]
        {
            // The destination file must be the very same inode, not a copy.
            assert_eq!(
                source.join("pkg").join("index.js").metadata()?.ino(),
                destination.join("pkg").join("index.js").metadata()?.ino()
            );
            let link = destination.join(".bin").join("tool");
            assert!(link.symlink_metadata()?.file_type().is_symlink());
            assert_eq!(read_link(&link)?, PathBuf::from("../pkg/index.js"));
        }
        Ok(())
    }

    #[test]
    fn wants_hardlink_tree_only_for_named_directories() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let root = temp_dir.path();
        create_dir_all(root.join("web").join("node_modules"))?;
        create_dir_all(root.join("target"))?;
        write(root.join("node_modules"), "not a directory")?;

        assert!(wants_hardlink_tree(
            Path::new("web/node_modules"),
            &root.join("web").join("node_modules")
        ));
        assert!(!wants_hardlink_tree(Path::new("target"), &root.join("target")));
        // A file that happens to carry the name is left to the symlink path.
        assert!(!wants_hardlink_tree(Path::new("node_modules"), &root.join("node_modules")));
        Ok(())
    }

    #[test]
    fn collect_ignored_paths_symlinks_whole_untracked_directory() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let root = temp_dir.path();
        write(root.join(".gitignore"), "node_modules/\n")?;
        create_dir_all(root.join("node_modules").join("pkg"))?;
        write(root.join("node_modules").join("pkg").join("file"), "contents")?;

        let ignored_paths =
            collect_ignored_paths(root, &TrackedPaths::default(), &GitIgnoreFile::empty())?;
        assert_eq!(ignored_paths, vec![PathBuf::from("node_modules")]);
        Ok(())
    }

    #[test]
    fn collect_ignored_paths_includes_unconditional_paths_even_when_not_ignored() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let root = temp_dir.path();
        // No .gitignore — nothing is ignored via gitignore rules
        write(root.join("CLAUDE.md"), "instructions")?;
        write(root.join(".mcp.json"), "{}")?;
        write(root.join("AGENTS.md"), "agents")?;
        write(root.join(".env"), "SECRET=x")?;
        create_dir_all(root.join("scratch"))?;
        write(root.join("scratch").join("notes.txt"), "tmp")?;
        create_dir_all(root.join(".pi"))?;
        create_dir_all(root.join("sub"))?;
        write(root.join("sub").join("CLAUDE.local.md"), "local")?;
        // Non-special file should NOT appear
        write(root.join("README.md"), "hello")?;

        let tracked = TrackedPaths {
            tracked_paths: HashSet::from(["CLAUDE.md".into()]),
            tracked_dirs: HashSet::new(),
        };
        let paths = collect_ignored_paths(root, &tracked, &GitIgnoreFile::empty())?;

        assert!(paths.contains(&PathBuf::from("CLAUDE.md")));
        assert!(paths.contains(&PathBuf::from(".mcp.json")));
        assert!(paths.contains(&PathBuf::from("AGENTS.md")));
        assert!(paths.contains(&PathBuf::from(".env")));
        assert!(paths.contains(&PathBuf::from("scratch")));
        assert!(paths.contains(&PathBuf::from(".pi")));
        assert!(paths.contains(&PathBuf::from("sub/CLAUDE.local.md")));
        assert!(!paths.contains(&PathBuf::from("README.md")));
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
            tracked_paths: HashSet::from([String::from("build/tracked.txt")]),
            tracked_dirs: HashSet::from([String::from("build")]),
        };
        let ignored_paths = collect_ignored_paths(root, &tracked_paths, &GitIgnoreFile::empty())?;
        assert_eq!(ignored_paths, vec![PathBuf::from("build/cache.bin")]);
        Ok(())
    }

    #[test]
    fn worktreeinclude_uses_gitignore_patterns_and_copies_only_ignored_untracked_files()
    -> Result<()> {
        let temp_dir = TempDir::new()?;
        let root = temp_dir.path();
        write(
            root.join(".gitignore"),
            "*.local\nconfig/*.json\ncache/\nlinks/*\n",
        )?;
        write(
            root.join(".worktreeinclude"),
            "# local setup\n*.local\n/config/*.json\n!config/skip.json\ncache/\nlinks/*\nREADME.md\n",
        )?;
        write(root.join("env.local"), "env")?;
        create_dir_all(root.join("nested").join("config"))?;
        write(root.join("nested").join("app.local"), "nested")?;
        write(root.join("nested").join("config").join("nested.json"), "nested")?;
        create_dir_all(root.join("config"))?;
        write(root.join("config").join("secrets.json"), "secret")?;
        write(root.join("config").join("skip.json"), "skip")?;
        create_dir_all(root.join("cache").join("deep"))?;
        write(root.join("cache").join("deep").join("state.bin"), "state")?;
        create_dir_all(root.join("links"))?;
        #[cfg(unix)]
        symlink("../env.local", root.join("links").join("env"))?;
        write(root.join("README.md"), "tracked or ordinary untracked")?;
        write(root.join("tracked.local"), "tracked")?;

        let tracked = TrackedPaths {
            tracked_paths: HashSet::from(["tracked.local".into()]),
            tracked_dirs: HashSet::new(),
        };
        let ignores = GitIgnoreFile::empty().chain_with_file(
            RepoPath::root(),
            root.join(".gitignore"),
        )?;
        let includes = GitIgnoreFile::empty().chain_with_file(
            RepoPath::root(),
            root.join(".worktreeinclude"),
        )?;

        let paths = collect_worktree_included_paths(root, &tracked, &ignores, &includes)?;
        let mut expected = vec![
            PathBuf::from("cache/deep/state.bin"),
            PathBuf::from("config/secrets.json"),
            PathBuf::from("env.local"),
            PathBuf::from("nested/app.local"),
        ];
        #[cfg(unix)]
        expected.push(PathBuf::from("links/env"));
        expected.sort();
        assert_eq!(paths, expected);
        Ok(())
    }

    #[test]
    fn copying_worktreeinclude_paths_preserves_symlinks_and_never_overwrites() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let source = temp_dir.path().join("source");
        let destination = temp_dir.path().join("destination");
        create_dir_all(source.join("config"))?;
        create_dir_all(destination.join("config"))?;
        write(source.join("config").join("new.env"), "source")?;
        write(source.join("config").join("existing.env"), "source")?;
        write(destination.join("config").join("existing.env"), "destination")?;
        #[cfg(unix)]
        symlink("new.env", source.join("config").join("linked.env"))?;

        let mut paths = vec![
            PathBuf::from("config/new.env"),
            PathBuf::from("config/existing.env"),
        ];
        #[cfg(unix)]
        paths.push(PathBuf::from("config/linked.env"));
        let copied = copy_worktree_included_paths(&source, &destination, &paths)?;

        #[cfg(unix)]
        assert_eq!(copied, 2);
        #[cfg(not(unix))]
        assert_eq!(copied, 1);
        assert_eq!(read_to_string(destination.join("config").join("new.env"))?, "source");
        assert_eq!(
            read_to_string(destination.join("config").join("existing.env"))?,
            "destination"
        );
        #[cfg(unix)]
        assert_eq!(
            read_link(destination.join("config").join("linked.env"))?,
            PathBuf::from("new.env")
        );
        Ok(())
    }
}
