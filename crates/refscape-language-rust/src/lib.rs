//! Cargo discovery and rust-analyzer profile; queries use the common opened session.
mod files;
mod project;
mod server;
use refscape_analysis::{
    AnalysisFactory, AnalysisResult, ErrorKind, OperationContext, PreparedProject, RefscapeError,
};
use refscape_language_support::resolver::{ConfiguredExecutable, ServerKind, resolve};
use refscape_language_support::{
    EnvironmentSnapshot, LspAnalysisSession, Metadata, MetadataProvider, SearchMergePolicy,
};
use refscape_lsp::ServerConfiguration;
use refscape_model::{ProjectLanguage, ProjectOpenOptions};
use serde_json::json;
use std::{
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};
#[derive(Clone)]
pub struct RustAnalyzer {
    executable: ConfiguredExecutable,
    environment: EnvironmentSnapshot,
}
pub fn supports(root: &Path) -> bool {
    root.join("Cargo.toml").is_file()
}
impl Default for RustAnalyzer {
    fn default() -> Self {
        let environment = EnvironmentSnapshot::capture();
        Self::from_environment(
            environment.configured_executable("REFSCAPE_RUST_ANALYZER", "rust-analyzer"),
            environment,
        )
    }
}
impl RustAnalyzer {
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
    environment: EnvironmentSnapshot,
    project: project::Project,
    fingerprint: Option<u64>,
    revision: u64,
}
impl Provider {
    fn files(
        project: &project::Project,
        context: &OperationContext,
    ) -> Result<Vec<PathBuf>, RefscapeError> {
        let mut files = vec![];
        let mut roots: Vec<_> = project
            .crates
            .iter()
            .map(|package| package.root.clone())
            .collect();
        roots.sort();
        let mut walked = vec![];
        for root in roots {
            if walked
                .iter()
                .any(|parent: &PathBuf| root.starts_with(parent))
            {
                continue;
            }
            files.extend(refscape_language_support::catalog::walk_with_context(
                &root,
                files::POLICY,
                false,
                context,
            )?);
            walked.push(root);
        }
        files.extend(
            project
                .targets
                .iter()
                .filter(|path| path.is_file())
                .cloned(),
        );
        files.sort();
        files.dedup();
        Ok(files)
    }
    fn fingerprint(
        &self,
        project: &project::Project,
        files: &[PathBuf],
        context: &OperationContext,
    ) -> Result<u64, RefscapeError> {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        let mut inputs = vec![self.root.join("Cargo.toml")];
        inputs.extend(
            project
                .crates
                .iter()
                .map(|package| package.root.join("Cargo.toml")),
        );
        inputs.sort();
        inputs.dedup();
        for input in inputs {
            context.check()?;
            match std::fs::read(&input) {
                Ok(bytes) => bytes.hash(&mut hash),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => input.hash(&mut hash),
                Err(error) => {
                    return Err(RefscapeError::new(
                        ErrorKind::Io,
                        format!("cannot read {}: {error}", input.display()),
                    ));
                }
            }
        }
        files.hash(&mut hash);
        project.targets.hash(&mut hash);
        Ok(hash.finish())
    }
}
impl MetadataProvider for Provider {
    fn refresh(&mut self, context: &OperationContext) -> AnalysisResult<Metadata> {
        context.check()?;
        let (mut files, mut catalog_error) = match Self::files(&self.project, context) {
            Ok(files) => (files, None),
            Err(error) => (vec![], Some(error)),
        };
        let fingerprint = self.fingerprint(&self.project, &files, context)?;
        if self.fingerprint.is_some_and(|old| old != fingerprint) {
            let project = project::Project::discover(&self.root, context, &self.environment)?;
            let new_files = Self::files(&project, context)?;
            let new_fingerprint = self.fingerprint(&project, &new_files, context)?;
            context.check()?;
            self.project = project;
            self.fingerprint = Some(new_fingerprint);
            self.revision = self.revision.checked_add(1).ok_or_else(|| {
                RefscapeError::new(
                    ErrorKind::InternalInvariant,
                    "Cargo metadata revision overflow",
                )
            })?;
            files = new_files;
            catalog_error = None;
        } else if self.fingerprint.is_none() && catalog_error.is_none() {
            self.fingerprint = Some(fingerprint);
        }
        Ok(Metadata {
            options: ProjectOpenOptions {
                language: ProjectLanguage::Rust,
                compilation_database: None,
            }
            .try_into()?,
            files,
            catalog_error,
            crates: self.project.crates.clone(),
            seed: None,
            search: SearchMergePolicy::WorkspaceOnly,
            prewarm_references: false,
            revision: self.revision,
        })
    }
}
impl AnalysisFactory for RustAnalyzer {
    fn prepare(
        &self,
        root: &Path,
        options: &ProjectOpenOptions,
        context: &OperationContext,
    ) -> AnalysisResult<PreparedProject> {
        context.check()?;
        if !matches!(
            options.language,
            ProjectLanguage::Auto | ProjectLanguage::Rust
        ) || options.compilation_database.is_some()
        {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                "rust-analyzer analyzes Cargo projects and does not accept C/C++ compilation databases",
            ));
        }
        let root = root.canonicalize().map_err(|e| {
            RefscapeError::new(
                ErrorKind::Io,
                format!("cannot open {}: {e}", root.display()),
            )
        })?;
        if !supports(&root) {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                format!(
                    "{} is not a Cargo project (Cargo.toml is missing)",
                    root.display()
                ),
            ));
        }
        let project = project::Project::discover(&root, context, &self.environment)?;
        let provider = Provider {
            root: root.clone(),
            environment: self.environment.clone(),
            project,
            fingerprint: None,
            revision: 0,
        };
        let launch = resolve(
            &root,
            &self.executable,
            ServerKind::Native,
            &self.environment,
        )?;
        LspAnalysisSession::prepare(root, launch.command(), ServerConfiguration { name: "rust-analyzer".into(), installation_hint: "Install `rustup component add rust-analyzer` or set REFSCAPE_RUST_ANALYZER to its executable".into(), initialization_options: json!({"checkOnSave":false}), experimental_capabilities: json!({"serverStatusNotification":true}), language_id: |_| "rust", behavior: Box::<server::RustServer>::default() }, Box::new(provider), context)
    }
}
