# Fish completions for jjws

# Helper: extract workspace names from `jjws list` output
function __jjws_workspaces
    jjws list 2>/dev/null | string replace -r '^[* ] ([^\t]+)\t.*' '$1'
end

# Disable file completions by default
complete -c jjws -f

# Global options
complete -c jjws -l workspace-root -r -F -d 'Root directory for workspaces'
complete -c jjws -l help -s h -d 'Print help'
complete -c jjws -l version -s V -d 'Print version'

# Subcommands (only when no subcommand given yet)
complete -c jjws -n __fish_use_subcommand -a add -d 'Create a new workspace and open it in Ghostty'
complete -c jjws -n __fish_use_subcommand -a cd -d 'Open a Ghostty tab at a workspace'
complete -c jjws -n __fish_use_subcommand -a list -d 'List workspaces associated with the repo'
complete -c jjws -n __fish_use_subcommand -a forget -d 'Forget workspaces and remove directories'
complete -c jjws -n __fish_use_subcommand -a help -d 'Print help for a subcommand'

# add: positional name argument (no dynamic completions), then options
complete -c jjws -n '__fish_seen_subcommand_from add' -l no-tab -d 'Skip opening a Ghostty tab'

# cd: complete workspace names
complete -c jjws -n '__fish_seen_subcommand_from cd' -a '(__jjws_workspaces)' -d 'Workspace name'

# forget: complete workspace names
complete -c jjws -n '__fish_seen_subcommand_from forget' -a '(__jjws_workspaces)' -d 'Workspace name'

# list: no additional arguments

# help: complete subcommand names
complete -c jjws -n '__fish_seen_subcommand_from help' -a 'add cd list forget' -d 'Subcommand'
