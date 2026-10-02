//! Source discovery and compilation-database selection, independent of clangd startup.
use refscape_model::{
    ErrorKind, OperationContext, ProjectLanguage, ProjectOpenOptions, RefscapeError,
};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fs,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompilationConfig {
    ExplicitDatabase(PathBuf),
    DetectedDatabase(PathBuf),
    DelegateToClangd,
    Fallback,
}

pub(crate) struct CppProject {
    pub(crate) database: Option<PathBuf>,
    translation_units: Vec<PathBuf>,
    pub(crate) configuration: CompilationConfig,
    pub(crate) metadata_fingerprint: u64,
}

impl CppProject {
    #[cfg(test)]
    pub(crate) fn discover(root: &Path, options: &ProjectOpenOptions) -> Result<Self, String> {
        Self::discover_with_context(
            root,
            options,
            &OperationContext::detached(std::time::Duration::from_secs(120)),
        )
        .map_err(|error| error.to_string())
    }

    pub(crate) fn discover_with_context(
        root: &Path,
        options: &ProjectOpenOptions,
        context: &OperationContext,
    ) -> Result<Self, RefscapeError> {
        context.check()?;
        let result = Self::discover_snapshot(root, options, context);
        context.check()?;
        result
    }

    fn discover_snapshot(
        root: &Path,
        options: &ProjectOpenOptions,
        context: &OperationContext,
    ) -> Result<Self, RefscapeError> {
        let delegate_to_clangd = root.join(".clangd").is_file();
        let database = if let Some(selected) = &options.compilation_database {
            let selected = if selected.is_absolute() {
                selected.clone()
            } else {
                root.join(selected)
            };
            let selected = if selected.is_dir() {
                selected.join("compile_commands.json")
            } else {
                selected
            };
            if selected
                .file_name()
                .is_none_or(|name| name != "compile_commands.json")
            {
                return Err(RefscapeError::new(
                    ErrorKind::InvalidData,
                    "Select compile_commands.json or the directory containing it",
                ));
            }
            Some(selected.canonicalize().map_err(|e| {
                RefscapeError::new(
                    ErrorKind::Io,
                    format!(
                        "cannot open compilation database {}: {e}",
                        selected.display()
                    ),
                )
            })?)
        } else if delegate_to_clangd {
            // Conditional project configuration belongs to clangd. Saving a guessed
            // database here would force it over .clangd during session restoration.
            None
        } else {
            let candidates = compilation_databases_with_context(root, context)?;
            match candidates.as_slice() {
                [] => None,
                [only] => Some(only.clone()),
                _ => {
                    return Err(RefscapeError::new(
                        ErrorKind::InvalidData,
                        format!(
                            "Multiple compilation databases found. Select compile_commands.json for the desired build configuration:\n{}",
                            candidates
                                .iter()
                                .map(|path| path.display().to_string())
                                .collect::<Vec<_>>()
                                .join("\n")
                        ),
                    ));
                }
            }
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        database.hash(&mut hasher);
        let translation_units = database
            .as_deref()
            .map(|database| database_files(database, context, &mut hasher))
            .transpose()?
            .unwrap_or_default();
        for name in [".clangd", "compile_flags.txt"] {
            context.check()?;
            let path = root.join(name);
            if path.is_file() {
                path.hash(&mut hasher);
                fs::read(&path)
                    .map_err(|error| {
                        RefscapeError::new(
                            ErrorKind::Io,
                            format!("cannot read {}: {error}", path.display()),
                        )
                    })?
                    .hash(&mut hasher);
            }
        }
        let configuration = match &database {
            Some(path) if options.compilation_database.is_some() => {
                CompilationConfig::ExplicitDatabase(path.clone())
            }
            Some(path) => CompilationConfig::DetectedDatabase(path.clone()),
            None if delegate_to_clangd => CompilationConfig::DelegateToClangd,
            None => CompilationConfig::Fallback,
        };
        Ok(Self {
            configuration,
            database,
            translation_units,
            metadata_fingerprint: hasher.finish(),
        })
    }

    pub(crate) fn options(&self) -> ProjectOpenOptions {
        ProjectOpenOptions {
            language: ProjectLanguage::Cpp,
            compilation_database: self.database.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn files(&self, root: &Path) -> Result<Vec<PathBuf>, String> {
        self.files_with_context(
            root,
            &OperationContext::detached(std::time::Duration::from_secs(120)),
        )
        .map_err(|error| error.to_string())
    }

    pub(crate) fn files_with_context(
        &self,
        root: &Path,
        context: &OperationContext,
    ) -> Result<Vec<PathBuf>, RefscapeError> {
        let mut output: BTreeSet<_> =
            refscape_language_support::catalog::walk_with_context(root, POLICY, false, context)?
                .into_iter()
                .collect();
        // Database entries are authoritative even when a TU lies outside the source root,
        // has a nonstandard extension, or lives in a normally excluded build directory.
        for path in &self.translation_units {
            context.check()?;
            if path.is_file() {
                output.insert(path.clone());
            }
        }
        Ok(output.into_iter().collect())
    }

    pub(crate) fn index_seed_from_files(&self, files: &[PathBuf]) -> Option<PathBuf> {
        if let Some(path) = self.translation_units.first() {
            return Some(path.clone());
        }
        files
            .iter()
            .find(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        matches!(
                            extension.to_ascii_lowercase().as_str(),
                            "c" | "cc" | "cpp" | "cxx" | "c++" | "ixx" | "cppm"
                        )
                    })
            })
            .or_else(|| files.first())
            .cloned()
    }
}

pub(crate) fn probe_markers(root: &Path) -> Result<bool, String> {
    Ok([
        "CMakeLists.txt",
        "CMakePresets.json",
        ".clangd",
        "compile_flags.txt",
    ]
    .iter()
    .any(|name| root.join(name).is_file())
        || !compilation_databases(root)?.is_empty())
}

pub(crate) fn supports(root: &Path) -> Result<bool, String> {
    Ok(probe_markers(root)? || contains_cpp(root)?)
}

/// Search conventional build locations without descending an arbitrary directory tree.
pub(crate) fn compilation_databases(root: &Path) -> Result<Vec<PathBuf>, String> {
    compilation_databases_with_context(
        root,
        &OperationContext::detached(std::time::Duration::from_secs(120)),
    )
    .map_err(|error| error.to_string())
}
fn compilation_databases_with_context(
    root: &Path,
    context: &OperationContext,
) -> Result<Vec<PathBuf>, RefscapeError> {
    let mut directories = vec![
        root.to_path_buf(),
        root.join("build"),
        root.join("out/build"),
    ];
    for parent in [root.join("build"), root.join("out/build")] {
        context.check()?;
        if !parent.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&parent).map_err(|e| {
            RefscapeError::new(
                ErrorKind::Io,
                format!("cannot list {}: {e}", parent.display()),
            )
        })? {
            context.check()?;
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                directories.push(entry.path());
            }
        }
    }
    let mut output = BTreeSet::new();
    for directory in directories {
        context.check()?;
        let path = directory.join("compile_commands.json");
        if path.is_file() {
            output.insert(path.canonicalize().map_err(|e| {
                RefscapeError::new(
                    ErrorKind::Io,
                    format!("cannot open {}: {e}", path.display()),
                )
            })?);
        }
    }
    Ok(output.into_iter().collect())
}

#[derive(Deserialize)]
struct CompileCommand {
    directory: String,
    file: String,
    command: Option<String>,
    arguments: Option<Vec<String>>,
}
struct SnapshotReader<'a, R> {
    reader: R,
    hasher: &'a mut std::collections::hash_map::DefaultHasher,
    context: &'a OperationContext,
}
impl<R: Read> Read for SnapshotReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.context.check().map_err(std::io::Error::other)?;
        let length = self.reader.read(buffer)?;
        self.hasher.write(&buffer[..length]);
        Ok(length)
    }
}
fn database_files(
    database: &Path,
    context: &OperationContext,
    hasher: &mut std::collections::hash_map::DefaultHasher,
) -> Result<Vec<PathBuf>, RefscapeError> {
    let reader = fs::File::open(database).map_err(|e| {
        RefscapeError::new(
            ErrorKind::Io,
            format!(
                "cannot read compilation database {}: {e}",
                database.display()
            ),
        )
    })?;
    let reader = SnapshotReader {
        reader,
        hasher,
        context,
    };
    let entries: Vec<CompileCommand> = serde_json::from_reader(std::io::BufReader::new(reader))
        .map_err(|e| {
            RefscapeError::new(
                if e.is_io() {
                    ErrorKind::Io
                } else {
                    ErrorKind::InvalidData
                },
                format!("invalid compilation database {}: {e}", database.display()),
            )
        })?;
    let mut output = BTreeSet::new();
    for (index, entry) in entries.into_iter().enumerate() {
        context.check()?;
        let invalid = || {
            RefscapeError::new(
                ErrorKind::InvalidData,
                format!(
                    "invalid compilation database {}: entry {} needs directory, file, and command or nonempty arguments",
                    database.display(),
                    index + 1
                ),
            )
        };
        if entry.directory.is_empty()
            || entry.file.is_empty()
            || (!entry
                .arguments
                .as_ref()
                .is_some_and(|args| !args.is_empty())
                && entry
                    .command
                    .as_ref()
                    .is_none_or(|command| command.trim().is_empty()))
        {
            return Err(invalid());
        }
        let directory = Path::new(&entry.directory);
        let directory = if directory.is_absolute() {
            directory.to_path_buf()
        } else {
            database.parent().ok_or_else(invalid)?.join(directory)
        };
        let file = Path::new(&entry.file);
        let path = if file.is_absolute() {
            file.to_path_buf()
        } else {
            directory.join(file)
        };
        if path.is_file() {
            output.insert(path.canonicalize().map_err(|e| {
                RefscapeError::new(
                    ErrorKind::Io,
                    format!("cannot resolve {}: {e}", path.display()),
                )
            })?);
        }
    }
    Ok(output.into_iter().collect())
}
pub(crate) fn language_id(path: &Path) -> &'static str {
    // Uppercase .C is conventionally C++, while .h may be shared by both languages.
    match path.extension().and_then(|s| s.to_str()) {
        Some("c" | "h") => "c",
        _ => "cpp",
    }
}

pub const POLICY: refscape_language_support::catalog::WalkPolicy =
    refscape_language_support::catalog::WalkPolicy {
        extensions: &[
            "c", "cc", "cpp", "cxx", "c++", "h", "hh", "hpp", "hxx", "h++", "inc", "inl", "ipp",
            "tpp", "ixx", "cppm",
        ],
        excluded: &[
            ".git",
            ".hg",
            ".svn",
            "node_modules",
            "target",
            ".cache",
            ".clangd",
            "CMakeFiles",
            ".refscape",
        ],
        case_insensitive: true,
        symlink_files: true,
        exclude_virtual_environments: false,
        canonical_paths: true,
    };
fn contains_cpp(root: &Path) -> Result<bool, String> {
    let mut policy = POLICY;
    policy.symlink_files = false;
    Ok(!refscape_language_support::catalog::walk(root, policy, true)?.is_empty())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "refscape-cpp-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
        fn write(&self, path: &str, text: &str) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let temporary = env::temp_dir().canonicalize().unwrap();
            assert_eq!(self.0.parent(), Some(temporary.as_path()));
            assert!(
                self.0
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("refscape-cpp-")
            );
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn database_discovery_is_bounded_and_ambiguity_is_actionable() {
        let fixture = Fixture::new();
        fixture.write("build/debug/compile_commands.json", "[]");
        fixture.write("out/build/release/compile_commands.json", "[]");
        fixture.write("vendor/nested/compile_commands.json", "[]");
        fixture.write("build/deep/nested/compile_commands.json", "[]");
        assert_eq!(compilation_databases(&fixture.0).unwrap().len(), 2);
        let error = CppProject::discover(&fixture.0, &ProjectOpenOptions::default())
            .err()
            .unwrap();
        assert!(
            error.contains("Multiple compilation databases")
                && error.contains("debug")
                && error.contains("release")
        );
        let project = CppProject::discover(
            &fixture.0,
            &ProjectOpenOptions {
                language: ProjectLanguage::Cpp,
                compilation_database: Some("build/debug".into()),
            },
        )
        .unwrap();
        assert_eq!(
            project.database.unwrap(),
            fixture.0.join("build/debug/compile_commands.json")
        );
    }
    #[test]
    fn invalid_databases_are_rejected_before_server_startup() {
        let fixture = Fixture::new();
        let options = ProjectOpenOptions {
            language: ProjectLanguage::Cpp,
            compilation_database: Some("compile_commands.json".into()),
        };
        assert!(
            CppProject::discover(&fixture.0, &options)
                .err()
                .unwrap()
                .contains("cannot open")
        );
        for text in [
            "bad JSON",
            "{}",
            "[{}]",
            r#"[{"directory":".","file":"main.c","arguments":[]}]"#,
        ] {
            fixture.write("compile_commands.json", text);
            assert!(
                CppProject::discover(&fixture.0, &options).is_err(),
                "{text}"
            );
        }
        fixture.write("custom.json", "[]");
        assert!(
            CppProject::discover(
                &fixture.0,
                &ProjectOpenOptions {
                    language: ProjectLanguage::Cpp,
                    compilation_database: Some("custom.json".into())
                }
            )
            .is_err()
        );
    }
    #[test]
    fn source_root_database_and_command_working_directory_are_distinct() {
        let fixture = Fixture::new();
        fixture.write("source/include/api.hpp", "struct API {};");
        fixture.write("source/main.cpp", "int main() {}");
        fixture.write("external/custom.unit", "void external() {}");
        fixture.write("source/vendor/helper.h", "void helper();");
        fixture.write("source/target/ignored.cpp", "int ignored;");
        fixture.write("build/compile_commands.json", &serde_json::json!([
            {"directory":fixture.0.join("source"),"file":"main.cpp","arguments":["clang++","-c","main.cpp"]},
            {"directory":"../external","file":"custom.unit","command":"clang++ -x c++ -c custom.unit"}
        ]).to_string());
        let source = fixture.0.join("source");
        let project = CppProject::discover(
            &source,
            &ProjectOpenOptions {
                language: ProjectLanguage::Cpp,
                compilation_database: Some("../build/compile_commands.json".into()),
            },
        )
        .unwrap();
        assert_eq!(
            project.options().compilation_database,
            Some(fixture.0.join("build/compile_commands.json"))
        );
        assert_eq!(
            project.files(&source).unwrap(),
            [
                fixture.0.join("external/custom.unit"),
                source.join("include/api.hpp"),
                source.join("main.cpp"),
                source.join("vendor/helper.h")
            ]
        );
    }
    #[test]
    fn detection_and_language_ids_cover_c_and_cpp() {
        let fixture = Fixture::new();
        assert!(!supports(&fixture.0).unwrap());
        fixture.write("src/main.c", "int main(void) { return 0; }");
        assert!(supports(&fixture.0).unwrap());
        for (path, expected) in [
            ("a.c", "c"),
            ("a.C", "cpp"),
            ("a.h", "c"),
            ("a.hpp", "cpp"),
            ("a.cc", "cpp"),
        ] {
            assert_eq!(language_id(Path::new(path)), expected);
        }
    }
    #[test]
    fn fallback_and_clangd_configuration_are_preserved() {
        let fixture = Fixture::new();
        fixture.write("main.cpp", "int main() {}");
        let project = CppProject::discover(&fixture.0, &ProjectOpenOptions::default()).unwrap();
        assert!(project.database.is_none());
        assert_eq!(project.files(&fixture.0).unwrap().len(), 1);
        fixture.write(".clangd", "CompileFlags:\n  Add: [-std=c++20]\n");
        fixture.write("build/compile_commands.json", "[]");
        let project = CppProject::discover(&fixture.0, &ProjectOpenOptions::default()).unwrap();
        assert!(project.database.is_none());
        let project = CppProject::discover(
            &fixture.0,
            &ProjectOpenOptions {
                compilation_database: Some("build".into()),
                ..ProjectOpenOptions::default()
            },
        )
        .unwrap();
        assert!(project.database.is_some());
    }
}
