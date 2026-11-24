//! Clap arguments for `grit manager`.

use carapace_rs_clap::completer;
use clap::Args;

use crate::completions::grit;

/// Git credential helper backed by the Windows Credential Manager.
///
/// You don't normally run this yourself — `grit auth` wires it into
/// `credential.helper` on Windows. It speaks Git's credential protocol.
#[derive(Debug, Args)]
pub(crate) struct ManagerArgs {
    /// The credential operation: get, store, or erase.
    #[arg(add = completer!(grit::action_credential_operations()))]
    pub(crate) operation: String,
}
