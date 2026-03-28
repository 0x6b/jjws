# jjws

A small CLI for managing [Jujutsu](https://github.com/jj-vcs/jj) workspaces with a few local conveniences.

- **Creates workspaces** outside the repo tree (under `<data-dir>/jjws` by default)
- **Symlinks jj-ignored paths** (e.g. `node_modules/`, `build/`) from the source workspace so tools just work
- **Opens a Ghostty tab** in the new workspace (macOS, opt-out with `--no-tab`)
- **Cleans up** forgotten workspaces by removing their directories when safe

## Usage

```console
$ jjws --help
Manage jj workspaces with a few local conveniences

Usage: jjws [OPTIONS] <COMMAND>

Commands:
  add     Create a new workspace and open it in Ghostty
  list    List workspaces associated with the repo
  forget  Forget workspaces, then remove their directories when safe. Must be run from the repo-host workspace
  help    Print this message or the help of the given subcommand(s)

Options:
      --workspace-root <DIR>  Root directory where workspaces are created as <DIR>/<name>. Defaults to
                              <data-dir>/jjws (e.g. ~/Library/Application Support/jjws)
  -h, --help                  Print help
  -V, --version               Print version
```

## Install

```console
$ cargo install --git https://github.com/0x6b/jjws
```

## License

MIT. See [LICENSE](LICENSE) for details.
