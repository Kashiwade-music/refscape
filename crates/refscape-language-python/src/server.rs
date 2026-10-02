//! Resolve Pyright-family pip executables and npm entry points without a shell.
use refscape_lsp::transport::ServerBehavior;
use serde_json::{Value, json};

pub(crate) struct PyrightBehavior;

impl ServerBehavior for PyrightBehavior {
    fn configuration(&self, section: Option<&str>) -> Value {
        let analysis = json!({
            "diagnosticMode":"workspace",
            "autoSearchPaths":true,
            "useLibraryCodeForTypes":true
        });
        match section {
            Some("python" | "basedpyright") => json!({"analysis":analysis}),
            Some("python.analysis" | "basedpyright.analysis") => analysis,
            None => json!({"python":{"analysis":analysis},"basedpyright":{"analysis":analysis}}),
            _ => json!({}),
        }
    }
}

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
        refscape_language_support::resolver::ServerKind::Python,
    )
}
#[cfg(test)]
fn executable_in_directory(directory: &Path, executable: &Path) -> Option<PathBuf> {
    refscape_language_support::resolver::executable_candidates(directory, executable, cfg!(windows))
        .into_iter()
        .find(|path| path.is_file())
}
#[cfg(test)]
fn command(root: &Path, executable: &Path) -> Result<Command, String> {
    Ok(refscape_language_support::resolver::resolve(
        root,
        &refscape_language_support::resolver::ConfiguredExecutable::default_name(executable),
        refscape_language_support::resolver::ServerKind::Python,
        &refscape_language_support::EnvironmentSnapshot::capture(),
    )
    .map_err(|error| error.to_string())?
    .command())
}
#[cfg(test)]
mod tests;
