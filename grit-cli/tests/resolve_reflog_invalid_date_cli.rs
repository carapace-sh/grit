//! `grit resolve` surfaces invalid reflog date selectors (issue #2224).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn grit(cwd: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_grit");
    let out = Command::new(bin)
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("spawn grit");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn resolve_invalid_reflog_date_not_unknown_revision() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = Command::new("git")
        .current_dir(dir.path())
        .args(["init", "-q", "-b", "main"])
        .output()
        .expect("git init");
    assert!(out.status.success());
    let out = Command::new("git")
        .current_dir(dir.path())
        .args(["commit", "--allow-empty", "-q", "-m", "seed"])
        .output()
        .expect("git commit");
    assert!(out.status.success());

    let (code, stdout, stderr) = grit(dir.path(), &["resolve", "main@{definitely-not-a-date}"]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(
        stderr.contains("invalid reflog date 'definitely-not-a-date'"),
        "stderr: {stderr:?}"
    );
    assert!(
        !stderr.contains("unknown revision"),
        "must not erase typed date error: {stderr:?}"
    );
}
