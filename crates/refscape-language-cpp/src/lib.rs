//! C/C++ metadata and clangd policy.
mod project;
pub use project::{CompilationConfig, POLICY as FILE_POLICY};
pub fn probe_markers(root: &Path) -> Result<bool, String> {
    project::probe_markers(root)
}
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
pub struct Clangd {
    executable: ConfiguredExecutable,
    environment: EnvironmentSnapshot,
}
pub fn supports(root: &Path) -> Result<bool, String> {
    project::supports(root)
}
impl Default for Clangd {
    fn default() -> Self {
        let environment = EnvironmentSnapshot::capture();
        Self::from_environment(
            environment.configured_executable("REFSCAPE_CLANGD", "clangd"),
            environment,
        )
    }
}
impl Clangd {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self::from_environment(
            ConfiguredExecutable::explicit(executable),
            EnvironmentSnapshot::capture(),
        )
    }
    pub fn from_environment(
        executable: impl Into<ConfiguredExecutable>,
        environment: EnvironmentSnapshot,
    ) -> Self {
        Self {
            executable: executable.into(),
            environment,
        }
    }
}
struct Provider {
    root: PathBuf,
    options: ProjectOpenOptions,
    fingerprint: u64,
    revision: u64,
    initial_project: Option<project::CppProject>,
}
impl MetadataProvider for Provider {
    fn refresh(&mut self, context: &OperationContext) -> AnalysisResult<Metadata> {
        context.check()?;
        let project = match self.initial_project.take() {
            Some(project) => project,
            None => project::CppProject::discover_with_context(&self.root, &self.options, context)?,
        };
        let fingerprint = project.metadata_fingerprint;
        if fingerprint != self.fingerprint {
            self.fingerprint = fingerprint;
            self.revision = self.revision.checked_add(1).ok_or_else(|| {
                RefscapeError::new(
                    ErrorKind::InternalInvariant,
                    "C/C++ metadata revision overflow",
                )
            })?;
        }
        let (files, catalog_error) = match project.files_with_context(&self.root, context) {
            Ok(files) => (files, None),
            Err(error) => (vec![], Some(error)),
        };
        context.check()?;
        Ok(Metadata {
            options: project.options().try_into()?,
            seed: project.index_seed_from_files(&files),
            files,
            catalog_error,
            crates: vec![],
            search: if matches!(
                project.configuration,
                project::CompilationConfig::DelegateToClangd | project::CompilationConfig::Fallback
            ) {
                SearchMergePolicy::WorkspaceFirst
            } else {
                SearchMergePolicy::WorkspaceOnly
            },
            prewarm_references: false,
            revision: self.revision,
        })
    }
}
impl AnalysisFactory for Clangd {
    fn prepare(
        &self,
        root: &Path,
        options: &ProjectOpenOptions,
        context: &OperationContext,
    ) -> AnalysisResult<PreparedProject> {
        context.check()?;
        if !matches!(
            options.language,
            ProjectLanguage::Auto | ProjectLanguage::Cpp
        ) {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                "clangd analyzes C/C++ projects; choose the C/C++ language",
            ));
        }
        if options.language == ProjectLanguage::Auto
            && options.compilation_database.is_none()
            && !supports(root)?
        {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                format!("{} does not contain a C/C++ project", root.display()),
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
        let project = project::CppProject::discover_with_context(&root, options, context)?;
        let mut launch = resolve(
            &root,
            &self.executable,
            ServerKind::Native,
            &self.environment,
        )?;
        launch
            .args
            .extend(["--background-index".into(), "--enable-config".into()]);
        if let Some(database) = &project.database {
            let mut flag = std::ffi::OsString::from("--compile-commands-dir=");
            flag.push(
                database
                    .parent()
                    .ok_or("compilation database has no directory")?,
            );
            launch.args.push(flag);
        }
        LspAnalysisSession::prepare(
            root.clone(),
            launch.command(),
            ServerConfiguration {
                name: "clangd".into(),
                installation_hint: "Install clangd (LLVM) or set REFSCAPE_CLANGD to its executable"
                    .into(),
                initialization_options: json!({}),
                experimental_capabilities: json!({}),
                language_id: project::language_id,
                behavior: Box::new(DefaultServerBehavior),
            },
            Box::new(Provider {
                root,
                options: options.clone(),
                fingerprint: 0,
                revision: 0,
                initial_project: Some(project),
            }),
            context,
        )
    }
}
