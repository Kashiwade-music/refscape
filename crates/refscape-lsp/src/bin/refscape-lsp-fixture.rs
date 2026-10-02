//! Deterministic stdio peer used by ordinary LSP contract tests; never a production backend.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    time::Duration,
};
fn read(reader: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = Some(value.trim().parse::<usize>().ok()?);
        }
    }
    let mut body = vec![0; length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}
fn send(value: Value) {
    let body = serde_json::to_vec(&value).unwrap();
    let mut stdout = io::stdout().lock();
    write!(stdout, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
    stdout.write_all(&body).unwrap();
    stdout.flush().unwrap();
}
fn response(id: &Value, result: Value) {
    send(json!({"jsonrpc":"2.0","id":id,"result":result}));
}
fn main() {
    let scenario = std::env::args().nth(1).unwrap_or_default();
    if scenario == "unread" {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    if scenario == "header" {
        io::stdout().write_all(&vec![b'x'; 8193]).unwrap();
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    if scenario == "truncated" {
        io::stdout()
            .write_all(b"Content-Length: 100\r\n\r\n{}")
            .unwrap();
        return;
    }
    if scenario == "exit" {
        std::process::exit(7);
    }
    let mut input = io::BufReader::new(io::stdin());
    let mut text = String::new();
    let mut uri = String::new();
    let mut symbols_count = 0;
    let mut tokens_count = 0;
    let mut version = 0;
    let mut idle_response = Value::Null;
    let mut documents = BTreeMap::<String, (String, i64)>::new();
    while let Some(message) = read(&mut input) {
        let method = message["method"].as_str().unwrap_or("");
        let id = &message["id"];
        if let Some(path) = std::env::var_os("REFSCAPE_FIXTURE_LOG") {
            let mut log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap();
            writeln!(log, "{}", message).unwrap();
        }
        if method.starts_with("textDocument/")
            && method != "textDocument/didOpen"
            && let Some(request_uri) = message["params"]["textDocument"]["uri"].as_str()
        {
            uri = request_uri.into();
            if let Some((stored, stored_version)) = documents.get(&uri) {
                text = stored.clone();
                version = *stored_version;
            }
        }
        match method {
            "initialize" => {
                let mut capabilities = json!({"positionEncoding":"utf-16","documentSymbolProvider":true,"workspaceSymbolProvider":true,"definitionProvider":true,"referencesProvider":true,"typeDefinitionProvider":true,"documentHighlightProvider":true,"hoverProvider":true,"semanticTokensProvider":{"full":true,"legend":{"tokenTypes":["variable"],"tokenModifiers":[]}}});
                if scenario == "range" {
                    capabilities["semanticTokensProvider"] = json!({"range":true,"legend":{"tokenTypes":["variable"],"tokenModifiers":[]}});
                }
                if scenario == "empty" || scenario == "dynamic" {
                    capabilities = json!({"positionEncoding":"utf-16"});
                }
                if scenario == "encoding" {
                    capabilities["positionEncoding"] = json!("utf-8");
                }
                if scenario == "invalid-encoding" {
                    capabilities["positionEncoding"] = json!(42);
                }
                if scenario == "invalid-provider" {
                    capabilities["semanticTokensProvider"]["full"] = json!(42);
                }
                response(id, json!({"capabilities":capabilities}));
            }
            "initialized" => {
                if scenario == "dynamic" {
                    send(
                        json!({"jsonrpc":"2.0","id":"registered","method":"client/registerCapability","params":{"registrations":[{"id":"symbols","method":"textDocument/documentSymbol"},{"id":"tokens","method":"textDocument/semanticTokens","registerOptions":{"full":true,"legend":{"tokenTypes":["property"],"tokenModifiers":[]}}}]}}),
                    );
                }
                if scenario == "folders" {
                    send(
                        json!({"jsonrpc":"2.0","id":"idle-config","method":"workspace/workspaceFolders","params":null}),
                    );
                }
                if scenario == "apply" {
                    send(
                        json!({"jsonrpc":"2.0","id":"idle-config","method":"workspace/applyEdit","params":{"edit":{"changes":{}}}}),
                    );
                }
                if scenario == "idle" || scenario == "flood" {
                    if scenario == "flood" {
                        for i in 0..10000 {
                            send(
                                json!({"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":i.to_string()}}),
                            );
                        }
                    }
                    send(
                        json!({"jsonrpc":"2.0","id":"idle-config","method":"workspace/configuration","params":{"items":[{"section":"test","scopeUri":"file:///scope"}]}}),
                    );
                }
            }
            "textDocument/didOpen" => {
                text = message["params"]["textDocument"]["text"]
                    .as_str()
                    .unwrap()
                    .into();
                uri = message["params"]["textDocument"]["uri"]
                    .as_str()
                    .unwrap()
                    .into();
                version = message["params"]["textDocument"]["version"]
                    .as_i64()
                    .unwrap();
                documents.insert(uri.clone(), (text.clone(), version));
            }
            "textDocument/didChange" => {
                text = message["params"]["contentChanges"][0]["text"]
                    .as_str()
                    .unwrap()
                    .into();
                version = message["params"]["textDocument"]["version"]
                    .as_i64()
                    .unwrap();
                documents.insert(uri.clone(), (text.clone(), version));
            }
            "textDocument/didClose" => {
                documents.remove(&uri);
            }
            "textDocument/documentSymbol" => {
                symbols_count += 1;
                if scenario == "mutate" {
                    std::fs::write(
                        std::env::var_os("REFSCAPE_FIXTURE_PATH").unwrap(),
                        "changed unrelated text\n",
                    )
                    .unwrap();
                }
                if scenario == "malformed" {
                    response(id, json!({"unexpected":"object"}));
                } else if scenario == "null" {
                    response(id, Value::Null);
                } else {
                    let end = text.lines().next().unwrap_or("").encode_utf16().count();
                    response(
                        id,
                        json!([{"name":"sample","kind":13,"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":end}},"selectionRange":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}}}]),
                    );
                }
            }
            "textDocument/semanticTokens/full" => {
                tokens_count += 1;
                if scenario == "refresh" && tokens_count == 1 {
                    send(
                        json!({"jsonrpc":"2.0","id":"refresh","method":"workspace/semanticTokens/refresh","params":null}),
                    );
                }
                response(
                    id,
                    json!({"data":if scenario == "invalid-token" { vec![999,0,1,0,0] } else { vec![0,0,1,0,0] }}),
                );
            }
            "textDocument/hover" => response(
                id,
                if scenario == "invalid-hover" {
                    json!({"contents":"hover","range":{"start":{"line":0,"character":0},"end":{"line":999,"character":0}}})
                } else {
                    json!({"contents":if scenario == "inspection-mutate" { text.as_str() } else { "hover" }})
                },
            ),
            "textDocument/documentHighlight" => {
                if scenario == "inspection-mutate" {
                    std::fs::write(
                        std::env::var_os("REFSCAPE_FIXTURE_PATH").unwrap(),
                        "changed unrelated text\n",
                    )
                    .unwrap();
                }
                response(
                    id,
                    if scenario == "invalid-highlight" {
                        json!([{"range":{"start":{"line":0,"character":0},"end":{"line":999,"character":0}}}])
                    } else {
                        Value::Null
                    },
                );
            }
            "textDocument/definition" | "textDocument/typeDefinition" => {
                let mut link = json!({"targetUri":uri,"targetRange":{"start":{"line":0,"character":0},"end":{"line":0,"character":text.lines().next().unwrap_or("").encode_utf16().count()}},"targetSelectionRange":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"originSelectionRange":{"start":{"line":0,"character":1},"end":{"line":0,"character":2}}});
                if scenario == "invalid-target" {
                    link["targetRange"]["end"]["line"] = json!(999);
                }
                if scenario == "invalid-origin" {
                    link["originSelectionRange"]["end"]["line"] = json!(999);
                }
                if scenario == "invalid-selection" {
                    link["targetRange"]["end"]["character"] = json!(0);
                }
                response(id, json!([link]));
            }
            "textDocument/references" => {
                if scenario == "repeated-location" {
                    let location = json!({"uri":uri,"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}}});
                    response(id, json!(vec![location; 6]));
                } else {
                    response(id, json!([]));
                }
            }
            "workspace/symbol" => response(id, json!([])),
            "test/status" => response(
                id,
                json!({"symbols":symbols_count,"tokens":tokens_count,"version":version,"idle":idle_response}),
            ),
            "test/folders" => {
                send(
                    json!({"jsonrpc":"2.0","id":"folders","method":"workspace/workspaceFolders","params":null}),
                );
                response(id, Value::Null);
            }
            "test/stall" => {}
            "test/late" => {
                std::thread::sleep(Duration::from_millis(150));
                response(id, json!("late"));
            }
            "test/retry" => {
                if symbols_count < 3 {
                    symbols_count += 1;
                    send(
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32801,"message":"content changed"}}),
                    );
                } else {
                    response(id, json!("retried"));
                }
            }
            "shutdown" => {
                if scenario != "shutdown-stall" {
                    response(id, Value::Null);
                }
            }
            "exit" => {
                if scenario != "shutdown-stall" {
                    break;
                }
            }
            "" if id == "idle-config" => {
                idle_response = message["result"].clone();
            }
            _ => {}
        }
    }
}
