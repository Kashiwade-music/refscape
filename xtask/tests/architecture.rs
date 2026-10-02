use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use xtask::{check_architecture, dependency_graph, write_dependency_graph};

const NAMES: &[&str] = &[
    "refscape-model",
    "refscape-analysis",
    "refscape-canvas",
    "refscape-application",
    "refscape-lsp",
    "refscape-language-support",
    "refscape-language-rust",
    "refscape-language-cpp",
    "refscape-language-typescript",
    "refscape-language-python",
    "refscape-language",
    "refscape-storage",
    "refscape-ui",
    "refscape-app",
];
const EDGES: &[(&str, &[&str])] = &[
    ("refscape-model", &[]),
    ("refscape-analysis", &["refscape-model"]),
    ("refscape-canvas", &["refscape-model"]),
    (
        "refscape-application",
        &["refscape-model", "refscape-analysis", "refscape-canvas"],
    ),
    ("refscape-lsp", &["refscape-model", "refscape-analysis"]),
    (
        "refscape-language-support",
        &["refscape-model", "refscape-analysis", "refscape-lsp"],
    ),
    (
        "refscape-language-rust",
        &[
            "refscape-model",
            "refscape-analysis",
            "refscape-lsp",
            "refscape-language-support",
        ],
    ),
    (
        "refscape-language-cpp",
        &[
            "refscape-model",
            "refscape-analysis",
            "refscape-lsp",
            "refscape-language-support",
        ],
    ),
    (
        "refscape-language-typescript",
        &[
            "refscape-model",
            "refscape-analysis",
            "refscape-lsp",
            "refscape-language-support",
        ],
    ),
    (
        "refscape-language-python",
        &[
            "refscape-model",
            "refscape-analysis",
            "refscape-lsp",
            "refscape-language-support",
        ],
    ),
    (
        "refscape-language",
        &[
            "refscape-model",
            "refscape-analysis",
            "refscape-language-support",
            "refscape-language-rust",
            "refscape-language-cpp",
            "refscape-language-typescript",
            "refscape-language-python",
        ],
    ),
    (
        "refscape-storage",
        &["refscape-model", "refscape-application"],
    ),
    ("refscape-ui", &["refscape-model", "refscape-application"]),
    (
        "refscape-app",
        &[
            "refscape-model",
            "refscape-analysis",
            "refscape-application",
            "refscape-language",
            "refscape-storage",
            "refscape-ui",
        ],
    ),
];

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "refscape-architecture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Never reuse an existing directory or take ownership of its contents.
        fs::create_dir(&root).unwrap();
        let fixture = Self { root };
        let members = NAMES
            .iter()
            .map(|name| format!("\"crates/{name}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let shared = NAMES
            .iter()
            .map(|name| format!("{name} = {{ path = \"crates/{name}\" }}\n"))
            .collect::<String>();
        fixture.write("Cargo.toml", &format!(
            "[workspace]\nmembers = [{members}]\nexclude = [\"xtask\"]\nresolver = \"3\"\n\n[workspace.dependencies]\n{shared}"
        ));
        for (name, dependencies) in EDGES {
            let dependencies = dependencies
                .iter()
                .map(|name| format!("{name}.workspace = true\n"))
                .collect::<String>();
            fixture.write(&format!("crates/{name}/Cargo.toml"), &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{dependencies}"
            ));
            fixture.write(&format!("crates/{name}/src/lib.rs"), "");
        }
        fixture.append("crates/refscape-ui/Cargo.toml", "gpui = { git = \"https://github.com/zed-industries/zed\", rev = \"40180d9c40e2d20eb63d388bff920818f2910b53\" }\n");
        fixture.write("xtask/Cargo.toml", "[package]\nname = \"xtask\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\nresolver = \"3\"\n");
        fixture.write("xtask/src/main.rs", "fn main() {}\n");
        fixture
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn replace(&self, relative: &str, from: &str, to: &str) {
        let contents = fs::read_to_string(self.root.join(relative)).unwrap();
        assert!(contents.contains(from), "fixture replacement must match");
        self.write(relative, &contents.replace(from, to));
    }

    fn append(&self, relative: &str, contents: &str) {
        let original = fs::read_to_string(self.root.join(relative)).unwrap();
        self.write(relative, &format!("{original}\n{contents}"));
    }

    fn error(&self) -> String {
        check_architecture(&self.root).unwrap_err()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // This is the unique directory exclusively created and owned by this fixture.
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn valid_workspace_passes_without_fetching_the_reserved_gpui_dependency() {
    let fixture = Fixture::new();
    let report = check_architecture(&fixture.root).unwrap();
    assert!(report.contains("14 product crates, 43 declared internal dependencies"));
    assert!(report.contains("xtask isolated"));
    assert!(!fixture.root.join("Cargo.lock").exists());
}

#[test]
fn graph_contains_actual_edges_and_the_isolated_xtask() {
    let fixture = Fixture::new();
    fixture.replace(
        "crates/refscape-ui/Cargo.toml",
        "refscape-model.workspace = true",
        "",
    );
    let graph = dependency_graph(&fixture.root).unwrap();
    let graph = graph
        .split("## Direct external dependencies")
        .next()
        .unwrap();
    assert!(graph.contains("```mermaid\nflowchart TD"));
    assert!(graph.contains("refscape_model[\"refscape-model\"]"));
    assert!(graph.contains("refscape_ui -->|\"normal\"| refscape_application"));
    assert!(!graph.contains("refscape_ui -->|\"normal\"| refscape_model"));
    assert_eq!(graph.matches(" -->|").count(), 42);
    assert!(graph.contains("xtask[\"xtask\"]"));
    assert!(!graph.contains("xtask -->"));
    assert!(!graph.contains("gpui"));
}

#[test]
fn graph_is_reproducible_across_paths_and_dependency_aliases() {
    let first = Fixture::new();
    let second = Fixture::new();
    second.append(
        "Cargo.toml",
        "model_alias = { package = \"refscape-model\", path = \"crates/refscape-model\" }\n",
    );
    second.replace(
        "crates/refscape-application/Cargo.toml",
        "refscape-model.workspace = true",
        "model_alias.workspace = true",
    );
    assert_eq!(
        dependency_graph(&first.root).unwrap(),
        dependency_graph(&second.root).unwrap()
    );
}

#[test]
fn graph_labels_include_inactive_conditions_and_escape_target_quotes() {
    let fixture = Fixture::new();
    fixture.append("crates/refscape-ui/Cargo.toml", "[target.'cfg(target_os = \"windows\")'.build-dependencies]\nrefscape-model = { workspace = true, optional = true }\n");
    let graph = dependency_graph(&fixture.root).unwrap();
    assert!(graph.contains("refscape_ui -->|\"build, optional, target=cfg(target_os = #34;windows#34;)\"| refscape_model"), "{graph}");
}

fn external_node(graph: &str, package: &str) -> String {
    let label = format!("[\"{package}\"]");
    graph
        .lines()
        .find_map(|line| line.trim().strip_suffix(&label).map(str::to_owned))
        .unwrap_or_else(|| panic!("external package {package} missing in {graph}"))
}

#[test]
fn external_graph_contains_direct_product_and_tooling_dependencies_with_conditions() {
    let fixture = Fixture::new();
    fixture.append(
        "Cargo.toml",
        "data = { package = \"serde\", version = \"1\" }\nunused-external = \"1\"\n",
    );
    fixture.append(
        "crates/refscape-model/Cargo.toml",
        "serde = \"1\"\n[target.'cfg(target_os = \"windows\")'.build-dependencies]\nexternal-build = { version = \"1\", optional = true }\n",
    );
    fixture.append(
        "crates/refscape-storage/Cargo.toml",
        "[dev-dependencies]\ndata.workspace = true\n",
    );
    fixture.append("xtask/Cargo.toml", "[dependencies]\ntoml = \"0.9\"\n");
    let document = dependency_graph(&fixture.root).unwrap();
    let graph = document
        .split("## Direct external dependencies")
        .nth(1)
        .unwrap();
    let serde = external_node(graph, "serde");
    let build = external_node(graph, "external-build");
    let toml = external_node(graph, "toml");
    assert!(graph.contains("```mermaid\nflowchart LR"));
    assert!(graph.contains(&format!("refscape_model -->|\"normal\"| {serde}")));
    assert!(graph.contains(&format!(
        "refscape_storage -->|\"dev, alias=data, workspace\"| {serde}"
    )));
    assert!(graph.contains(&format!(
        "refscape_model -->|\"build, optional, target=cfg(target_os = #34;windows#34;)\"| {build}"
    )));
    assert!(graph.contains(&format!("xtask -->|\"normal\"| {toml}")));
    assert_eq!(graph.matches("[\"serde\"]").count(), 1);
    assert!(!graph.contains("unused-external"));
    assert!(!graph.contains("refscape_storage -->|\"normal\"| refscape_model"));
    assert!(!fixture.root.join("Cargo.lock").exists());
}

#[test]
fn external_graph_is_reproducible_and_keeps_distinct_package_names_separate() {
    let first = Fixture::new();
    let second = Fixture::new();
    first.append(
        "crates/refscape-model/Cargo.toml",
        "foo-bar = \"1\"\nfoo_bar = \"2\"\n",
    );
    second.append(
        "crates/refscape-model/Cargo.toml",
        "foo_bar = \"2\"\nfoo-bar = \"1\"\n",
    );
    let document = dependency_graph(&first.root).unwrap();
    assert_eq!(document, dependency_graph(&second.root).unwrap());
    let graph = document
        .split("## Direct external dependencies")
        .nth(1)
        .unwrap();
    assert_ne!(
        external_node(graph, "foo-bar"),
        external_node(graph, "foo_bar")
    );
}

#[test]
fn graph_is_created_and_automatically_updated_after_manifest_changes() {
    let fixture = Fixture::new();
    let path = fixture.root.join("docs/dependency-graph.md");
    assert!(!path.exists());
    write_dependency_graph(&fixture.root).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        dependency_graph(&fixture.root).unwrap()
    );
    fixture.write("docs/dependency-graph.md", "outdated graph\r\n");
    write_dependency_graph(&fixture.root).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        dependency_graph(&fixture.root).unwrap()
    );
    fixture.replace(
        "crates/refscape-app/Cargo.toml",
        "refscape-ui.workspace = true",
        "",
    );
    write_dependency_graph(&fixture.root).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        dependency_graph(&fixture.root).unwrap()
    );
    assert!(
        !fs::read_to_string(&path)
            .unwrap()
            .contains("refscape_app -->|\"normal\"| refscape_ui")
    );
}

#[test]
fn invalid_architecture_does_not_overwrite_the_existing_graph() {
    let fixture = Fixture::new();
    write_dependency_graph(&fixture.root).unwrap();
    let path = fixture.root.join("docs/dependency-graph.md");
    let original = fs::read_to_string(&path).unwrap();
    fixture.append(
        "crates/refscape-model/Cargo.toml",
        "[dev-dependencies]\nrefscape-storage.workspace = true\n",
    );
    assert!(
        write_dependency_graph(&fixture.root)
            .unwrap_err()
            .contains("dependency cycle:")
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn normal_cycle_reports_the_path() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-model/Cargo.toml",
        "refscape-storage.workspace = true\n",
    );
    let error = fixture.error();
    assert!(error.contains("dependency cycle:"), "{error}");
    assert!(error.contains("--normal--> refscape-model"), "{error}");
    assert!(error.contains("--normal--> refscape-storage"), "{error}");
}

#[test]
fn build_self_dependency_is_rejected() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-model/Cargo.toml",
        "[build-dependencies]\nrefscape-model.workspace = true\n",
    );
    let error = fixture.error();
    assert!(error.contains("dependency cycle:"), "{error}");
    assert!(error.contains("--build--> refscape-model"), "{error}");
}

#[test]
fn target_expression_whitespace_does_not_hide_the_declaration() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-model/Cargo.toml",
        "[target.'cfg( unix )'.dev-dependencies]\nrefscape-storage.workspace = true\n",
    );
    let error = fixture.error();
    assert!(error.contains("dependency cycle:"), "{error}");
    assert!(error.contains("dev, target=cfg(unix)"), "{error}");
}

#[test]
fn renamed_dependency_cannot_hide_a_cycle() {
    let fixture = Fixture::new();
    fixture.append(
        "Cargo.toml",
        "storage_alias = { package = \"refscape-storage\", path = \"crates/refscape-storage\" }\n",
    );
    fixture.append(
        "crates/refscape-model/Cargo.toml",
        "storage_alias.workspace = true\n",
    );
    let error = fixture.error();
    assert!(error.contains("dependency cycle:"), "{error}");
    assert!(error.contains("--normal--> refscape-storage"), "{error}");
}

#[test]
fn acyclic_but_forbidden_dependency_is_rejected() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-ui/Cargo.toml",
        "refscape-storage.workspace = true\n",
    );
    let error = fixture.error();
    assert!(!error.contains("dependency cycle:"), "{error}");
    assert!(
        error.contains("forbidden dependency: refscape-ui --normal--> refscape-storage"),
        "{error}"
    );
}

#[test]
fn adapters_cannot_depend_on_the_selector_even_in_tests() {
    for adapter in [
        "refscape-language-rust",
        "refscape-language-cpp",
        "refscape-language-typescript",
        "refscape-language-python",
    ] {
        let fixture = Fixture::new();
        fixture.append(
            &format!("crates/{adapter}/Cargo.toml"),
            "[dev-dependencies]\nrefscape-language.workspace = true\n",
        );
        let error = fixture.error();
        assert!(error.contains("dependency cycle:"), "{error}");
        assert!(
            error.contains(&format!(
                "forbidden dependency: {adapter} --dev--> refscape-language"
            )),
            "{error}"
        );
    }
}

#[test]
fn language_adapters_cannot_depend_on_each_other() {
    for (from, to) in [
        ("refscape-language-rust", "refscape-language-cpp"),
        ("refscape-language-cpp", "refscape-language-rust"),
        ("refscape-language-typescript", "refscape-language-rust"),
        ("refscape-language-typescript", "refscape-language-cpp"),
        ("refscape-language-rust", "refscape-language-typescript"),
        ("refscape-language-cpp", "refscape-language-typescript"),
        ("refscape-language-python", "refscape-language-rust"),
        ("refscape-language-python", "refscape-language-cpp"),
        ("refscape-language-python", "refscape-language-typescript"),
        ("refscape-language-rust", "refscape-language-python"),
        ("refscape-language-cpp", "refscape-language-python"),
        ("refscape-language-typescript", "refscape-language-python"),
    ] {
        let fixture = Fixture::new();
        fixture.append(
            &format!("crates/{from}/Cargo.toml"),
            &format!("{to}.workspace = true\n"),
        );
        let error = fixture.error();
        assert!(!error.contains("dependency cycle:"), "{error}");
        assert!(
            error.contains(&format!("forbidden dependency: {from} --normal--> {to}")),
            "{error}"
        );
    }
}

#[test]
fn common_lsp_cannot_depend_on_application() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-lsp/Cargo.toml",
        "refscape-application.workspace = true\n",
    );
    let error = fixture.error();
    assert!(!error.contains("dependency cycle:"), "{error}");
    assert!(
        error.contains("forbidden dependency: refscape-lsp --normal--> refscape-application"),
        "{error}"
    );
}

#[test]
fn canvas_cannot_depend_on_application_even_in_tests() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-canvas/Cargo.toml",
        "[dev-dependencies]\nrefscape-application.workspace = true\n",
    );
    let error = fixture.error();
    assert!(error.contains("dependency cycle:"), "{error}");
    assert!(
        error.contains("forbidden dependency: refscape-canvas --dev--> refscape-application"),
        "{error}"
    );
}

#[test]
fn executable_cannot_depend_on_concrete_rust_adapter() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-app/Cargo.toml",
        "refscape-language-rust.workspace = true\n",
    );
    assert!(
        fixture
            .error()
            .contains("forbidden dependency: refscape-app --normal--> refscape-language-rust")
    );
}

#[test]
fn executable_cannot_add_cpp_adapter_as_a_dev_dependency() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-app/Cargo.toml",
        "[dev-dependencies]\nrefscape-language-cpp.workspace = true\n",
    );
    assert!(
        fixture
            .error()
            .contains("forbidden dependency: refscape-app --dev--> refscape-language-cpp")
    );
}

#[test]
fn optional_target_test_dependencies_cannot_cycle_between_language_adapters() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-language-rust/Cargo.toml",
        "[target.'cfg(windows)'.dependencies]\nrefscape-language-cpp = { workspace = true, optional = true }\n",
    );
    fixture.append(
        "crates/refscape-language-cpp/Cargo.toml",
        "[target.'cfg(unix)'.dev-dependencies]\nrefscape-language-rust.workspace = true\n",
    );
    let error = fixture.error();
    assert!(error.contains("dependency cycle:"), "{error}");
    assert!(
        error.contains("normal, optional, target=cfg(windows)"),
        "{error}"
    );
    assert!(error.contains("dev, target=cfg(unix)"), "{error}");
}

#[test]
fn new_workspace_member_needs_an_explicit_policy() {
    let fixture = Fixture::new();
    fixture.write(
        "crates/new-crate/Cargo.toml",
        "[package]\nname = \"new-crate\"\nversion = \"0.1.0\"\n",
    );
    fixture.write("crates/new-crate/src/lib.rs", "");
    fixture.replace(
        "Cargo.toml",
        "members = [",
        "members = [\"crates/new-crate\", ",
    );
    assert!(
        fixture
            .error()
            .contains("unregistered workspace member: new-crate")
    );
}

#[test]
fn direct_internal_path_cannot_bypass_workspace_inheritance() {
    let fixture = Fixture::new();
    fixture.replace(
        "crates/refscape-application/Cargo.toml",
        "refscape-model.workspace = true",
        "refscape-model = { path = \"../refscape-model\" }",
    );
    assert!(fixture.error().contains("must use workspace = true"));
}

#[test]
fn internal_registry_source_cannot_bypass_path_identity() {
    let fixture = Fixture::new();
    fixture.replace(
        "crates/refscape-application/Cargo.toml",
        "refscape-model.workspace = true",
        "refscape-model = \"0.1\"",
    );
    assert!(fixture.error().contains("must use a workspace path"));
}

#[test]
fn unregistered_local_path_is_rejected_even_outside_the_workspace() {
    let fixture = Fixture::new();
    fixture.write(
        "helper/Cargo.toml",
        "[package]\nname = \"helper\"\nversion = \"0.1.0\"\n\n[workspace]\n",
    );
    fixture.write("helper/src/lib.rs", "");
    fixture.append(
        "crates/refscape-model/Cargo.toml",
        "helper = { path = \"../../helper\" }\n",
    );
    assert!(
        fixture
            .error()
            .contains("unregistered local path dependency: helper")
    );
}

#[test]
fn xtask_cannot_depend_on_the_product_even_for_tests() {
    let fixture = Fixture::new();
    fixture.append(
        "xtask/Cargo.toml",
        "[dev-dependencies]\nrefscape-model = { path = \"../crates/refscape-model\" }\n",
    );
    assert!(
        fixture
            .error()
            .contains("xtask must not depend on product or local crates")
    );
}

#[test]
fn gpui_cannot_leak_into_the_model_even_with_a_renamed_dev_dependency() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-model/Cargo.toml",
        "[dev-dependencies]\ngui = { package = \"gpui\", version = \"0.2\" }\n",
    );
    assert!(
        fixture
            .error()
            .contains("GPUI dependencies are only allowed")
    );
}

#[test]
fn gpui_platform_cannot_leak_into_the_executable_even_on_inactive_targets() {
    let fixture = Fixture::new();
    fixture.append("crates/refscape-app/Cargo.toml", "[target.'cfg(unix)'.dev-dependencies]\nplatform = { package = \"gpui_platform\", version = \"0.2\" }\n");
    assert!(
        fixture
            .error()
            .contains("GPUI dependencies are only allowed in refscape-ui")
    );
}

#[test]
fn unused_gpui_workspace_declarations_are_rejected_even_under_an_alias() {
    let fixture = Fixture::new();
    fixture.append("Cargo.toml", "gui = { package = \"gpui\", git = \"https://github.com/zed-industries/zed\", rev = \"40180d9c40e2d20eb63d388bff920818f2910b53\" }\n");
    assert!(
        fixture
            .error()
            .contains("GPUI dependencies must be declared directly in refscape-ui")
    );
}

#[test]
fn gpui_platform_and_core_must_use_the_same_commit() {
    let fixture = Fixture::new();
    fixture.append("crates/refscape-ui/Cargo.toml", "gpui_platform = { git = \"https://github.com/zed-industries/zed\", rev = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" }\n");
    assert!(
        fixture
            .error()
            .contains("all GPUI dependencies must use the same pinned revision")
    );
}

#[test]
fn ui_cannot_use_a_registry_gpui_instead_of_the_pinned_source() {
    let fixture = Fixture::new();
    fixture.replace("crates/refscape-ui/Cargo.toml", "gpui = { git = \"https://github.com/zed-industries/zed\", rev = \"40180d9c40e2d20eb63d388bff920818f2910b53\" }", "gpui = \"0.2\"");
    assert!(fixture.error().contains("GPUI must come from"));
}

#[test]
fn lsp_protocol_types_are_confined_to_common_lsp() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-application/Cargo.toml",
        "lsp-types = \"0.97\"\n",
    );
    assert!(
        fixture
            .error()
            .contains("lsp-types is only allowed in refscape-lsp")
    );
}

#[test]
fn common_lsp_can_use_protocol_types() {
    let fixture = Fixture::new();
    fixture.append("crates/refscape-lsp/Cargo.toml", "lsp-types = \"0.97\"\n");
    check_architecture(&fixture.root).unwrap();
}

#[test]
fn language_selector_and_adapters_cannot_import_protocol_types_even_for_tests() {
    for name in [
        "refscape-language",
        "refscape-language-rust",
        "refscape-language-cpp",
        "refscape-language-typescript",
        "refscape-language-python",
    ] {
        let fixture = Fixture::new();
        fixture.append(
            &format!("crates/{name}/Cargo.toml"),
            "[dev-dependencies]\nprotocol = { package = \"lsp-types\", version = \"0.97\" }\n",
        );
        assert!(
            fixture
                .error()
                .contains("lsp-types is only allowed in refscape-lsp")
        );
    }
}

#[test]
fn floating_gpui_revision_is_rejected() {
    let fixture = Fixture::new();
    fixture.replace(
        "crates/refscape-ui/Cargo.toml",
        "rev = \"40180d9c40e2d20eb63d388bff920818f2910b53\"",
        "rev = \"main\"",
    );
    assert!(fixture.error().contains("40-character commit hash"));
}

#[test]
fn missing_required_member_is_rejected() {
    let fixture = Fixture::new();
    fixture.replace(
        "Cargo.toml",
        "\"crates/refscape-ui\", \"crates/refscape-app\"",
        "\"crates/refscape-ui\"",
    );
    let error = fixture.error();
    assert!(
        error.contains("required workspace member is missing: refscape-app"),
        "{error}"
    );
}

#[test]
fn invalid_manifest_fails_closed() {
    let fixture = Fixture::new();
    fixture.write("crates/refscape-model/Cargo.toml", "[invalid");
    assert!(fixture.error().contains("cannot inspect"));
}
