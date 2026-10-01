//! Source discovery and compilation-database selection, independent of clangd startup.
use refscape_model::{ProjectLanguage, ProjectOptions};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

pub(crate) struct CppProject {
    pub(crate) database: Option<PathBuf>,
    translation_units: Vec<PathBuf>,
}

impl CppProject {
    pub(crate) fn discover(root: &Path, options: &ProjectOptions) -> Result<Self, String> {
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
                return Err("Select compile_commands.json or the directory containing it".into());
            }
            Some(selected.canonicalize().map_err(|e| {
                format!(
                    "cannot open compilation database {}: {e}",
                    selected.display()
                )
            })?)
        } else if root.join(".clangd").is_file() {
            // Conditional project configuration belongs to clangd. Saving a guessed
            // database here would force it over .clangd during session restoration.
            None
        } else {
            let candidates = compilation_databases(root)?;
            match candidates.as_slice() {
                [] => None,
                [only] => Some(only.clone()),
                _ => {
                    return Err(format!(
                        "Multiple compilation databases found. Select compile_commands.json for the desired build configuration:\n{}",
                        candidates
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join("\n")
                    ));
                }
            }
        };
        let translation_units = database
            .as_deref()
            .map(database_files)
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            database,
            translation_units,
        })
    }

    pub(crate) fn options(&self) -> ProjectOptions {
        ProjectOptions {
            language: ProjectLanguage::Cpp,
            compilation_database: self.database.clone(),
        }
    }

    pub(crate) fn files(&self, root: &Path) -> Result<Vec<PathBuf>, String> {
        let mut output = BTreeSet::new();
        collect_cpp_files(root, &mut output)?;
        // Database entries are authoritative even when a TU lies outside the source root,
        // has a nonstandard extension, or lives in a normally excluded build directory.
        output.extend(
            self.translation_units
                .iter()
                .filter(|path| path.is_file())
                .cloned(),
        );
        Ok(output.into_iter().collect())
    }

    pub(crate) fn index_seed(&self, root: &Path) -> Result<Option<PathBuf>, String> {
        if let Some(path) = self.translation_units.first() {
            return Ok(Some(path.clone()));
        }
        let files = self.files(root)?;
        Ok(files
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
            .cloned())
    }
}

pub(crate) fn supports(root: &Path) -> Result<bool, String> {
    Ok([
        "CMakeLists.txt",
        "CMakePresets.json",
        ".clangd",
        "compile_flags.txt",
    ]
    .iter()
    .any(|name| root.join(name).is_file())
        || !compilation_databases(root)?.is_empty()
        || contains_cpp(root)?)
}

/// Search conventional build locations without descending an arbitrary directory tree.
pub(crate) fn compilation_databases(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut directories = vec![
        root.to_path_buf(),
        root.join("build"),
        root.join("out/build"),
    ];
    for parent in [root.join("build"), root.join("out/build")] {
        if !parent.is_dir() {
            continue;
        }
        for entry in
            fs::read_dir(&parent).map_err(|e| format!("cannot list {}: {e}", parent.display()))?
        {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                directories.push(entry.path());
            }
        }
    }
    let mut output = BTreeSet::new();
    for directory in directories {
        let path = directory.join("compile_commands.json");
        if path.is_file() {
            output.insert(
                path.canonicalize()
                    .map_err(|e| format!("cannot open {}: {e}", path.display()))?,
            );
        }
    }
    Ok(output.into_iter().collect())
}

fn database_files(database: &Path) -> Result<Vec<PathBuf>, String> {
    let text = fs::read_to_string(database).map_err(|e| {
        format!(
            "cannot read compilation database {}: {e}",
            database.display()
        )
    })?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| format!("invalid compilation database {}: {e}", database.display()))?;
    let entries = value.as_array().ok_or_else(|| {
        format!(
            "invalid compilation database {}: expected an array of compile commands",
            database.display()
        )
    })?;
    let mut output = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let invalid = || {
            format!(
                "invalid compilation database {}: entry {} needs directory, file, and command or nonempty arguments",
                database.display(),
                index + 1
            )
        };
        let directory = entry["directory"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(invalid)?;
        let file = entry["file"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(invalid)?;
        let valid_arguments = entry["arguments"]
            .as_array()
            .is_some_and(|args| !args.is_empty() && args.iter().all(|arg| arg.as_str().is_some()));
        if !valid_arguments
            && entry["command"]
                .as_str()
                .is_none_or(|s| s.trim().is_empty())
        {
            return Err(invalid());
        }
        let directory = Path::new(directory);
        let directory = if directory.is_absolute() {
            directory.to_path_buf()
        } else {
            database.parent().ok_or_else(invalid)?.join(directory)
        };
        let file = Path::new(file);
        let path = if file.is_absolute() {
            file.to_path_buf()
        } else {
            directory.join(file)
        };
        // Stale entries can describe sources that no longer exist; clangd handles them.
        if path.is_file() {
            output.insert(
                path.canonicalize()
                    .map_err(|e| format!("cannot resolve {}: {e}", path.display()))?,
            );
        }
    }
    Ok(output.into_iter().collect())
}

fn excluded_directory(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | "node_modules"
            | "target"
            | ".cache"
            | ".clangd"
            | "CMakeFiles"
            | ".refscape"
    )
}

pub(crate) fn cpp_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "c" | "cc"
                    | "cpp"
                    | "cxx"
                    | "c++"
                    | "h"
                    | "hh"
                    | "hpp"
                    | "hxx"
                    | "h++"
                    | "inc"
                    | "inl"
                    | "ipp"
                    | "tpp"
                    | "ixx"
                    | "cppm"
            )
        })
}

pub(crate) fn language_id(path: &Path) -> &'static str {
    // Uppercase .C is conventionally C++, while .h may be shared by both languages.
    match path.extension().and_then(|s| s.to_str()) {
        Some("c" | "h") => "c",
        _ => "cpp",
    }
}

fn contains_cpp(root: &Path) -> Result<bool, String> {
    for entry in fs::read_dir(root).map_err(|e| format!("cannot list {}: {e}", root.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() && !excluded_directory(&entry.file_name().to_string_lossy()) {
            if contains_cpp(&entry.path())? {
                return Ok(true);
            }
        } else if kind.is_file() && cpp_extension(&entry.path()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn collect_cpp_files(root: &Path, output: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|e| format!("cannot list {}: {e}", root.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if kind.is_dir() && !excluded_directory(&entry.file_name().to_string_lossy()) {
            collect_cpp_files(&path, output)?;
        } else if (kind.is_file() || kind.is_symlink() && path.is_file()) && cpp_extension(&path) {
            output.insert(
                path.canonicalize()
                    .map_err(|e| format!("cannot resolve {}: {e}", path.display()))?,
            );
        }
    }
    Ok(())
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
        let error = CppProject::discover(&fixture.0, &ProjectOptions::default())
            .err()
            .unwrap();
        assert!(
            error.contains("Multiple compilation databases")
                && error.contains("debug")
                && error.contains("release")
        );
        let project = CppProject::discover(
            &fixture.0,
            &ProjectOptions {
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
        let options = ProjectOptions {
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
                &ProjectOptions {
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
            &ProjectOptions {
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
        let project = CppProject::discover(&fixture.0, &ProjectOptions::default()).unwrap();
        assert!(project.database.is_none());
        assert_eq!(project.files(&fixture.0).unwrap().len(), 1);
        fixture.write(".clangd", "CompileFlags:\n  Add: [-std=c++20]\n");
        fixture.write("build/compile_commands.json", "[]");
        let project = CppProject::discover(&fixture.0, &ProjectOptions::default()).unwrap();
        assert!(project.database.is_none());
        let project = CppProject::discover(
            &fixture.0,
            &ProjectOptions {
                compilation_database: Some("build".into()),
                ..ProjectOptions::default()
            },
        )
        .unwrap();
        assert!(project.database.is_some());
    }
}
