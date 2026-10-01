use std::{ffi::OsString, path::PathBuf};

pub const HELP: &str = "Refscape — a spatial Rust code explorer\n\n\
Usage: refscape [PROJECT] [OPTIONS]\n\n\
  --session FILE       Open/save a named session (default: PROJECT/.refscape/session.json)\n\
  --theme FILE         Add a custom JSON theme\n\
  --rust-analyzer EXE  Override the rust-analyzer executable\n\
  --check PROJECT      Verify Rust analysis and session persistence without a window\n\
  --export-theme NAME FILE  Write the light or dark theme as a customizable JSON file\n\
  --help               Show this help\n\n\
Without PROJECT, choose a Cargo project using Open project in the window.\n\
Install the Rust backend with: rustup component add rust-analyzer rust-src\n";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub project: Option<PathBuf>,
    pub session: Option<PathBuf>,
    pub theme: Option<PathBuf>,
    pub analyzer: Option<PathBuf>,
    pub check: bool,
    pub help: bool,
    pub export_theme: Option<(String, PathBuf)>,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.into_iter();
        let mut positional = false;
        while let Some(arg) = args.next() {
            if positional {
                if options.project.replace(arg.into()).is_some() {
                    return Err("only one project can be opened".into());
                }
                continue;
            }
            let value = arg.to_str().unwrap_or("");
            match value {
                "--" => positional = true,
                "--help" | "-h" => options.help = true,
                "--session" | "--theme" | "--rust-analyzer" | "--check" => {
                    let path = args
                        .next()
                        .ok_or_else(|| format!("{value} requires a path"))?;
                    if path.to_str().is_some_and(|s| s.starts_with("--")) {
                        return Err(format!("{value} requires a path"));
                    }
                    match value {
                        "--session" => options.session = Some(path.into()),
                        "--theme" => options.theme = Some(path.into()),
                        "--rust-analyzer" => options.analyzer = Some(path.into()),
                        _ => {
                            options.check = true;
                            if options.project.replace(path.into()).is_some() {
                                return Err("only one project can be opened".into());
                            }
                        }
                    }
                }
                "--export-theme" => {
                    let name = args
                        .next()
                        .and_then(|name| name.into_string().ok())
                        .ok_or("--export-theme requires light or dark and an output path")?;
                    if name != "light" && name != "dark" {
                        return Err("theme name must be light or dark".into());
                    }
                    let path = args
                        .next()
                        .ok_or("--export-theme requires an output path")?;
                    if path.to_str().is_some_and(|s| s.starts_with("--")) {
                        return Err("--export-theme requires an output path".into());
                    }
                    options.export_theme = Some((name, path.into()));
                }
                other if other.starts_with('-') => return Err(format!("unknown option: {other}")),
                _ => {
                    if options.project.replace(arg.into()).is_some() {
                        return Err("only one project can be opened".into());
                    }
                }
            }
        }
        if options.export_theme.is_some() && (options.check || options.project.is_some()) {
            return Err("--export-theme cannot be combined with a project or --check".into());
        }
        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn accepts_project_paths_and_named_sessions() {
        let options = parse(&["日本語 project", "--session", "my reading.json"]).unwrap();
        assert_eq!(options.project, Some(PathBuf::from("日本語 project")));
        assert_eq!(options.session, Some(PathBuf::from("my reading.json")));
    }

    #[test]
    fn rejects_missing_values_and_conflicting_projects() {
        for args in [
            &["--check"][..],
            &["--theme", "--check", "."],
            &["a", "b"],
            &["a", "--check", "b"],
        ] {
            assert!(parse(args).is_err());
        }
    }
}
