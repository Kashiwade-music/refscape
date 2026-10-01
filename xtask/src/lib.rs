mod architecture;

use std::{env, path::Path, process::Command};

pub use architecture::{check_architecture, dependency_graph, write_dependency_graph};

/// Validate architecture and regenerate its graph before checking product code.
pub fn gate(root: &Path) -> Result<(), String> {
    println!("{}", check_architecture(root)?);
    println!("{}", write_dependency_graph(root)?);

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
