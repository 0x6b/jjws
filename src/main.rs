use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use jjws::{AddOptions, add, forget, list};

#[derive(Parser, Debug)]
#[command(about, version)]
struct Cli {
    /// Root directory where workspaces are created as <DIR>/<name>.
    /// Defaults to <data-dir>/jjws (e.g. ~/Library/Application Support/jjws)
    #[arg(long, global = true, value_name = "DIR")]
    workspace_root: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create a new workspace and open it in Ghostty
    Add {
        /// Name of the new workspace
        name: String,

        /// Skip opening a Ghostty tab
        #[arg(long)]
        no_tab: bool,
    },
    /// List workspaces associated with the repo
    List,
    /// Forget workspaces, then remove their directories when safe.
    /// Must be run from the repo-host workspace.
    Forget {
        /// Workspace names to forget
        #[arg(required = true)]
        workspaces: Vec<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let ws_root = cli.workspace_root.as_deref();

    match cli.command {
        Command::Add { name, no_tab } => add(AddOptions { name, no_tab }, ws_root),
        Command::Forget { workspaces } => forget(workspaces, ws_root),
        Command::List => list(ws_root),
    }
}
