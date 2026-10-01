//! Bounded LSP framing and a synchronous request pump over a background reader.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

const MAX_MESSAGE: usize = 64 * 1024 * 1024;

pub(crate) fn read_message(reader: &mut impl BufRead) -> Result<Value, String> {
    let mut length = None;
    let mut header_bytes = 0;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("rust-analyzer closed its output stream".into());
        }
        header_bytes += read;
        if header_bytes > 8192 {
            return Err("LSP headers exceed 8192 bytes".into());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let (name, value) = line.split_once(':').ok_or("invalid LSP header")?;
        if name.eq_ignore_ascii_case("Content-Length") {
            if length.is_some() {
                return Err("duplicate LSP Content-Length".into());
            }
            length = Some(value.trim().parse::<usize>().map_err(|e| e.to_string())?);
        }
    }
    let length = length.ok_or("missing LSP Content-Length")?;
    if length > MAX_MESSAGE {
        return Err("LSP message exceeds 64 MiB".into());
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    serde_json::from_slice(&body).map_err(|e| format!("invalid LSP JSON: {e}"))
}

fn write_message(writer: &mut impl Write, message: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len()).map_err(|e| e.to_string())?;
    writer.write_all(&body).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

pub(crate) struct Transport {
    child: Option<Child>,
    writer: Box<dyn Write + Send>,
    receiver: mpsc::Receiver<Result<Value, String>>,
    stderr: Arc<Mutex<String>>,
    next_id: u64,
    pub(crate) timeout: Duration,
    pub(crate) quiescent: bool,
    pub(crate) status: Option<String>,
    health: String,
}

impl Transport {
    pub(crate) fn spawn(command: &mut Command, timeout: Duration) -> Result<Self, String> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(|e| format!("cannot start rust-analyzer: {e}; install `rustup component add rust-analyzer` or set REFSCAPE_RUST_ANALYZER"))?;
        let writer = child.stdin.take().ok_or("missing rust-analyzer stdin")?;
        let stdout = child.stdout.take().ok_or("missing rust-analyzer stdout")?;
        let mut stderr_pipe = child.stderr.take().ok_or("missing rust-analyzer stderr")?;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let message = read_message(&mut reader);
                let failed = message.is_err();
                if sender.send(message).is_err() || failed {
                    break;
                }
            }
        });
        let stderr = Arc::new(Mutex::new(String::new()));
        let log = stderr.clone();
        thread::spawn(move || {
            let mut buffer = [0; 4096];
            while let Ok(count) = stderr_pipe.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                if let Ok(mut log) = log.lock() {
                    log.push_str(&String::from_utf8_lossy(&buffer[..count]));
                    if log.len() > 16384 {
                        let mut start = log.len() - 8192;
                        while !log.is_char_boundary(start) {
                            start += 1;
                        }
                        log.drain(..start);
                    }
                }
            }
        });
        Ok(Self {
            child: Some(child),
            writer: Box::new(writer),
            receiver,
            stderr,
            next_id: 0,
            timeout,
            quiescent: false,
            status: None,
            health: "ok".into(),
        })
    }

    pub(crate) fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({"jsonrpc":"2.0","method":method,"params":params}))
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        write_message(&mut self.writer, message)
    }

    fn receive(&self, deadline: Instant) -> Result<Value, String> {
        let duration = deadline.saturating_duration_since(Instant::now());
        let result = self
            .receiver
            .recv_timeout(duration)
            .map_err(|e| format!("rust-analyzer response failed: {e}"))?;
        result.map_err(|e| {
            let log = self.stderr.lock().map(|s| s.clone()).unwrap_or_default();
            format!("{e}. {log} Install `rustup component add rust-analyzer` or set REFSCAPE_RUST_ANALYZER to an executable.")
        })
    }

    pub(crate) fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let deadline = Instant::now() + self.timeout;
        // Servers may cancel a request while reloading the workspace; retry with a new id.
        for _ in 0..4 {
            self.next_id += 1;
            let id = self.next_id;
            self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
            loop {
                let message = match self.receive(deadline) {
                    Ok(message) => message,
                    Err(error) => {
                        let _ = self.notify("$/cancelRequest", json!({"id":id}));
                        return Err(format!("{method}: {error}"));
                    }
                };
                if message.get("method").is_some() {
                    self.handle_server_message(&message)?;
                    continue;
                }
                if message.get("id").and_then(Value::as_u64) != Some(id) {
                    continue;
                }
                if let Some(error) = message.get("error") {
                    let code = error["code"].as_i64().unwrap_or(0);
                    if matches!(code, -32801 | -32802) {
                        break;
                    }
                    return Err(format!(
                        "{method}: LSP error {code}: {}",
                        error["message"].as_str().unwrap_or("unknown error")
                    ));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
        }
        Err(format!(
            "{method}: rust-analyzer repeatedly cancelled analysis; retry after indexing"
        ))
    }

    fn handle_server_message(&mut self, message: &Value) -> Result<(), String> {
        let method = message["method"].as_str().unwrap_or("");
        if let Some(id) = message.get("id") {
            let result = match method {
                "workspace/configuration" => Value::Array(message["params"]["items"].as_array().map(|items| items.iter().map(|_| json!({"checkOnSave":false})).collect()).unwrap_or_default()),
                "workspace/workspaceFolders" => Value::Null,
                "workspace/applyEdit" => json!({"applied":false,"failureReason":"Refscape is a read-only explorer"}),
                "client/registerCapability" | "client/unregisterCapability" | "window/workDoneProgress/create" | "workspace/semanticTokens/refresh" | "workspace/inlayHint/refresh" | "workspace/diagnostic/refresh" | "workspace/codeLens/refresh" => Value::Null,
                _ => return self.send(&json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported client request"}})),
            };
            self.send(&json!({"jsonrpc":"2.0","id":id,"result":result}))?;
        } else if method == "experimental/serverStatus" {
            self.quiescent = message["params"]["quiescent"].as_bool().unwrap_or(false);
            self.status = message["params"]["message"].as_str().map(str::to_owned);
            self.health = message["params"]["health"].as_str().unwrap_or("ok").into();
        }
        Ok(())
    }

    pub(crate) fn wait_for_index(&mut self) -> Result<(), String> {
        let deadline = Instant::now() + self.timeout;
        while !self.quiescent {
            let message = self
                .receive(deadline)
                .map_err(|e| format!("waiting for Rust project analysis: {e}"))?;
            self.handle_server_message(&message)?;
        }
        if self.health == "error" {
            return Err(format!(
                "rust-analyzer cannot analyze this project: {}",
                self.status
                    .as_deref()
                    .unwrap_or("check Cargo.toml and the Rust toolchain")
            ));
        }
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        if self.child.is_none() {
            return;
        }
        self.timeout = Duration::from_secs(2);
        let _ = self.request("shutdown", Value::Null);
        let _ = self.notify("exit", Value::Null);
        if let Some(child) = &mut self.child {
            let deadline = Instant::now() + Duration::from_millis(500);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    _ if Instant::now() >= deadline => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    _ => thread::sleep(Duration::from_millis(10)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn framing_preserves_utf8_and_consecutive_messages() {
        let value = json!({"text":"🦀 日本語"});
        let mut bytes = vec![];
        write_message(&mut bytes, &value).unwrap();
        write_message(&mut bytes, &json!(null)).unwrap();
        let mut reader = Cursor::new(bytes);
        assert_eq!(read_message(&mut reader).unwrap(), value);
        assert_eq!(read_message(&mut reader).unwrap(), Value::Null);
    }

    #[test]
    fn malformed_and_oversize_frames_are_rejected() {
        for bytes in [
            "Content-Length: 999999999\r\n\r\n",
            "X: y\r\n\r\n",
            "Content-Length: 5\r\n\r\n{}",
            "Content-Length: 0\r\nContent-Length: 0\r\n\r\n",
        ] {
            assert!(read_message(&mut Cursor::new(bytes)).is_err());
        }
    }

    #[test]
    fn request_pump_answers_server_requests_and_ignores_stale_responses() {
        let (sender, receiver) = mpsc::channel();
        for value in [
            json!({"id":99,"result":"stale"}),
            json!({"id":"cfg","method":"workspace/configuration","params":{"items":[{}]}}),
            json!({"method":"experimental/serverStatus","params":{"quiescent":true}}),
            json!({"id":1,"result":"done"}),
            json!({"id":2,"result":null}),
        ] {
            sender.send(Ok(value)).unwrap();
        }
        let mut client = Transport {
            child: None,
            writer: Box::new(vec![]),
            receiver,
            stderr: Arc::default(),
            next_id: 0,
            timeout: Duration::from_millis(20),
            quiescent: false,
            status: None,
            health: "ok".into(),
        };
        assert_eq!(client.request("example", json!({})).unwrap(), "done");
        assert!(client.quiescent);
    }

    #[test]
    fn cancelled_requests_retry_but_server_errors_and_deadlines_propagate() {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(
                json!({"id":1,"error":{"code":-32801,"message":"content changed"}}),
            ))
            .unwrap();
        sender
            .send(Ok(json!({"id":2,"result":["retry succeeded"]})))
            .unwrap();
        sender
            .send(Ok(
                json!({"id":3,"error":{"code":-32602,"message":"invalid position"}}),
            ))
            .unwrap();
        let mut client = Transport {
            child: None,
            writer: Box::new(vec![]),
            receiver,
            stderr: Arc::default(),
            next_id: 0,
            timeout: Duration::from_millis(10),
            quiescent: false,
            status: None,
            health: "ok".into(),
        };
        assert_eq!(
            client.request("definition", json!({})).unwrap(),
            json!(["retry succeeded"])
        );
        assert!(
            client
                .request("definition", json!({}))
                .unwrap_err()
                .contains("invalid position")
        );
        assert!(
            client
                .request("definition", json!({}))
                .unwrap_err()
                .contains("response failed")
        );
    }
}
