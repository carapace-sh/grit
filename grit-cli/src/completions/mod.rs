//! Shell completion for `grit`, served by carapace.
//!
//! `grit _carapace <shell> …` answers completion requests from the shell
//! integration scripts, and `grit completions <shell>` prints those scripts.
//! Both use the same clap tree as the parser, so completion follows the CLI
//! automatically: what an argument offers is declared next to the argument
//! itself with `add = completer!(…)`, and commands only need wiring when the
//! completion lives on the command rather than on one argument.

pub(crate) mod actions;

use std::sync::Arc;

use carapace_rs::shell::Shell;
use carapace_rs::{Action, Context};
use carapace_rs_clap::Carapace;

/// Answer a `_carapace` invocation.
///
/// Returns the text to print when `args` is a completion, snippet, spec, macro
/// or style request, and `None` when the program should run normally.
pub fn handle(args: &[String]) -> Option<String> {
    carapace_rs_clap::complete(command, args)
}

/// The command tree with the wiring carapace cannot attach to one argument.
///
/// The tree is rebuilt per request; nothing is cached, so a rebuilt binary
/// always completes against its own arguments.
pub fn command() -> clap::Command {
    let root =
        crate::cli_command::full_cli_command("grit").pre_invoke(Arc::new(repository_context));
    GROUPS.iter().fold(root, |root, (group, commands)| {
        commands.iter().fold(root, |root, name| {
            root.mut_subcommand(name, |sub| sub.completion_group(*group))
        })
    })
}

/// Shell names `grit completions` accepts.
pub fn shell_names() -> &'static [&'static str] {
    actions::shell_names()
}

/// Parse a shell name accepted by `grit completions`.
///
/// `export` parses as a carapace shell but is not an integration script, so it
/// is rejected like any unknown name.
///
/// # Errors
///
/// Returns the accepted shell names when `name` is not one of them.
pub fn parse_shell(name: &str) -> Result<Shell, String> {
    match Shell::parse(name).filter(|shell| *shell != Shell::Export) {
        Some(shell) => Ok(shell),
        None => Err(format!(
            "unknown shell '{name}' (expected one of: {})",
            shell_names().join(", ")
        )),
    }
}

/// The shell integration script for `shell`.
///
/// The script registers a completion function that calls `grit _carapace`.
pub fn snippet(shell: Shell) -> String {
    carapace_rs_clap::snippet(command, shell)
}

/// Top-level commands grouped the way the documentation groups them; the group
/// becomes the completion tag, so related commands sit together in the menu.
///
/// This is command metadata rather than argument completion, which is why it is
/// applied here instead of at a definition.
const GROUPS: &[(&str, &[&str])] = &[
    (
        "Getting started",
        &["checkout", "clone", "config", "init", "status"],
    ),
    (
        "Making changes",
        &["add", "commit", "diff", "mv", "restore", "rm", "stash"],
    ),
    (
        "History",
        &[
            "blame",
            "history",
            "log",
            "merge-base",
            "notes",
            "patch-id",
            "rebase",
            "reflog",
            "revs",
            "shortlog",
            "show",
        ],
    ),
    (
        "Branches and tags",
        &[
            "branch", "merge", "pick", "reset", "revert", "switch", "tag", "worktree",
        ],
    ),
    (
        "Remotes",
        &[
            "auth",
            "bundle",
            "checkin",
            "fetch",
            "integrate",
            "pull",
            "push",
            "remote",
            "serve",
            "slice",
        ],
    ),
    (
        "Maintenance",
        &[
            "clean",
            "completions",
            "fsck",
            "gc",
            "repack",
            "skill",
            "update",
        ],
    ),
    (
        "Plumbing",
        &[
            "cat",
            "hooks",
            "ignore",
            "manager",
            "receive-pack",
            "resolve",
            "upload-pack",
        ],
    ),
];

fn repository_context(context: &Context, action: Action) -> Action {
    let mut context = context.clone();
    let flags = |name: &str| -> Vec<String> {
        context
            .parsed_flags
            .iter()
            .filter(|(flag, _)| flag == name)
            .flat_map(|(_, values)| values.iter().cloned())
            .collect()
    };
    let dirs = flags("dirs");
    let work_tree = flags("work-tree").pop();
    let git_dir = flags("git-dir").pop();
    let overrides = flags("config");
    for dir in dirs {
        let path = context.dir.join(dir);
        context.dir = path.canonicalize().unwrap_or(path);
    }
    if let Some(work_tree) = work_tree {
        let path = context.dir.join(work_tree);
        let path = path.canonicalize().unwrap_or(path);
        context
            .env
            .insert("GIT_WORK_TREE".into(), path.to_string_lossy().into_owned());
        context.dir = path;
    }
    if let Some(git_dir) = git_dir {
        let path = context.dir.join(git_dir);
        let path = path.canonicalize().unwrap_or(path);
        context
            .env
            .insert("GIT_DIR".into(), path.to_string_lossy().into_owned());
    }
    if !overrides.is_empty() {
        context.env.remove("GIT_CONFIG_PARAMETERS");
        context
            .env
            .insert("GIT_CONFIG_COUNT".into(), overrides.len().to_string());
        for (index, override_) in overrides.iter().enumerate() {
            let (key, value) = override_
                .split_once('=')
                .map_or((override_.as_str(), "true"), |(key, value)| (key, value));
            context
                .env
                .insert(format!("GIT_CONFIG_KEY_{index}"), key.to_owned());
            context
                .env
                .insert(format!("GIT_CONFIG_VALUE_{index}"), value.to_owned());
        }
    }
    action.invoke(&context).to_action()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    /// Complete `words` the way the shell would, as plain text output.
    ///
    /// Building the clap tree in a debug build needs far more stack than a
    /// test thread has, so this runs on [`crate::cli_test_stack`].
    fn complete(shell: Shell, words: &[&str]) -> String {
        let words: Vec<String> = words.iter().map(|word| (*word).to_string()).collect();
        crate::cli_test_stack::run(move || {
            carapace_rs_clap::complete_output(command, shell, &words)
        })
    }

    #[test]
    fn subcommands_are_offered() {
        let output = complete(Shell::Zsh, &[""]);
        for subcommand in ["commit", "status", "worktree", "completions"] {
            assert!(output.contains(subcommand), "{output}");
        }
    }

    #[test]
    fn nested_subcommands_are_offered() {
        let output = complete(Shell::Bash, &["remote", ""]);
        for subcommand in ["add", "remove", "set-url", "refs"] {
            assert!(output.contains(subcommand), "{output}");
        }
    }

    #[test]
    fn snippet_calls_the_hidden_entry_point() {
        let snippet = crate::cli_test_stack::run(|| snippet(Shell::Bash));
        assert!(snippet.contains("_carapace"), "{snippet}");
        assert!(snippet.contains("grit"), "{snippet}");
    }

    #[test]
    fn non_completion_invocations_are_ignored() {
        assert!(handle(&[]).is_none());
        assert!(handle(&["status".to_owned()]).is_none());
    }

    #[test]
    fn shell_request_returns_a_script() {
        let output = crate::cli_test_stack::run(|| {
            handle(&["_carapace".to_owned(), "fish".to_owned()]).expect("snippet")
        });
        assert!(output.contains("_carapace"), "{output}");
    }
}
