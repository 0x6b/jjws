use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use jj_ws::{RunOptions, run};

#[derive(Parser, Debug)]
#[command(about = "Create and open a jj workspace", version)]
struct Args {
    /// Name of the new workspace
    name: String,

    /// Parent directory for the new workspace
    #[arg(long, value_name = "DIR")]
    parent_dir: Option<PathBuf>,

    /// Skip opening a Ghostty tab
    #[arg(long)]
    no_tab: bool,
}

fn main() -> Result<()> {
    let Args { name, parent_dir, no_tab } = Args::parse();
    run(RunOptions { name, parent_dir, no_tab })
}
