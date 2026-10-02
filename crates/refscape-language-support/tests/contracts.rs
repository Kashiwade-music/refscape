use refscape_language_support::{
    EnvironmentSnapshot, SearchMergePolicy,
    catalog::{CatalogSnapshot, WalkPolicy, walk},
    resolver::{ServerKind, executable_candidates, resolve},
    runtime::{collect_matches, merge_search},
};
use refscape_model::{Position, SourceRange, Symbol};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "refscape-language-contract-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("refscape-language-contract-")
        );
        assert_eq!(
            self.0.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn symbol(path: PathBuf, name: &str, kind: &str, id: &str) -> Symbol {
    Symbol {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        path,
        range: SourceRange {
            start: Position::new(0, 0),
            end: Position::new(0, 1),
        },
        selection_range: SourceRange {
            start: Position::new(0, 0),
            end: Position::new(0, 1),
        },
        children: vec![],
    }
}
#[test]
fn merge_keeps_each_languages_equal_rank_provider_and_distinct_ids() {
    let workspace = symbol("a.py".into(), "名前", "workspace", "same");
    let document = symbol("a.py".into(), "名前", "document", "same");
    for (policy, expected) in [
        (SearchMergePolicy::WorkspaceFirst, "workspace"),
        (SearchMergePolicy::DocumentsFirst, "document"),
    ] {
        assert_eq!(
            merge_search(vec![workspace.clone()], vec![document.clone()], policy)[0].kind,
            expected
        );
        let mut other = document.clone();
        other.id = "different".into();
        other.range.start = Position::new(1, 0);
        other.range.end = Position::new(1, 1);
        assert_eq!(
            merge_search(vec![workspace.clone()], vec![other], policy).len(),
            2
        );
    }
    let mut parent = symbol("a.py".into(), "Parent", "function", "parent");
    parent.children.push(document);
    let mut matches = vec![];
    collect_matches(&[parent.clone()], "", &mut matches);
    assert_eq!(matches.len(), 2);
    matches.clear();
    collect_matches(&[parent], "名前", &mut matches);
    assert_eq!(matches.len(), 1);
}
#[test]
fn catalog_tracks_same_size_changes_deletion_and_atomic_failure() {
    let fixture = Fixture::new();
    let a = fixture.write("a.py", "alpha");
    let (first, delta) = CatalogSnapshot::default().refresh(vec![a.clone()]).unwrap();
    assert_eq!(delta.added.as_slice(), std::slice::from_ref(&a));
    let (same, delta) = first.refresh(vec![a.clone()]).unwrap();
    assert_eq!(same.revision, first.revision);
    assert!(delta.changed.is_empty());
    fixture.write("a.py", "bravo");
    let (changed, delta) = first.refresh(vec![a.clone()]).unwrap();
    assert_eq!(delta.changed.as_slice(), std::slice::from_ref(&a));
    assert_eq!(changed.revision, first.revision + 1);
    assert!(changed.refresh(vec![fixture.0.join("missing.py")]).is_err());
    assert_eq!(changed.ordered_files.as_slice(), std::slice::from_ref(&a));
    let (removed, delta) = changed.refresh(vec![]).unwrap();
    assert_eq!(delta.removed, [a]);
    assert!(removed.ordered_files.is_empty());
}
#[test]
fn cancelled_or_expired_catalog_work_never_reads_or_publishes_a_snapshot() {
    use refscape_language_support::catalog::{ProjectProbe, walk_with_context};
    use refscape_model::{ErrorKind, OperationContext};
    use std::time::Duration;

    let fixture = Fixture::new();
    let file = fixture.write("a.py", "alpha");
    let (snapshot, _) = CatalogSnapshot::default()
        .refresh(vec![file.clone()])
        .unwrap();
    let policy = WalkPolicy {
        extensions: &["py"],
        excluded: &[],
        case_insensitive: false,
        symlink_files: false,
        exclude_virtual_environments: false,
        canonical_paths: true,
    };
    for (context, kind) in [
        (
            OperationContext::detached(Duration::from_secs(5)),
            ErrorKind::Cancelled,
        ),
        (
            OperationContext::detached(Duration::ZERO),
            ErrorKind::Timeout,
        ),
    ] {
        if kind == ErrorKind::Cancelled {
            context.cancel.cancel();
        }
        assert_eq!(
            walk_with_context(&fixture.0.join("missing"), policy, false, &context)
                .unwrap_err()
                .kind,
            kind
        );
        assert_eq!(
            ProjectProbe::scan_with_context(&fixture.0.join("missing"), &[policy], &context)
                .err()
                .unwrap()
                .kind,
            kind
        );
        assert_eq!(
            snapshot
                .refresh_with_context(vec![fixture.0.join("missing.py")], &context)
                .unwrap_err()
                .kind,
            kind
        );
        assert_eq!(
            snapshot.ordered_files.as_slice(),
            std::slice::from_ref(&file)
        );
        assert_eq!(snapshot.revision, 1);
    }
}
#[test]
fn walker_has_per_profile_exclusions_and_regular_file_acceptance() {
    let fixture = Fixture::new();
    fixture.write("main.py", "x");
    fixture.write(".next/tool.py", "x");
    fixture.write("venv/ignored.py", "x");
    fixture.write("custom/pyvenv.cfg", "");
    fixture.write("custom/ignored.py", "x");
    fs::create_dir(fixture.0.join("directory.py")).unwrap();
    let policy = WalkPolicy {
        extensions: &["py"],
        excluded: &["venv"],
        case_insensitive: false,
        symlink_files: false,
        exclude_virtual_environments: true,
        canonical_paths: true,
    };
    assert_eq!(walk(&fixture.0, policy, false).unwrap().len(), 2);
    assert_eq!(walk(&fixture.0, policy, true).unwrap().len(), 1);
    let mut ts_policy = policy;
    ts_policy.excluded = &["venv", ".next"];
    assert_eq!(walk(&fixture.0, ts_policy, false).unwrap().len(), 1);
}
#[test]
fn windows_candidate_order_and_explicit_extension_are_shared() {
    assert_eq!(
        executable_candidates(Path::new("dir"), Path::new("server"), true),
        [
            "dir/server.exe",
            "dir/server.cmd",
            "dir/server.bat",
            "dir/server"
        ]
        .map(PathBuf::from)
    );
    assert_eq!(
        executable_candidates(Path::new("dir"), Path::new("server.CMD"), true),
        [PathBuf::from("dir/server.CMD")]
    );
}
#[test]
fn nearest_python_npm_precedes_farther_virtual_environment_and_node_override_is_frozen() {
    let fixture = Fixture::new();
    let script = fixture.write("near/node_modules/basedpyright/langserver.index.js", "");
    let native = if cfg!(windows) {
        ".venv/Scripts/basedpyright-langserver.exe"
    } else {
        ".venv/bin/basedpyright-langserver"
    };
    fixture.write(native, "");
    let root = fixture.0.join("near/src");
    fs::create_dir_all(&root).unwrap();
    let environment =
        EnvironmentSnapshot::from_values([("REFSCAPE_NODE".into(), "frozen-node".into())]);
    let launch = resolve(
        &root,
        &refscape_language_support::resolver::ConfiguredExecutable::default_name(
            "basedpyright-langserver",
        ),
        ServerKind::Python,
        &environment,
    )
    .unwrap();
    assert_eq!(launch.executable, PathBuf::from("frozen-node"));
    assert_eq!(
        Path::new(&launch.args[0]).canonicalize().unwrap(),
        script.canonicalize().unwrap()
    );
    assert_eq!(launch.cwd, root);
    fixture.write("near/.venv/Scripts/basedpyright-langserver.exe", "");
    fixture.write("near/.venv/bin/basedpyright-langserver", "");
    let launch = resolve(
        &root,
        &refscape_language_support::resolver::ConfiguredExecutable::default_name(
            "basedpyright-langserver",
        ),
        ServerKind::Python,
        &environment,
    )
    .unwrap();
    assert!(launch.executable.to_string_lossy().contains("near"));
    assert_eq!(launch.args, [std::ffi::OsString::from("--stdio")]);
}
#[test]
fn shared_probe_does_not_union_profile_exclusions() {
    let fixture = Fixture::new();
    fixture.write(".next/module.py", "x");
    fixture.write(".next/module.ts", "x");
    let python = WalkPolicy {
        extensions: &["py"],
        excluded: &[],
        case_insensitive: false,
        symlink_files: false,
        exclude_virtual_environments: false,
        canonical_paths: true,
    };
    let typescript = WalkPolicy {
        extensions: &["ts"],
        excluded: &[".next"],
        ..python
    };
    let probe =
        refscape_language_support::catalog::ProjectProbe::scan(&fixture.0, &[typescript, python])
            .unwrap();
    assert!(probe.catalogs[0].is_empty());
    assert_eq!(probe.catalogs[1].len(), 1);
}
#[test]
#[cfg(windows)]
fn captured_windows_environment_names_are_case_insensitive() {
    let snapshot = EnvironmentSnapshot::from_values([
        ("Path".into(), "C:/node".into()),
        ("Refscape_Node".into(), "frozen-node".into()),
    ]);
    assert_eq!(snapshot.path(), [PathBuf::from("C:/node")]);
    assert_eq!(
        snapshot.configured_executable("REFSCAPE_NODE", "node").path,
        PathBuf::from("frozen-node")
    );
}

#[test]
fn configured_launch_origins_preserve_explicit_environment_and_local_default_precedence() {
    use refscape_language_support::resolver::{ConfiguredExecutable, LaunchOrigin};
    for (kind, name, variable, entry) in [
        (
            ServerKind::TypeScript,
            "typescript-language-server",
            "REFSCAPE_TYPESCRIPT_LANGUAGE_SERVER",
            "node_modules/typescript-language-server/lib/cli.mjs",
        ),
        (
            ServerKind::Python,
            "basedpyright-langserver",
            "REFSCAPE_PYRIGHT",
            "node_modules/basedpyright/langserver.index.js",
        ),
    ] {
        let fixture = Fixture::new();
        let local = fixture.write(entry, "");
        let global_name = if cfg!(windows) {
            format!("bin/{name}.exe")
        } else {
            format!("bin/{name}")
        };
        let global = fixture.write(&global_name, "");
        let path = std::env::join_paths([fixture.0.join("bin")]).unwrap();
        let environment = EnvironmentSnapshot::from_values([
            (variable.into(), name.into()),
            ("PATH".into(), path),
            ("REFSCAPE_NODE".into(), "frozen-node".into()),
        ]);
        let from_env = environment.configured_executable(variable, name);
        let launch = resolve(&fixture.0, &from_env, kind, &environment).unwrap();
        assert_eq!(launch.origin, LaunchOrigin::ProjectLocal);
        assert_eq!(launch.configured_origin, LaunchOrigin::Environment);
        assert_eq!(launch.executable, PathBuf::from("frozen-node"));
        assert_eq!(
            Path::new(&launch.args[0]).canonicalize().unwrap(),
            local.canonicalize().unwrap()
        );
        let explicit = ConfiguredExecutable::explicit(name);
        let launch = resolve(&fixture.0, &explicit, kind, &environment).unwrap();
        assert_eq!(launch.origin, LaunchOrigin::ProjectLocal);
        assert_eq!(launch.configured_origin, LaunchOrigin::Explicit);
        assert_eq!(launch.executable, PathBuf::from("frozen-node"));
        let default = ConfiguredExecutable::default_name(name);
        let launch = resolve(&fixture.0, &default, kind, &environment).unwrap();
        assert_eq!(launch.origin, LaunchOrigin::ProjectLocal);
        assert_eq!(launch.configured_origin, LaunchOrigin::Default);
        assert_eq!(launch.node_origin, Some(LaunchOrigin::Environment));
        assert_eq!(
            Path::new(&launch.args[0]).canonicalize().unwrap(),
            local.canonicalize().unwrap()
        );
        let unrelated = ConfiguredExecutable::explicit("custom-server");
        let launch = resolve(&fixture.0, &unrelated, kind, &environment).unwrap();
        assert_eq!(launch.origin, LaunchOrigin::Explicit);
        assert_eq!(launch.executable, PathBuf::from("custom-server"));
        let explicit_global = ConfiguredExecutable::explicit(&global);
        let launch = resolve(&fixture.0, &explicit_global, kind, &environment).unwrap();
        assert_eq!(launch.origin, LaunchOrigin::Explicit);
        assert_eq!(launch.configured_origin, LaunchOrigin::Explicit);
        assert_eq!(launch.executable, global.canonicalize().unwrap());
        assert!(launch.node_origin.is_none());
    }
}
