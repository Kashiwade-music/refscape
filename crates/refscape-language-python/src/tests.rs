use super::*;
#[test]
fn incompatible_options_are_rejected_before_launching_a_server() {
    for options in [
        ProjectOpenOptions {
            language: ProjectLanguage::Rust,
            compilation_database: None,
        },
        ProjectOpenOptions {
            language: ProjectLanguage::Python,
            compilation_database: Some("compile_commands.json".into()),
        },
    ] {
        let language = Python::new("missing-language-server");
        let error = language
            .prepare(
                Path::new("."),
                &options,
                &OperationContext::detached(std::time::Duration::from_secs(5)),
            )
            .err()
            .unwrap();
        assert!(error.to_string().contains("compilation databases"));
    }
}
#[test]
fn factory_preparation_errors_are_repeatable_without_an_unopened_session() {
    let factory =
        Python::from_environment("missing-language-server", EnvironmentSnapshot::default());
    let options = ProjectOpenOptions {
        language: ProjectLanguage::Python,
        compilation_database: None,
    };
    for _ in 0..2 {
        let error = factory
            .prepare(
                Path::new("."),
                &options,
                &OperationContext::detached(std::time::Duration::from_secs(5)),
            )
            .err()
            .unwrap();
        assert_eq!(error.kind, ErrorKind::BackendUnavailable);
    }
}
