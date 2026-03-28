use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use jj_ws::{AddOptions, ForgetOptions, add, forget, list};

#[derive(Parser, Debug)]
#[command(about = "Manage jj workspaces with a few local conveniences", version)]
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
    /// Forget workspaces, then remove their directories when safe
    Forget {
        /// Workspace names to forget. Defaults to the current workspace.
        workspaces: Vec<String>,
    },
    /// List workspaces associated with the repo
    List,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let ws_root = cli.workspace_root.as_deref();

    match cli.command {
        Command::Add { name, no_tab } => add(AddOptions { name, no_tab }, ws_root),
        Command::Forget { workspaces } => forget(ForgetOptions { workspaces }, ws_root),
        Command::List => list(ws_root),
    }
}
