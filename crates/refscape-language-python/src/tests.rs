use super::*;

#[test]
fn incompatible_options_are_rejected_before_launching_a_server() {
    for options in [
        ProjectOptions {
            language: ProjectLanguage::Rust,
            compilation_database: None,
        },
        ProjectOptions {
            language: ProjectLanguage::Python,
            compilation_database: Some("compile_commands.json".into()),
        },
    ] {
        let mut language = Pyright::new("missing-language-server");
        assert!(
            language
                .open_project(Path::new("."), &options)
                .unwrap_err()
                .contains("compilation databases")
        );
        assert_eq!(language.project_options(), ProjectOptions::default());
    }
}

#[test]
fn requests_without_an_open_project_return_errors() {
    let mut language = Python::default();
    assert!(language.files().is_err());
    assert!(language.search("").is_err());
    assert!(language.symbols(Path::new("module.py")).is_err());
    assert!(
        language
            .definitions(Path::new("module.py"), Position::default())
            .is_err()
    );
}
