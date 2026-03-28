use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use jj_ws::{AddOptions, ForgetOptions, ListOptions, add, forget, list};

#[derive(Parser, Debug)]
#[command(about = "Manage jj workspaces with a few local conveniences", version)]
struct Cli {
    /// Parent directory for non-host workspaces
    #[arg(long, global = true, value_name = "DIR")]
    parent_dir: Option<PathBuf>,

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

    match cli.command {
        Command::Add { name, no_tab } => add(AddOptions {
            name,
            parent_dir: cli.parent_dir,
            no_tab,
        }),
        Command::Forget { workspaces } => forget(ForgetOptions {
            workspaces,
            parent_dir: cli.parent_dir,
        }),
        Command::List => list(ListOptions {
            parent_dir: cli.parent_dir,
        }),
    }
}
