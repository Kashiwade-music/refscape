//! File discovery only; Pyright owns Python parsing, configuration, and imports.
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn supports(root: &Path) -> Result<bool, String> {
    if root.join("pyrightconfig.json").is_file() {
        return Ok(true);
    }
    // pyproject.toml alone can configure tools in a non-Python repository.
    Ok(!files(root)?.is_empty())
}

pub(crate) fn language_id(_path: &Path) -> &'static str {
    "python"
}

fn excluded(directory: &Path) -> bool {
    matches!(
        directory.file_name().and_then(|name| name.to_str()),
        Some(
            ".git"
                | ".hg"
                | ".svn"
                | ".refscape"
                | "node_modules"
                | "target"
                | "build"
                | "dist"
                | "coverage"
                | "__pycache__"
                | ".pytest_cache"
                | ".mypy_cache"
                | ".ruff_cache"
                | ".pytype"
                | ".tox"
                | ".nox"
                | ".venv"
                | "venv"
                | ".env"
                | "env"
                | "site-packages"
                | "__pypackages__"
        )
    ) || directory.join("pyvenv.cfg").is_file()
}

pub(crate) fn files(root: &Path) -> Result<Vec<PathBuf>, String> {
    fn collect(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(directory)
            .map_err(|error| format!("cannot list {}: {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !excluded(&path) {
                    collect(&path, output)?;
                }
            } else if kind.is_file()
                && matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("py" | "pyi")
                )
            {
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
