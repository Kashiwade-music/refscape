//! Resolve Pyright-family pip executables and npm entry points without a shell.
use refscape_lsp::transport::ServerBehavior;
use serde_json::{Value, json};
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

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

fn package_entry(package: &Path) -> Option<PathBuf> {
    ["langserver.index.js", "dist/pyright-langserver.js"]
        .into_iter()
        .map(|entry| package.join(entry))
        .find(|entry| entry.is_file())
}

fn package_cli(directory: &Path, package: &str) -> Option<PathBuf> {
    package_entry(&directory.join("node_modules").join(package))
}

fn package_name(executable: &Path) -> &'static str {
    if executable
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("pyright-langserver"))
    {
        "pyright"
    } else {
        "basedpyright"
    }
}

fn shim_cli(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let package = package_name(path);
    // npm global shims live beside node_modules; local shims live in .bin.
    package_cli(parent, package).or_else(|| package_entry(&parent.parent()?.join(package)))
}

fn venv_server(directory: &Path, name: &Path) -> Option<PathBuf> {
    [".venv", "venv"].into_iter().find_map(|environment| {
        let environment = directory.join(environment);
        #[cfg(windows)]
        let candidate = environment.join("Scripts").join(name).with_extension("exe");
        #[cfg(not(windows))]
        let candidate = environment.join("bin").join(name);
        candidate.is_file().then_some(candidate)
    })
}

fn executable_in_directory(directory: &Path, executable: &Path) -> Option<PathBuf> {
    let path = directory.join(executable);
    // npm also installs a POSIX shell shim without an extension on Windows.
    // Prefer native executables and npm's Windows shim before that sibling.
    #[cfg(windows)]
    if executable.extension().is_none() {
        for extension in ["exe", "cmd", "bat"] {
            let candidate = path.with_extension(extension);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    path.is_file().then_some(path)
}

pub(crate) fn command(root: &Path, executable: &Path) -> Result<Command, String> {
    let local = if matches!(
        executable.to_str(),
        Some("basedpyright-langserver" | "pyright-langserver")
    ) {
        root.ancestors().find_map(|directory| {
            venv_server(directory, executable)
                .or_else(|| package_cli(directory, package_name(executable)))
        })
    } else {
        None
    };
    let resolved = local
        .or_else(|| {
            if executable.is_file() {
                return executable.canonicalize().ok();
            }
            if executable.components().count() != 1 {
                return None;
            }
            env::var_os("PATH").and_then(|paths| {
                env::split_paths(&paths)
                    .find_map(|directory| executable_in_directory(&directory, executable))
            })
        })
        .unwrap_or_else(|| executable.to_path_buf());
    let extension = resolved
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let script = match extension.as_deref() {
        Some("js" | "mjs" | "cjs") => Some(resolved.clone()),
        Some("cmd" | "bat") => Some(shim_cli(&resolved).ok_or_else(|| format!(
            "Cannot resolve the npm entry point beside {}. Set REFSCAPE_PYRIGHT to basedpyright/langserver.index.js or basedpyright/dist/pyright-langserver.js (or the equivalent Pyright entry point)", resolved.display()))?),
        #[cfg(windows)]
        None if matches!(resolved.file_name().and_then(|name| name.to_str()), Some("basedpyright-langserver" | "pyright-langserver")) => shim_cli(&resolved),
        _ => None,
    };
    let mut command = if let Some(script) = script {
        // Node's entry-point resolver does not accept Windows verbatim paths.
        #[cfg(windows)]
        let script = {
            let text = script.to_string_lossy();
            if let Some(unc) = text.strip_prefix("\\\\?\\UNC\\") {
                PathBuf::from(format!("\\\\{unc}"))
            } else {
                PathBuf::from(text.strip_prefix("\\\\?\\").unwrap_or(&text))
            }
        };
        let mut command =
            Command::new(env::var_os("REFSCAPE_NODE").unwrap_or_else(|| "node".into()));
        command.arg(script);
        command
    } else {
        Command::new(resolved)
    };
    command.arg("--stdio").current_dir(root);
    Ok(command)
}

#[cfg(test)]
mod tests;
