//! File discovery only; TypeScript owns tsconfig/jsconfig and module resolution.
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn supports(root: &Path) -> Result<bool, String> {
    if root.join("tsconfig.json").is_file() || root.join("jsconfig.json").is_file() {
        return Ok(true);
    }
    Ok(!files(root)?.is_empty())
}

pub(crate) fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("tsx") => "typescriptreact",
        Some("jsx") => "javascriptreact",
        Some("js" | "mjs" | "cjs") => "javascript",
        _ => "typescript",
    }
}

pub(crate) fn files(root: &Path) -> Result<Vec<PathBuf>, String> {
    fn collect(root: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(root)
            .map_err(|error| format!("cannot list {}: {error}", root.display()))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(
                        "node_modules"
                            | ".git"
                            | ".hg"
                            | ".svn"
                            | ".refscape"
                            | ".yarn"
                            | ".next"
                            | ".expo"
                            | "target"
                            | "build"
                            | "dist"
                            | "coverage"
                            | "Pods"
                            | ".gradle"
                            | ".venv"
                            | "venv"
                            | ".env"
                            | "env"
                            | "__pypackages__"
                            | "site-packages"
                            | "__pycache__"
                            | ".pytest_cache"
                            | ".mypy_cache"
                            | ".ruff_cache"
                            | ".pytype"
                            | ".tox"
                            | ".nox"
                    )
                ) && !path.join("pyvenv.cfg").is_file()
                {
                    collect(&path, output)?;
                }
            } else if matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs")
            ) {
                output.push(path);
            }
        }
        Ok(())
    }
    let mut output = Vec::new();
    collect(root, &mut output)?;
    output.sort();
    Ok(output)
}

#[cfg(test)]
mod tests;
