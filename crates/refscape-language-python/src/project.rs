//! File discovery only; Pyright owns Python parsing, configuration, and imports.
use std::path::Path;

pub fn supports(root: &Path) -> Result<bool, String> {
    if root.join("pyrightconfig.json").is_file() {
        return Ok(true);
    }
    // pyproject.toml alone can configure tools in a non-Python repository.
    Ok(!refscape_language_support::catalog::walk(root, POLICY, true)?.is_empty())
}

pub(crate) fn language_id(_path: &Path) -> &'static str {
    "python"
}

pub const POLICY: refscape_language_support::catalog::WalkPolicy =
    refscape_language_support::catalog::WalkPolicy {
        extensions: &["py", "pyi"],
        excluded: &[
            ".git",
            ".hg",
            ".svn",
            ".refscape",
            "node_modules",
            "target",
            "build",
            "dist",
            "coverage",
            "__pycache__",
            ".pytest_cache",
            ".mypy_cache",
            ".ruff_cache",
            ".pytype",
            ".tox",
            ".nox",
            ".venv",
            "venv",
            ".env",
            "env",
            "site-packages",
            "__pypackages__",
        ],
        case_insensitive: false,
        symlink_files: false,
        exclude_virtual_environments: true,
        canonical_paths: false,
    };
#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
pub(crate) fn files(root: &Path) -> Result<Vec<PathBuf>, String> {
    refscape_language_support::catalog::walk(root, POLICY, false)
}
#[cfg(test)]
mod tests;
