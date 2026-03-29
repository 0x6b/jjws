use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use jjws::{NewOptions, cd, forget, list, new_workspace};

#[derive(Parser, Debug)]
#[command(about, version)]
struct Cli {
    /// Root directory where workspaces are created as <DIR>/<repo>/<name>.
    /// Defaults to <data-dir>/jjws (e.g. ~/Library/Application Support/jjws)
    #[arg(long, global = true, value_name = "DIR")]
    workspace_root: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create a new workspace and open it in Ghostty with auto-generated name
    New {
        /// Name of the new workspace (auto-generated if omitted)
        #[arg(long)]
        name: Option<String>,

        /// Command to run in the new tab after cd-ing into the workspace
        command: Option<String>,

        /// Skip opening a Ghostty tab
        #[arg(long)]
        no_tab: bool,
    },
    /// Open a Ghostty tab at a workspace (defaults to repo-host)
    Cd {
        /// Workspace name (defaults to repo-host workspace)
        name: Option<String>,
    },
    /// List workspaces associated with the repo
    List {
        /// Machine-readable output (no commit details)
        #[arg(long)]
        porcelain: bool,
    },
    /// Forget workspaces, then remove their directories when safe.
    /// Must be run from the repo-host workspace.
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

    match cli.command.unwrap_or(Command::List { porcelain: false }) {
        Command::New { name, command, no_tab } => {
            new_workspace(NewOptions { name, command, no_tab }, ws_root).await
        }
        Command::Forget { workspaces } => forget(workspaces, ws_root).await,
        Command::List { porcelain } => list(porcelain, ws_root).await,
        Command::Cd { name } => cd(name.as_deref(), ws_root).await,
    }
}
