use crate::EnvironmentSnapshot;
use refscape_model::{ErrorKind, RefscapeError};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchOrigin {
    Default,
    Environment,
    Explicit,
    ProjectLocal,
}
#[derive(Clone, Debug)]
pub struct ConfiguredExecutable {
    pub path: PathBuf,
    pub origin: LaunchOrigin,
}
impl ConfiguredExecutable {
    pub fn explicit(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            origin: LaunchOrigin::Explicit,
        }
    }
    pub fn default_name(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            origin: LaunchOrigin::Default,
        }
    }
}
impl From<PathBuf> for ConfiguredExecutable {
    fn from(path: PathBuf) -> Self {
        Self::explicit(path)
    }
}
impl From<&PathBuf> for ConfiguredExecutable {
    fn from(path: &PathBuf) -> Self {
        Self::explicit(path)
    }
}
impl From<&Path> for ConfiguredExecutable {
    fn from(path: &Path) -> Self {
        Self::explicit(path)
    }
}
impl From<&str> for ConfiguredExecutable {
    fn from(path: &str) -> Self {
        Self::explicit(path)
    }
}
impl From<String> for ConfiguredExecutable {
    fn from(path: String) -> Self {
        Self::explicit(path)
    }
}
impl From<&ConfiguredExecutable> for ConfiguredExecutable {
    fn from(value: &ConfiguredExecutable) -> Self {
        value.clone()
    }
}
#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub environment: EnvironmentSnapshot,
    pub origin: LaunchOrigin,
    pub configured_origin: LaunchOrigin,
    pub node_origin: Option<LaunchOrigin>,
}
impl LaunchSpec {
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .args(&self.args)
            .current_dir(&self.cwd)
            .env_clear()
            .envs(self.environment.values());
        command
    }
}
#[derive(Clone, Copy)]
pub enum ServerKind {
    TypeScript,
    Python,
    Native,
}
pub fn executable_candidates(directory: &Path, executable: &Path, windows: bool) -> Vec<PathBuf> {
    let path = directory.join(executable);
    let mut candidates = vec![];
    if windows && executable.extension().is_none() {
        candidates.extend(
            ["exe", "cmd", "bat"]
                .into_iter()
                .map(|extension| path.with_extension(extension)),
        );
    }
    candidates.push(path);
    candidates
}
fn package_name(kind: ServerKind, executable: &Path) -> &'static str {
    match kind {
        ServerKind::TypeScript => "typescript-language-server",
        ServerKind::Python
            if executable
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("pyright-langserver")) =>
        {
            "pyright"
        }
        ServerKind::Python => "basedpyright",
        ServerKind::Native => "",
    }
}
pub fn package_entry(package: &Path, kind: ServerKind) -> Option<PathBuf> {
    let entries: &[&str] = match kind {
        ServerKind::TypeScript => &["lib/cli.mjs"],
        ServerKind::Python => &["langserver.index.js", "dist/pyright-langserver.js"],
        ServerKind::Native => &[],
    };
    entries
        .iter()
        .map(|entry| package.join(entry))
        .find(|entry| entry.is_file())
}
pub fn shim_entry(path: &Path, kind: ServerKind) -> Option<PathBuf> {
    let parent = path.parent()?;
    let package = package_name(kind, path);
    package_entry(&parent.join("node_modules").join(package), kind)
        .or_else(|| package_entry(&parent.parent()?.join(package), kind))
}
fn node_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(unc) = text.strip_prefix("\\\\?\\UNC\\") {
            return PathBuf::from(format!("\\\\{unc}"));
        }
        PathBuf::from(text.strip_prefix("\\\\?\\").unwrap_or(&text))
    }
    #[cfg(not(windows))]
    {
        path
    }
}
pub fn resolve(
    root: &Path,
    configured: &ConfiguredExecutable,
    kind: ServerKind,
    environment: &EnvironmentSnapshot,
) -> Result<LaunchSpec, RefscapeError> {
    let executable = configured.path.as_path();
    let local_default = match kind {
        ServerKind::TypeScript => executable == Path::new("typescript-language-server"),
        ServerKind::Python => matches!(
            executable.to_str(),
            Some("basedpyright-langserver" | "pyright-langserver")
        ),
        ServerKind::Native => false,
    };
    let local = if local_default {
        root.ancestors().find_map(|directory| {
            let venv = if matches!(kind, ServerKind::Python) {
                [".venv", "venv"].into_iter().find_map(|name| {
                    let directory =
                        directory
                            .join(name)
                            .join(if cfg!(windows) { "Scripts" } else { "bin" });
                    executable_candidates(&directory, executable, cfg!(windows))
                        .into_iter()
                        .find(|candidate| candidate.is_file())
                })
            } else {
                None
            };
            venv.or_else(|| {
                package_entry(
                    &directory
                        .join("node_modules")
                        .join(package_name(kind, executable)),
                    kind,
                )
            })
        })
    } else {
        None
    };
    let origin = if local.is_some() {
        LaunchOrigin::ProjectLocal
    } else {
        configured.origin.clone()
    };
    let resolved = local
        .or_else(|| {
            if executable.is_file() {
                return executable.canonicalize().ok();
            }
            if executable.components().count() != 1 {
                return None;
            }
            environment.path().iter().find_map(|directory| {
                executable_candidates(directory, executable, cfg!(windows))
                    .into_iter()
                    .find(|path| path.is_file())
            })
        })
        .unwrap_or_else(|| executable.to_path_buf());
    let extension = resolved
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    let script = match extension.as_deref() {
        Some("js" | "mjs" | "cjs") if !matches!(kind, ServerKind::Native) => Some(resolved.clone()),
        Some("cmd" | "bat") if !matches!(kind, ServerKind::Native) => Some(shim_entry(&resolved, kind).ok_or_else(|| RefscapeError::new(ErrorKind::BackendUnavailable, format!("Cannot resolve the npm entry point beside {}. Set {} to the server JavaScript entry point", resolved.display(), if matches!(kind, ServerKind::TypeScript) { "REFSCAPE_TYPESCRIPT_LANGUAGE_SERVER" } else { "REFSCAPE_PYRIGHT" })).with_path(&resolved))?),
        None if cfg!(windows) && !matches!(kind, ServerKind::Native) => shim_entry(&resolved, kind), _ => None,
    };
    let node = environment.configured_executable("REFSCAPE_NODE", "node");
    let (executable, mut args, node_origin) = if let Some(script) = script {
        (
            resolve(root, &node, ServerKind::Native, environment)?.executable,
            vec![node_path(script).into_os_string()],
            Some(node.origin),
        )
    } else {
        (resolved, vec![], None)
    };
    if !matches!(kind, ServerKind::Native) {
        args.push("--stdio".into());
    }
    Ok(LaunchSpec {
        executable,
        args,
        cwd: root.to_path_buf(),
        environment: environment.clone(),
        origin,
        configured_origin: configured.origin.clone(),
        node_origin,
    })
}
