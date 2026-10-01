use std::{env, path::Path, process::ExitCode};

const USAGE: &str = "usage: cargo xtask gate";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let command = args.next();
    if args.next().is_some() {
        return Err(USAGE.into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask must be located immediately below the repository root")?;
    match command.as_deref() {
        Some("gate") => xtask::gate(root),
        Some("--help" | "-h") | None => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown xtask command: {other}")),
    }
}
