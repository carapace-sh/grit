//! Clap arguments for `grit completions`.

use carapace_rs_clap::completer;
use clap::Args;

use crate::completions::grit;

/// Print the shell integration script that answers with live candidates.
#[derive(Debug, Args)]
pub(crate) struct CompletionsArgs {
    /// Target shell (bash, zsh, fish, nushell, powershell, elvish, xonsh, …).
    #[arg(add = completer!(grit::action_shells()))]
    pub(crate) shell: String,
}
