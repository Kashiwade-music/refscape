//! CLI normalization: execution consumes one exclusive launch mode.
use crate::options::Options;
use refscape_model::ProjectOpenOptions;
use std::path::PathBuf;
#[derive(Debug)]
pub struct LaunchConfig {
    pub project: Option<PathBuf>,
    pub session: Option<PathBuf>,
    pub theme: Option<PathBuf>,
    pub analyzer: Option<PathBuf>,
    pub clangd: Option<PathBuf>,
    pub typescript: Option<PathBuf>,
    pub pyright: Option<PathBuf>,
    pub project_options: ProjectOpenOptions,
}
#[derive(Debug)]
pub enum LaunchRequest {
    Help,
    ExportTheme { name: String, path: PathBuf },
    Check { root: PathBuf, config: LaunchConfig },
    Gui(LaunchConfig),
}
impl Options {
    pub fn into_request(self) -> Result<LaunchRequest, String> {
        // Retain the old CLI priority, including accepted combinations of switches.
        if self.help {
            return Ok(LaunchRequest::Help);
        }
        if let Some((name, path)) = self.export_theme {
            return Ok(LaunchRequest::ExportTheme { name, path });
        }
        let config = LaunchConfig {
            project: self.project,
            session: self.session,
            theme: self.theme,
            analyzer: self.analyzer,
            clangd: self.clangd,
            typescript: self.typescript,
            pyright: self.pyright,
            project_options: self.project_options,
        };
        if self.check {
            let root = config.project.clone().ok_or("--check requires a project")?;
            Ok(LaunchRequest::Check { root, config })
        } else {
            Ok(LaunchRequest::Gui(config))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    #[test]
    fn launch_modes_preserve_priority_aliases_unicode_and_double_dash() {
        for (args, mode) in [
            (vec!["--help", "--check", "p"], "help"),
            (vec!["--export-theme", "light", "out"], "export"),
            (vec!["--help", "--export-theme", "dark", "out"], "help"),
            (vec!["--check", "日本語", "--language", "py"], "check"),
            (vec!["--", "--named-root"], "gui"),
        ] {
            let request = Options::parse(args.into_iter().map(OsString::from))
                .unwrap()
                .into_request()
                .unwrap();
            let actual = match request {
                LaunchRequest::Help => "help",
                LaunchRequest::ExportTheme { .. } => "export",
                LaunchRequest::Check { .. } => "check",
                LaunchRequest::Gui(_) => "gui",
            };
            assert_eq!(actual, mode);
        }
    }
}
