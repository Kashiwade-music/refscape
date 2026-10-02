//! File discovery only; TypeScript owns tsconfig/jsconfig and module resolution.
use std::path::Path;

pub fn supports(root: &Path) -> Result<bool, String> {
    if root.join("tsconfig.json").is_file() || root.join("jsconfig.json").is_file() {
        return Ok(true);
    }
    Ok(!refscape_language_support::catalog::walk(root, POLICY, true)?.is_empty())
}

pub(crate) fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("tsx") => "typescriptreact",
        Some("jsx") => "javascriptreact",
        Some("js" | "mjs" | "cjs") => "javascript",
        _ => "typescript",
    }
}

pub const POLICY: refscape_language_support::catalog::WalkPolicy =
    refscape_language_support::catalog::WalkPolicy {
        extensions: &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"],
        excluded: &[
            "node_modules",
            ".git",
            ".hg",
            ".svn",
            ".refscape",
            ".yarn",
            ".next",
            ".expo",
            "target",
            "build",
            "dist",
            "coverage",
            "Pods",
            ".gradle",
            ".venv",
            "venv",
            ".env",
            "env",
            "__pypackages__",
            "site-packages",
            "__pycache__",
            ".pytest_cache",
            ".mypy_cache",
            ".ruff_cache",
            ".pytype",
            ".tox",
            ".nox",
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
