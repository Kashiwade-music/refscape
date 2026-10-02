mod architecture;
mod source_rules;

use std::{env, path::Path, process::Command};

pub use architecture::{check_architecture, dependency_graph, write_dependency_graph};
pub use source_rules::{MAX_FILE_LINES, check_source_rules};

/// Validate source rules, architecture and the checked-in graph, then check both workspaces.
pub fn gate(root: &Path) -> Result<(), String> {
    println!("{}", check_source_rules(root)?);
    println!("{}", check_architecture(root)?);
    check_dependency_graph(root)?;

    cargo(root, &["fmt", "--all", "--", "--check"])?;
    cargo(
        root,
        &[
            "fmt",
            "--manifest-path",
            "xtask/Cargo.toml",
            "--all",
            "--",
            "--check",
        ],
    )?;
    cargo(
        root,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    cargo(
        root,
        &[
            "clippy",
            "--manifest-path",
            "xtask/Cargo.toml",
            "--all-targets",
            "--all-features",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    cargo(
        root,
        &[
            "clippy",
            "--manifest-path",
            "examples/demo/Cargo.toml",
            "--all-targets",
            "--all-features",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    cargo(root, &["test", "--workspace", "--all-features", "--locked"])?;
    cargo(
        root,
        &[
            "test",
            "--manifest-path",
            "xtask/Cargo.toml",
            "--all-features",
            "--locked",
        ],
    )?;
    println!("gate passed");
    Ok(())
}

fn cargo(root: &Path, args: &[&str]) -> Result<(), String> {
    println!("running: cargo {}", args.join(" "));
    let status = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(root)
        .args(args)
        .status()
        .map_err(|error| format!("could not run cargo {}: {error}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo {} failed ({status})", args.join(" ")))
    }
}
/// A normal gate checks generated documentation without modifying it.
pub fn check_dependency_graph(root: &Path) -> Result<(), String> {
    let expected = dependency_graph(root)?;
    let path = root.join("docs/dependency-graph.md");
    let actual = std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "cannot read {}: {error}; run cargo xtask graph",
            path.display()
        )
    })?;
    if actual.replace("\r\n", "\n") != expected {
        return Err("dependency graph is outdated; run cargo xtask graph and review it".into());
    }
    Ok(())
}
