use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn discovery_includes_jsx_module_extensions_and_native_variants_but_skips_dependencies() {
    let root = std::env::temp_dir().join(format!(
        "refscape-ts-files-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    assert!(!supports(&root).unwrap());
    fs::write(root.join("package.json"), "{}").unwrap();
    assert!(!supports(&root).unwrap()); // A package manifest alone does not identify JS/TS.
    let names = [
        "model.ts",
        "App.tsx",
        "App.native.tsx",
        "App.ios.tsx",
        "App.android.tsx",
        "types.d.ts",
        "types.d.mts",
        "module.mts",
        "module.cts",
        "web.jsx",
        "app.js",
        "config.mjs",
        "config.cjs",
    ];
    for name in names {
        fs::write(root.join(name), "").unwrap();
    }
    for directory in [
        "node_modules",
        ".next",
        ".expo",
        "build",
        "dist",
        "Pods",
        ".git",
    ] {
        fs::create_dir(root.join(directory)).unwrap();
        fs::write(root.join(directory).join("ignored.tsx"), "").unwrap();
    }
    fs::create_dir_all(root.join("packages/mobile/src")).unwrap();
    fs::write(root.join("packages/mobile/src/Screen.jsx"), "").unwrap();
    let found = files(&root).unwrap();
    assert_eq!(found.len(), names.len() + 1);
    assert!(supports(&root).unwrap());
    assert!(found.windows(2).all(|pair| pair[0] < pair[1]));
    for (file, id) in [
        ("App.native.tsx", "typescriptreact"),
        ("Screen.jsx", "javascriptreact"),
        ("config.mjs", "javascript"),
        ("config.cjs", "javascript"),
        ("model.cts", "typescript"),
    ] {
        assert_eq!(language_id(Path::new(file)), id);
    }
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("refscape-ts-files-")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn python_environments_and_caches_do_not_identify_a_typescript_project() {
    let root = std::env::temp_dir().join(format!(
        "refscape-ts-python-environments-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("main.py"), "def main(): pass\n").unwrap();
    for directory in [
        ".venv/Lib/site-packages/pkg/static",
        "venv",
        ".env",
        "env",
        "__pypackages__",
        "site-packages",
        "__pycache__",
        ".pytest_cache",
        ".mypy_cache",
        ".ruff_cache",
        ".pytype",
        ".tox",
        ".nox",
    ] {
        fs::create_dir_all(root.join(directory)).unwrap();
        fs::write(root.join(directory).join("client.js"), "").unwrap();
    }
    fs::create_dir_all(root.join("custom-env/Lib/package/static")).unwrap();
    fs::write(root.join("custom-env/pyvenv.cfg"), "home = python\n").unwrap();
    fs::write(root.join("custom-env/Lib/package/static/client.js"), "").unwrap();
    assert!(files(&root).unwrap().is_empty());
    assert!(!supports(&root).unwrap());

    fs::write(root.join("main.ts"), "export const answer = 42;\n").unwrap();
    assert!(supports(&root).unwrap());
    assert_eq!(files(&root).unwrap(), [root.join("main.ts")]);

    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("refscape-ts-python-environments-")
    );
    fs::remove_dir_all(root).unwrap();
}
