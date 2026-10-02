use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn discovery_includes_python_and_stubs_and_excludes_environments_and_generated_files() {
    let root = std::env::temp_dir().join(format!(
        "refscape-python-files-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    assert!(!supports(&root).unwrap());
    fs::write(root.join("pyproject.toml"), "[tool.ruff]\n").unwrap();
    assert!(!supports(&root).unwrap());
    for file in [
        "module.py",
        "stubs.pyi",
        "ignore.ipynb",
        "ignore.pyc",
        "ignore.rs",
    ] {
        fs::write(root.join(file), "").unwrap();
    }
    for directory in [
        ".git",
        "node_modules",
        "target",
        "build",
        "dist",
        "coverage",
        ".venv",
        "venv",
        ".env",
        "env",
        "__pycache__",
        ".pytest_cache",
        ".mypy_cache",
        ".ruff_cache",
        ".tox",
        ".nox",
        "site-packages",
        "__pypackages__",
    ] {
        fs::create_dir(root.join(directory)).unwrap();
        fs::write(root.join(directory).join("excluded.py"), "").unwrap();
    }
    fs::create_dir(root.join("custom-environment")).unwrap();
    fs::write(
        root.join("custom-environment/pyvenv.cfg"),
        "home = python\n",
    )
    .unwrap();
    fs::write(root.join("custom-environment/excluded.pyi"), "").unwrap();
    fs::create_dir_all(root.join("packages/nested")).unwrap();
    fs::write(root.join("packages/nested/worker.py"), "").unwrap();
    let found = files(&root).unwrap();
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(supports(&root).unwrap());
    assert_eq!(language_id(Path::new("module.py")), "python");
    assert_eq!(language_id(Path::new("stubs.pyi")), "python");
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("refscape-python-files-")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn pyright_config_identifies_an_empty_python_workspace() {
    let root = std::env::temp_dir().join(format!(
        "refscape-python-config-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("pyrightconfig.json"), "{}").unwrap();
    assert!(supports(&root).unwrap());
    assert!(files(&root).unwrap().is_empty());
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("refscape-python-config-")
    );
    fs::remove_dir_all(root).unwrap();
}
