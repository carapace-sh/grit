//! Completion candidates built from the repository.
//!
//! Every action is lazy: it opens the repository only when the shell asks for
//! candidates, and reports an empty list rather than an error when there is no
//! repository to open. Styles, group tags, and `git://` uids follow the layout
//! carapace-bin uses for Git itself, so branches, tags, stashes, and remotes
//! read alike across both tools. Carapace emits the uids only in its
//! experimental tabdance mode; they name the kind and the repository a value
//! came from, so two repositories' branches stay apart.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use carapace_rs::{batch, Action, Context, InvokedAction};
use grit_lib::config::environment::{Environment, RepositoryOptions};
use grit_lib::diff::structured::StructuredHunk;
use grit_lib::diff::{hunk_diff_bundle, hunk_ids_with_duplicate_peers, DiffOptions, DiffSide};
use grit_lib::index::Index;
use grit_lib::porcelain::ref_summaries::{list_ref_summaries, RefSummary};
use grit_lib::refs::listing::{RefKind, RefNamespace};
use grit_lib::repo::Repository;
use grit_lib::repo_handles::ReflogQuery;
use grit_lib::worktree::worktree_rules::WorktreeRules;

/// Style for local and remote branch names (carapace-bin `styles.Git.Branch`).
const BRANCH_STYLE: &str = "blue";
/// Style for tag names (carapace-bin `styles.Git.Tag`).
const TAG_STYLE: &str = "yellow";
/// Style for the pseudo refs (`HEAD` and friends, carapace-bin `styles.Git.Head`).
const HEAD_STYLE: &str = "bold";
/// Style for stash entries (carapace-bin `styles.Git.Stash`).
const STASH_STYLE: &str = "green";
/// Style for notes refs (carapace-bin `styles.Git.Note`).
const NOTE_STYLE: &str = "cyan";
/// Style for hunk ids (carapace-bin has no hunk style; magenta matches commit ids).
const HUNK_STYLE: &str = "magenta";

/// Refs whose reflog is listed for `ref@{n}` completion.
const REFLOG_REF_LIMIT: usize = 50;
/// Newest entries listed per ref for `ref@{n}` completion.
const REFLOG_ENTRY_LIMIT: usize = 10;

/// Pseudo refs Git understands besides `refs/*` (see `gitrevisions(7)`).
const PSEUDO_REFS: &[&str] = &[
    "HEAD",
    "FETCH_HEAD",
    "ORIG_HEAD",
    "MERGE_HEAD",
    "REBASE_HEAD",
    "REVERT_HEAD",
    "CHERRY_PICK_HEAD",
    "BISECT_HEAD",
];

/// Shell names accepted by `grit completions`.
const SHELLS: &[&str] = &[
    "bash",
    "bash-ble",
    "cmd-clink",
    "elvish",
    "fish",
    "nushell",
    "oil",
    "powershell",
    "tcsh",
    "xonsh",
    "zsh",
];

/// Configuration keys that are commonly set by hand, completed alongside the
/// keys already present in the effective configuration.
const CONFIG_KEYS: &[&str] = &[
    "commit.autoStage",
    "commit.gpgsign",
    "core.autocrlf",
    "core.editor",
    "core.hooksPath",
    "diff.algorithm",
    "gc.auto",
    "gpg.program",
    "init.defaultBranch",
    "merge.conflictStyle",
    "pull.rebase",
    "push.default",
    "tag.gpgSign",
    "user.email",
    "user.name",
];

/// Uid options that scope a `git://` uid to `repo`: the work tree, and the git
/// directory when it is not the work tree's `.git` (a bare repository or a
/// separate git directory), the way carapace-bin scopes its git uids.
fn repository_uid_opts(repo: &Repository) -> Vec<(&'static str, String)> {
    let Some(work_tree) = repo.work_tree() else {
        return vec![("GIT_DIR", repo.git_dir().to_string_lossy().into_owned())];
    };
    let mut options = vec![("GIT_WORK_TREE", work_tree.to_string_lossy().into_owned())];
    if repo.git_dir() != work_tree.join(".git") {
        options.push(("GIT_DIR", repo.git_dir().to_string_lossy().into_owned()));
    }
    options
}

/// Object hash formats `grit init --object-format` accepts.
pub(crate) fn action_object_formats() -> Action {
    Action::values(["sha1", "sha256"]).tag("object formats")
}

/// Ref backends `grit init --ref-format` accepts.
pub(crate) fn action_ref_formats() -> Action {
    Action::values(["files", "reftable"]).tag("ref formats")
}

/// Diff algorithms `--diff-algorithm` accepts (`default` is another spelling of
/// `myers`).
pub(crate) fn action_diff_algorithms() -> Action {
    Action::values(["histogram", "minimal", "myers", "patience"]).tag("diff algorithms")
}

/// Merge-diff modes `grit show --diff-merges` accepts.
pub(crate) fn action_diff_merges() -> Action {
    Action::values(["combined", "dense-combined", "first-parent"]).tag("diff merge modes")
}

/// Credential-helper operations `grit manager` accepts.
pub(crate) fn action_credential_operations() -> Action {
    Action::values(["erase", "get", "store"]).tag("operations")
}

/// Untracked-path modes `grit status --untracked` accepts.
pub(crate) fn action_untracked_modes() -> Action {
    Action::values(["all", "no", "normal"]).tag("untracked modes")
}

/// Sort keys the branch listing accepts.
pub(crate) fn action_sort_keys() -> Action {
    Action::values(["date", "name", "version"]).tag("sort keys")
}

/// Shell names `grit completions` accepts.
pub(crate) fn shell_names() -> &'static [&'static str] {
    SHELLS
}

/// The shells `grit completions` can print a script for.
pub(crate) fn action_shells() -> Action {
    Action::values(SHELLS.iter().copied()).tag("shells")
}

/// Local branches (`refs/heads/*`) with the subject of their tip commit.
pub(crate) fn action_local_branches() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let summaries = list_ref_summaries(&repo, &[RefNamespace::Heads]).unwrap_or_default();
        Action::values_described(described(summaries.iter()))
            .style(BRANCH_STYLE)
            .tag("local branches")
            .uid("git", "local-branch", repository_uid_opts(&repo))
    })
}

/// Remote-tracking branches (`refs/remotes/*`) with their tip subject.
pub(crate) fn action_remote_branches() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let summaries = list_ref_summaries(&repo, &[RefNamespace::Remotes]).unwrap_or_default();
        Action::values_described(described(summaries.iter()))
            .style(BRANCH_STYLE)
            .tag("remote branches")
            .uid("git", "remote-branch", repository_uid_opts(&repo))
    })
}

/// Tags (`refs/tags/*`) with the subject of the commit they peel to.
pub(crate) fn action_tags() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let summaries = list_ref_summaries(&repo, &[RefNamespace::Tags]).unwrap_or_default();
        Action::values_described(described(summaries.iter()))
            .style(TAG_STYLE)
            .tag("tags")
            .uid("git", "tag", repository_uid_opts(&repo))
    })
}

/// Local and remote branch names, as `switch` accepts them.
pub(crate) fn action_branch_targets() -> Action {
    batch(vec![action_remote_branches(), action_local_branches()])
}

/// The pseudo refs that name a tip right now: `HEAD`, and the operation refs
/// such as `MERGE_HEAD` that exist only while that operation is in progress.
fn pseudo_refs(repo: &Repository) -> Action {
    let git_dir = repo.git_dir();
    let existing = PSEUDO_REFS
        .iter()
        .copied()
        .filter(|name| git_dir.join(name).exists());
    Action::values(existing)
        .style(HEAD_STYLE)
        .tag("pseudo refs")
        .uid("git", "ref", repository_uid_opts(repo))
}

/// Stash entries (`stash@{N}`) with the message they were created with.
pub(crate) fn action_stashes() -> Action {
    Action::callback(|context| match repository(context) {
        Some(repo) => stash_entries(&repo),
        None => Action::default(),
    })
}

fn stash_entries(repo: &Repository) -> Action {
    let entries = repo.stash().list().unwrap_or_default();
    Action::values_described(
        entries
            .into_iter()
            .map(|entry| (format!("stash@{{{}}}", entry.index), entry.message)),
    )
    .style(STASH_STYLE)
    .tag("stashes")
    .uid("git", "stash", repository_uid_opts(repo))
}

/// Reflog selectors (`main@{0}`, `HEAD@{2}`, …), described by the entry message.
///
/// Only refs that actually have a reflog are offered, newest entries first, so
/// `grit reflog delete` can only be pointed at entries that exist. The list is
/// capped because every ref with a reflog contributes its own entries.
pub(crate) fn action_reflog_selectors() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let refs = repo.reflogs().refs().unwrap_or_default();
        let query = ReflogQuery {
            limit: Some(REFLOG_ENTRY_LIMIT),
            ..ReflogQuery::default()
        };
        let mut values = Vec::new();
        for refname in refs.iter().take(REFLOG_REF_LIMIT) {
            let Ok(page) = repo.reflogs().query(refname, &query) else {
                continue;
            };
            let short = short_refname(refname.as_str());
            values.extend(
                page.entries
                    .into_iter()
                    .map(|entry| (format!("{short}@{{{}}}", entry.index), entry.entry.message)),
            );
        }
        Action::values_described(values)
            .style(BRANCH_STYLE)
            .tag("reflog entries")
            .uid("git", "reflog", repository_uid_opts(&repo))
    })
}

/// `refs/heads/main` and `HEAD` as the names reflog selectors are written with.
fn short_refname(refname: &str) -> &str {
    refname
        .strip_prefix("refs/heads/")
        .or_else(|| refname.strip_prefix("refs/remotes/"))
        .or_else(|| refname.strip_prefix("refs/"))
        .unwrap_or(refname)
}

/// Every revision name a user is likely to type: branches, tags, stashes, and
/// the pseudo refs.
///
/// One repository open and one ref listing feed all of it; the kinds still read
/// apart in the menu.
pub(crate) fn action_revisions() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let summaries = list_ref_summaries(
            &repo,
            &[
                RefNamespace::Heads,
                RefNamespace::Remotes,
                RefNamespace::Tags,
            ],
        )
        .unwrap_or_default();
        let uid = repository_uid_opts(&repo);
        batch(vec![
            Action::values_described(described(
                summaries
                    .iter()
                    .filter(|summary| summary.kind == RefKind::Head),
            ))
            .style(BRANCH_STYLE)
            .tag("local branches")
            .uid("git", "local-branch", uid.clone()),
            Action::values_described(described(
                summaries
                    .iter()
                    .filter(|summary| summary.kind == RefKind::Remote),
            ))
            .style(BRANCH_STYLE)
            .tag("remote branches")
            .uid("git", "remote-branch", uid.clone()),
            Action::values_described(described(
                summaries
                    .iter()
                    .filter(|summary| summary.kind == RefKind::Tag),
            ))
            .style(TAG_STYLE)
            .tag("tags")
            .uid("git", "tag", uid),
            stash_entries(&repo),
            pseudo_refs(&repo),
        ])
    })
}

/// Revisions plus range syntax: `A..B`, `A...B`, and `^A`.
pub(crate) fn action_revision_ranges() -> Action {
    Action::callback(|context| {
        if context.value.starts_with('^') {
            return action_revisions().prefix("^");
        }
        let delimiter = if context.value.contains("...") {
            "..."
        } else {
            ".."
        };
        let Some(index) = context.value.find(delimiter) else {
            return action_revisions();
        };
        let start = context.value[..index].to_owned();
        let range = context.value[..index + delimiter.len()].to_owned();
        // the end of a range is another revision, but not the one it starts at
        action_revisions().filter([start]).prefix(range)
    })
}

/// Configured remotes, described with their fetch URL.
pub(crate) fn action_remotes() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let remotes = repo.remotes().configured().unwrap_or_default();
        Action::values_described(
            remotes
                .into_iter()
                .map(|remote| (remote.name, remote.url.unwrap_or_default())),
        )
        .tag("remotes")
        .uid("git", "remote", repository_uid_opts(&repo))
    })
}

/// Branch names on the remote typed as the command's first positional.
///
/// `grit remote set-head` names a branch inside the remote, so `origin/main`
/// from the remote-tracking refs is trimmed to `main`.
pub(crate) fn action_remote_branch_names() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let summaries = list_ref_summaries(&repo, &[RefNamespace::Remotes]).unwrap_or_default();
        Action::values_described(remote_branch_values(&summaries, &context.args[0]))
            .style(BRANCH_STYLE)
            .tag("remote branches")
            .uid("git", "remote-branch", repository_uid_opts(&repo))
    })
}

/// Refspecs `fetch` accepts: a remote branch (or tag) source and a local
/// destination, as in `main:incoming`.
pub(crate) fn action_fetch_refspecs() -> Action {
    Action::multiparts_n(":", 2, |context| {
        if !context.parts.is_empty() {
            return action_local_branches();
        }
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let summaries = list_ref_summaries(&repo, &[RefNamespace::Remotes, RefNamespace::Tags])
            .unwrap_or_default();
        let uid = repository_uid_opts(&repo);
        batch(vec![
            Action::values_described(remote_branch_values(&summaries, &context.args[0]))
                .style(BRANCH_STYLE)
                .tag("remote branches")
                .uid("git", "remote-branch", uid.clone()),
            Action::values_described(described(
                summaries
                    .iter()
                    .filter(|summary| summary.kind == RefKind::Tag),
            ))
            .style(TAG_STYLE)
            .tag("tags")
            .uid("git", "tag", uid),
        ])
    })
}

/// Remote-tracking branches of `remote` in `summaries` as `(name on the
/// remote, tip subject)`.
///
/// A refspec source and `remote set-head` both name the branch the way the
/// remote does, so the tracking ref's remote component is dropped.
fn remote_branch_values(summaries: &[RefSummary], remote: &str) -> Vec<(String, String)> {
    let prefix = format!("{remote}/");
    summaries
        .iter()
        .filter(|summary| summary.kind == RefKind::Remote)
        .filter_map(|summary| {
            let name = summary.name.strip_prefix(prefix.as_str())?.to_owned();
            Some((name, summary.subject.clone().unwrap_or_default()))
        })
        .collect()
}

/// Notes refs, as the short names `grit notes --ref` accepts.
pub(crate) fn action_notes_refs() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let mut names: Vec<String> = repo
            .references()
            .list("refs/notes/")
            .unwrap_or_default()
            .into_iter()
            .map(|(refname, _)| refname.trim_start_matches("refs/notes/").to_owned())
            .collect();
        names.sort();
        names.dedup();
        Action::values(names)
            .style(NOTE_STYLE)
            .tag("notes refs")
            .uid("git", "note", repository_uid_opts(&repo))
    })
}

/// Linked worktree paths, with the branch each one has checked out.
pub(crate) fn action_worktrees() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let entries = repo.worktrees().list().unwrap_or_default();
        Action::values_described(entries.into_iter().map(|entry| {
            (
                entry.path.to_string_lossy().into_owned(),
                match &entry.head {
                    grit_lib::state::HeadState::Branch { short_name, .. } => short_name.clone(),
                    _ => "detached".to_owned(),
                },
            )
        }))
        .tag("worktrees")
        .uid("git", "worktree", repository_uid_opts(&repo))
    })
}

/// Hook scripts installed for this repository, flagged when not executable.
pub(crate) fn action_hooks() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let hooks = repo
            .hooks()
            .list(None)
            .map(|hooks| hooks.items)
            .unwrap_or_default();
        Action::values_described(hooks.into_iter().map(|hook| {
            let description = if hook.executable {
                "hook"
            } else {
                "not executable"
            };
            (hook.name, description.to_owned())
        }))
        .tag("hooks")
        .uid("git", "hook", repository_uid_opts(&repo))
    })
}

/// Config keys: those already in the effective configuration plus the ones
/// people set most often.
pub(crate) fn action_config_keys() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::values(CONFIG_KEYS.iter().copied()).tag("config keys");
        };
        let mut keys: Vec<String> = repo
            .config()
            .map(|config| config.entries().iter().map(|e| e.key.clone()).collect())
            .unwrap_or_default();
        keys.extend(CONFIG_KEYS.iter().map(|key| (*key).to_owned()));
        keys.sort();
        keys.dedup();
        Action::values(keys)
            .tag("config keys")
            .uid("git", "config", repository_uid_opts(&repo))
    })
}

/// Values already configured for `key`.
pub(crate) fn action_config_values(key: &str) -> Action {
    let key = key.to_owned();
    Action::callback(move |context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let values = repo
            .config()
            .map(|config| config.get_all(&key))
            .unwrap_or_default();
        Action::values(values).tag("config values").uid(
            "git",
            "config-value",
            repository_uid_opts(&repo),
        )
    })
}

/// Trailer keys `grit commit --trailer` understands, ready for `KEY=value`.
pub(crate) fn action_trailers() -> Action {
    Action::values([
        "Co-authored-by",
        "Fixes",
        "Refs",
        "Reviewed-by",
        "Signed-off-by",
    ])
    .tag("trailers")
    .suffix("=")
    .nospace(['='])
}

/// `KEY=VALUE` support for the global `-c KEY=VALUE` flag.
pub(crate) fn action_config_overrides() -> Action {
    Action::multiparts_n("=", 2, |context| match context.parts.len() {
        0 => action_config_keys().nospace(['=']).suffix("="),
        _ => action_config_values(&context.parts[0]),
    })
}

/// Paths a pathspec can name.
///
/// The worktree below the current directory supplies every file and directory a
/// user can see, ignored paths excepted. The index adds the tracked paths whose
/// file is gone, which no directory listing can show; a path that still exists
/// keeps the worktree rendering (LS_COLORS style), so `tracked paths` means
/// deleted.
pub(crate) fn action_paths() -> Action {
    Action::callback(|context| {
        let files = Action::files([]).invoke(context);
        let Some(repo) = repository(context) else {
            return files.to_action();
        };
        let Some(work_tree) = repo.work_tree() else {
            return files.to_action();
        };
        let Ok(index) = repo.index_handle().load() else {
            return files.to_action();
        };
        let files = without_ignored(&repo, &index, work_tree, context, files);
        let deleted = deleted_paths(&repo, &index, work_tree).invoke(context);
        files.merge([deleted]).to_action()
    })
}

/// `files` without the entries the ignore rules exclude.
fn without_ignored(
    repo: &Repository,
    index: &Index,
    work_tree: &Path,
    context: &Context,
    files: InvokedAction,
) -> InvokedAction {
    let Ok(rules) = WorktreeRules::from_repository(repo, index) else {
        return files;
    };
    let keep: Vec<String> = files
        .to_export()
        .to_raw_values()
        .into_iter()
        .filter(|value| {
            let Some(relative) = repo_relative(work_tree, context, &value.value) else {
                return true;
            };
            let is_dir = value.nospace == Some(true);
            !rules
                .ignore_mut()
                .is_path_ignored(repo, Some(index), &relative, is_dir)
                .unwrap_or(false)
        })
        .map(|value| value.value)
        .collect();
    files.retain(keep)
}

/// `value`, relative to the completion directory, as a work-tree-relative path.
fn repo_relative(work_tree: &Path, context: &Context, value: &str) -> Option<String> {
    let absolute = PathBuf::from(context.abs(value).ok()?);
    let relative = absolute.strip_prefix(work_tree).ok()?;
    let mut path = String::new();
    for component in relative.components() {
        let part = component.as_os_str().to_str()?;
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(part);
    }
    Some(path)
}

/// Index paths below the current directory whose file is gone from the worktree.
fn deleted_paths(repo: &Repository, index: &Index, work_tree: &Path) -> Action {
    let prefix = repo
        .pathspec_cwd_prefix()
        .map(|prefix| format!("{prefix}/"))
        .unwrap_or_default();
    let paths: Vec<String> = index
        .entries()
        .iter()
        .filter_map(|entry| {
            let path = String::from_utf8_lossy(&entry.path);
            let relative = path.strip_prefix(prefix.as_str())?;
            if relative.is_empty() {
                return None;
            }
            work_tree
                .join(path.as_ref())
                .symlink_metadata()
                .is_err()
                .then(|| relative.to_owned())
        })
        .collect();
    Action::values(paths)
        .tag("tracked paths")
        .uid("git", "tracked-path", repository_uid_opts(repo))
}

/// One hunk of the change the current command acts on.
struct HunkCandidate {
    path: String,
    start: usize,
    end: usize,
    id: String,
}

impl HunkCandidate {
    /// `path start-end`, or `path start` when the hunk covers one line.
    fn label(&self) -> String {
        if self.start == self.end {
            format!("{} {}", self.path, self.start)
        } else {
            format!("{} {}-{}", self.path, self.start, self.end)
        }
    }

    /// The new-side range the way `--lines` writes it (`3-7`, or `3`).
    fn range(&self) -> String {
        if self.start == self.end {
            self.start.to_string()
        } else {
            format!("{}-{}", self.start, self.end)
        }
    }
}

/// Hunk ids `--hunks` accepts, described with their path and new-side range.
///
/// The side follows `--staged`, so `grit restore --staged --hunks` is offered
/// index hunks and every other command is offered unstaged ones.
pub(crate) fn action_hunks() -> Action {
    Action::callback(|context| {
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let side = selected_side(context);
        let candidates = hunk_candidates(&repo, context, side);
        Action::values_described(candidates.into_iter().map(|hunk| {
            let label = hunk.label();
            (hunk.id, label)
        }))
        .style(HUNK_STYLE)
        .tag(hunk_tag(side))
        .uid("git", "hunk", repository_uid_opts(&repo))
    })
    .list(",")
}

/// Line ranges `--lines` accepts: a path, then the new-side range of one hunk.
pub(crate) fn action_line_ranges() -> Action {
    Action::multiparts_n(":", 2, |context| {
        if context.parts.is_empty() {
            return action_paths();
        }
        let Some(repo) = repository(context) else {
            return Action::default();
        };
        let path = context.parts[0].clone();
        let side = selected_side(context);
        let ranges: Vec<String> = hunk_candidates(&repo, context, side)
            .into_iter()
            .filter(|hunk| hunk.path == path)
            .map(|hunk| hunk.range())
            .collect();
        Action::values(ranges).tag("line ranges")
    })
}

/// The side `--hunks` / `--lines` resolve against, following `--staged`.
fn selected_side(context: &Context) -> DiffSide {
    let staged = context
        .parsed_flags
        .iter()
        .any(|(flag, _)| flag == "staged");
    if staged {
        DiffSide::Staged
    } else {
        DiffSide::Unstaged
    }
}

fn hunk_tag(side: DiffSide) -> &'static str {
    match side {
        DiffSide::Staged => "staged hunks",
        _ => "unstaged hunks",
    }
}

/// Hunks of the current changes, with the ids `grit diff --json` assigns them.
///
/// The unstaged side is diffed against the index and the staged side against
/// `HEAD`; each side keeps the other side's hunks as peers so a hunk body that
/// appears twice gets the same disambiguated id the diff command reports.
fn hunk_candidates(repo: &Repository, context: &Context, side: DiffSide) -> Vec<HunkCandidate> {
    let Ok(config) = repo.config() else {
        return Vec::new();
    };
    let mut options = DiffOptions::from_config(config.as_ref());
    if let Some(lines) = unified_context(context) {
        options.context_lines = lines;
    }
    let Ok(bundle) = hunk_diff_bundle(repo, &[], options) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    match side {
        DiffSide::Staged => {
            let unstaged: HashMap<&str, &[StructuredHunk]> = bundle
                .unstaged
                .iter()
                .map(|file| (file.path.as_str(), file.hunks.as_slice()))
                .collect();
            for (path, hunks) in &bundle.peer_hunks {
                let peers = unstaged.get(path.as_str()).copied().unwrap_or_default();
                candidates.extend(describe_hunks(side, path, hunks, peers));
            }
        }
        _ => {
            for file in &bundle.unstaged {
                let peers: &[StructuredHunk] = bundle
                    .peer_hunks
                    .get(&file.path)
                    .map_or(&[][..], Vec::as_slice);
                candidates.extend(describe_hunks(side, &file.path, &file.hunks, peers));
            }
        }
    }
    candidates.sort_by(|left, right| {
        (left.path.as_str(), left.start).cmp(&(right.path.as_str(), right.start))
    });
    candidates
}

fn describe_hunks(
    side: DiffSide,
    path: &str,
    hunks: &[StructuredHunk],
    peers: &[StructuredHunk],
) -> Vec<HunkCandidate> {
    let ids = hunk_ids_with_duplicate_peers(side, path, hunks, peers);
    hunks
        .iter()
        .zip(ids)
        .map(|(hunk, id)| {
            let start = hunk.new_start;
            let end = if hunk.new_lines == 0 {
                start
            } else {
                start + hunk.new_lines - 1
            };
            HunkCandidate {
                path: path.to_owned(),
                start,
                end,
                id: id.to_string(),
            }
        })
        .collect()
}

/// Unified context lines from `-U` / `--unified`, when the command set one.
fn unified_context(context: &Context) -> Option<usize> {
    context
        .parsed_flags
        .iter()
        .filter(|(flag, _)| flag == "unified_context")
        .flat_map(|(_, values)| values.iter().rev())
        .find_map(|value| value.parse().ok())
}

/// `(name, tip subject)` pairs for `summaries`.
fn described<'a>(summaries: impl IntoIterator<Item = &'a RefSummary>) -> Vec<(String, String)> {
    summaries
        .into_iter()
        .map(|summary| {
            (
                summary.name.clone(),
                summary.subject.clone().unwrap_or_default(),
            )
        })
        .collect()
}

/// Open the repository the completion request runs in.
///
/// The environment comes from the completion context rather than the process
/// so the global `-C` / `--git-dir` / `--work-tree` / `-c` flags seen earlier
/// on the command line apply to the candidates.
fn repository(context: &Context) -> Option<Repository> {
    let environment = Environment::from_vars(
        context
            .env
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value))),
        context.dir.clone(),
    );
    let mut options = RepositoryOptions::default();
    options.environment = environment;
    Repository::discover(None, &options).ok()
}
