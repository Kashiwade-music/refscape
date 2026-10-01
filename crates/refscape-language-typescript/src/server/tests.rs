use super::*;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn local_and_global_npm_shims_resolve_to_node_without_a_shell() {
    let root = env::temp_dir().join(format!(
        "refscape-ts-server-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let package = root.join("node_modules/typescript-language-server/lib");
    fs::create_dir_all(&package).unwrap();
    fs::create_dir(root.join("node_modules/.bin")).unwrap();
    let cli = package.join("cli.mjs");
    fs::write(&cli, "").unwrap();
    let global = root.join("typescript-language-server.cmd");
    let local = root.join("node_modules/.bin/typescript-language-server.cmd");
    fs::write(&global, "").unwrap();
    fs::write(&local, "").unwrap();
    assert_eq!(shim_cli(&global), Some(cli.clone()));
    assert_eq!(shim_cli(&local), Some(cli.clone()));
    for executable in [
        global,
        local,
        cli,
        PathBuf::from("typescript-language-server"),
    ] {
        let command = command(&root, &executable).unwrap();
        assert_ne!(command.get_program(), "cmd");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args.len(), 2);
        assert_eq!(args[1], "--stdio");
        assert_eq!(
            Path::new(args[0]).canonicalize().unwrap(),
            package.join("cli.mjs").canonicalize().unwrap()
        );
    }
    let missing = root.join("absent/server.cmd");
    assert!(
        command(&root, &missing)
            .unwrap_err()
            .contains("npm entry point")
    );
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("refscape-ts-server-")
    );
    fs::remove_dir_all(root).unwrap();
}
