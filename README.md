# jjws

A small CLI for managing [Jujutsu](https://github.com/jj-vcs/jj) workspaces with a few local conveniences.

When run inside [Herdr](https://herdr.dev), `jjws` creates tabs through Herdr's socket-backed CLI API.

- **Creates workspaces** outside the repo tree (under `<data-dir>/jjws` by default), with auto-generated animal names
- **Symlinks jj-ignored paths** (e.g. `build/`, `.env`) from the source workspace so tools just work
- **Hard-links `node_modules/`** into a real directory instead, because npm refuses to install into a symlinked one. Files share inodes, so this costs no disk space, and an `npm install` in one workspace leaves the others alone
- **Opens a Herdr tab** in the new workspace (opt-out with `--no-tab`), optionally running a command
- **Opens workspace tabs** with `tab`
- **Lists all workspaces or one selected workspace**, with optional path-only output
- **Cleans up** forgotten workspaces by removing their directories when safe

## Usage

```console
$ jjws --help
Manage jj workspaces with a few local conveniences

Usage: jjws [OPTIONS] [COMMAND]

Commands:
  new     Create a new workspace and open it in Herdr with auto-generated name
  tab     Open a workspace in a new Herdr tab
  list    List workspaces associated with the repo
  forget  Forget workspaces, then remove their directories when safe. Must be
          run from the repo-host workspace
  help    Print this message or the help of the given subcommand(s)

Options:
      --workspace-root <DIR>  Root directory where workspaces are created as
                              <DIR>/<parent>/<repo>/<name>. Defaults to
                              <data-dir>/jjws (e.g. ~/Library/Application
                              Support/jjws)
  -h, --help                  Print help
  -V, --version               Print version
```

To change the current shell's directory to a workspace:

```console
$ cd -- "$(jjws list --path-only <workspace>)"
```

The `list` command (also available as `ls`) accepts an optional workspace name, while
`--path-only` independently controls the output format:

```console
$ jjws ls
$ jjws ls <workspace>
$ jjws ls --path-only
$ jjws ls --path-only <workspace>
```

## Install

```console
$ cargo install --git https://github.com/0x6b/jjws
```

## Fish Completions

```console
$ ln -s (realpath completions/jjws.fish) ~/.config/fish/completions/jjws.fish
```

## License

MIT. See [LICENSE](LICENSE) for details.
