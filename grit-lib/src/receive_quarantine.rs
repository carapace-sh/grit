//! Temporary object directories for receive-pack (Git quarantine).
//!
//! Incoming push packs are unpacked into a quarantined `objects/` tree that lists
//! the repository's main object directory as an alternate. Hooks see
//! `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`, and
//! `GIT_QUARANTINE_PATH`. On success the quarantine contents are migrated into
//! the main store; on failure the directory is removed.

use crate::git_path::CanonicalizeExt;
use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Error, Result};
use crate::odb::Odb;

static QUARANTINE_SEQ: AtomicU64 = AtomicU64::new(0);

/// A receive-pack quarantine directory under the repository's `objects/`.
pub struct ReceiveQuarantine {
    path: PathBuf,
    main_objects: PathBuf,
    config_git_dir: Option<PathBuf>,
    migrated: bool,
}

impl ReceiveQuarantine {
    /// Create `tmp_objdir-incoming-*` under the repository's primary `objects/` tree.
    ///
    /// `source` supplies the hash algorithm and optional config git dir so pack
    /// ingestion matches the receiving repository (including SHA-256).
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedObjectStore`] when `source` is not files-backed,
    /// or [`Error::Io`] when the directory cannot be created.
    pub fn create(source: &Odb, git_dir: &Path) -> Result<Self> {
        let main_objects = source
            .files_objects_dir()
            .ok_or(Error::UnsupportedObjectStore {
                operation: "receive quarantine",
            })?
            .to_path_buf();
        let seq = QUARANTINE_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = main_objects.join(format!(
            "tmp_objdir-incoming-{}-{}",
            std::process::id(),
            seq
        ));
        fs::create_dir_all(path.join("pack")).map_err(Error::Io)?;
        Ok(Self {
            path,
            main_objects,
            config_git_dir: Some(git_dir.to_path_buf()),
            migrated: false,
        })
    }

    /// Object database that reads/writes the quarantine and falls back to alternates.
    #[must_use]
    pub fn odb(&self) -> Odb {
        let mut odb = Odb::new(&self.path).with_env_alternate_dirs(vec![self.main_objects.clone()]);
        if let Some(git_dir) = &self.config_git_dir {
            odb = odb.with_config_git_dir(git_dir.clone());
        }
        odb
    }

    /// Quarantine `objects/` root (for tests and pack staging).
    #[cfg(test)]
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Environment pairs for hook subprocesses (absolute paths when possible).
    #[must_use]
    pub fn hook_env(&self) -> Vec<(String, String)> {
        let quarantine = abs_or_path(&self.path);
        let main = format_alternate_object_directories(&abs_or_path(&self.main_objects));
        vec![
            ("GIT_OBJECT_DIRECTORY".to_owned(), quarantine.clone()),
            ("GIT_ALTERNATE_OBJECT_DIRECTORIES".to_owned(), main),
            ("GIT_QUARANTINE_PATH".to_owned(), quarantine),
        ]
    }

    /// Move loose objects and packfiles from the quarantine into the main object store.
    ///
    /// Newly installed `pack-*` files get an empty sibling `.keep` marker so concurrent
    /// maintenance cannot delete them before ref updates from this push are published.
    /// Drop the returned [`MigratedPackKeepGuard`] after ref publication (or push failure
    /// handling) to remove those markers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] when migration fails.
    pub fn migrate(&mut self) -> Result<MigratedPackKeepGuard> {
        let keep_markers = migrate_tree(&self.path, &self.main_objects)?;
        self.migrated = true;
        remove_quarantine_dir(&self.path)?;
        Ok(MigratedPackKeepGuard { keep_markers })
    }
}

/// Temporary `.keep` markers on packs migrated out of a receive quarantine.
///
/// While held, garbage collection and repack treat those packs as pinned. Drop after
/// ref updates from the push are committed.
pub(crate) struct MigratedPackKeepGuard {
    keep_markers: Vec<PathBuf>,
}

impl MigratedPackKeepGuard {
    fn clear_keeps(&mut self) {
        for keep in &self.keep_markers {
            let _ = fs::remove_file(keep);
        }
        self.keep_markers.clear();
    }
}

impl Drop for MigratedPackKeepGuard {
    fn drop(&mut self) {
        self.clear_keeps();
    }
}

impl Drop for ReceiveQuarantine {
    fn drop(&mut self) {
        if !self.migrated {
            let _ = remove_quarantine_dir(&self.path);
        }
    }
}

fn abs_or_path(path: &Path) -> String {
    path.canonicalize_simplified()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Format one path for `GIT_ALTERNATE_OBJECT_DIRECTORIES` (Git `env_append` quoting).
fn format_alternate_object_directories(path: &str) -> String {
    match quote_git_env_path(path) {
        Cow::Borrowed(p) => p.to_owned(),
        Cow::Owned(p) => p,
    }
}

fn quote_git_env_path(path: &str) -> Cow<'_, str> {
    if !path.contains(':') && !path.contains('"') {
        return Cow::Borrowed(path);
    }
    let mut out = String::from('"');
    for ch in path.chars() {
        match ch {
            '"' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            c if c.is_control() => {
                let b = c as u32;
                out.push_str(&format!("\\{b:03o}"));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    Cow::Owned(out)
}

fn remove_quarantine_dir(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(Error::Io)?;
    }
    Ok(())
}

fn migrate_tree(src: &Path, dst: &Path) -> Result<Vec<PathBuf>> {
    if !src.is_dir() {
        return Ok(Vec::new());
    }
    fs::create_dir_all(dst).map_err(Error::Io)?;
    let mut names: Vec<String> = fs::read_dir(src)
        .map_err(Error::Io)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort_by_key(|name| pack_copy_priority(name));
    let mut keep_markers = Vec::new();
    for name in names {
        let from = src.join(&name);
        let to = dst.join(&name);
        let meta = fs::symlink_metadata(&from).map_err(Error::Io)?;
        if meta.is_dir() {
            keep_markers.extend(migrate_tree(&from, &to)?);
        } else if migrate_file(&from, &to)? {
            if let Some(keep) = keep_marker_for_newly_migrated_pack(&to) {
                fs::write(&keep, b"").map_err(Error::Io)?;
                keep_markers.push(keep);
            }
        }
    }
    Ok(keep_markers)
}

fn keep_marker_for_newly_migrated_pack(pack_path: &Path) -> Option<PathBuf> {
    let name = pack_path.file_name()?.to_str()?;
    if !(name.starts_with("pack-") && name.ends_with(".pack")) {
        return None;
    }
    let stem = pack_path.file_stem()?.to_str()?;
    Some(pack_path.with_file_name(format!("{stem}.keep")))
}

fn pack_copy_priority(name: &str) -> u8 {
    if !name.starts_with("pack-") && name != "pack" {
        return 0;
    }
    if name.ends_with(".keep") {
        1
    } else if name.ends_with(".pack") {
        2
    } else if name.ends_with(".rev") {
        3
    } else if name.ends_with(".idx") {
        4
    } else {
        5
    }
}

fn migrate_file(from: &Path, to: &Path) -> Result<bool> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(Error::Io)?;
    }
    if to.exists() {
        return Ok(false);
    }
    if fs::rename(from, to).is_err() {
        fs::copy(from, to).map_err(Error::Io)?;
        fs::remove_file(from).map_err(Error::Io)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::ObjectKind;

    #[test]
    fn quarantine_ingest_and_migrate_makes_objects_visible_in_main() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let main = tmp.path().join("objects");
        fs::create_dir_all(&main).unwrap();
        let main_odb = Odb::new(&main);
        let git_dir = tmp.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        let mut q = ReceiveQuarantine::create(&main_odb, &git_dir).expect("quarantine");
        let oid = q
            .odb()
            .write(ObjectKind::Blob, b"quarantined")
            .expect("write");
        assert!(q.odb().exists(&oid));
        assert!(!Odb::new(&main).exists(&oid));
        let _keeps = q.migrate().expect("migrate");
        assert!(Odb::new(&main).exists(&oid));
    }

    #[test]
    fn migrate_creates_keep_marker_for_new_pack() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let main = tmp.path().join("objects");
        fs::create_dir_all(main.join("pack")).unwrap();
        let git_dir = tmp.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        let main_odb = Odb::new(&main);
        let mut q = ReceiveQuarantine::create(&main_odb, &git_dir).expect("quarantine");
        let pack_body = b"mock pack bytes";
        let pack_path = q.path().join("pack/pack-deadbeef.pack");
        fs::write(&pack_path, pack_body).unwrap();
        let guard = q.migrate().expect("migrate");
        let keep = main.join("pack/pack-deadbeef.keep");
        assert!(
            keep.is_file(),
            "migrate must pin new pack until guard drops"
        );
        drop(guard);
        assert!(
            !keep.is_file(),
            "keep marker must be removed when guard drops"
        );
    }

    #[cfg_attr(
        not(feature = "full-tests"),
        ignore = "starts system git or is heavy; run with --features full-tests"
    )]
    #[test]
    fn quarantine_odb_matches_repository_hash_algorithm() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let git_dir = tmp.path().join("repo.git");
        std::fs::create_dir_all(&git_dir).unwrap();
        let init = std::process::Command::new("git")
            .current_dir(&git_dir)
            .args(["init", "-q", "--bare", "--object-format=sha256", "."])
            .status();
        if init.map(|s| !s.success()).unwrap_or(true) {
            return;
        }
        let objects = git_dir.join("objects");
        let source = Odb::new(&objects).with_config_git_dir(git_dir.clone());
        assert_eq!(source.hash_algo(), crate::objects::HashAlgo::Sha256);
        let q = ReceiveQuarantine::create(&source, &git_dir).expect("quarantine");
        assert_eq!(q.odb().hash_algo(), crate::objects::HashAlgo::Sha256);
    }

    #[test]
    fn alternate_env_quotes_paths_containing_colons() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let main = tmp.path().join("obj:ects");
        std::fs::create_dir_all(&main).unwrap();
        let git_dir = tmp.path().join("repo.git");
        std::fs::create_dir_all(&git_dir).unwrap();
        let source = Odb::new(&main).with_config_git_dir(git_dir.clone());
        let q = ReceiveQuarantine::create(&source, &git_dir).expect("quarantine");
        let alt = q
            .hook_env()
            .into_iter()
            .find(|(k, _)| k == "GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .map(|(_, v)| v)
            .expect("alt env");
        assert!(
            alt.starts_with('"') && alt.ends_with('"'),
            "colon in path must be quoted: {alt}"
        );
    }

    #[test]
    fn drop_removes_quarantine_without_migrating() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let main = tmp.path().join("objects");
        fs::create_dir_all(&main).unwrap();
        let main_odb = Odb::new(&main);
        let git_dir = tmp.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        let quarantine_path = {
            let q = ReceiveQuarantine::create(&main_odb, &git_dir).expect("quarantine");
            let oid = q.odb().write(ObjectKind::Blob, b"x").expect("write");
            let path = q
                .hook_env()
                .into_iter()
                .find(|(k, _)| k == "GIT_QUARANTINE_PATH")
                .map(|(_, v)| PathBuf::from(v))
                .expect("quarantine path");
            drop(q);
            assert!(!path.exists());
            assert!(!Odb::new(&main).exists(&oid));
            path
        };
        let _ = quarantine_path;
    }
}
