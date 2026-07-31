use std::{
    env::{var, var_os},
    path::Path,
    process::Command,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::from_slice;

#[derive(Deserialize)]
struct HerdrResponse {
    result: TabCreated,
}

#[derive(Deserialize)]
struct TabCreated {
    tab: Tab,
    root_pane: RootPane,
}

#[derive(Deserialize)]
struct Tab {
    tab_id: String,
}

#[derive(Deserialize)]
struct RootPane {
    pane_id: String,
}

pub fn open_tab(
    workspace_path: &Path,
    repo_root: &Path,
    command: Option<&str>,
) -> Result<Option<String>> {
    if !is_available() {
        return Ok(None);
    }

    let label = tab_label(workspace_path, repo_root);
    let workspace_path = workspace_path.to_str().context("workspace path is not valid UTF-8")?;

    let mut create = Command::new(herdr_bin());
    create.args(["tab", "create"]);
    if let Ok(workspace_id) = var("HERDR_WORKSPACE_ID")
        && !workspace_id.is_empty()
    {
        create.args(["--workspace", &workspace_id]);
    }
    create.args(["--cwd", workspace_path, "--label", &label, "--focus"]);

    let output = create.output().context("failed to start `herdr tab create`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`herdr tab create` failed: {}", stderr.trim());
    }

    let response: HerdrResponse =
        from_slice(&output.stdout).context("`herdr tab create` returned invalid JSON")?;

    if let Some(command) = command {
        let run = Command::new(herdr_bin())
            .args(["pane", "run", &response.result.root_pane.pane_id, command])
            .output()
            .context("failed to start `herdr pane run`")?;
        if !run.status.success() {
            let stderr = String::from_utf8_lossy(&run.stderr);
            bail!("`herdr pane run` failed: {}", stderr.trim());
        }
    }

    Ok(Some(response.result.tab.tab_id))
}

/// `name (repo)`, where `repo` is the directory name of the repo-host workspace.
/// The repo-host tab itself keeps the bare name.
fn tab_label(workspace_path: &Path, repo_root: &Path) -> String {
    let name = last_segment(workspace_path);
    match last_segment(repo_root) {
        repo if repo.is_empty() || workspace_path == repo_root => name.into(),
        repo => format!("{name} ({repo})"),
    }
}

fn last_segment(path: &Path) -> &str {
    path.file_name().and_then(|name| name.to_str()).unwrap_or_default()
}

fn is_available() -> bool {
    var_os("HERDR_ENV").is_some() && var_os("HERDR_SOCKET_PATH").is_some()
}

fn herdr_bin() -> String {
    var("HERDR_BIN_PATH")
        .ok()
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| "herdr".into())
}

#[cfg(test)]
mod tests {
    #[cfg(test)]
    use serde_json::from_str;

    use super::*;

    #[test]
    fn parses_tab_create_response() {
        let response: HerdrResponse = from_str(
            r#"{
                "id": "cli:tab:create",
                "result": {
                    "type": "tab_created",
                    "tab": { "tab_id": "w1:t2" },
                    "root_pane": { "pane_id": "w1:p3" }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(response.result.tab.tab_id, "w1:t2");
        assert_eq!(response.result.root_pane.pane_id, "w1:p3");
    }

    #[test]
    fn label_appends_repo_directory_name() {
        let label = tab_label(
            Path::new("/data/jjws/my-repo/bold-otter"),
            Path::new("/home/me/src/my-repo"),
        );
        assert_eq!(label, "bold-otter (my-repo)");
    }

    #[test]
    fn repo_host_label_has_no_suffix() {
        let repo_root = Path::new("/home/me/src/my-repo");
        assert_eq!(tab_label(repo_root, repo_root), "my-repo");
    }
}
