//! Clap arguments for `grit completions`.

use clap::Args;

/// Print the shell integration script that answers with live candidates.
#[derive(Debug, Args)]
pub(crate) struct CompletionsArgs {
    /// Target shell (bash, zsh, fish, nushell, powershell, elvish, xonsh, …).
    pub(crate) shell: String,
}
