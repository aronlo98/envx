
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'envx' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'envx'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'envx' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('run', 'run', [CompletionResultType]::ParameterValue, 'Evaluate a .envx file and run a command with those variables injected')
            [CompletionResult]::new('export', 'export', [CompletionResultType]::ParameterValue, 'Print all variables as `export KEY="VALUE"` statements')
            [CompletionResult]::new('eval', 'eval', [CompletionResultType]::ParameterValue, 'Evaluate a single expression using OS environment for variable references')
            [CompletionResult]::new('print', 'print', [CompletionResultType]::ParameterValue, 'Print all resolved variables as KEY=VALUE pairs')
            [CompletionResult]::new('completions', 'completions', [CompletionResultType]::ParameterValue, 'Print the shell completion script for the given shell')
            [CompletionResult]::new('fmt', 'fmt', [CompletionResultType]::ParameterValue, 'Format a .envx file — aligns `=` across all assignments')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'envx;run' {
            [CompletionResult]::new('-t', '-t', [CompletionResultType]::ParameterName, 'Only inject variables belonging to these section tags (repeatable: -t db -t app)')
            [CompletionResult]::new('--tag', '--tag', [CompletionResultType]::ParameterName, 'Only inject variables belonging to these section tags (repeatable: -t db -t app)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'envx;export' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            break
        }
        'envx;eval' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            break
        }
        'envx;print' {
            [CompletionResult]::new('-t', '-t', [CompletionResultType]::ParameterName, 'Only show variables belonging to these section tags (repeatable: -t db -t app)')
            [CompletionResult]::new('--tag', '--tag', [CompletionResultType]::ParameterName, 'Only show variables belonging to these section tags (repeatable: -t db -t app)')
            [CompletionResult]::new('-T', '-T ', [CompletionResultType]::ParameterName, 'Show a TAG column and sort rows by tag name ascending')
            [CompletionResult]::new('--tags', '--tags', [CompletionResultType]::ParameterName, 'Show a TAG column and sort rows by tag name ascending')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            break
        }
        'envx;completions' {
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            break
        }
        'envx;fmt' {
            [CompletionResult]::new('--check', '--check', [CompletionResultType]::ParameterName, 'Exit with a non-zero code if the file is not already formatted (useful in CI)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            break
        }
        'envx;help' {
            [CompletionResult]::new('run', 'run', [CompletionResultType]::ParameterValue, 'Evaluate a .envx file and run a command with those variables injected')
            [CompletionResult]::new('export', 'export', [CompletionResultType]::ParameterValue, 'Print all variables as `export KEY="VALUE"` statements')
            [CompletionResult]::new('eval', 'eval', [CompletionResultType]::ParameterValue, 'Evaluate a single expression using OS environment for variable references')
            [CompletionResult]::new('print', 'print', [CompletionResultType]::ParameterValue, 'Print all resolved variables as KEY=VALUE pairs')
            [CompletionResult]::new('completions', 'completions', [CompletionResultType]::ParameterValue, 'Print the shell completion script for the given shell')
            [CompletionResult]::new('fmt', 'fmt', [CompletionResultType]::ParameterValue, 'Format a .envx file — aligns `=` across all assignments')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'envx;help;run' {
            break
        }
        'envx;help;export' {
            break
        }
        'envx;help;eval' {
            break
        }
        'envx;help;print' {
            break
        }
        'envx;help;completions' {
            break
        }
        'envx;help;fmt' {
            break
        }
        'envx;help;help' {
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
