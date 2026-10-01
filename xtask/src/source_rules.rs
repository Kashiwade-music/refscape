mod code_lines;

use std::{collections::BTreeSet, fs, io::ErrorKind, path::Path, process::Command};

use code_lines::count_code_lines;

/// Maximum code lines after excluding whitespace, comments, and string literals.
pub const MAX_FILE_LINES: usize = 1000;

/// Check tracked and non-ignored new files, including tooling and examples.
pub fn check_source_rules(root: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()
        .map_err(|error| format!("cannot list repository files: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot list repository files: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let listing = String::from_utf8(output.stdout)
        .map_err(|error| format!("repository paths must be UTF-8: {error}"))?;
    let paths: BTreeSet<_> = listing
        .split('\0')
        .filter(|path| !path.is_empty())
        .collect();
    let mut files = BTreeSet::new();
    let mut violations = Vec::new();
    let mut checked = 0;
    for relative in paths {
        let path = root.join(relative);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            // Tracked files deleted in the working tree no longer need checking.
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot inspect {relative}: {error}")),
        };
        if !metadata.is_file() {
            continue;
        }
        files.insert(relative);
        let extension = path.extension().and_then(|extension| extension.to_str());
        if !matches!(
            extension,
            Some("rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx")
        ) {
            continue;
        }
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {relative} as UTF-8 source: {error}"))?;
        checked += 1;
        let lines = count_code_lines(&text, extension == Some("rs"))
            .map_err(|error| format!("{relative}: {error}"))?;
        if lines > MAX_FILE_LINES {
            violations.push(format!(
                "{relative}: max_file_lines exceeded ({lines} > {MAX_FILE_LINES})"
            ));
        }
    }
    check_module_layout(&files, &mut violations);
    if violations.is_empty() {
        Ok(format!(
            "source rules passed: {checked} source files; max_file_lines = {MAX_FILE_LINES} (excluding blanks, comments, strings); parent module files checked"
        ))
    } else {
        Err(format!("source rules failed:\n{}", violations.join("\n")))
    }
}

fn check_module_layout(files: &BTreeSet<&str>, violations: &mut Vec<String>) {
    let mut roots = BTreeSet::new();
    let mut target_containers = BTreeSet::new();
    for file in files {
        let path = Path::new(file);
        if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            let package = path.parent().unwrap();
            for container in ["src", "src/bin", "tests", "examples", "benches"] {
                roots.insert(package.join(container));
                if container != "src" {
                    target_containers.insert(package.join(container));
                }
            }
        }
        // Clippy's mod_module_files handles legacy mod.rs layouts. Do not
        // reject these first through the parent-file correspondence check.
        if path.file_name().is_some_and(|name| name == "mod.rs") {
            roots.insert(path.parent().unwrap().to_owned());
        }
    }
    for file in files {
        let path = Path::new(file);
        // Directory-based Cargo targets may use main.rs or lib.rs as their root.
        let directory = path.parent().unwrap();
        if path
            .file_name()
            .is_some_and(|name| name == "main.rs" || name == "lib.rs")
            && directory
                .parent()
                .is_some_and(|parent| target_containers.contains(parent))
        {
            roots.insert(directory.to_owned());
        }
    }
    let mut directories = BTreeSet::new();
    for file in files {
        let path = Path::new(file);
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let mut parent = path.parent();
        while let Some(directory) = parent {
            if roots.contains(directory) || directory.as_os_str().is_empty() {
                break;
            }
            directories.insert(directory);
            parent = directory.parent();
        }
    }
    for directory in directories {
        // Append rather than replace an extension: a module name can contain dots.
        let directory = directory.to_string_lossy().replace('\\', "/");
        let parent_file = format!("{directory}.rs");
        if !files.contains(parent_file.as_str()) {
            violations.push(format!(
                "{directory}/: child module directory requires {parent_file}"
            ));
        }
    }
}
