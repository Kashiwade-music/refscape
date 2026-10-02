//! Static registry. Factory preparation never mutates a currently opened session.
use refscape_analysis::{
    AnalysisFactory, AnalysisResult, ErrorKind, OperationContext, PreparedProject, RefscapeError,
};
use refscape_language_cpp::Clangd;
use refscape_language_python::Python;
use refscape_language_rust::RustAnalyzer;
pub use refscape_language_support::EnvironmentSnapshot;
use refscape_language_support::resolver::ConfiguredExecutable;
use refscape_language_typescript::TypeScript;
use refscape_model::{ProjectLanguage, ProjectOpenOptions};
use std::path::{Path, PathBuf};
#[derive(Clone)]
pub struct LanguageBackend {
    rust_analyzer: ConfiguredExecutable,
    clangd: ConfiguredExecutable,
    typescript: ConfiguredExecutable,
    pyright: ConfiguredExecutable,
    environment: EnvironmentSnapshot,
    _runtime: refscape_language_support::process::RuntimeOwner,
}
impl Default for LanguageBackend {
    fn default() -> Self {
        Self::from_environment(EnvironmentSnapshot::capture())
    }
}
impl LanguageBackend {
    pub fn from_environment(environment: EnvironmentSnapshot) -> Self {
        Self {
            rust_analyzer: environment
                .configured_executable("REFSCAPE_RUST_ANALYZER", "rust-analyzer"),
            clangd: environment.configured_executable("REFSCAPE_CLANGD", "clangd"),
            typescript: environment.configured_executable(
                "REFSCAPE_TYPESCRIPT_LANGUAGE_SERVER",
                "typescript-language-server",
            ),
            pyright: environment
                .configured_executable("REFSCAPE_PYRIGHT", "basedpyright-langserver"),
            environment,
            _runtime: refscape_language_support::process::RuntimeOwner::acquire(),
        }
    }
    pub fn new(rust_analyzer: impl Into<PathBuf>, clangd: impl Into<PathBuf>) -> Self {
        Self {
            rust_analyzer: ConfiguredExecutable::explicit(rust_analyzer),
            clangd: ConfiguredExecutable::explicit(clangd),
            ..Self::default()
        }
    }
    pub fn with_rust_analyzer(mut self, executable: impl Into<PathBuf>) -> Self {
        self.rust_analyzer = ConfiguredExecutable::explicit(executable);
        self
    }
    pub fn with_clangd(mut self, executable: impl Into<PathBuf>) -> Self {
        self.clangd = ConfiguredExecutable::explicit(executable);
        self
    }
    pub fn with_typescript_server(mut self, executable: impl Into<PathBuf>) -> Self {
        self.typescript = ConfiguredExecutable::explicit(executable);
        self
    }
    pub fn with_pyright_server(mut self, executable: impl Into<PathBuf>) -> Self {
        self.pyright = ConfiguredExecutable::explicit(executable);
        self
    }
}
fn select_language(root: &Path, options: &ProjectOpenOptions) -> Result<ProjectLanguage, String> {
    options.validate().map_err(|error| error.to_string())?;
    match options.language {
        ProjectLanguage::Rust
        | ProjectLanguage::Cpp
        | ProjectLanguage::TypeScript
        | ProjectLanguage::Python => Ok(options.language),
        ProjectLanguage::Auto if options.compilation_database.is_some() => Ok(ProjectLanguage::Cpp),
        ProjectLanguage::Auto if refscape_language_rust::supports(root) => {
            Ok(ProjectLanguage::Rust)
        }
        ProjectLanguage::Auto if refscape_language_typescript::supports(root)? => {
            Ok(ProjectLanguage::TypeScript)
        }
        ProjectLanguage::Auto if refscape_language_python::supports(root)? => {
            Ok(ProjectLanguage::Python)
        }
        ProjectLanguage::Auto if refscape_language_cpp::supports(root)? => Ok(ProjectLanguage::Cpp),
        ProjectLanguage::Auto => Err(format!(
            "Cannot detect a Rust, C/C++, TypeScript/JavaScript, or Python project in {}. Select a source folder or choose its language explicitly",
            root.display()
        )),
    }
}

impl AnalysisFactory for LanguageBackend {
    fn prepare(
        &self,
        root: &Path,
        options: &ProjectOpenOptions,
        context: &OperationContext,
    ) -> AnalysisResult<PreparedProject> {
        context.check()?;
        options.validate()?;
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
        let probe = if options.language == ProjectLanguage::Auto
            && options.compilation_database.is_none()
            && !refscape_language_rust::supports(&root)
        {
            Some(
                refscape_language_support::catalog::ProjectProbe::scan_with_context(
                    &root,
                    &[
                        refscape_language_typescript::FILE_POLICY,
                        refscape_language_python::FILE_POLICY,
                        {
                            let mut policy = refscape_language_cpp::FILE_POLICY;
                            policy.symlink_files = false;
                            policy
                        },
                    ],
                    context,
                )?,
            )
        } else {
            None
        };
        let language = if let Some(probe) = &probe {
            if root.join("tsconfig.json").is_file()
                || root.join("jsconfig.json").is_file()
                || !probe.catalogs[0].is_empty()
            {
                ProjectLanguage::TypeScript
            } else if root.join("pyrightconfig.json").is_file() || !probe.catalogs[1].is_empty() {
                ProjectLanguage::Python
            } else if !probe.catalogs[2].is_empty() || refscape_language_cpp::probe_markers(&root)?
            {
                ProjectLanguage::Cpp
            } else {
                return Err(RefscapeError::new(
                    ErrorKind::InvalidData,
                    format!(
                        "Cannot detect a Rust, C/C++, TypeScript/JavaScript, or Python project in {}. Select a source folder or choose its language explicitly",
                        root.display()
                    ),
                ));
            }
        } else {
            select_language(&root, options)
                .map_err(|e| RefscapeError::new(ErrorKind::InvalidData, e))?
        };
        let resolved = refscape_model::ResolvedProjectOptions::try_from(ProjectOpenOptions {
            language,
            compilation_database: options.compilation_database.clone(),
        })?;
        let factory: Box<dyn AnalysisFactory> = match language {
            ProjectLanguage::Rust => Box::new(RustAnalyzer::from_environment(
                &self.rust_analyzer,
                self.environment.clone(),
            )),
            ProjectLanguage::Cpp => Box::new(Clangd::from_environment(
                &self.clangd,
                self.environment.clone(),
            )),
            ProjectLanguage::TypeScript => Box::new(if let Some(probe) = &probe {
                TypeScript::from_environment(&self.typescript, self.environment.clone())
                    .with_discovery_files(probe.catalogs[0].clone())
            } else {
                TypeScript::from_environment(&self.typescript, self.environment.clone())
            }),
            ProjectLanguage::Python => Box::new(if let Some(probe) = &probe {
                Python::from_environment(&self.pyright, self.environment.clone())
                    .with_discovery_files(probe.catalogs[1].clone())
            } else {
                Python::from_environment(&self.pyright, self.environment.clone())
            }),
            ProjectLanguage::Auto => unreachable!("selection resolves automatic detection"),
        };
        factory.prepare(&root, &resolved.to_open_options(), context)
    }
}
#[cfg(test)]
mod tests;
