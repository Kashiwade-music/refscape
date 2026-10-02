use crate::{ErrorKind, RefscapeError};
use std::path::{Path, PathBuf};

/// Language backend used to analyze a source project.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ProjectLanguage {
    #[default]
    Auto,
    Rust,
    Cpp,
    TypeScript,
    Python,
}

/// Analysis settings are independent of the source root and travel with sessions.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProjectOpenOptions {
    pub language: ProjectLanguage,
    pub compilation_database: Option<PathBuf>,
}

impl ProjectOpenOptions {
    pub fn validate(&self) -> Result<(), RefscapeError> {
        if self.compilation_database.is_some()
            && !matches!(self.language, ProjectLanguage::Auto | ProjectLanguage::Cpp)
        {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                "A compilation database applies to C/C++; choose the C/C++ language",
            ));
        }
        Ok(())
    }

    /// The single decision table for CLI/session/open overrides.
    pub fn merge_overrides(&self, overrides: &Self) -> Result<Self, RefscapeError> {
        overrides.validate()?;
        let mut result = self.clone();
        match overrides.language {
            ProjectLanguage::Auto => {
                if overrides.compilation_database.is_some() {
                    result.language = ProjectLanguage::Cpp;
                    result.compilation_database = overrides.compilation_database.clone();
                }
            }
            ProjectLanguage::Cpp => {
                result.language = ProjectLanguage::Cpp;
                if overrides.compilation_database.is_some() {
                    result.compilation_database = overrides.compilation_database.clone();
                }
            }
            language => {
                result.language = language;
                result.compilation_database = None;
            }
        }
        result.validate()?;
        Ok(result)
    }
}

/// Effective analysis configuration. Auto detection belongs only to open inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProjectOptions(ProjectOpenOptions);

impl TryFrom<ProjectOpenOptions> for ResolvedProjectOptions {
    type Error = RefscapeError;

    fn try_from(options: ProjectOpenOptions) -> Result<Self, Self::Error> {
        options.validate()?;
        if options.language == ProjectLanguage::Auto {
            return Err(RefscapeError::new(
                ErrorKind::InvalidData,
                "Effective project language must be resolved",
            ));
        }
        Ok(Self(options))
    }
}

impl ResolvedProjectOptions {
    pub fn language(&self) -> ProjectLanguage {
        self.0.language
    }

    pub fn compilation_database(&self) -> Option<&Path> {
        self.0.compilation_database.as_deref()
    }

    pub fn to_open_options(&self) -> ProjectOpenOptions {
        self.0.clone()
    }
}

/// Package container reported by the language backend's official project metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCrate {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_configuration_rejects_detection_and_incompatible_database() {
        assert!(ResolvedProjectOptions::try_from(ProjectOpenOptions::default()).is_err());
        for language in [
            ProjectLanguage::Rust,
            ProjectLanguage::TypeScript,
            ProjectLanguage::Python,
        ] {
            assert!(
                ResolvedProjectOptions::try_from(ProjectOpenOptions {
                    language,
                    compilation_database: Some("build/compile_commands.json".into()),
                })
                .is_err()
            );
        }
        let input = ProjectOpenOptions {
            language: ProjectLanguage::Cpp,
            compilation_database: Some("build/compile_commands.json".into()),
        };
        let resolved = ResolvedProjectOptions::try_from(input.clone()).unwrap();
        assert_eq!(resolved.language(), ProjectLanguage::Cpp);
        assert_eq!(
            resolved.compilation_database(),
            input.compilation_database.as_deref()
        );
        assert_eq!(resolved.to_open_options(), input);
    }

    #[test]
    fn open_override_decision_table_preserves_and_clears_database_as_specified() {
        let saved = ProjectOpenOptions {
            language: ProjectLanguage::Cpp,
            compilation_database: Some("saved/compile_commands.json".into()),
        };
        for language in [
            ProjectLanguage::Auto,
            ProjectLanguage::Cpp,
            ProjectLanguage::Rust,
            ProjectLanguage::TypeScript,
            ProjectLanguage::Python,
        ] {
            let overrides = ProjectOpenOptions {
                language,
                compilation_database: None,
            };
            let merged = saved.merge_overrides(&overrides).unwrap();
            assert_eq!(
                merged.language,
                if language == ProjectLanguage::Auto {
                    ProjectLanguage::Cpp
                } else {
                    language
                }
            );
            assert_eq!(
                merged.compilation_database.is_some(),
                matches!(language, ProjectLanguage::Auto | ProjectLanguage::Cpp)
            );
            let overrides = ProjectOpenOptions {
                compilation_database: Some("override/compile_commands.json".into()),
                ..overrides
            };
            if matches!(language, ProjectLanguage::Auto | ProjectLanguage::Cpp) {
                let merged = saved.merge_overrides(&overrides).unwrap();
                assert_eq!(merged.language, ProjectLanguage::Cpp);
                assert_eq!(merged.compilation_database, overrides.compilation_database);
            } else {
                assert_eq!(
                    saved.merge_overrides(&overrides).unwrap_err().kind,
                    ErrorKind::InvalidData
                );
            }
        }
        let auto = ProjectOpenOptions::default();
        assert_eq!(auto.merge_overrides(&auto).unwrap(), auto);
    }
}
