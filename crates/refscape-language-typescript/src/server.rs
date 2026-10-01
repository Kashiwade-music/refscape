//! Resolve npm entry points and launch Node directly, including on Windows.
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

fn package_cli(directory: &Path) -> Option<PathBuf> {
    let cli = directory.join("node_modules/typescript-language-server/lib/cli.mjs");
    cli.is_file().then_some(cli)
}

fn shim_cli(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    // npm global shims live beside node_modules; local shims live in .bin.
    package_cli(parent).or_else(|| {
        let cli = parent
            .parent()?
            .join("typescript-language-server/lib/cli.mjs");
        cli.is_file().then_some(cli)
    })
}

pub(crate) fn command(root: &Path, executable: &Path) -> Result<Command, String> {
    let default = executable == Path::new("typescript-language-server");
    let local = if default {
        root.ancestors().find_map(package_cli)
    } else {
        None
    };
    let resolved = local.or_else(|| {
        if executable.is_file() {
            return executable.canonicalize().ok();
        }
        if executable.components().count() != 1 {
            return None;
        }
        env::var_os("PATH").and_then(|paths| {
            env::split_paths(&paths).find_map(|directory| {
                let path = directory.join(executable);
                if path.is_file() {
                    return Some(path);
                }
                #[cfg(windows)]
                for extension in ["exe", "cmd"] {
                    let path = path.with_extension(extension);
                    if path.is_file() {
                        return Some(path);
                    }
                }
                None
            })
        })
    });
    let resolved = resolved.unwrap_or_else(|| executable.to_path_buf());
    let script = match resolved.extension().and_then(|extension| extension.to_str()) {
        Some("js" | "mjs" | "cjs") => Some(resolved.clone()),
        Some("cmd" | "bat") => Some(shim_cli(&resolved).ok_or_else(|| format!(
            "Cannot resolve the npm entry point beside {}. Set REFSCAPE_TYPESCRIPT_LANGUAGE_SERVER to typescript-language-server/lib/cli.mjs", resolved.display()))?),
        _ => None,
    };
    let mut command = if let Some(script) = script {
        // Node's entry-point resolver cannot reliably handle Windows verbatim paths.
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
