//! `grit merge` and `grit rebase` run `pre-merge-commit`, `commit-msg` and `pre-rebase`, and
//! `--no-verify` skips them; `grit commit` reports `prepare-commit-msg` (issue #2201).

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

const GRIT: &str = env!("CARGO_BIN_EXE_grit");

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@e.x")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@e.x")
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn grit(dir: &Path, args: &[&str]) -> Output {
    Command::new(GRIT)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@e.x")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@e.x")
        .output()
        .expect("run grit")
}

fn write_hook(dir: &Path, name: &str, body: &str) {
    let hook = dir.join(".git/hooks").join(name);
    fs::create_dir_all(hook.parent().expect("hooks dir")).expect("mkdir");
    fs::write(&hook, format!("#!/bin/sh\n{body}\n")).expect("write hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod");
}

fn commit_file(dir: &Path, path: &str, contents: &str, msg: &str) {
    fs::write(dir.join(path), contents).expect("write");
    git(dir, &["add", path]);
    git(dir, &["commit", "-qm", msg]);
}

/// `main` and `f` each have one commit on top of `a` (the issue's reproduction).
fn diverged() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    commit_file(dir, "a", "1\n", "a");
    git(dir, &["switch", "-qc", "f"]);
    commit_file(dir, "f", "f\n", "f");
    git(dir, &["switch", "-q", "main"]);
    commit_file(dir, "m", "m\n", "m");
    tmp
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap_or_else(|err| {
        panic!(
            "{err}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

#[test]
fn refused_merge_keeps_the_branch_and_records_the_merge() {
    let repo = diverged();
    let dir = repo.path();
    write_hook(dir, "pre-merge-commit", "echo blocked >&2; exit 1");
    let main = git(dir, &["rev-parse", "main"]);

    let out = grit(dir, &["merge", "f", "-m", "WIP merge", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let err = json(&out);
    assert_eq!(err["kind"], "hook_failed");
    assert_eq!(err["hook"]["name"], "pre-merge-commit");
    assert_eq!(err["hook"]["stderr"], "blocked\n");
    assert!(
        err["hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("grit merge --continue")),
        "{err}"
    );
    assert_eq!(git(dir, &["rev-parse", "main"]), main);
    assert_eq!(
        git(dir, &["rev-parse", "MERGE_HEAD"]),
        git(dir, &["rev-parse", "f"])
    );

    let out = grit(dir, &["merge", "--continue", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(git(dir, &["rev-parse", "main^1"]), main);
    assert_eq!(git(dir, &["log", "-1", "--format=%s"]), "WIP merge");
}

#[test]
fn merge_no_verify_skips_pre_merge_commit_and_commit_msg() {
    let repo = diverged();
    let dir = repo.path();
    write_hook(dir, "pre-merge-commit", "exit 1");
    write_hook(dir, "commit-msg", "exit 1");
    write_hook(dir, "prepare-commit-msg", "echo PREPARED >> \"$1\"");

    let out = grit(
        dir,
        &["merge", "f", "-m", "merge f", "--no-verify", "--json"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(json(&out)["result"], "merged");
    assert_eq!(git(dir, &["log", "-1", "--format=%B"]), "merge f\nPREPARED");
    assert!(!dir.join(".git/MERGE_HEAD").exists());
}

#[test]
fn commit_json_lists_prepare_commit_msg() {
    let repo = diverged();
    let dir = repo.path();
    write_hook(dir, "prepare-commit-msg", "echo PREPARED >> \"$1\"");
    fs::write(dir.join("z"), "z\n").expect("write");

    let out = grit(dir, &["commit", "-m", "normal thing", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        json(&out)["hooks_run"],
        serde_json::json!(["prepare-commit-msg"])
    );
    assert_eq!(
        git(dir, &["log", "-1", "--format=%B"]),
        "normal thing\nPREPARED"
    );
}

#[test]
fn rebase_runs_pre_rebase_on_every_path_unless_no_verify() {
    for extra in [&[][..], &["-X", "ours"][..], &["--exec", "true"][..]] {
        let repo = diverged();
        let dir = repo.path();
        git(dir, &["switch", "-q", "f"]);
        let log = dir.join(".git/pre-rebase.log");
        write_hook(
            dir,
            "pre-rebase",
            &format!("echo \"$# $*\" >> '{}'; exit 1", log.display()),
        );
        let f = git(dir, &["rev-parse", "f"]);

        let mut args = vec!["rebase", "main"];
        args.extend_from_slice(extra);
        let out = grit(dir, &args);
        assert_eq!(out.status.code(), Some(1), "{extra:?}");
        assert_eq!(fs::read_to_string(&log).expect("log"), "1 main\n");
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), f, "{extra:?}");
        assert!(!dir.join(".git/rebase-merge").exists());

        args.push("--no-verify");
        let out = grit(dir, &args);
        assert!(
            out.status.success(),
            "{extra:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            git(dir, &["rev-parse", "HEAD~1"]),
            git(dir, &["rev-parse", "main"])
        );
        assert_eq!(fs::read_to_string(&log).expect("log"), "1 main\n");
    }
}

/// `topic` modifies `g` while `main` deletes it: replaying `topic` conflicts even with `-X ours`.
fn modify_delete_topic() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main"]);
    commit_file(dir, "g", "1\n", "base");
    git(dir, &["switch", "-qc", "topic"]);
    commit_file(dir, "g", "topic\n", "t1");
    git(dir, &["switch", "-q", "main"]);
    git(dir, &["rm", "-q", "g"]);
    git(dir, &["commit", "-qm", "drop g"]);
    git(dir, &["switch", "-q", "topic"]);
    tmp
}

#[test]
fn pre_rebase_refuses_before_a_replay_conflict_on_every_path() {
    for extra in [&[][..], &["-X", "ours"][..], &["--exec", "true"][..]] {
        let repo = modify_delete_topic();
        let dir = repo.path();
        write_hook(dir, "pre-rebase", "echo refused >&2; exit 1");
        let topic = git(dir, &["rev-parse", "topic"]);

        let mut args = vec!["rebase", "main", "--json"];
        args.extend_from_slice(extra);
        let out = grit(dir, &args);
        assert_eq!(out.status.code(), Some(1), "{extra:?}");
        let err = json(&out);
        assert_eq!(err["kind"], "hook_failed", "{extra:?}: {err}");
        assert_eq!(err["hook"]["name"], "pre-rebase");
        assert_eq!(git(dir, &["rev-parse", "HEAD"]), topic);
        assert!(!dir.join(".git/rebase-merge").exists());

        args.push("--no-verify");
        let out = grit(dir, &args);
        assert_eq!(out.status.code(), Some(1), "{extra:?}");
        assert_ne!(json(&out)["kind"], "hook_failed", "{extra:?}");
    }
}

/// The `prepare-commit-msg` source `tool` passes when committing a resolved conflict.
fn conflicted_commit_source(squash: bool, use_grit: bool) -> String {
    let repo = tempfile::tempdir().expect("tempdir");
    let dir = repo.path();
    git(dir, &["init", "-q", "-b", "main"]);
    commit_file(dir, "f", "1\n", "c1");
    git(dir, &["switch", "-qc", "side"]);
    commit_file(dir, "f", "side\n", "s1");
    git(dir, &["switch", "-q", "main"]);
    commit_file(dir, "f", "main\n", "c2");
    let mut merge = vec!["-c", "core.hooksPath=/dev/null", "merge", "-q"];
    if squash {
        merge.push("--squash");
    }
    merge.push("side");
    let status = Command::new("git")
        .args(&merge)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git merge");
    assert!(!status.status.success(), "the merge conflicts");
    fs::write(dir.join("f"), "resolved\n").expect("resolve");
    git(dir, &["add", "f"]);
    let log = dir.join(".git/prepare.log");
    write_hook(
        dir,
        "prepare-commit-msg",
        &format!("echo \"$2\" >> '{}'", log.display()),
    );
    let out = if use_grit {
        grit(dir, &["commit", "--staged"])
    } else {
        Command::new("git")
            .args(["commit", "-q", "--no-edit"])
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git commit")
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    fs::read_to_string(&log).expect("log").trim().to_owned()
}

#[test]
fn committing_a_conflicted_merge_or_squash_passes_its_source_like_git() {
    for (squash, source) in [(false, "merge"), (true, "squash")] {
        assert_eq!(
            conflicted_commit_source(squash, false),
            source,
            "system git reference changed"
        );
        assert_eq!(conflicted_commit_source(squash, true), source);
    }
}

#[test]
fn pull_autostash_waits_while_a_hook_refused_merge_is_in_progress() {
    for finish in ["--continue", "--abort"] {
        let upstream = diverged();
        let local = tempfile::tempdir().expect("tempdir");
        let dir = local.path();
        git(dir, &["init", "-q", "-b", "main"]);
        let url = upstream.path().to_string_lossy().into_owned();
        git(dir, &["remote", "add", "origin", &url]);
        git(dir, &["fetch", "-q", "origin"]);
        git(dir, &["reset", "-q", "--hard", "origin/main~1"]);
        git(dir, &["branch", "-q", "-u", "origin/main"]);
        commit_file(dir, "l", "l\n", "local");
        fs::write(dir.join("a"), "local edit\n").expect("edit");
        write_hook(dir, "pre-merge-commit", "exit 1");

        let out = grit(dir, &["pull", "--no-rebase", "--autostash", "--json"]);
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(json(&out)["kind"], "hook_failed");
        assert!(dir.join(".git/MERGE_HEAD").exists());
        assert_eq!(
            fs::read_to_string(dir.join("a")).expect("a"),
            "1\n",
            "the stashed edit stays out of the merge"
        );

        let out = grit(dir, &["merge", finish]);
        assert!(
            out.status.success(),
            "{finish}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            fs::read_to_string(dir.join("a")).expect("a"),
            "local edit\n",
            "{finish} puts the edit back"
        );
        assert!(!dir.join(".git/MERGE_AUTOSTASH").exists());
        if finish == "--continue" {
            assert_eq!(git(dir, &["show", "HEAD:a"]), "1");
            assert_eq!(
                git(dir, &["log", "-1", "--format=%P"])
                    .split_whitespace()
                    .count(),
                2
            );
        } else {
            assert!(!dir.join(".git/MERGE_HEAD").exists());
        }
    }
}

#[test]
fn rebase_dry_run_and_check_run_no_pre_rebase() {
    let repo = diverged();
    let dir = repo.path();
    git(dir, &["switch", "-q", "f"]);
    write_hook(dir, "pre-rebase", "exit 1");
    for flag in ["--dry-run", "--check"] {
        let out = grit(dir, &["rebase", "main", flag]);
        assert!(
            out.status.success(),
            "{flag}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
