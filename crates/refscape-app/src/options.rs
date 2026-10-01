use refscape_model::{ProjectLanguage, ProjectOptions};
use std::{ffi::OsString, path::PathBuf};

pub const HELP: &str = "Refscape — a spatial Rust, C/C++, and TypeScript/React code explorer\n\n\
Usage: refscape [PROJECT] [OPTIONS]\n\n\
  --session FILE       Open/save a named session (default: PROJECT/.refscape/session.json)\n\
  --theme FILE         Add a custom JSON theme\n\
  --rust-analyzer EXE  Override the rust-analyzer executable\n\
  --clangd EXE         Override the C/C++ language server executable\n\
  --typescript-language-server PATH  Override the JS/TS server executable or lib/cli.mjs\n\
  --language LANGUAGE  Choose auto (default), rust, c, cpp, or typescript (ts)\n\
  --compile-commands PATH  Use compile_commands.json or its containing directory\n\
  --check PROJECT      Verify analysis and session persistence without a window\n\
  --export-theme NAME FILE  Write the light or dark theme as a customizable JSON file\n\
  --help               Show this help\n\n\
Without PROJECT, choose a source folder using Open project in the window.\n\
Rust: rustup component add rust-analyzer rust-src\n\
C/C++: install clangd; build settings are detected or chosen with Build settings.\n\
TypeScript/React/React Native: install Node.js and npm install -g typescript typescript-language-server.\n";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub project: Option<PathBuf>,
    pub session: Option<PathBuf>,
    pub theme: Option<PathBuf>,
    pub analyzer: Option<PathBuf>,
    pub clangd: Option<PathBuf>,
    pub typescript: Option<PathBuf>,
    pub project_options: ProjectOptions,
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
                "--language" => {
                    let language = args
                        .next()
                        .ok_or("--language requires auto, rust, c, cpp, or typescript (ts)")?;
                    options.project_options.language = match language.to_str() {
                        Some("auto") => ProjectLanguage::Auto,
                        Some("rust") => ProjectLanguage::Rust,
                        Some("c" | "cpp" | "c++") => ProjectLanguage::Cpp,
                        Some(
                            "typescript" | "ts" | "javascript" | "js" | "react" | "react-native",
                        ) => ProjectLanguage::TypeScript,
                        _ => {
                            return Err(
                                "--language requires auto, rust, c, cpp, or typescript (ts)".into(),
                            );
                        }
                    };
                }
                "--session"
                | "--theme"
                | "--rust-analyzer"
                | "--clangd"
                | "--compile-commands"
                | "--typescript-language-server"
                | "--check" => {
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
                        "--clangd" => options.clangd = Some(path.into()),
                        "--typescript-language-server" => options.typescript = Some(path.into()),
                        "--compile-commands" => {
                            options.project_options.compilation_database = Some(path.into())
                        }
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
        if matches!(
            options.project_options.language,
            ProjectLanguage::Rust | ProjectLanguage::TypeScript
        ) && options.project_options.compilation_database.is_some()
        {
            return Err("--compile-commands is only supported for C/C++ projects".into());
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
            &["--clangd", "--check", "."],
            &["--compile-commands"],
            &["--language"],
            &["--language", "python"],
            &["--language", "rust", "--compile-commands", "build"],
        ] {
            assert!(parse(args).is_err());
        }
    }

    #[test]
    fn accepts_c_and_cpp_with_separate_build_settings() {
        for language in ["c", "cpp", "c++"] {
            let options = parse(&[
                "source folder",
                "--language",
                language,
                "--compile-commands",
                "../debug build/compile_commands.json",
                "--clangd",
                "custom clangd.exe",
            ])
            .unwrap();
            assert_eq!(options.project, Some("source folder".into()));
            assert_eq!(options.project_options.language, ProjectLanguage::Cpp);
            assert_eq!(
                options.project_options.compilation_database,
                Some("../debug build/compile_commands.json".into())
            );
            assert_eq!(options.clangd, Some("custom clangd.exe".into()));
        }
        assert_eq!(
            parse(&[]).unwrap().project_options,
            ProjectOptions::default()
        );
    }

    #[test]
    fn accepts_typescript_react_and_javascript_and_rejects_cpp_settings() {
        for language in [
            "typescript",
            "ts",
            "javascript",
            "js",
            "react",
            "react-native",
        ] {
            let options = parse(&[
                "source folder",
                "--language",
                language,
                "--typescript-language-server",
                "custom server/lib/cli.mjs",
            ])
            .unwrap();
            assert_eq!(
                options.project_options.language,
                ProjectLanguage::TypeScript
            );
            assert_eq!(options.typescript, Some("custom server/lib/cli.mjs".into()));
            assert!(parse(&["--language", language, "--compile-commands", "build"]).is_err());
        }
        assert!(parse(&["--typescript-language-server", "--check", "."]).is_err());
    }
}
