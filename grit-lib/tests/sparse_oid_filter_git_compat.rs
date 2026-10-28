//! `sparse:oid` partial-clone filters must match system `git rev-list --objects --filter`.

#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::Path;
use std::process::Command;

use grit_lib::config::environment::RepositoryOptions;
use grit_lib::repo::Repository;
use grit_lib::revwalk::rev_list::{ObjectFilter, RevListOptions};

fn git(args: &[&str], cwd: &Path) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn sparse_fixture(dir: &Path) -> String {
    let work = dir.join("work");
    std::fs::create_dir_all(work.join("d1/sub")).unwrap();
    std::fs::create_dir_all(work.join("src/deep")).unwrap();
    std::fs::create_dir_all(work.join("other")).unwrap();
    for (path, contents) in [
        ("a.txt", "a.txt\n"),
        ("d1/x", "d1/x\n"),
        ("d1/sub/y", "d1/sub/y\n"),
        ("src/z", "src/z\n"),
        ("src/deep/w", "src/deep/w\n"),
        ("other/q", "other/q\n"),
    ] {
        std::fs::write(work.join(path), contents).unwrap();
    }
    git(&["init", "-q"], &work);
    git(&["add", "-A"], &work);
    git(
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "one",
        ],
        &work,
    );
    let bare = dir.join("bare.git");
    git(
        &[
            "clone",
            "-q",
            "--bare",
            work.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
        dir,
    );
    bare.to_str().unwrap().to_string()
}

fn sparse_spec_oid(bare: &Path, patterns: &str) -> String {
    let mut child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(bare)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(patterns.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn git_blob_paths(bare: &Path, patterns: &str) -> Vec<String> {
    let oid = sparse_spec_oid(bare, patterns);
    let filter = format!("sparse:oid={oid}");
    let out = Command::new("git")
        .args([
            "rev-list",
            "--objects",
            &format!("--filter={filter}"),
            "HEAD",
        ])
        .current_dir(bare)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(out.status.success());
    blob_paths_from_rev_list_lines(bare, &out.stdout)
}

fn blob_paths_from_rev_list_lines(bare: &Path, stdout: &[u8]) -> Vec<String> {
    let mut blobs = Vec::new();
    for line in String::from_utf8_lossy(stdout).lines() {
        let Some((oid, path)) = line.split_once(' ') else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        let kind = git(&["cat-file", "-t", oid], bare);
        if kind == "blob" {
            blobs.push(path.to_string());
        }
    }
    blobs.sort();
    blobs
}

fn grit_blob_paths(bare: &Path, patterns: &str) -> Vec<String> {
    let oid = sparse_spec_oid(bare, patterns);
    let filter = ObjectFilter::parse(&format!("sparse:oid={oid}")).unwrap();
    let repo = Repository::open(bare, &RepositoryOptions::empty()).unwrap();
    let head = git(&["rev-parse", "HEAD"], bare);
    let opts = RevListOptions {
        objects: true,
        quiet: true,
        filter: Some(filter),
        ..Default::default()
    };
    let result = grit_lib::revwalk::rev_list::rev_list(&repo, &[head], &[], &opts).unwrap();
    let mut lines = String::new();
    for (oid, path) in result.objects {
        if path.is_empty() {
            continue;
        }
        lines.push_str(&oid.to_hex());
        lines.push(' ');
        lines.push_str(&path);
        lines.push('\n');
    }
    blob_paths_from_rev_list_lines(bare, lines.as_bytes())
}

#[test]
fn sparse_oid_non_cone_src_nested_matches_git() {
    let tmp = tempfile::tempdir().unwrap();
    let bare = sparse_fixture(tmp.path());
    let bare_path = Path::new(&bare);
    let patterns = "/*\n!/*/\n/src/\n";
    assert_eq!(
        grit_blob_paths(bare_path, patterns),
        git_blob_paths(bare_path, patterns),
        "blob paths for /* + !/*/ + /src/"
    );
}

#[test]
fn sparse_oid_non_cone_d1_and_star_neg_other_matches_git() {
    let tmp = tempfile::tempdir().unwrap();
    let bare = sparse_fixture(tmp.path());
    let bare_path = Path::new(&bare);
    for patterns in ["/*\n!/*/\n/src/\n/d1/\n", "*\n!other\n"] {
        assert_eq!(
            grit_blob_paths(bare_path, patterns),
            git_blob_paths(bare_path, patterns),
            "blob paths for {patterns:?}"
        );
    }
}
