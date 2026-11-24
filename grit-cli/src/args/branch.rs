//! Clap arguments for `grit branch`.

use carapace_rs_clap::completer;
use clap::Args;

use crate::branch_list_args;
use crate::completions::grit;

/// List branches, or create / delete / rename / configure one.
#[derive(Debug, Args)]
pub(crate) struct BranchArgs {
    /// Branch to create, delete, rename, or configure upstream for. Omit to list.
    #[arg(add = completer!(grit::action_local_branches()))]
    pub(crate) name: Option<String>,
    /// Start commit when creating (`grit branch <name> <start>`).
    #[arg(add = completer!(grit::action_revisions()))]
    pub(crate) start: Option<String>,
    /// Overwrite an existing branch when creating or renaming.
    #[arg(short = 'f', long = "force", conflicts_with = "force_rename")]
    pub(crate) force: bool,
    /// Configure upstream from the start point when creating.
    #[arg(short = 't', long = "track", conflicts_with = "no_track")]
    pub(crate) track: bool,
    /// Do not configure upstream when creating.
    #[arg(long = "no-track", conflicts_with = "track")]
    pub(crate) no_track: bool,
    /// Set upstream for the named branch (or the current branch when omitted).
    #[arg(short = 'u', long = "set-upstream-to", value_name = "UPSTREAM", add = completer!(grit::action_remote_branches()))]
    pub(crate) set_upstream_to: Option<String>,
    /// Remove upstream configuration for the named branch (or the current branch).
    #[arg(long = "unset-upstream")]
    pub(crate) unset_upstream: bool,
    /// Rename a branch: `-m <new>` or `-m <old> <new>`.
    #[arg(
            short = 'm',
            long = "move",
            value_name = "NAME",
            num_args = 1..=2,
            conflicts_with = "force_rename"
        )]
    pub(crate) rename: Option<Vec<String>>,
    /// Force rename: `-M <new>` or `-M <old> <new>`.
    #[arg(short = 'M', value_name = "NAME", num_args = 1..=2, conflicts_with = "rename")]
    pub(crate) force_rename: Option<Vec<String>>,
    /// Delete the named branch instead of creating it.
    #[arg(short = 'd', long = "delete")]
    pub(crate) delete: bool,
    /// Delete even when the branch is not fully merged into the current branch.
    #[arg(short = 'D', long = "force-delete")]
    pub(crate) force_delete: bool,
    #[command(flatten)]
    pub(crate) list: branch_list_args::BranchListArgs,
    /// Preview the operation without updating refs, config, or reflogs.
    #[arg(long = "dry-run")]
    pub(crate) dry_run: bool,
}
