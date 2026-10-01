use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use xtask::{MAX_FILE_LINES, check_source_rules, gate};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "refscape-source-rules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let fixture = Self { root };
        fixture.git(&["init", "--quiet"]);
        fixture.write("Cargo.toml", "[package]\nname = \"fixture\"\n");
        fixture
    }

    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(&self.root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn error(&self) -> String {
        check_source_rules(&self.root).unwrap_err()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // This unique directory was created and is exclusively owned by the fixture.
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn exactly_1000_lines_pass_with_lf_crlf_and_no_final_newline() {
    let fixture = Fixture::new();
    assert_eq!(MAX_FILE_LINES, 1000);
    fixture.write("src/lib.rs", &"code();\n".repeat(MAX_FILE_LINES));
    fixture.write("main.cpp", &"code();\r\n".repeat(MAX_FILE_LINES));
    fixture.write("notes.rs", &vec!["code();"; MAX_FILE_LINES].join("\n"));
    fixture.write("empty.txt", "");
    check_source_rules(&fixture.root).unwrap();
}

#[test]
fn all_line_violations_are_reported_including_tooling_and_examples() {
    let fixture = Fixture::new();
    for file in [
        "src/lib.rs",
        "xtask/src/lib.rs",
        "examples/demo/src/main.rs",
        "examples/cpp-demo/src/main.cpp",
    ] {
        fixture.write(file, &"code();\n".repeat(MAX_FILE_LINES + 1));
    }
    fixture.write("xtask/Cargo.toml", "");
    fixture.write("examples/demo/Cargo.toml", "");
    let error = fixture.error();
    assert_eq!(
        error
            .matches("max_file_lines exceeded (1001 > 1000)")
            .count(),
        4,
        "{error}"
    );
    assert!(!error.contains("child module directory"), "{error}");
}

#[test]
fn over_1000_physical_lines_pass_when_only_1000_contain_code() {
    let fixture = Fixture::new();
    let source = format!(
        "{}{}",
        "\n// comment\n/* block\ncomment */\nr#\"string\ncontents\"#\n".repeat(1001),
        "code();\n".repeat(1000)
    );
    fixture.write("src/lib.rs", &source);
    check_source_rules(&fixture.root).unwrap();
    fixture.write("src/lib.rs", &format!("{source}code();\n"));
    assert!(
        fixture
            .error()
            .contains("max_file_lines exceeded (1001 > 1000)")
    );
}

#[test]
fn mod_rs_is_rejected_by_clippy_using_each_workspace_policy() {
    let fixture = Fixture::new();
    fixture.write("src/lib.rs", "mod parser;\n");
    fixture.write("src/parser/mod.rs", "");
    // The custom check leaves mod.rs diagnostics to Clippy.
    check_source_rules(&fixture.root).unwrap();
    for (manifest, workspace) in [
        (include_str!("../../Cargo.toml"), true),
        (include_str!("../Cargo.toml"), false),
        (include_str!("../../examples/demo/Cargo.toml"), false),
    ] {
        let manifest: toml::Table = manifest.parse().unwrap();
        let lints = if workspace {
            &manifest["workspace"]["lints"]
        } else {
            &manifest["lints"]
        };
        let level = lints["clippy"]["mod_module_files"].as_str().unwrap();
        fixture.write("Cargo.toml", &format!(
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n[lints.clippy]\nmod_module_files = \"{level}\"\n"
        ));
        let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .current_dir(&fixture.root)
            .args(["clippy", "--offline", "--all-targets"])
            .output()
            .unwrap();
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{diagnostics}");
        assert!(diagnostics.contains("#mod_module_files"), "{diagnostics}");
    }
}

#[test]
fn nested_child_directories_require_each_parent_file() {
    let fixture = Fixture::new();
    fixture.write("src/parser/token/kind.rs", "");
    let error = fixture.error();
    assert!(
        error.contains("src/parser/: child module directory requires src/parser.rs"),
        "{error}"
    );
    assert!(
        error.contains("src/parser/token/: child module directory requires src/parser/token.rs"),
        "{error}"
    );
    fixture.write("src/parser.rs", "mod token;\n");
    fixture.write("src/parser/token.rs", "mod kind;\n");
    check_source_rules(&fixture.root).unwrap();
}

#[test]
fn cargo_target_directories_are_roots_but_nested_main_is_not_an_escape() {
    let fixture = Fixture::new();
    for container in ["src/bin", "tests", "examples", "benches"] {
        fixture.write(&format!("{container}/demo/main.rs"), "");
        fixture.write(&format!("{container}/demo/helper.rs"), "");
        fixture.write(&format!("{container}/single.rs"), "");
    }
    fixture.write("tests/library/lib.rs", "");
    check_source_rules(&fixture.root).unwrap();
    fixture.write("src/parser/main.rs", "");
    assert!(fixture.error().contains("requires src/parser.rs"));
}

#[test]
fn non_rust_asset_directories_do_not_require_module_parents() {
    let fixture = Fixture::new();
    fixture.write("assets/icons/readme.txt", "");
    fixture.write("examples/cpp-demo/src/main.cpp", "int main() {}\n");
    check_source_rules(&fixture.root).unwrap();
}

#[test]
fn new_files_are_checked_ignored_files_are_skipped_and_tracked_ignored_files_are_checked() {
    let fixture = Fixture::new();
    fixture.write(".gitignore", "target/\nignored.rs\n");
    fixture.write("target/generated/mod.rs", &"code();\n".repeat(1001));
    fixture.write("ignored.rs", &"code();\n".repeat(1001));
    check_source_rules(&fixture.root).unwrap();
    fixture.git(&["add", "--force", "ignored.rs"]);
    assert!(
        fixture
            .error()
            .contains("ignored.rs: max_file_lines exceeded")
    );
    fs::remove_file(fixture.root.join("ignored.rs")).unwrap();
    check_source_rules(&fixture.root).unwrap();
    fixture.write("new file.rs", &"code();\n".repeat(1001));
    assert!(
        fixture
            .error()
            .contains("new file.rs: max_file_lines exceeded")
    );
}

#[test]
fn generated_lockfiles_graph_and_binary_assets_are_exempt() {
    let fixture = Fixture::new();
    fixture.write("Cargo.lock", &"code();\n".repeat(1001));
    fixture.write("examples/demo/Cargo.lock", &"code();\n".repeat(1001));
    fixture.write("docs/dependency-graph.md", &"code();\n".repeat(1001));
    fixture.write("docs/notes.md", &"text\n".repeat(1001));
    fs::write(fixture.root.join("asset.bin"), [0xff, 0xfe, b'\n']).unwrap();
    fs::write(fixture.root.join("asset.png"), [0, b'\n']).unwrap();
    check_source_rules(&fixture.root).unwrap();
}

#[test]
fn gate_rejects_source_rules_before_architecture_or_graph_generation() {
    let fixture = Fixture::new();
    fixture.write("src/lib.rs", &"code();\n".repeat(1001));
    fixture.write("docs/dependency-graph.md", "existing graph\n");
    assert!(
        gate(&fixture.root)
            .unwrap_err()
            .contains("max_file_lines exceeded")
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("docs/dependency-graph.md")).unwrap(),
        "existing graph\n"
    );
}

#[test]
fn missing_git_repository_fails_closed() {
    let fixture = Fixture::new();
    let isolated = fixture.root.join("isolated");
    fs::create_dir(&isolated).unwrap();
    // An invalid Git directory prevents discovery of the enclosing repository.
    fs::write(isolated.join(".git"), "gitdir: missing\n").unwrap();
    assert!(
        check_source_rules(&isolated)
            .unwrap_err()
            .contains("cannot list repository files")
    );
}
