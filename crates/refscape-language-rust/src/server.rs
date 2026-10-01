use refscape_lsp::transport::ServerBehavior;
use serde_json::{Value, json};

#[derive(Default)]
pub(crate) struct RustServer {
    quiescent: bool,
    status: Option<String>,
    failed: bool,
}
impl ServerBehavior for RustServer {
    fn configuration(&self, _section: Option<&str>) -> Value {
        json!({"checkOnSave":false})
    }
    fn notification(&mut self, method: &str, params: &Value) {
        if method == "experimental/serverStatus" {
            self.quiescent = params["quiescent"].as_bool().unwrap_or(false);
            self.status = params["message"].as_str().map(str::to_owned);
            self.failed = params["health"].as_str() == Some("error");
        }
    }
    fn ready(&self) -> Result<bool, String> {
        if self.quiescent && self.failed {
            Err(format!(
                "rust-analyzer cannot analyze this project: {}",
                self.status
                    .as_deref()
                    .unwrap_or("check Cargo.toml and the Rust toolchain")
            ))
        } else {
            Ok(self.quiescent)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexing_waits_for_status_and_reports_analysis_failures() {
        let mut server = RustServer::default();
        assert!(!server.ready().unwrap());
        server.notification("unrelated", &json!({"quiescent":true}));
        assert!(!server.ready().unwrap());
        server.notification(
            "experimental/serverStatus",
            &json!({"quiescent":true,"health":"ok"}),
        );
        assert!(server.ready().unwrap());
        server.notification(
            "experimental/serverStatus",
            &json!({"quiescent":true,"health":"error","message":"missing toolchain"}),
        );
        assert!(server.ready().unwrap_err().contains("missing toolchain"));
    }
}
