//! `grit completions` — emit the shell integration script for dynamic completion.

use anyhow::{anyhow, Result};

use crate::completions;

/// Write the completion script for `shell` to stdout.
///
/// The script registers a shell function that asks `grit _carapace` for
/// candidates, so completion covers branch names, tags, remotes, paths and
/// flags without regenerating anything after a `grit` upgrade.
pub fn run(shell: &str) -> Result<()> {
    let shell = completions::parse_shell(shell).map_err(|error| anyhow!(error))?;
    print!("{}", completions::snippet(shell));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use carapace_rs::shell::Shell;

    #[test]
    fn unknown_shell_is_rejected() {
        let error = run("nushell-but-not-quite").expect_err("unknown shell");
        assert!(error.to_string().contains("unknown shell"), "{error}");
    }

    #[test]
    fn export_is_not_a_shell() {
        assert!(run("export").is_err());
    }

    #[test]
    fn known_shells_have_scripts() {
        for shell in completions::shell_names() {
            assert!(Shell::parse(shell).is_some(), "{shell}");
        }
    }
}
