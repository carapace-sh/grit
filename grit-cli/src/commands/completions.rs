//! `grit completions` — emit the shell integration script for dynamic completion.

use anyhow::{anyhow, Result};

/// Write the completion script for `shell` to stdout.
///
/// The script registers a shell function that asks `grit _carapace` for
/// candidates, so completion covers branch names, tags, remotes, paths and
/// flags without regenerating anything after a `grit` upgrade.
pub fn run(shell: &str) -> Result<()> {
    let shell = crate::completions::parse_shell(shell).map_err(|error| anyhow!(error))?;
    print!("{}", crate::completions::snippet(shell));
    Ok(())
}

/// Dispatch parsed CLI args for `Completions`.
pub fn dispatch(
    args: crate::args::CompletionsArgs,
    env: crate::dispatch::CommandEnv<'_>,
) -> anyhow::Result<crate::exit_status::ExitStatus> {
    use crate::dispatch::CommandEnv;
    use crate::exit_status::ExitStatus;

    let CommandEnv {
        opts: _,
        ctx: _,
        process_env: _,
    } = env;
    let crate::args::CompletionsArgs { shell } = args;
    run(&shell)?;
    Ok(ExitStatus::Success)
}
