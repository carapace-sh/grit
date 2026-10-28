---
title: grit completions
summary: Print the shell completion script that completes branches, tags, remotes, paths, and flags.
group: Maintenance
order: 3
---

## Synopsis

```text
grit completions <shell>
```

## Description

Prints the shell integration script for `grit`. The script registers a completion function that asks the running `grit` binary for candidates, so completion follows the CLI you actually have installed: subcommands and flags come from the same command tree the parser uses, and values come from the repository you are standing in.

What completion offers:

| Argument | Candidates |
| -------- | ---------- |
| Revisions (`log`, `show`, `diff`, `merge`, `reset`, …) | Local and remote branches and tags, described with the subject of the commit they point at; stashes, described with the message they were created with; `HEAD` and the other pseudo refs; plus `A..B` / `A...B` ranges and `^` negation |
| Paths (`add`, `rm`, `restore`, `status`, …) | Files and directories in the worktree below the current directory, ignoring paths excluded by `.gitignore`, plus tracked paths whose file was deleted |
| Remotes (`fetch`, `pull`, `push`, `remote`, …) | Configured remotes, described with their URL |
| Stashes, hooks, worktrees, notes refs | The entries of the current repository |
| Config keys and values (`config`, `-c`) | Keys already present in the effective configuration, common keys, and the values set for a key |
| Shells | The shells this command accepts |

Candidates are styled like Git completions elsewhere: branches blue, tags yellow, pseudo refs bold, stashes green, notes cyan, with each kind in its own group in the menu.

The global [`-C`](../global-options/), [`--git-dir`](../global-options/), [`--work-tree`](../global-options/), and [`-c`](../global-options/) flags apply to completion too, so `grit -C ../other branch <TAB>` lists the branches of `../other`.

Supported shells: `bash`, `bash-ble`, `cmd-clink`, `elvish`, `fish`, `nushell`, `oil`, `powershell`, `tcsh`, `xonsh`, and `zsh`.

Because the script calls the binary, it does not need regenerating after [`grit update`](../update/): new commands and flags appear automatically. Release archives also ship pre-generated scripts under `completions/` when you install from a tarball.

The script is the visible half of a hidden entry point, `grit _carapace`, which the shells call with the words typed so far. It prints shell-specific completion text, not human-readable output.

## Options

| Argument | Meaning |
| -------- | ------- |
| `<shell>` | One of `bash`, `bash-ble`, `cmd-clink`, `elvish`, `fish`, `nushell`, `oil`, `powershell`, `tcsh`, `xonsh`, or `zsh`. |

An unknown shell is a usage error that names the accepted shells.

`grit completions` rejects the global [`--json`](../global-options/) and [`--markdown`](../global-options/) flags; stdout is always the raw completion script.

## Examples

Load bash completion in the current session:

```console
$ eval "$(grit completions bash)"
```

Install zsh completion for your user (typical layout):

```bash
grit completions zsh > ~/.local/share/zsh/site-functions/_grit
```

Install fish completion:

```bash
mkdir -p ~/.config/fish/completions
grit completions fish > ~/.config/fish/completions/grit.fish
```

Load nushell completion on startup:

```bash
grit completions nushell | save -f ~/.config/nushell/grit.nu
```

Setting an unknown shell reports the accepted names:

```console
$ grit completions not-a-shell
error: invalid value 'not-a-shell' for '<SHELL>': unknown shell 'not-a-shell' (expected one of: bash, bash-ble, cmd-clink, elvish, fish, nushell, oil, powershell, tcsh, xonsh, zsh)

For more information, try '--help'.
```

## JSON output

There is no JSON form. `--json` and `--markdown` exit with an error; stdout carries the completion script only.

## Markdown output

None. `--markdown` is rejected for the same reason as `--json`.

```console
$ grit completions bash --markdown
**Error:** `grit completions` does not support --json or --markdown
```

## Exit status

| Code | Meaning |
| ---: | --- |
| `0` | Success ([`success`](../exit-codes/)). |
| `1` | Command failed ([`failure`](../exit-codes/)). |
| `2` | Invalid flags or arguments, including an unknown shell ([`usage`](../exit-codes/)). |

See [Exit codes](../exit-codes/) for shared rules and structured `--json` `kind` values.

## See also

[Install grit](../install/), [grit update](../update/), [Global options](../global-options/)
