//! Always-on bounded JSON-RPC runtime with interruptible pipe writes and owned process supervision.
use refscape_model::{ErrorKind, OperationContext, RefscapeError};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot, watch},
};

type Result<T> = std::result::Result<T, RefscapeError>;
fn protocol(message: impl Into<String>) -> RefscapeError {
    RefscapeError::new(ErrorKind::Protocol, message)
}
pub trait ServerBehavior: Send {
    fn configuration(&self, _section: Option<&str>) -> Value {
        json!({})
    }
    fn scoped_configuration(&self, section: Option<&str>, _scope_uri: Option<&str>) -> Value {
        self.configuration(section)
    }
    fn notification(&mut self, _method: &str, _params: &Value) {}
    fn ready(&self) -> std::result::Result<bool, String> {
        Ok(true)
    }
}
#[derive(Default)]
pub struct DefaultServerBehavior;
impl ServerBehavior for DefaultServerBehavior {}

const MAX_HEADER: usize = 8192;
const MAX_MESSAGE: usize = 64 * 1024 * 1024;
const QUEUE: usize = 128;

async fn read_message(reader: &mut (impl AsyncRead + Unpin)) -> Result<Value> {
    let mut header = Vec::with_capacity(128);
    loop {
        let byte = reader
            .read_u8()
            .await
            .map_err(|e| protocol(format!("language server closed its output stream: {e}")))?;
        header.push(byte);
        if header.len() > MAX_HEADER {
            return Err(protocol("LSP headers exceed 8192 bytes"));
        }
        if header.ends_with(b"\r\n\r\n") || header.ends_with(b"\n\n") {
            break;
        }
    }
    let header =
        std::str::from_utf8(&header).map_err(|_| protocol("invalid LSP header encoding"))?;
    let mut length = None;
    for line in header.lines().filter(|line| !line.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| protocol("invalid LSP header"))?;
        if name.eq_ignore_ascii_case("Content-Length") {
            if length.is_some() {
                return Err(protocol("duplicate LSP Content-Length"));
            }
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| protocol("invalid LSP Content-Length"))?,
            );
        }
    }
    let length = length.ok_or_else(|| protocol("missing LSP Content-Length"))?;
    if length > MAX_MESSAGE {
        return Err(protocol("LSP message exceeds 64 MiB"));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|e| protocol(format!("truncated LSP body: {e}")))?;
    let message: Value =
        serde_json::from_slice(&body).map_err(|e| protocol(format!("invalid LSP JSON: {e}")))?;
    if !message.is_object() || message["jsonrpc"] != "2.0" {
        return Err(protocol("invalid JSON-RPC envelope"));
    }
    if let Some(id) = message.get("id")
        && !id.is_string()
        && !id.is_i64()
        && !id.is_u64()
    {
        return Err(protocol("invalid JSON-RPC id"));
    }
    if message.get("method").is_some() && !message["method"].is_string() {
        return Err(protocol("invalid JSON-RPC method"));
    }
    if message.get("method").is_none()
        && (message.get("id").is_none()
            || message.get("result").is_some() == message.get("error").is_some())
    {
        return Err(protocol("malformed JSON-RPC response"));
    }
    if let Some(error) = message.get("error")
        && (!error.is_object()
            || error["code"].as_i64().is_none()
            || error["message"].as_str().is_none())
    {
        return Err(protocol("malformed JSON-RPC error"));
    }
    Ok(message)
}
async fn write_message(writer: &mut (impl AsyncWrite + Unpin), message: &Value) -> Result<()> {
    let body = serde_json::to_vec(message).map_err(|e| protocol(e.to_string()))?;
    if body.len() > MAX_MESSAGE {
        return Err(protocol("LSP message exceeds 64 MiB"));
    }
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await
        .map_err(|e| protocol(e.to_string()))?;
    writer
        .write_all(&body)
        .await
        .map_err(|e| protocol(e.to_string()))?;
    writer.flush().await.map_err(|e| protocol(e.to_string()))
}

struct Frame {
    value: Value,
    context: OperationContext,
    written: Option<oneshot::Sender<Result<()>>>,
}
enum Dispatch {
    Request {
        id: u64,
        value: Value,
        context: OperationContext,
        reply: oneshot::Sender<Result<Value>>,
    },
    Cancel(u64),
    Folders(Value),
}
struct Pending {
    reply: oneshot::Sender<Result<Value>>,
    context: OperationContext,
}
#[derive(Clone, Debug)]
pub struct TokenLegend {
    pub types: Vec<String>,
    pub modifiers: Vec<String>,
}
#[derive(Clone)]
enum RegisteredFeature {
    Symbols,
    Tokens(TokenLegend),
}
fn registration(item: &Value) -> Result<(String, RegisteredFeature)> {
    let id = item["id"]
        .as_str()
        .ok_or_else(|| protocol("registration missing id"))?
        .to_owned();
    let feature = match item["method"].as_str() {
        Some("textDocument/documentSymbol") => RegisteredFeature::Symbols,
        Some("textDocument/semanticTokens") => {
            let options = &item["registerOptions"];
            if !matches!(options["full"], Value::Bool(true) | Value::Object(_)) {
                return Err(protocol("range-only dynamic tokens unsupported"));
            }
            let legend = |key: &str| -> Result<Vec<String>> {
                options["legend"][key]
                    .as_array()
                    .ok_or_else(|| protocol("registration missing token legend"))?
                    .iter()
                    .map(|entry| {
                        entry
                            .as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| protocol("invalid dynamic token legend"))
                    })
                    .collect()
            };
            RegisteredFeature::Tokens(TokenLegend {
                types: legend("tokenTypes")?,
                modifiers: legend("tokenModifiers")?,
            })
        }
        _ => return Err(protocol("unsupported dynamic registration")),
    };
    Ok((id, feature))
}

pub struct Transport {
    commands: mpsc::Sender<Dispatch>,
    writer: mpsc::Sender<Frame>,
    stop: watch::Sender<bool>,
    dispose: watch::Sender<bool>,
    closed: watch::Receiver<bool>,
    ready: watch::Receiver<std::result::Result<bool, String>>,
    epoch: Arc<AtomicU64>,
    next_id: AtomicU64,
    runtime: crate::runtime::RuntimeOwner,
    registrations: Arc<Mutex<std::collections::BTreeMap<String, RegisteredFeature>>>,
    stderr: Arc<Mutex<VecDeque<u8>>>,
    notifications: Arc<Mutex<VecDeque<String>>>,
    diagnostics: Mutex<VecDeque<RpcDiagnostic>>,
}
#[derive(Clone, Debug)]
pub struct RpcDiagnostic {
    pub time: SystemTime,
    pub operation: String,
    pub method: String,
    pub elapsed: Duration,
    pub outcome: Option<ErrorKind>,
}
impl Transport {
    pub fn spawn(
        command: &mut Command,
        server: impl Into<String>,
        hint: impl Into<String>,
        behavior: Box<dyn ServerBehavior>,
    ) -> Result<Self> {
        let server = server.into();
        let hint = hint.into();
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let program = command.get_program().to_owned();
        let command = std::mem::replace(command, Command::new(program));
        let mut command = tokio::process::Command::from(command);
        command.kill_on_drop(true);
        let owner = crate::runtime::RuntimeOwner::acquire();
        let _enter = owner.runtime().enter();
        let mut child = command.spawn().map_err(|e| {
            RefscapeError::new(
                ErrorKind::BackendUnavailable,
                format!("cannot start {server}: {e}; {hint}"),
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| protocol("missing language server stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| protocol("missing language server stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| protocol("missing language server stderr"))?;
        let (commands, command_rx) = mpsc::channel(QUEUE);
        let (writer, mut write_rx) = mpsc::channel::<Frame>(QUEUE);
        let (control, mut control_rx) = mpsc::channel::<Frame>(32);
        let (stop, mut supervisor_stop) = watch::channel(false);
        let (dispose, mut disposal_request) = watch::channel(false);
        let (closed_tx, closed) = watch::channel(false);
        let (events, event_rx) = mpsc::channel(QUEUE);
        let (ready_tx, ready) = watch::channel(behavior.ready());
        let epoch = Arc::new(AtomicU64::new(0));
        let registrations = Arc::new(Mutex::new(std::collections::BTreeMap::new()));
        let notifications = Arc::new(Mutex::new(VecDeque::with_capacity(128)));
        let reader_notifications = notifications.clone();
        let mut reader_stop = stop.subscribe();
        let reader_stop_tx = stop.clone();
        let reader = owner.runtime().spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                let result = tokio::select! { biased; _ = reader_stop.changed() => break, result = read_message(&mut reader) => result };
                let failed = result.is_err();
                if let Ok(message)=&result && message.get("id").is_none() && matches!(message["method"].as_str(),Some("window/logMessage"|"window/showMessage"|"$/progress"|"textDocument/publishDiagnostics")) {
                    let mut log=reader_notifications.lock().unwrap_or_else(|e|e.into_inner());
                    if log.len()==128{log.pop_front();}
                    let mut text=message.to_string();let mut end=text.len().min(4096);while !text.is_char_boundary(end){end-=1;}text.truncate(end);log.push_back(text);continue;
                }
                if !tokio::select! { biased; _ = reader_stop.changed() => false, result = events.send(result) => result.is_ok() } { break; }
                if failed { reader_stop_tx.send_replace(true); break; }
            }
        });
        let mut writer_stop = stop.subscribe();
        let writer_stop_tx = stop.clone();
        let writer_task = owner.runtime().spawn(async move {
            let mut stdin = stdin;
            loop {
                let frame = tokio::select! { biased; _ = writer_stop.changed() => break, frame=control_rx.recv()=>match frame{Some(f)=>f,None=>break}, frame = write_rx.recv() => match frame {Some(f)=>f,None=>break} };
                let result = tokio::select! { biased; _=writer_stop.changed()=>Err(protocol("language server stopped")), _=frame.context.cancel.cancelled()=>Err(RefscapeError::new(ErrorKind::Cancelled,"operation cancelled")), _=tokio::time::sleep_until(frame.context.deadline.into())=>Err(RefscapeError::new(ErrorKind::Timeout,"LSP write deadline exceeded")), result=write_message(&mut stdin,&frame.value)=>result };
                let failed = result.is_err();
                if let Some(written)=frame.written { let _=written.send(result); }
                if failed { writer_stop_tx.send_replace(true); break; }
            }
        });
        let stderr_log = Arc::new(Mutex::new(VecDeque::with_capacity(16384)));
        let stderr_log_task = stderr_log.clone();
        let stderr_task = owner.runtime().spawn(async move {
            let mut stderr = stderr;
            let mut buffer = [0; 4096];
            while let Ok(count) = stderr.read(&mut buffer).await {
                if count == 0 {
                    break;
                }
                let mut ring = stderr_log_task.lock().unwrap_or_else(|e| e.into_inner());
                for byte in &buffer[..count] {
                    if ring.len() == 16384 {
                        ring.pop_front();
                    }
                    ring.push_back(*byte);
                }
            }
        });
        let dispatcher = owner.runtime().spawn(
            Dispatcher {
                pending: HashMap::new(),
                folders: Value::Array(vec![]),
                writer: writer.clone(),
                control,
                stop: stop.clone(),
                behavior,
                ready: ready_tx,
                epoch: epoch.clone(),
                registrations: registrations.clone(),
            }
            .run(command_rx, event_rx),
        );
        let supervisor_writer = writer.clone();
        let supervisor_stop_tx = stop.clone();
        let supervisor_commands = commands.clone();
        let supervisor_runtime = owner.clone();
        owner.runtime().spawn(async move {
            let graceful=tokio::select! { _=disposal_request.changed()=>true, _=supervisor_stop.changed()=>false, _=child.wait()=>false };
            // Process and all pipes have one owner. Disposal never waits on the caller/UI thread.
            if graceful {
                let context=OperationContext::detached(Duration::from_millis(250));
                let (reply,receiver)=oneshot::channel();
                let _=supervisor_commands.try_send(Dispatch::Request{id:0,value:json!({"jsonrpc":"2.0","id":0,"method":"shutdown","params":null}),context:context.clone(),reply});
                let _=tokio::time::timeout_at(context.deadline.into(),receiver).await;
                let (written,receiver)=oneshot::channel();
                let context=OperationContext::detached(Duration::from_millis(100));
                let _=supervisor_writer.try_send(Frame{value:json!({"jsonrpc":"2.0","method":"exit","params":null}),context:context.clone(),written:Some(written)});
                let _=tokio::time::timeout_at(context.deadline.into(),receiver).await;
            }
            supervisor_stop_tx.send_replace(true);
            if tokio::time::timeout(Duration::from_millis(500),child.wait()).await.is_err() { let _=child.kill().await; let _=child.wait().await; }
            reader.abort(); writer_task.abort(); dispatcher.abort(); stderr_task.abort();
            let _=reader.await; let _=writer_task.await; let _=dispatcher.await; let _=stderr_task.await;
            closed_tx.send_replace(true);
            drop(supervisor_runtime);
        });
        Ok(Self {
            commands,
            writer,
            stop,
            dispose,
            closed,
            ready,
            epoch,
            registrations,
            next_id: AtomicU64::new(1),
            runtime: owner.clone(),
            stderr: stderr_log,
            notifications,
            diagnostics: Mutex::new(VecDeque::with_capacity(128)),
        })
    }
    pub fn analysis_epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    pub fn invalidate_analysis(&self) -> u64 {
        self.epoch.fetch_add(1, Ordering::AcqRel) + 1
    }
    pub fn stderr_tail(&self) -> String {
        let bytes = self
            .stderr
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .copied()
            .collect::<Vec<_>>();
        String::from_utf8_lossy(&bytes).into_owned()
    }
    pub fn recent_notifications(&self) -> Vec<String> {
        self.notifications
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }
    pub fn diagnostics(&self) -> Vec<RpcDiagnostic> {
        self.diagnostics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }
    pub fn dynamic_capabilities(&self) -> (bool, Option<TokenLegend>) {
        let registrations = self.registrations.lock().unwrap_or_else(|e| e.into_inner());
        (
            registrations
                .values()
                .any(|feature| matches!(feature, RegisteredFeature::Symbols)),
            registrations.values().find_map(|feature| match feature {
                RegisteredFeature::Tokens(legend) => Some(legend.clone()),
                _ => None,
            }),
        )
    }
    pub fn disposal(&self) -> ProcessDisposal {
        ProcessDisposal(self.closed.clone(), self.runtime.clone())
    }
    pub fn set_folders(&self, folders: Value, context: &OperationContext) -> Result<()> {
        self.block(context, self.commands.send(Dispatch::Folders(folders)))?
            .map_err(|_| protocol("language server stopped"))
    }
    fn block<T>(
        &self,
        context: &OperationContext,
        future: impl std::future::Future<Output = T>,
    ) -> Result<T> {
        context.check()?;
        self.runtime.block_on(async {tokio::select! {biased; _=context.cancel.cancelled()=>Err(RefscapeError::new(ErrorKind::Cancelled,"operation cancelled")), _=tokio::time::sleep_until(context.deadline.into())=>Err(RefscapeError::new(ErrorKind::Timeout,"language server response deadline exceeded")), value=future=>Ok(value)}})
    }
    pub fn notify(&self, method: &str, params: Value, context: &OperationContext) -> Result<()> {
        let (written, reply) = oneshot::channel();
        self.block(
            context,
            self.writer.send(Frame {
                value: json!({"jsonrpc":"2.0","method":method,"params":params}),
                context: context.clone(),
                written: Some(written),
            }),
        )?
        .map_err(|_| protocol("language server stopped"))?;
        self.block(context, reply)?
            .map_err(|_| protocol("language server stopped"))?
    }
    pub fn request(
        &self,
        method: &str,
        params: Value,
        context: &OperationContext,
    ) -> Result<Value> {
        let start = Instant::now();
        let mut rpc_context = context.clone();
        rpc_context.deadline = rpc_context.deadline.min(start + Duration::from_secs(120));
        let result = self.request_inner(method, params, &rpc_context);
        let mut diagnostics = self.diagnostics.lock().unwrap_or_else(|e| e.into_inner());
        if diagnostics.len() == 128 {
            diagnostics.pop_front();
        }
        diagnostics.push_back(RpcDiagnostic {
            time: SystemTime::now(),
            operation: context.id.to_string(),
            method: method.into(),
            elapsed: start.elapsed(),
            outcome: result.as_ref().err().map(|error| error.kind),
        });
        result.map_err(|error| {
            error
                .with_operation(context.id)
                .with_method(method)
                .with_cause(self.stderr_tail())
        })
    }
    fn request_inner(
        &self,
        method: &str,
        params: Value,
        context: &OperationContext,
    ) -> Result<Value> {
        for _ in 0..4 {
            context.check()?;
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let (reply, receiver) = oneshot::channel();
            self.block(
                context,
                self.commands.send(Dispatch::Request {
                    id,
                    value: json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
                    context: context.clone(),
                    reply,
                }),
            )?
            .map_err(|_| protocol("language server stopped"))?;
            let result = self.block(context, receiver);
            let message = match result {
                Ok(Ok(result)) => result?,
                Ok(Err(_)) => return Err(protocol("language server stopped")),
                Err(error) => {
                    if self.commands.try_send(Dispatch::Cancel(id)).is_err() {
                        self.stop.send_replace(true);
                    }
                    return Err(error.with_method(method));
                }
            };
            if let Some(error) = message.get("error") {
                let code = error["code"]
                    .as_i64()
                    .ok_or_else(|| protocol("malformed RPC error"))?;
                if matches!(code, -32801 | -32802) {
                    continue;
                }
                return Err(protocol(format!(
                    "{method}: LSP error {code}: {}",
                    string_error(error)?
                ))
                .with_method(method));
            }
            return message
                .get("result")
                .cloned()
                .ok_or_else(|| protocol("LSP response has no result").with_method(method));
        }
        Err(protocol(format!(
            "{method}: language server repeatedly cancelled analysis; retry after indexing"
        )))
    }
    pub fn wait_until_ready(&self, context: &OperationContext) -> Result<()> {
        let mut ready = self.ready.clone();
        self.block(context, async move {
            loop {
                match ready.borrow_and_update().clone() {
                    Ok(true) => return Ok(()),
                    Err(e) => return Err(RefscapeError::new(ErrorKind::BackendUnavailable, e)),
                    Ok(false) => {}
                }
                if ready.changed().await.is_err() {
                    return Err(protocol("language server stopped"));
                }
            }
        })?
    }
}
fn string_error(error: &Value) -> Result<&str> {
    error["message"]
        .as_str()
        .ok_or_else(|| protocol("malformed RPC error message"))
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.dispose.send_replace(true);
    }
}

/// Worker-only completion wait, detached from the handle's immediate disposal.
pub struct ProcessDisposal(watch::Receiver<bool>, crate::runtime::RuntimeOwner);
impl ProcessDisposal {
    pub fn wait(mut self, context: &OperationContext) -> Result<()> {
        context.check()?;
        self.1.block_on(async {tokio::select! {
            _=context.cancel.cancelled()=>Err(RefscapeError::new(ErrorKind::Cancelled,"disposal cancelled")),
            _=tokio::time::sleep_until(context.deadline.into())=>Err(RefscapeError::new(ErrorKind::Timeout,"process disposal deadline exceeded")),
            result=async {while !*self.0.borrow_and_update(){self.0.changed().await.map_err(|_|protocol("process supervisor disappeared"))?;} Ok(())}=>result
        }})
    }
}

struct Dispatcher {
    pending: HashMap<u64, Pending>,
    folders: Value,
    writer: mpsc::Sender<Frame>,
    control: mpsc::Sender<Frame>,
    stop: watch::Sender<bool>,
    behavior: Box<dyn ServerBehavior>,
    ready: watch::Sender<std::result::Result<bool, String>>,
    epoch: Arc<AtomicU64>,
    registrations: Arc<Mutex<std::collections::BTreeMap<String, RegisteredFeature>>>,
}
impl Dispatcher {
    fn register(&mut self, params: &Value) -> std::result::Result<Value, i64> {
        let items = params["registrations"].as_array().ok_or(-32602)?;
        let items = items
            .iter()
            .map(registration)
            .collect::<Result<Vec<_>>>()
            .map_err(|_| -32601)?;
        let mut registrations = self.registrations.lock().unwrap_or_else(|e| e.into_inner());
        if items.len() + registrations.len() > 128 {
            return Err(-32602);
        }
        for (id, feature) in items {
            registrations.insert(id, feature);
        }
        self.epoch.fetch_add(1, Ordering::AcqRel);
        Ok(Value::Null)
    }
    fn unregister(&mut self, params: &Value) -> std::result::Result<Value, i64> {
        let items = params["unregisterations"].as_array().ok_or(-32602)?;
        let ids = items
            .iter()
            .map(|item| item["id"].as_str().ok_or(-32602))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut registrations = self.registrations.lock().unwrap_or_else(|e| e.into_inner());
        for id in ids {
            registrations.remove(id);
        }
        self.epoch.fetch_add(1, Ordering::AcqRel);
        Ok(Value::Null)
    }
    fn incoming(&mut self, message: Value) -> Option<Value> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            if let Some(id) = message["id"].as_u64()
                && let Some(request) = self.pending.remove(&id)
            {
                let _ = request
                    .reply
                    .send(request.context.check().map(|()| message));
            }
            return None;
        };
        let Some(id) = message.get("id") else {
            self.behavior.notification(method, &message["params"]);
            let _ = self.ready.send_replace(self.behavior.ready());
            return None;
        };
        let result = match method {
            "workspace/configuration" => message["params"]["items"]
                .as_array()
                .map(|items| {
                    Value::Array(
                        items
                            .iter()
                            .map(|item| {
                                self.behavior.scoped_configuration(
                                    item["section"].as_str(),
                                    item["scopeUri"].as_str(),
                                )
                            })
                            .collect(),
                    )
                })
                .ok_or(-32602),
            "workspace/workspaceFolders" => Ok(self.folders.clone()),
            "workspace/applyEdit" => {
                Ok(json!({"applied":false,"failureReason":"Refscape is a read-only explorer"}))
            }
            "window/workDoneProgress/create" => Ok(Value::Null),
            "workspace/semanticTokens/refresh" => {
                self.epoch.fetch_add(1, Ordering::AcqRel);
                Ok(Value::Null)
            }
            "client/registerCapability" => self.register(&message["params"]),
            "client/unregisterCapability" => self.unregister(&message["params"]),
            _ => Err(-32601),
        };
        Some(match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err(code) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":"Unsupported client request"}})
            }
        })
    }
    fn command(&mut self, command: Dispatch) -> Option<Value> {
        match command {
            Dispatch::Folders(value) => {
                self.folders = value;
                None
            }
            Dispatch::Cancel(id) => {
                self.pending.remove(&id);
                Some(json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":id}}))
            }
            Dispatch::Request {
                id,
                value,
                context,
                reply,
            } => {
                if let Err(error) = context.check() {
                    let _ = reply.send(Err(error));
                    return None;
                }
                if self.pending.len() >= QUEUE {
                    let _ = reply.send(Err(protocol("LSP in-flight request capacity exceeded")));
                    return None;
                }
                self.pending.insert(id, Pending { reply, context });
                Some(value)
            }
        }
    }
    async fn run(
        mut self,
        mut commands: mpsc::Receiver<Dispatch>,
        mut events: mpsc::Receiver<Result<Value>>,
    ) {
        let mut stopped = self.stop.subscribe();
        loop {
            let value = tokio::select! {
                _=stopped.changed()=>break,
                command=commands.recv()=>match command{Some(command)=>self.command(command),None=>break},
                event=events.recv()=>match event{Some(Ok(message))=>self.incoming(message),Some(Err(error))=>{for (_,request)in self.pending.drain(){let _=request.reply.send(Err(error.clone()));}break;},None=>break},
            };
            if let Some(value) = value {
                let own_request =
                    value.get("method").is_some() && value["method"] != "$/cancelRequest";
                let context = if own_request {
                    value["id"]
                        .as_u64()
                        .and_then(|id| self.pending.get(&id).map(|p| p.context.clone()))
                        .unwrap_or_else(|| OperationContext::detached(Duration::from_secs(2)))
                } else {
                    OperationContext::detached(Duration::from_secs(2))
                };
                let control = value.get("method").is_none()
                    || value["method"] == "$/cancelRequest"
                    || value["id"] == 0;
                let queue = if control { &self.control } else { &self.writer };
                if queue
                    .try_send(Frame {
                        value,
                        context,
                        written: None,
                    })
                    .is_err()
                {
                    break;
                }
            }
        }
        for (_, request) in self.pending {
            let _ = request.reply.send(Err(protocol("language server stopped")));
        }
        self.stop.send_replace(true);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_incremental_headers_and_truncated_bodies() {
        crate::runtime::RuntimeOwner::acquire().block_on(async {
            for bytes in [
                vec![b'x'; 8193],
                b"Content-Length: 999999999\r\n\r\n".to_vec(),
                b"Content-Length: 5\r\n\r\n{}".to_vec(),
                b"Content-Length: 0\r\nContent-Length: 0\r\n\r\n".to_vec(),
            ] {
                assert!(read_message(&mut bytes.as_slice()).await.is_err());
            }
        });
    }
    #[test]
    fn fragmented_unicode_frames_and_consecutive_messages() {
        crate::runtime::RuntimeOwner::acquire().block_on(async {
            let (mut peer, mut input) = tokio::io::duplex(128);
            let task = tokio::spawn(async move {
                write_message(
                    &mut peer,
                    &json!({"jsonrpc":"2.0","id":1,"result":"🦀 日本語"}),
                )
                .await
                .unwrap();
                write_message(&mut peer, &json!({"jsonrpc":"2.0","id":2,"result":null}))
                    .await
                    .unwrap();
            });
            assert_eq!(
                read_message(&mut input).await.unwrap()["result"],
                "🦀 日本語"
            );
            assert!(read_message(&mut input).await.unwrap()["result"].is_null());
            task.await.unwrap();
        });
    }
}
