use super::*;
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
