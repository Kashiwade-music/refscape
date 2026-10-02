use super::*;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

fn fixture() -> PathBuf {
    let root = env::temp_dir().join(format!(
        "refscape-python-server-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    root
}

fn remove_fixture(root: &Path) {
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("refscape-python-server-")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn npm_shims_and_javascript_entry_points_use_node_without_a_shell() {
    let root = fixture();
    for package_name in ["basedpyright", "pyright"] {
        let package = root.join("node_modules").join(package_name);
        fs::create_dir_all(&package).unwrap();
        fs::create_dir_all(root.join("node_modules/.bin")).unwrap();
        let cli = package.join("langserver.index.js");
        fs::write(&cli, "").unwrap();
        let server = format!("{package_name}-langserver");
        let global = root.join(format!("{server}.cmd"));
        let local = root.join("node_modules/.bin").join(format!("{server}.cmd"));
        for shim in [&global, &local] {
            fs::write(shim, "").unwrap();
        }
        assert_eq!(shim_cli(&global), Some(cli.clone()));
        assert_eq!(shim_cli(&local), Some(cli.clone()));
        for executable in [global, local, cli, PathBuf::from(server)] {
            let command = command(&root, &executable).unwrap();
            let args: Vec<_> = command.get_args().collect();
            assert_ne!(command.get_program(), "cmd");
            assert_eq!(args.len(), 2);
            assert_eq!(args[1], "--stdio");
            assert_eq!(
                Path::new(args[0]).canonicalize().unwrap(),
                package.join("langserver.index.js").canonicalize().unwrap()
            );
            assert_eq!(command.get_current_dir(), Some(root.as_path()));
        }
    }
    assert!(
        command(&root, &root.join("missing/server.cmd"))
            .unwrap_err()
            .contains("npm entry point")
    );
    remove_fixture(&root);
}

#[test]
fn distribution_entry_point_and_native_executable_are_supported() {
    let root = fixture();
    let package = root.join("node_modules/basedpyright/dist");
    fs::create_dir_all(&package).unwrap();
    let script = package.join("pyright-langserver.js");
    fs::write(&script, "").unwrap();
    let command = command(&root, Path::new("basedpyright-langserver")).unwrap();
    assert_eq!(
        Path::new(command.get_args().next().unwrap())
            .canonicalize()
            .unwrap(),
        script.canonicalize().unwrap()
    );
    let native = root.join("custom-server.exe");
    fs::write(&native, "").unwrap();
    let native_command = super::command(&root, &native).unwrap();
    assert_eq!(
        Path::new(native_command.get_program()),
        native.canonicalize().unwrap()
    );
    assert_eq!(native_command.get_args().collect::<Vec<_>>(), ["--stdio"]);
    remove_fixture(&root);
}

#[test]
fn project_virtual_environment_is_preferred_over_npm() {
    let root = fixture();
    #[cfg(windows)]
    let server = root.join(".venv/Scripts/basedpyright-langserver.exe");
    #[cfg(not(windows))]
    let server = root.join(".venv/bin/basedpyright-langserver");
    fs::create_dir_all(server.parent().unwrap()).unwrap();
    fs::write(&server, "").unwrap();
    fs::create_dir_all(root.join("src/package")).unwrap();
    let command = command(
        &root.join("src/package"),
        Path::new("basedpyright-langserver"),
    )
    .unwrap();
    assert_eq!(Path::new(command.get_program()), server);
    assert_eq!(command.get_args().collect::<Vec<_>>(), ["--stdio"]);
    remove_fixture(&root);
}

#[test]
fn workspace_configuration_targets_both_pyright_families() {
    let behavior = PyrightBehavior;
    for section in ["python", "basedpyright"] {
        let settings = behavior.configuration(Some(section));
        assert_eq!(settings["analysis"]["diagnosticMode"], "workspace");
        assert_eq!(settings["analysis"]["autoSearchPaths"], true);
    }
    assert_eq!(
        behavior.configuration(Some("basedpyright.analysis"))["diagnosticMode"],
        "workspace"
    );
    assert_eq!(behavior.configuration(Some("unrelated")), json!({}));
}

#[test]
fn path_lookup_preserves_explicit_extensions_and_missing_executables() {
    let root = fixture();
    let executable = root.join("custom-server.exe");
    fs::write(&executable, "").unwrap();
    assert_eq!(
        executable_in_directory(&root, Path::new("custom-server.exe")),
        Some(executable)
    );
    assert!(executable_in_directory(&root, Path::new("missing-server")).is_none());
    remove_fixture(&root);
}

#[cfg(windows)]
#[test]
fn windows_path_lookup_prefers_npm_windows_shims_to_posix_shims() {
    let root = fixture();
    let name = Path::new("basedpyright-langserver");
    let posix = root.join(name);
    let windows = posix.with_extension("cmd");
    fs::write(&posix, "#!/bin/sh\n").unwrap();
    fs::write(&windows, "@echo off\n").unwrap();
    assert_eq!(executable_in_directory(&root, name), Some(windows.clone()));
    let package = root.join("node_modules/basedpyright");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("langserver.index.js"), "").unwrap();
    let selected = executable_in_directory(&root, name).unwrap();
    let command = command(&root, &selected).unwrap();
    assert_eq!(
        Path::new(command.get_args().next().unwrap())
            .canonicalize()
            .unwrap(),
        package.join("langserver.index.js").canonicalize().unwrap()
    );
    let native = posix.with_extension("exe");
    fs::write(&native, "").unwrap();
    assert_eq!(executable_in_directory(&root, name), Some(native));
    let extensionless = super::command(&root, &posix).unwrap();
    assert_eq!(extensionless.get_args().len(), 2);
    remove_fixture(&root);
}
