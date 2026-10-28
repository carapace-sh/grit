//! `reference-transaction` hook around ref store updates (issue #2220).

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use grit_lib::config::environment::RepositoryOptions;
use grit_lib::error::Error;
use grit_lib::objects::ObjectId;
use grit_lib::refs::store::{Expected, RawRef, RefUpdate, RefUpdateFlags};
use grit_lib::refs::{
    commit_ref_update_on_store, update_branch_for_commit_on_store, BranchCommitRefUpdate,
};
use grit_lib::repo::Repository;
use grit_test_support::git;

fn install_reference_transaction_hook(repo: &Repository, log_path: &Path, deny_prepared: bool) {
    let hooks = repo.git_dir().join("hooks");
    fs::create_dir_all(&hooks).expect("hooks dir");
    let deny = if deny_prepared { "1" } else { "0" };
    let script = format!(
        "#!/bin/sh\n\
         echo \"$1: $(cat | tr '\\n' '|')\" >> {log}\n\
         [ \"{deny}\" = 1 ] && [ \"$1\" = prepared ] && exit 1\n\
         exit 0\n",
        log = log_path.display()
    );
    let hook = hooks.join("reference-transaction");
    fs::write(&hook, script).expect("write hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod");
}

fn read_log(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn branch_create_runs_prepared_and_committed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let log = tmp.path().join("rt.log");
    git(tmp.path(), &["init", "-q", "-b", "main", "."]);
    git(tmp.path(), &["config", "user.email", "a@b.c"]);
    git(tmp.path(), &["config", "user.name", "n"]);
    fs::write(tmp.path().join("f"), b"x").expect("file");
    git(tmp.path(), &["add", "f"]);
    git(tmp.path(), &["commit", "-qm", "c1"]);

    let repo = Repository::discover(Some(tmp.path()), &RepositoryOptions::empty()).expect("open");
    install_reference_transaction_hook(&repo, &log, false);

    let tip = repo.refs().resolve("HEAD").expect("head");
    commit_ref_update_on_store(
        repo.refs(),
        RefUpdate {
            name: "refs/heads/topic".into(),
            new_value: Some(RawRef::Direct(tip)),
            expected: Expected::Missing,
            reflog: None,
            flags: RefUpdateFlags::default(),
        },
    )
    .expect("create branch");

    let text = read_log(&log);
    assert!(text.contains("prepared:"), "{text}");
    assert!(text.contains("committed:"), "{text}");
    assert!(text.contains("refs/heads/topic"), "{text}");
    assert!(
        text.contains(&format!(
            "{} {}",
            ObjectId::null(repo.odb().hash_algo()).to_hex(),
            tip.to_hex()
        )),
        "{text}"
    );
}

#[test]
fn prepared_refusal_leaves_ref_unchanged_and_logs_aborted() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let log = tmp.path().join("rt.log");
    git(tmp.path(), &["init", "-q", "-b", "main", "."]);
    git(tmp.path(), &["config", "user.email", "a@b.c"]);
    git(tmp.path(), &["config", "user.name", "n"]);
    fs::write(tmp.path().join("f"), b"x").expect("file");
    git(tmp.path(), &["add", "f"]);
    git(tmp.path(), &["commit", "-qm", "c1"]);

    let repo = Repository::discover(Some(tmp.path()), &RepositoryOptions::empty()).expect("open");
    install_reference_transaction_hook(&repo, &log, true);

    let tip = repo.refs().resolve("HEAD").expect("head");
    let err = commit_ref_update_on_store(
        repo.refs(),
        RefUpdate {
            name: "refs/heads/blocked".into(),
            new_value: Some(RawRef::Direct(tip)),
            expected: Expected::Missing,
            reflog: None,
            flags: RefUpdateFlags::default(),
        },
    )
    .expect_err("hook blocks");
    assert!(matches!(err, Error::Hook(_)), "{err}");
    assert!(repo
        .refs()
        .read_raw("refs/heads/blocked")
        .expect("read")
        .is_none());
    let text = read_log(&log);
    assert!(text.contains("prepared:"), "{text}");
    assert!(text.contains("aborted:"), "{text}");
    assert!(!text.contains("committed:"), "{text}");
}

#[test]
fn files_backend_commit_emits_head_and_branch_lines() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let log = tmp.path().join("rt.log");
    git(tmp.path(), &["init", "-q", "-b", "main", "."]);
    git(tmp.path(), &["config", "user.email", "a@b.c"]);
    git(tmp.path(), &["config", "user.name", "n"]);
    fs::write(tmp.path().join("f"), b"x").expect("file");
    git(tmp.path(), &["add", "f"]);
    git(tmp.path(), &["commit", "-qm", "c1"]);

    let repo = Repository::discover(Some(tmp.path()), &RepositoryOptions::empty()).expect("open");
    install_reference_transaction_hook(&repo, &log, false);

    git(tmp.path(), &["commit", "--allow-empty", "-qm", "c2"]);
    let new_commit = repo.refs().resolve("HEAD").expect("new tip");
    git(tmp.path(), &["reset", "--hard", "HEAD~1"]);
    let old = repo.refs().resolve("HEAD").expect("old tip");

    update_branch_for_commit_on_store(
        repo.refs(),
        &BranchCommitRefUpdate {
            branch_ref: "refs/heads/main",
            expected_old: Some(old),
            new_oid: new_commit,
            identity: "T <t@e.x> 1700000000 +0000",
            reflog_message: "commit: c2",
        },
        &repo.config().expect("config"),
    )
    .expect("advance branch");

    let text = read_log(&log);
    assert!(text.contains("HEAD|"), "{text}");
    assert!(text.contains("refs/heads/main|"), "{text}");
    assert!(text.contains(&format!("{old} {new_commit} HEAD")), "{text}");
}

#[test]
fn core_hooks_path_is_honored() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    let work = root.join("work");
    fs::create_dir_all(&work).expect("mkdir");
    git(&work, &["init", "-q", "-b", "main", "."]);
    git(&work, &["config", "user.email", "a@b.c"]);
    git(&work, &["config", "user.name", "n"]);
    let hooks = root.join("alt-hooks");
    fs::create_dir_all(&hooks).expect("hooks");
    let log = root.join("rt.log");
    git(&work, &["config", "core.hooksPath", "../alt-hooks"]);
    let script = format!("#!/bin/sh\n echo prepared >> {}\n exit 0\n", log.display());
    let hook = hooks.join("reference-transaction");
    fs::write(&hook, script).expect("hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod");
    fs::write(work.join("f"), b"x").expect("file");
    git(&work, &["add", "f"]);
    git(&work, &["commit", "-qm", "c1"]);

    let repo = Repository::discover(Some(&work), &RepositoryOptions::empty()).expect("open");
    let tip = repo.refs().resolve("HEAD").expect("head");
    commit_ref_update_on_store(
        repo.refs(),
        RefUpdate {
            name: "refs/heads/x".into(),
            new_value: Some(RawRef::Direct(tip)),
            expected: Expected::Missing,
            reflog: None,
            flags: RefUpdateFlags::default(),
        },
    )
    .expect("branch");
    assert!(log.exists(), "hook under core.hooksPath should run");
}
