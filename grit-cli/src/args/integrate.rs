//! Clap arguments for `grit integrate`.

use carapace_rs_clap::completer;
use clap::Args;

use crate::completions::grit;

/// Merge a branch into the server's target branch (checking it in first), then refresh this checkout.
#[derive(Debug, Args)]
pub(crate) struct IntegrateArgs {
    /// The branch to merge.
    #[arg(value_name = "BRANCH", add = completer!(grit::action_local_branches()))]
    pub(crate) branch: String,
    /// For a shared branch: integrate without waiting for this member's
    /// changes, which stay in their copy. Repeatable.
    #[arg(long, value_name = "USER")]
    pub(crate) without: Vec<String>,
}
