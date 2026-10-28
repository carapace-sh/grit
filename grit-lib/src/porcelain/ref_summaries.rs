//! Ref names paired with the subject line of the commit they resolve to.
//!
//! This is the data behind `for-each-ref`-style listings that show a short
//! description next to each ref name: branch pickers, tag browsers, and shell
//! completion menus. The library returns plain values; callers decide how to
//! lay them out and colour them.

use crate::commit_pretty::message_subject;
use crate::diff_rename_parallel::parallel_map_indices;
use crate::error::{Error, Result};
use crate::objects::{parse_commit, ObjectId, ObjectKind};
use crate::parallel::ParallelOp;
use crate::refs::listing::{
    query_refs, ref_query_options_for_repo, RefKind, RefNamespace, RefQuery, RefQueryError,
};
use crate::repo::Repository;

/// One ref together with a short description of what it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefSummary {
    /// Short ref name within its namespace (for example `main` or `origin/main`).
    pub name: String,
    /// Full ref name (for example `refs/heads/main`).
    pub full_name: String,
    /// Object id stored at the ref (the tag object id for annotated tags).
    pub oid: ObjectId,
    /// Namespace the ref belongs to.
    pub kind: RefKind,
    /// First line of the tip commit's message, or `None` when the ref does not
    /// resolve to a commit or that commit has an empty message.
    pub subject: Option<String>,
}

/// List refs in `namespaces`, sorted by name, with the subject of each tip.
///
/// Annotated tags report the subject of the commit they peel to, so a tag and a
/// branch pointing at the same commit read alike. A ref whose object cannot be
/// read keeps its name and [`RefSummary::oid`] with an empty
/// [`RefSummary::subject`] instead of failing the whole listing.
///
/// # Errors
///
/// Propagates ref-store and config read failures. Missing objects are reported
/// through an empty subject, not as an error.
///
/// # Examples
///
/// ```no_run
/// use grit_lib::porcelain::ref_summaries::list_ref_summaries;
/// use grit_lib::refs::listing::RefNamespace;
/// # fn demo(repo: &grit_lib::repo::Repository) -> grit_lib::error::Result<()> {
/// let branches = list_ref_summaries(repo, &[RefNamespace::Heads])?;
/// for branch in branches {
///     println!("{} {}", branch.name, branch.subject.unwrap_or_default());
/// }
/// # Ok(())
/// # }
/// ```
pub fn list_ref_summaries(
    repo: &Repository,
    namespaces: &[RefNamespace],
) -> Result<Vec<RefSummary>> {
    let query = RefQuery {
        namespaces: namespaces.to_vec(),
        ..RefQuery::default()
    };
    let options = ref_query_options_for_repo(repo)?;
    let listings = query_refs(repo, &query, &options).map_err(ref_query_error)?;
    if listings.is_empty() {
        return Ok(Vec::new());
    }
    let threads = repo
        .thread_policy()
        .resolve(ParallelOp::DiffContent, repo.config()?.as_ref());
    let odb = repo.odb();
    odb.with_pack_read_context(|| {
        parallel_map_indices(
            odb,
            listings.len(),
            threads,
            listings.len() * 256,
            |index| {
                let listing = &listings[index];
                Ok::<_, Error>(RefSummary {
                    name: listing.short_name.clone(),
                    full_name: listing.full_name.clone(),
                    oid: listing.oid,
                    kind: listing.kind,
                    subject: tip_subject(repo, listing.oid),
                })
            },
        )
    })
}

fn ref_query_error(error: RefQueryError) -> Error {
    match error {
        RefQueryError::Store(error) => error,
        other => Error::Message(other.to_string()),
    }
}

/// Subject line for the commit `oid` resolves to, reading through tags.
fn tip_subject(repo: &Repository, oid: ObjectId) -> Option<String> {
    let odb = repo.odb();
    let kind = odb.read_info(&oid).ok()?.kind;
    let commit = match kind {
        ObjectKind::Commit => oid,
        ObjectKind::Tag => odb.peel_to_commit(oid).ok().flatten()?,
        ObjectKind::Tree | ObjectKind::Blob => return None,
    };
    let object = odb.read(&commit).ok()?;
    let parsed = parse_commit(&object.data).ok()?;
    let subject = message_subject(&parsed.message);
    (!subject.is_empty()).then_some(subject)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::porcelain::commit::{create_commit, CommitRequest};
    use crate::porcelain::tag::{create_tag, CreateTagRequest};
    use crate::progress::{NeverCancel, NullProgress};
    use crate::repo::InitOptions;

    fn repo_with_commit() -> (tempfile::TempDir, Repository, ObjectId) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = Repository::init(tmp.path(), &InitOptions::default()).expect("init");
        let req = CommitRequest {
            message: "first subject\n\nbody\n".into(),
            author: Some(
                "T <t@example.com> 1700000000 +0000"
                    .parse()
                    .expect("signature"),
            ),
            committer: Some(
                "T <t@example.com> 1700000000 +0000"
                    .parse()
                    .expect("signature"),
            ),
            allow_empty: true,
            sign_override: None,
            amend: false,
            assume_worktree_synced: false,
            skip_pre_commit_tree_match: false,
            ..Default::default()
        };
        let oid = create_commit(&repo, &req, &mut NullProgress, &NeverCancel)
            .expect("commit")
            .oid;
        (tmp, repo, oid)
    }

    fn branch(repo: &Repository, name: &str, oid: ObjectId) {
        let name = crate::refs::store::RefName::try_from(format!("refs/heads/{name}").as_str())
            .expect("valid ref");
        repo.references().update(&name, &oid).expect("write branch");
    }

    #[test]
    fn lists_branches_with_their_subject() {
        let (_tmp, repo, oid) = repo_with_commit();
        branch(&repo, "topic", oid);

        let summaries = list_ref_summaries(&repo, &[RefNamespace::Heads]).expect("summaries");

        assert_eq!(
            summaries
                .iter()
                .map(|summary| (summary.name.as_str(), summary.subject.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("main", Some("first subject")),
                ("topic", Some("first subject"))
            ]
        );
        assert_eq!(summaries[1].full_name, "refs/heads/topic");
        assert_eq!(summaries[1].kind, RefKind::Head);
        assert_eq!(summaries[1].oid, oid);
    }

    #[test]
    fn annotated_tags_describe_the_peeled_commit() {
        let (_tmp, repo, oid) = repo_with_commit();
        create_tag(
            &repo,
            &CreateTagRequest {
                name: "v1".into(),
                target: oid,
                message: Some("release one\n".into()),
                tagger: Some("T <t@example.com> 1700000000 +0000".into()),
                sign: None,
                signing_key: None,
                tree_sha256: None,
                force: false,
                dry_run: false,
            },
        )
        .expect("create tag");

        let summaries = list_ref_summaries(&repo, &[RefNamespace::Tags]).expect("summaries");

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "v1");
        assert_eq!(summaries[0].kind, RefKind::Tag);
        assert_eq!(summaries[0].subject.as_deref(), Some("first subject"));
    }

    #[test]
    fn merged_namespaces_are_sorted_by_name() {
        let (_tmp, repo, oid) = repo_with_commit();
        branch(&repo, "alpha", oid);
        branch(&repo, "zulu", oid);

        let summaries =
            list_ref_summaries(&repo, &[RefNamespace::Heads, RefNamespace::Tags]).expect("list");

        let names: Vec<&str> = summaries
            .iter()
            .map(|summary| summary.name.as_str())
            .collect();
        assert_eq!(names, vec!["alpha", "main", "zulu"]);
    }

    #[test]
    fn empty_repository_lists_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = Repository::init(tmp.path(), &InitOptions::default()).expect("init");

        assert!(list_ref_summaries(&repo, &[RefNamespace::Heads])
            .expect("list")
            .is_empty());
    }
}
