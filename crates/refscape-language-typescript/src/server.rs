//! Tests exercise the shared server resolver with the TypeScript profile.
#[cfg(test)]
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};
#[cfg(test)]
fn shim_cli(path: &Path) -> Option<PathBuf> {
    refscape_language_support::resolver::shim_entry(
        path,
        refscape_language_support::resolver::ServerKind::TypeScript,
    )
}
#[cfg(test)]
fn command(root: &Path, executable: &Path) -> Result<Command, String> {
    Ok(refscape_language_support::resolver::resolve(
        root,
        &refscape_language_support::resolver::ConfiguredExecutable::default_name(executable),
        refscape_language_support::resolver::ServerKind::TypeScript,
        &refscape_language_support::EnvironmentSnapshot::capture(),
    )
    .map_err(|error| error.to_string())?
    .command())
}
#[cfg(test)]
mod tests;
