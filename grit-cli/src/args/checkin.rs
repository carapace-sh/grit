//! Clap arguments for `grit checkin`.

use carapace_rs_clap::completer;
use clap::Args;

use crate::completions::grit;

/// Send a snapshot checkout's branches back to the server, grafted onto its real history.
#[derive(Debug, Args)]
pub(crate) struct CheckinArgs {
    /// Branches to check in (default: every branch with commits on top of the snapshot).
    #[arg(value_name = "BRANCH", add = completer!(grit::action_local_branches()))]
    pub(crate) branches: Vec<String>,
    /// Replace a server branch even where that drops commits it has.
    #[arg(short = 'f', long)]
    pub(crate) force: bool,
}
