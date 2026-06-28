# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_envx_global_optspecs
	string join \n h/help V/version
end

function __fish_envx_needs_command
	# Figure out if the current invocation already has a command.
	set -l cmd (commandline -opc)
	set -e cmd[1]
	argparse -s (__fish_envx_global_optspecs) -- $cmd 2>/dev/null
	or return
	if set -q argv[1]
		# Also print the command, so this can be used to figure out what it is.
		echo $argv[1]
		return 1
	end
	return 0
end

function __fish_envx_using_subcommand
	set -l cmd (__fish_envx_needs_command)
	test -z "$cmd"
	and return 1
	contains -- $cmd[1] $argv
end

complete -c envx -n "__fish_envx_needs_command" -s h -l help -d 'Print help'
complete -c envx -n "__fish_envx_needs_command" -s V -l version -d 'Print version'
complete -c envx -n "__fish_envx_needs_command" -f -a "run" -d 'Evaluate a .envx file and run a command with those variables injected'
complete -c envx -n "__fish_envx_needs_command" -f -a "export" -d 'Print all variables as `export KEY="VALUE"` statements'
complete -c envx -n "__fish_envx_needs_command" -f -a "eval" -d 'Evaluate a single expression using OS environment for variable references'
complete -c envx -n "__fish_envx_needs_command" -f -a "print" -d 'Print all resolved variables as KEY=VALUE pairs'
complete -c envx -n "__fish_envx_needs_command" -f -a "completions" -d 'Print the shell completion script for the given shell'
complete -c envx -n "__fish_envx_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c envx -n "__fish_envx_using_subcommand run" -s h -l help -d 'Print help'
complete -c envx -n "__fish_envx_using_subcommand export" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c envx -n "__fish_envx_using_subcommand eval" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c envx -n "__fish_envx_using_subcommand print" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c envx -n "__fish_envx_using_subcommand completions" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c envx -n "__fish_envx_using_subcommand help; and not __fish_seen_subcommand_from run export eval print completions help" -f -a "run" -d 'Evaluate a .envx file and run a command with those variables injected'
complete -c envx -n "__fish_envx_using_subcommand help; and not __fish_seen_subcommand_from run export eval print completions help" -f -a "export" -d 'Print all variables as `export KEY="VALUE"` statements'
complete -c envx -n "__fish_envx_using_subcommand help; and not __fish_seen_subcommand_from run export eval print completions help" -f -a "eval" -d 'Evaluate a single expression using OS environment for variable references'
complete -c envx -n "__fish_envx_using_subcommand help; and not __fish_seen_subcommand_from run export eval print completions help" -f -a "print" -d 'Print all resolved variables as KEY=VALUE pairs'
complete -c envx -n "__fish_envx_using_subcommand help; and not __fish_seen_subcommand_from run export eval print completions help" -f -a "completions" -d 'Print the shell completion script for the given shell'
complete -c envx -n "__fish_envx_using_subcommand help; and not __fish_seen_subcommand_from run export eval print completions help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
