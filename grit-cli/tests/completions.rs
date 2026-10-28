//! Integration tests for shell completion: the scripts `grit completions`
//! prints, and the candidates `grit _carapace` answers with.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use grit_test_support::{grit_cmd, write_file};
use serde_json::Value;

/// A repository with a commit, a second branch, a tag, and a remote.
fn sample_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("project");
    std::fs::create_dir(&dir).expect("create project dir");
    grit_cmd(&["init", "."]).in_dir(&dir).suc();
    write_file(&dir, "tracked.txt", "one\n");
    grit_cmd(&["add", "tracked.txt"]).in_dir(&dir).suc();
    grit_cmd(&["commit", "-m", "first commit"])
        .in_dir(&dir)
        .suc();
    grit_cmd(&["branch", "feature-x"]).in_dir(&dir).suc();
    grit_cmd(&["tag", "v1"]).in_dir(&dir).suc();
    grit_cmd(&["remote", "add", "origin", "https://example.com/repo.git"])
        .in_dir(&dir)
        .suc();
    (tmp, dir)
}

/// The script `grit completions <shell>` prints.
fn script(shell: &str) -> String {
    grit_cmd(&["completions", shell])
        .in_dir(Path::new("."))
        .suc()
        .stdout
}

/// One completion request, as the export JSON carapace uses for bridging.
fn export(dir: &Path, words: &[&str]) -> Value {
    let mut args = vec!["_carapace", "export", "grit"];
    args.extend_from_slice(words);
    let output = grit_cmd(&args).in_dir(dir).suc();
    serde_json::from_str(output.stdout.trim()).unwrap_or_else(|error| {
        panic!(
            "export json for {words:?}: {error}\n{}",
            output.dump("grit _carapace export")
        )
    })
}

/// `(value, description, tag)` triples from an export payload.
fn values(payload: &Value) -> Vec<(String, String, String)> {
    payload["values"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|value| {
            (
                value["value"].as_str().unwrap_or_default().to_owned(),
                value["description"].as_str().unwrap_or_default().to_owned(),
                value["tag"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

fn has_value(payload: &Value, value: &str) -> bool {
    values(payload).iter().any(|candidate| candidate.0 == value)
}

#[test]
fn bash_completions_install_a_completion_function() {
    let script = script("bash");
    assert!(script.contains("_grit_completion"), "{script}");
    assert!(script.contains("_carapace"), "{script}");
}

#[test]
fn zsh_completions_install_a_completion_function() {
    let script = script("zsh");
    assert!(script.contains("_grit_completion"), "{script}");
    assert!(script.contains("_carapace"), "{script}");
}

#[test]
fn fish_completions_install_a_completion_function() {
    let script = script("fish");
    assert!(script.contains("_carapace"), "{script}");
}

#[test]
fn commands_are_described_with_their_own_descriptions() {
    let payload = export(Path::new("."), &[""]);
    let candidates = values(&payload);

    assert!(
        candidates.iter().any(|candidate| candidate.0 == "log"
            && candidate.1 == "Show recent commits reachable from HEAD"),
        "{candidates:?}"
    );
    assert!(
        candidates.iter().any(
            |candidate| candidate.0 == "stash" && candidate.1 == "Save or restore shelved work"
        ),
        "{candidates:?}"
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.1.starts_with("Subcommands of")),
        "nested subcommand enum docs must not replace command descriptions"
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.1 == "Mutually exclusive diff output formats"),
        "flattened argument group docs must not replace command descriptions"
    );
}

#[test]
fn branches_complete_with_their_subject() {
    let (_tmp, dir) = sample_repo();

    let payload = export(&dir, &["branch", ""]);
    let candidates = values(&payload);

    assert!(
        candidates.iter().any(|candidate| candidate.0 == "feature-x"
            && candidate.1 == "first commit"
            && candidate.2 == "local branches"),
        "{candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.0 == "main" && candidate.2 == "local branches"),
        "{candidates:?}"
    );
}

#[test]
fn tags_and_remotes_are_completed() {
    let (_tmp, dir) = sample_repo();

    let tags = export(&dir, &["tag", ""]);
    assert!(has_value(&tags, "v1"), "{:?}", values(&tags));

    let remotes = export(&dir, &["fetch", ""]);
    let candidates = values(&remotes);
    assert!(
        candidates.iter().any(|candidate| candidate.0 == "origin"
            && candidate.1 == "https://example.com/repo.git"
            && candidate.2 == "remotes"),
        "{candidates:?}"
    );
}

#[test]
fn paths_complete_worktree_and_tracked_files() {
    let (_tmp, dir) = sample_repo();
    write_file(&dir, "untracked.txt", "two\n");

    let payload = export(&dir, &["add", ""]);
    let candidates = values(&payload);

    for path in ["tracked.txt", "untracked.txt"] {
        assert!(
            candidates.iter().any(|candidate| candidate.0 == path),
            "{path} missing: {candidates:?}"
        );
    }
}

#[test]
fn paths_skip_ignored_files() {
    let (_tmp, dir) = sample_repo();
    write_file(&dir, ".gitignore", "ignored.log\n");
    write_file(&dir, "ignored.log", "noise\n");

    let payload = export(&dir, &["add", ""]);
    let candidates = values(&payload);

    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.0 == "ignored.log"),
        "ignored path was offered: {candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.0 == "tracked.txt"),
        "{candidates:?}"
    );
}

#[test]
fn paths_keep_tracked_files_deleted_from_the_worktree() {
    let (_tmp, dir) = sample_repo();
    std::fs::remove_file(dir.join("tracked.txt")).expect("delete tracked file");

    let payload = export(&dir, &["add", ""]);
    let candidates = values(&payload);

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.0 == "tracked.txt" && candidate.2 == "tracked paths"),
        "{candidates:?}"
    );
}

#[test]
fn revision_ranges_complete_both_sides() {
    let (_tmp, dir) = sample_repo();

    let payload = export(&dir, &["log", "main.."]);
    let candidates = values(&payload);

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.0 == "main..feature-x"),
        "{candidates:?}"
    );
}

#[test]
fn config_keys_and_values_are_completed() {
    let (_tmp, dir) = sample_repo();
    grit_cmd(&["config", "user.name", "Test User"])
        .in_dir(&dir)
        .suc();

    let keys = export(&dir, &["config", ""]);
    assert!(has_value(&keys, "user.name"), "{:?}", values(&keys));

    let payload = export(&dir, &["config", "user.name", ""]);
    assert!(has_value(&payload, "Test User"), "{:?}", values(&payload));
}

#[test]
fn flags_are_completed() {
    let (_tmp, dir) = sample_repo();

    let payload = export(&dir, &["branch", "-"]);
    let candidates = values(&payload);

    assert!(
        candidates.iter().any(|candidate| candidate.0 == "--delete"),
        "{candidates:?}"
    );
}

#[test]
fn repository_flags_select_the_completed_repository() {
    let (_tmp, dir) = sample_repo();
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("create sibling dir");

    let payload = export(
        &elsewhere,
        &["-C", dir.to_str().expect("utf-8 path"), "branch", ""],
    );

    assert!(has_value(&payload, "feature-x"), "{:?}", values(&payload));
}

#[test]
fn git_dir_flag_selects_the_completed_repository() {
    let (_tmp, dir) = sample_repo();
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("create sibling dir");
    let git_dir = dir.join(".git");

    let payload = export(
        &elsewhere,
        &[
            "--git-dir",
            git_dir.to_str().expect("utf-8 path"),
            "branch",
            "",
        ],
    );

    assert!(has_value(&payload, "feature-x"), "{:?}", values(&payload));
}

#[test]
fn repository_flag_completes_directories_only() {
    let (_tmp, dir) = sample_repo();
    std::fs::create_dir(dir.join("nested")).expect("create nested dir");

    let payload = export(&dir, &["-C", ""]);
    let candidates = values(&payload);

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.0 == "nested/" && candidate.2 == "directories"),
        "{candidates:?}"
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.0 == "tracked.txt"),
        "a path-valued flag must not fall back to clap's file hint: {candidates:?}"
    );
}

/// Write a loose remote-tracking ref without contacting the remote.
fn write_tracking_ref(dir: &Path, remote: &str, branch: &str, oid: &str) {
    let path = format!(".git/refs/remotes/{remote}/{branch}");
    std::fs::create_dir_all(dir.join(format!(".git/refs/remotes/{remote}")))
        .expect("create ref dir");
    write_file(dir, &path, &format!("{oid}\n"));
}

/// The tip oid recorded in a loose branch ref.
fn branch_oid(dir: &Path, branch: &str) -> String {
    std::fs::read_to_string(dir.join(format!(".git/refs/heads/{branch}")))
        .expect("read branch ref")
        .trim()
        .to_owned()
}

#[test]
fn fetch_refspecs_complete_remote_branches_and_local_destinations() {
    let (_tmp, dir) = sample_repo();
    write_tracking_ref(&dir, "origin", "main", &branch_oid(&dir, "main"));

    let payload = export(&dir, &["fetch", "origin", ""]);
    let candidates = values(&payload);
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.0 == "main" && candidate.2 == "remote branches"),
        "{candidates:?}"
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.0 == "origin/main"),
        "a refspec source names the branch on the remote: {candidates:?}"
    );

    let payload = export(&dir, &["fetch", "origin", "main:"]);
    let candidates = values(&payload);
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.0 == "main:feature-x" && candidate.2 == "local branches"),
        "{candidates:?}"
    );
}

#[test]
fn remote_set_head_completes_short_branch_names() {
    let (_tmp, dir) = sample_repo();
    write_tracking_ref(&dir, "origin", "main", &branch_oid(&dir, "main"));

    let payload = export(&dir, &["remote", "set-head", "origin", ""]);
    let candidates = values(&payload);

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.0 == "main" && candidate.1 == "first commit"),
        "{candidates:?}"
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.0 == "origin/main"),
        "set-head takes the branch name inside the remote: {candidates:?}"
    );
}

#[test]
fn hunk_ids_match_the_diff_report() {
    let (_tmp, dir) = sample_repo();
    write_file(&dir, "tracked.txt", "one\ntwo\n");

    let id = diff_hunk_id(&dir, &[]);
    let payload = export(&dir, &["add", "--hunks", ""]);
    let candidates = values(&payload);

    assert!(
        candidates.iter().any(|candidate| candidate.0 == id
            && candidate.1.starts_with("tracked.txt ")
            && candidate.2 == "unstaged hunks"),
        "{candidates:?}"
    );

    let narrow = diff_hunk_id(&dir, &["-U", "0"]);
    assert_ne!(narrow, id, "context lines change the hunk id");
    let payload = export(&dir, &["add", "-U", "0", "--hunks", ""]);
    let candidates = values(&payload);
    assert!(
        candidates.iter().any(|candidate| candidate.0 == narrow),
        "{candidates:?}"
    );
}

/// The first unstaged hunk id `grit diff --json` reports, with extra flags.
fn diff_hunk_id(dir: &Path, flags: &[&str]) -> String {
    let mut args = vec!["diff", "--unstaged"];
    args.extend_from_slice(flags);
    args.push("--json");
    let stdout = grit_cmd(&args).in_dir(dir).suc().stdout;
    let diff: Value = serde_json::from_str(&stdout).expect("diff json");
    diff["files"][0]["hunks"][0]["id"]
        .as_str()
        .expect("hunk id")
        .to_owned()
}

#[test]
fn line_ranges_complete_paths_and_hunk_ranges() {
    let (_tmp, dir) = sample_repo();
    write_file(&dir, "tracked.txt", "one\ntwo\n");

    let payload = export(&dir, &["add", "--lines", "tracked.txt:"]);
    let candidates = values(&payload);

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.0 == "tracked.txt:1-2" && candidate.2 == "line ranges"),
        "{candidates:?}"
    );
}

#[test]
fn staged_hunks_are_offered_to_staged_restore_only() {
    let (_tmp, dir) = sample_repo();
    write_file(&dir, "staged.txt", "content\n");
    grit_cmd(&["add", "staged.txt"]).in_dir(&dir).suc();

    let payload = export(&dir, &["restore", "--staged", "--hunks", ""]);
    let candidates = values(&payload);
    assert!(
        candidates.iter().any(
            |candidate| candidate.1.starts_with("staged.txt ") && candidate.2 == "staged hunks"
        ),
        "{candidates:?}"
    );

    let payload = export(&dir, &["add", "--hunks", ""]);
    assert!(
        !values(&payload)
            .iter()
            .any(|candidate| candidate.1.starts_with("staged.txt ")),
        "unstaged hunk ids must not include staged changes"
    );
}

#[test]
fn reflog_selectors_complete_existing_entries() {
    let (_tmp, dir) = sample_repo();

    let payload = export(&dir, &["reflog", "delete", ""]);
    let candidates = values(&payload);

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.0 == "HEAD@{0}" && candidate.2 == "reflog entries"),
        "{candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.0 == "feature-x@{0}"
                && candidate.1 == "branch: Created from main"),
        "{candidates:?}"
    );
}

#[test]
fn unknown_shell_is_reported() {
    let output = grit_cmd(&["completions", "not-a-shell"])
        .in_dir(Path::new("."))
        .exec();
    assert_eq!(output.status, Some(2), "{}", output.dump("completions"));
    assert!(
        output.stderr.contains("unknown shell"),
        "{}",
        output.dump("completions")
    );
}
