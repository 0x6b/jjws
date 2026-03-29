# Fish completions for jjws

# Helper: extract workspace names with descriptions from `jjws list --porcelain`
# Porcelain format: "{marker} {name}\t{created}\t{modified}\t{path}{suffix}"
function __jjws_workspaces
    jjws list --porcelain 2>/dev/null | while read -l line
        set -l marker (string sub -l 1 -- $line)
        set -l rest (string sub -s 3 -- $line)
        set -l fields (string split \t -- $rest)
        set -l name $fields[1]
        set -l created $fields[2]
        set -l modified $fields[3]
        set -l path_with_suffix $fields[4]

        set -l desc
        if string match -q '* \[repo-host\]' -- $path_with_suffix
            set desc "[repo-host]"
        else if string match -q '* \[out-of-control\]' -- $path_with_suffix
            set desc "[out-of-control]"
        end

        if test -n "$modified"
            if test -n "$desc"
                set desc "$desc modified $modified"
            else
                set desc "modified $modified"
            end
        end

        if test "$marker" = '*'
            if test -n "$desc"
                set desc "current, $desc"
            else
                set desc "current"
            end
        end

        if test -n "$desc"
            printf '%s\t%s\n' $name $desc
        else
            echo $name
        end
    end
end

# Disable file completions by default
complete -c jjws -f

# Global options
complete -c jjws -l workspace-root -r -F -d 'Root directory for workspaces'
complete -c jjws -l help -s h -d 'Print help'
complete -c jjws -l version -s V -d 'Print version'

# Subcommands (only when no subcommand given yet)
complete -c jjws -n __fish_use_subcommand -a new -d 'Create a new workspace and open it in Ghostty'
complete -c jjws -n __fish_use_subcommand -a cd -d 'Open a Ghostty tab at a workspace'
complete -c jjws -n __fish_use_subcommand -a list -d 'List workspaces associated with the repo'
complete -c jjws -n __fish_use_subcommand -a forget -d 'Forget workspaces and remove directories'
complete -c jjws -n __fish_use_subcommand -a help -d 'Print help for a subcommand'

# new: optional --name flag and --no-tab
complete -c jjws -n '__fish_seen_subcommand_from new' -l name -r -d 'Workspace name (auto-generated if omitted)'
complete -c jjws -n '__fish_seen_subcommand_from new' -l no-tab -d 'Skip opening a Ghostty tab'

# cd: complete workspace names (exclude "default" — no argument means default)
complete -c jjws -n '__fish_seen_subcommand_from cd' -a '(__jjws_workspaces | string match -rv "^default(\t|\$)")'

# forget: complete workspace names (exclude "default" — forgetting it makes no sense)
complete -c jjws -n '__fish_seen_subcommand_from forget' -a '(__jjws_workspaces | string match -rv "^default(\t|\$)")'

# list: --porcelain flag
complete -c jjws -n '__fish_seen_subcommand_from list' -l porcelain -d 'Machine-readable output (no commit details)'

# help: complete subcommand names
complete -c jjws -n '__fish_seen_subcommand_from help' -a 'new cd list forget' -d 'Subcommand'
