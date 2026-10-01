use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use xtask::{check_architecture, dependency_graph, write_dependency_graph};

const NAMES: &[&str] = &[
    "refscape-model",
    "refscape-application",
    "refscape-lsp",
    "refscape-storage",
    "refscape-ui",
    "refscape-app",
];
const EDGES: &[(&str, &[&str])] = &[
    ("refscape-model", &[]),
    ("refscape-application", &["refscape-model"]),
    ("refscape-lsp", &["refscape-application", "refscape-model"]),
    (
        "refscape-storage",
        &["refscape-application", "refscape-model"],
    ),
    ("refscape-ui", &["refscape-application", "refscape-model"]),
    (
        "refscape-app",
        &[
            "refscape-application",
            "refscape-lsp",
            "refscape-model",
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
            "[workspace]\nmembers = [{members}]\nexclude = [\"xtask\"]\nresolver = \"3\"\n\n[workspace.dependencies]\n{shared}gpui = {{ git = \"https://github.com/zed-industries/zed\", rev = \"40180d9c40e2d20eb63d388bff920818f2910b53\" }}\n"
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
    assert!(report.contains("6 product crates, 12 declared internal dependencies"));
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
    assert!(graph.contains("```mermaid\nflowchart TD"));
    assert!(graph.contains("refscape_model[\"refscape-model\"]"));
    assert!(graph.contains("refscape_ui -->|\"normal\"| refscape_application"));
    assert!(!graph.contains("refscape_ui -->|\"normal\"| refscape_model"));
    assert_eq!(graph.matches(" -->|").count(), 11);
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
fn cargo_permitted_dev_cycle_is_rejected() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-model/Cargo.toml",
        "[dev-dependencies]\nrefscape-storage.workspace = true\n",
    );
    let error = fixture.error();
    assert!(error.contains("dependency cycle:"), "{error}");
    assert!(error.contains("--dev--> refscape-storage"), "{error}");
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
fn inactive_optional_and_target_dependencies_are_included() {
    let fixture = Fixture::new();
    fixture.append("crates/refscape-model/Cargo.toml", "[target.'cfg(unix)'.dependencies]\nrefscape-storage = { workspace = true, optional = true }\n");
    let error = fixture.error();
    assert!(error.contains("dependency cycle:"), "{error}");
    assert!(
        error.contains("normal, optional, target=cfg(unix)"),
        "{error}"
    );
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
fn mutually_exclusive_targets_still_cannot_create_a_declared_cycle() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-model/Cargo.toml",
        "[target.'cfg(windows)'.dev-dependencies]\nrefscape-storage.workspace = true\n",
    );
    fixture.replace(
        "crates/refscape-storage/Cargo.toml",
        "refscape-model.workspace = true",
        "",
    );
    fixture.replace(
        "crates/refscape-storage/Cargo.toml",
        "refscape-application.workspace = true",
        "",
    );
    fixture.append(
        "crates/refscape-storage/Cargo.toml",
        "[target.'cfg(unix)'.dependencies]\nrefscape-model.workspace = true\n",
    );
    let error = fixture.error();
    assert!(error.contains("dependency cycle:"), "{error}");
    assert!(error.contains("target=cfg(windows)"), "{error}");
    assert!(error.contains("target=cfg(unix)"), "{error}");
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
fn allowed_renamed_dependency_is_identified_by_package_and_path() {
    let fixture = Fixture::new();
    fixture.append(
        "Cargo.toml",
        "model_alias = { package = \"refscape-model\", path = \"crates/refscape-model\" }\n",
    );
    fixture.replace(
        "crates/refscape-application/Cargo.toml",
        "refscape-model.workspace = true",
        "model_alias.workspace = true",
    );
    check_architecture(&fixture.root).unwrap();
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
fn ui_cannot_use_a_registry_gpui_instead_of_the_pinned_source() {
    let fixture = Fixture::new();
    fixture.append("crates/refscape-ui/Cargo.toml", "gpui = \"0.2\"\n");
    assert!(
        fixture
            .error()
            .contains("gpui must inherit the pinned workspace dependency")
    );
}

#[test]
fn lsp_protocol_types_are_confined_to_the_adapter() {
    let fixture = Fixture::new();
    fixture.append(
        "crates/refscape-application/Cargo.toml",
        "lsp-types = \"0.97\"\n",
    );
    assert!(fixture.error().contains("lsp-types is only allowed"));
}

#[test]
fn floating_gpui_revision_is_rejected() {
    let fixture = Fixture::new();
    fixture.replace(
        "Cargo.toml",
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
