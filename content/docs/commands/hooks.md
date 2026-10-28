---
title: grit hooks
summary: List hook scripts and run them by name.
group: Plumbing
order: 1
---

## Synopsis

```text
grit hooks
grit hooks list
grit hooks run <NAME> [-- ARGS...]
```

## Description

Discovers traditional hook scripts in the resolved hooks directory (`core.hooksPath` when set, otherwise `.git/hooks`). A relative `core.hooksPath` is relative to the directory hooks run in (the top of the work tree, or the git directory in a bare repository), so the same hooks are found from any subdirectory. Configured `hook.<name>.*` aliases are resolved when a hook runs, not in the list.

`grit hooks run` executes every hook registered for the event (config hooks first, then the script in the hooks directory) through the repository [`CommandRunner`](../../library/maintenance.md). The process exits with the hook's status when a hook ran and returned non-zero.

What a hook prints depends on the output mode. In human mode the hook writes straight to the terminal as it runs, with its stdout sent to stderr (every hook except `pre-push`, as in Git), so stdout stays free for grit's own output. With `--json` or `--markdown`, grit captures both streams and puts them in the result instead (`stdout` and `stderr` below), and nothing reaches the terminal.

Hooks run with the invoking environment (including `PATH`), from the top of the work tree. A script the OS will not execute directly runs through `sh`: `/bin/sh` on Unix, while on Windows a script's `#!` line names its interpreter (`#!/bin/sh` → `sh`, `#!/usr/bin/env bash` → `bash`), looked up first in the Git for Windows install that owns the `git` on `PATH` (`usr\bin`, `bin`), then on `PATH`, skipping the WSL launchers in `System32` and `WindowsApps`. When the interpreter comes from a Git for Windows install, that install's `mingw64\bin` (or `clangarm64\bin`, `ucrt64\bin`, `mingw32\bin`) and `usr\bin` go in front of the hook's `PATH`, as Git does, so `grep`, `sed`, `env` and the other MSYS tools are found. Hook paths, in `list` output and on the hook's command line, and the repository paths hooks receive (`GIT_DIR`, `GIT_COMMON_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`) never carry the Windows `\\?\` prefix, so `git` run inside a hook reads the same index grit wrote. Commit hooks (`pre-commit`, `pre-merge-commit`, `prepare-commit-msg`, `commit-msg`, `post-commit`) get `GIT_INDEX_FILE` for the index being committed, and `GIT_EDITOR=:` because grit never opens an editor.

When a hook cannot be started at all (for example, its interpreter is missing), the command fails with an error naming the hook, the program it tried to start and the OS error, instead of reporting an exit status:

```console
$ grit hooks run pre-commit
error: failed to run hook pre-commit (.git/hooks/pre-commit): No such file or directory (os error 2)
$ echo $?
1
```

In `--json` mode the same message is the `error` field. [`grit commit`](commit.md), [`grit merge`](merge.md) and [`grit rebase`](rebase.md) report it the same way for the hooks that run before they change anything (the table below); for `post-commit` and the other hooks that run after an operation completes, it is a warning and the operation still succeeds.

These hooks run before an operation records anything, with Git's arguments and in Git's order. A non-zero exit (or a hook that cannot be started) stops the operation:

| Hook | Run by | Arguments | Skipped by `--no-verify` |
| --- | --- | --- | --- |
| `pre-commit` | [`grit commit`](commit.md), `grit merge --continue` | none | yes |
| `pre-merge-commit` | [`grit merge`](merge.md) and [`grit pull`](pull.md) before a clean merge records its commit | none | yes |
| `prepare-commit-msg` | `grit commit`, then `grit merge` | the message file (`.git/COMMIT_EDITMSG`, or `.git/MERGE_MSG` for a merge) and its source: `message` (`-m`, `-F`), `commit <oid>` (`--amend` or `-C` reusing a message), `merge` (a merge message, always for `grit merge`) or `squash` (`SQUASH_MSG`) | no, as in Git |
| `commit-msg` | `grit commit`, then `grit merge` | the message file | yes |
| `pre-rebase` | [`grit rebase`](rebase.md) and `grit pull --rebase` when there are commits to replay | the upstream as you named it (the configured upstream's full ref, such as `refs/remotes/origin/main`, when omitted; the commit id for a pull) | yes |
| `pre-push` | [`grit push`](push.md) before sending anything | remote name and URL; stdin lists the refs | yes |

`prepare-commit-msg` and `commit-msg` may rewrite the message file, and the commit records what they leave (an empty message aborts). A merge's message hooks see `MERGE_HEAD`, as in Git. A refused merge leaves the merged files with `MERGE_HEAD` recorded and the branch unchanged; finish it with `grit merge --continue` (or [`grit commit`](commit.md)) or cancel it with `grit merge --abort`. Changes a `pre-merge-commit` hook stages (lint-staged style) go into the merge commit. A refused commit or rebase changes nothing. JSON commit output lists hook basenames that actually ran in `hooks_run`. Replayed commits ([`grit rebase`](rebase.md), [`grit pick`](pick.md), [`grit revert`](revert.md)) do not run the commit hooks.

Other commands run the hooks Git runs after them, with Git's arguments:

| Hook | Run by | Arguments and stdin |
| --- | --- | --- |
| `post-checkout` | [`grit switch`](switch.md), [`grit worktree add`](worktree.md) (after checkout), [`grit rebase`](rebase.md) and [`grit history`](history.md) rewrites that change `HEAD`'s tree | `<old HEAD> <new HEAD> 1` (all-zero old id from an unborn branch or a new linked worktree); for a rebase, `<old tip> <onto> 1` like Git |
| `post-merge` | [`grit merge`](merge.md) and [`grit pull`](pull.md) after a fast-forward or merge commit (`0`) or a squash (`1`) | `0` or `1` |
| `post-rewrite` | `grit commit --amend` (`amend`), `grit rebase` and `grit history` rewrites (`rebase`) | `amend` or `rebase`; stdin has one `<old> <new>` line per rewritten commit |

These hooks run after the operation is complete, so their exit status never changes the result, and a hook that cannot be started is a `hook_not_run` warning. `--dry-run` and `--check` run no hooks. [`grit switch`](switch.md) and [`grit commit`](commit.md) list the hooks that ran in their JSON `hooks_run` field.

### `reference-transaction`

Grit runs the [`reference-transaction`](https://git-scm.com/docs/githooks#_reference_transaction) hook for every ref-store transaction that changes direct object ids (create, update, delete, or reflog-only oid lines on `HEAD`). Pure symbolic updates (for example switching between two existing branches without moving a tip) do not run the hook, matching Git 2.43.

| Phase | When | Exit status |
| --- | --- | --- |
| `prepared` | After locks are taken, before any ref or reflog is written | Non-zero aborts the whole batch; grit then runs `aborted` (exit ignored) |
| `committed` | After the transaction commits successfully | Ignored |
| `aborted` | After `prepared` refused or the batch was aborted without writing | Ignored |

Stdin is one line per ref update, sorted by ref name: `<old-hex> <new-hex> <ref>\n`. Old and new ids use the repository hash width; absent refs or deletions use the null object id. Attached commits on the files backend emit the same `HEAD` and branch lines Git does (same old and new commit ids, `HEAD` first lexicographically).

This hook is **not** skipped by `--no-verify` on commands that honor `--no-verify` for other hooks. Configured `hook.reference-transaction.*` entries and `core.hooksPath` apply the same way as for other hooks.

## Options

### `grit hooks list`

| `-n`, `--limit <N>` | List at most `N` hook scripts (`truncated` in JSON when more exist). |

Other [global options](../global-options.md) apply.

### `grit hooks run`

| Argument | Description |
| --- | --- |
| `NAME` | Hook basename (e.g. `pre-commit`). |
| `ARGS` | Optional arguments after `--`, forwarded to the hook. |

## Examples

List installed scripts:

```console
$ grit hooks list
pre-commit  /path/to/repo/.git/hooks/pre-commit  executable=yes  hooks_dir
```

Run a hook:

```console
$ grit hooks run pre-commit
$ echo $?
0
```

## JSON output

Schema: [v1](/schemas/hooks/v1.json). Subcommands: [hooks.list](/schemas/hooks.list/v1.json), [hooks.run](/schemas/hooks.run/v1.json).

| Field | Type | Meaning |
| ----- | ---- | ------- |
| `action` | string | `list` or `run`. |
| `hooks` | array | For `list`: each entry has `name`, `path`, `executable`, and `source` (`hooks_dir` or `core_hooks_path`). |
| `truncated` | boolean | For `list`: `true` when more hook scripts existed than returned (`-n` / `--limit`). |
| `name` | string | For `run`: hook basename. |
| `exit_code` | number | For `run`: subprocess exit code (`0` when the hook was missing). |
| `executed` | boolean | For `run`: whether any hook script ran. |
| `stdout` | string | For `run`: what the hooks wrote to stdout (lossy UTF-8; empty when no hook ran). |
| `stderr` | string | For `run`: what the hooks wrote to stderr (lossy UTF-8; empty when no hook ran). |

Example list:

```json
{
  "action": "list",
  "hooks": [
    {
      "name": "pre-commit",
      "path": "/tmp/repo/.git/hooks/pre-commit",
      "executable": true,
      "source": "hooks_dir"
    }
  ]
}
```

Example run of a `pre-commit` hook that prints a line on each stream and exits 17:

```json
{
  "action": "run",
  "name": "pre-commit",
  "exit_code": 17,
  "executed": true,
  "stdout": "to-stdout\n",
  "stderr": "to-stderr\n"
}
```

## Markdown output

Schema: [v1](/schemas/hooks/v1.json)

`--markdown` renders a table of hooks for `list`, or a short summary for `run` followed by the hook's captured output under **Hook stdout:** and **Hook stderr:** headings, each in a `text` code block (a stream the hook left empty is omitted).

```text
## Hooks

| Name | Path | Executable | Source |
| --- | --- | --- | --- |
| `pre-commit` | `[REPO]/.git/hooks/pre-commit` | true | hooks_dir |
```

## Exit status

| Code | Meaning |
| ---: | --- |
| `0` | `hooks list` or `hooks run` when the hook exits 0 (or the hook is missing). |
| *varies* | `hooks run` returns the hook subprocess exit code ([`custom`](../exit-codes/)). |
| `1` | `hooks run` when a hook could not be started ([`failure`](../exit-codes/)). |
| `2` | Invalid flags ([`usage`](../exit-codes/)). |

See [Exit codes](../exit-codes/).


## See also

- [Library maintenance — hooks](../../library/maintenance.md)
- [`grit patch-id`](patch-id.md)
