use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use jjws::{ListOptions, NewOptions, forget, list, new_workspace, tab};

#[derive(Parser, Debug)]
#[command(about, version)]
struct Cli {
    /// Root directory where workspaces are created as <DIR>/<parent>/<repo>/<name>.
    /// Defaults to <data-dir>/jjws (e.g. ~/Library/Application Support/jjws)
    #[arg(long, global = true, value_name = "DIR")]
    workspace_root: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create a new workspace and open it in Herdr with auto-generated name
    New {
        /// Name of the new workspace (auto-generated if omitted)
        #[arg(long)]
        name: Option<String>,

        /// Command to run in the new Herdr tab
        command: Option<String>,

        /// Skip opening a Herdr tab
        #[arg(long)]
        no_tab: bool,
    },
    /// Open a workspace in a new Herdr tab
    Tab {
        /// Workspace name
        workspace: String,
    },
    /// List workspaces associated with the repo
    #[command(alias = "ls")]
    List {
        /// Machine-readable output (no commit details)
        #[arg(long, conflicts_with = "path_only")]
        porcelain: bool,

        /// Print only the workspace path
        #[arg(long)]
        path_only: bool,

        /// Workspace to list (lists all workspaces if omitted)
        workspace: Option<String>,
    },
    /// Forget workspaces, then remove their directories when safe.
    /// Must be run from the repo-host workspace.
    #[command(alias = "rm")]
    Forget {
        /// Workspace names to forget
        #[arg(required = true)]
        workspaces: Vec<String>,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let ws_root = cli.workspace_root.as_deref();

    match cli.command.unwrap_or(Command::List {
        porcelain: false,
        path_only: false,
        workspace: None,
    }) {
        Command::New { name, command, no_tab } => {
            new_workspace(NewOptions { name, command, no_tab }, ws_root).await
        }
        Command::Forget { workspaces } => forget(workspaces, ws_root).await,
        Command::List { porcelain, path_only, workspace } => {
            list(ListOptions { porcelain, path_only, workspace }, ws_root).await
        }
        Command::Tab { workspace } => tab(workspace, ws_root).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_list(args: &[&str]) -> (bool, Option<String>) {
        let cli = Cli::try_parse_from(args).unwrap();
        match cli.command.unwrap() {
            Command::List { path_only, workspace, .. } => (path_only, workspace),
            command => panic!("expected list command, got {command:?}"),
        }
    }

    #[test]
    fn list_accepts_all_path_and_workspace_combinations() {
        assert_eq!(parse_list(&["jjws", "ls"]), (false, None));
        assert_eq!(parse_list(&["jjws", "ls", "otter"]), (false, Some("otter".into())));
        assert_eq!(parse_list(&["jjws", "ls", "--path-only"]), (true, None));
        assert_eq!(
            parse_list(&["jjws", "ls", "--path-only", "otter"]),
            (true, Some("otter".into()))
        );
    }
}
