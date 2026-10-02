//! Public process boundary: mode priority, exit status, Unicode and pre-backend rejection.
use std::{fs, path::PathBuf, process::Command};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "refscape-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_refscape"));
    command.env("REFSCAPE_RUST_ANALYZER", "refscape-nonexistent-server");
    command
}
#[test]
fn help_and_parser_errors_preserve_stdout_stderr_and_exit_contract() {
    for args in [
        vec!["--help"],
        vec!["-h"],
        vec!["--help", "--check", "日本語"],
    ] {
        let result = command().args(args).output().unwrap();
        assert!(result.status.success());
        let stdout = String::from_utf8(result.stdout).unwrap();
        assert!(stdout.contains("--check") && stdout.contains("--language"));
        assert!(result.stderr.is_empty());
    }
    for args in [
        vec!["--unknown"],
        vec!["--language", "not-a-language"],
        vec!["--check"],
        vec!["--session"],
    ] {
        let result = command().args(args).output().unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(
            String::from_utf8(result.stderr)
                .unwrap()
                .starts_with("Refscape: ")
        );
    }
}
#[test]
fn unicode_theme_export_roundtrips_v1_and_rejects_check_conflict() {
    let directory = Directory::new();
    let path = directory.0.join("日本語 🙂 theme.json");
    let result = command()
        .arg("--export-theme")
        .arg("light")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let theme = refscape_storage::theme::load_theme(&path).unwrap();
    assert_eq!(theme, refscape_model::Theme::light());
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("日本語 🙂 theme.json")
    );
    let result = command()
        .arg("--export-theme")
        .arg("light")
        .arg(&path)
        .args(["--check", "unavailable-project"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("cannot be combined")
    );
}
#[test]
fn named_session_root_mismatch_fails_before_server_resolution() {
    use refscape_application::{ApplicationSnapshot, PersistableSession, SessionRepository};
    let directory = Directory::new();
    let first = directory.0.join("first");
    let second = directory.0.join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let path = directory.0.join("named.json");
    refscape_storage::session::JsonSessionRepository
        .save(
            &path,
            &PersistableSession {
                snapshot: std::sync::Arc::new(ApplicationSnapshot::new(first)),
                epoch: 1,
                revision: 1,
            },
        )
        .unwrap();
    let original = fs::read(&path).unwrap();
    let result = command()
        .arg(&second)
        .arg("--session")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!result.status.success());
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(
        stderr.contains("belongs to a different project"),
        "{stderr}"
    );
    assert!(!stderr.contains("nonexistent-server"));
    assert_eq!(fs::read(&path).unwrap(), original);
}
