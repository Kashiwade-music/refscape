//! TypeScript/JavaScript policy for the shared opened runtime.
mod project;
mod server;
pub use project::{POLICY as FILE_POLICY, supports};
use refscape_analysis::{
    AnalysisFactory, AnalysisResult, ErrorKind, OperationContext, PreparedProject, RefscapeError,
};
use refscape_language_support::resolver::{ConfiguredExecutable, ServerKind, resolve};
use refscape_language_support::{
    EnvironmentSnapshot, LspAnalysisSession, Metadata, MetadataProvider, SearchMergePolicy,
};
use refscape_lsp::{ServerConfiguration, transport::DefaultServerBehavior};
use refscape_model::{ProjectLanguage, ProjectOpenOptions};
use serde_json::json;
use std::path::{Path, PathBuf};
#[derive(Clone)]
pub struct TypeScript {
    executable: ConfiguredExecutable,
    environment: EnvironmentSnapshot,
    discovery_files: Option<Vec<PathBuf>>,
}
impl Default for TypeScript {
    fn default() -> Self {
        let environment = EnvironmentSnapshot::capture();
        Self::from_environment(
            environment.configured_executable(
                "REFSCAPE_TYPESCRIPT_LANGUAGE_SERVER",
                "typescript-language-server",
            ),
            environment,
        )
    }
}
impl TypeScript {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self::from_environment(
            ConfiguredExecutable::explicit(executable),
            EnvironmentSnapshot::capture(),
        )
    }
    pub fn with_discovery_files(mut self, files: Vec<PathBuf>) -> Self {
        self.discovery_files = Some(files);
        self
    }
    pub fn from_environment(
        executable: impl Into<ConfiguredExecutable>,
        environment: EnvironmentSnapshot,
    ) -> Self {
        Self {
            executable: executable.into(),
            environment,
            discovery_files: None,
        }
    }
}
struct Provider {
    root: PathBuf,
    initial_files: Option<Vec<PathBuf>>,
}
impl MetadataProvider for Provider {
    fn refresh(&mut self, context: &OperationContext) -> AnalysisResult<Metadata> {
        context.check()?;
        let listing = match self.initial_files.take() {
            Some(files) => Ok(files),
            None => refscape_language_support::catalog::walk_with_context(
                &self.root,
                FILE_POLICY,
                false,
                context,
            ),
        };
        let (files, catalog_error) = match listing {
            Ok(files) => (files, None),
            Err(error) => (vec![], Some(error)),
        };
        Ok(Metadata {
            options: ProjectOpenOptions {
                language: ProjectLanguage::TypeScript,
                compilation_database: None,
            }
            .try_into()?,
            seed: files.first().cloned(),
            files,
            catalog_error,
            crates: vec![],
            search: SearchMergePolicy::WorkspaceFirst,
            prewarm_references: true,
            revision: 0,
        })
    }
}
impl AnalysisFactory for TypeScript {
    fn prepare(
        &self,
        root: &Path,
        options: &ProjectOpenOptions,
        context: &OperationContext,
    ) -> AnalysisResult<PreparedProject> {
        context.check()?;
        if !matches!(
            options.language,
            ProjectLanguage::Auto | ProjectLanguage::TypeScript
        ) || options.compilation_database.is_some()
        {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                "TypeScript analyzes TypeScript/JavaScript projects; compilation databases apply only to C/C++",
            ));
        }
        let root = root.canonicalize().map_err(|e| {
            RefscapeError::new(
                ErrorKind::Io,
                format!("cannot open {}: {e}", root.display()),
            )
        })?;
        if !root.is_dir() {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                format!("{} is not a source folder", root.display()),
            ));
        }
        if options.language == ProjectLanguage::Auto && !supports(&root)? {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                format!(
                    "{} does not contain a TypeScript/JavaScript project",
                    root.display()
                ),
            ));
        }
        let launch = resolve(
            &root,
            &self.executable,
            ServerKind::TypeScript,
            &self.environment,
        )?;
        LspAnalysisSession::prepare(root.clone(), launch.command(), ServerConfiguration {
            name: "typescript-language-server".into(),
            installation_hint: "Install Node.js and npm install -g typescript typescript-language-server, or set REFSCAPE_TYPESCRIPT_LANGUAGE_SERVER to the server executable or lib/cli.mjs (REFSCAPE_NODE selects Node.js)".into(),
            initialization_options: json!({"hostInfo":"Refscape","disableAutomaticTypingAcquisition":true,"tsserver":{"useSyntaxServer":"never"}}),
            experimental_capabilities: json!({}), language_id: project::language_id, behavior: Box::new(DefaultServerBehavior),
        }, Box::new(Provider { root, initial_files: self.discovery_files.clone() }), context)
    }
}
