//! Read-only Rust analysis through rust-analyzer's official Language Server Protocol.
//! Source structure is always supplied by the language server, never inferred from text.
mod project;
mod transport;

use refscape_application::LanguageService;
use refscape_model::{Position, ProjectCrate, SemanticToken, SourceDocument, SourceRange, Symbol};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use transport::Transport;

/// One persistent rust-analyzer process per opened project.
pub struct RustAnalyzer {
    executable: PathBuf,
    timeout: Duration,
    root: Option<PathBuf>,
    transport: Option<Transport>,
    opened: BTreeMap<PathBuf, String>,
    symbol_cache: BTreeMap<PathBuf, Vec<Symbol>>,
    token_cache: BTreeMap<PathBuf, Vec<SemanticToken>>,
    token_types: Vec<String>,
    token_modifiers: Vec<String>,
    semantic_tokens: bool,
    project: Option<project::Project>,
}

impl Default for RustAnalyzer {
    fn default() -> Self {
        Self::new(
            env::var_os("REFSCAPE_RUST_ANALYZER")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("rust-analyzer")),
        )
    }
}

impl RustAnalyzer {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            timeout: Duration::from_secs(120),
            root: None,
            transport: None,
            opened: BTreeMap::new(),
            symbol_cache: BTreeMap::new(),
            token_cache: BTreeMap::new(),
            token_types: vec![],
            token_modifiers: vec![],
            semantic_tokens: false,
            project: None,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn client(&mut self) -> Result<&mut Transport, String> {
        self.transport
            .as_mut()
            .ok_or_else(|| "open a Rust project before requesting analysis".into())
    }

    fn resolve(&self, path: &Path) -> Result<PathBuf, String> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.as_ref().ok_or("no project open")?.join(path)
        };
        path.canonicalize()
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
    }

    fn open_document(&mut self, path: &Path) -> Result<(PathBuf, String, String), String> {
        let path = self.resolve(path)?;
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let uri = path_uri(&path)?;
        if let Some(old) = self.opened.get(&path)
            && old != &text
        {
            // The explorer never edits, but external edits must reach the server.
            self.client()?
                .notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}))?;
            self.opened.remove(&path);
            self.symbol_cache.remove(&path);
            self.token_cache.remove(&path);
        }
        if !self.opened.contains_key(&path) {
            self.client()?.notify(
                "textDocument/didOpen",
                json!({"textDocument":{"uri":uri,"languageId":"rust","version":1,"text":text}}),
            )?;
            self.opened.insert(path.clone(), text.clone());
        }
        Ok((path, uri, text))
    }

    /// Search the server's Rust symbol index, then resolve results to complete source ranges.
    pub fn search(&mut self, query: &str) -> Result<Vec<Symbol>, String> {
        let values = self
            .client()?
            .request("workspace/symbol", json!({"query":query}))?;
        let mut symbols = vec![];
        for value in values.as_array().into_iter().flatten() {
            let location = &value["location"];
            if location.get("range").is_none() {
                continue;
            }
            let path = uri_path(string(location, "uri")?)?;
            let range: SourceRange = decode(&location["range"])?;
            symbols.push(self.symbol_at(&path, range, Some(string(value, "name")?.into()))?);
        }
        deduplicate(&mut symbols);
        Ok(symbols)
    }

    fn symbol_at(
        &mut self,
        path: &Path,
        range: SourceRange,
        name: Option<String>,
    ) -> Result<Symbol, String> {
        let (path, _, text) = self.open_document(path)?;
        let symbols = self.symbols(&path)?;
        if let Some(symbol) = enclosing(&symbols, range.start) {
            return Ok(symbol.clone());
        }
        let name =
            name.unwrap_or_else(|| slice(&text, range).unwrap_or("location").trim().to_string());
        Ok(Symbol {
            id: symbol_id(&path, range),
            name,
            kind: "location".into(),
            path,
            range,
            selection_range: range,
            children: vec![],
        })
    }

    fn navigate(
        &mut self,
        path: &Path,
        position: Position,
        references: bool,
    ) -> Result<Vec<Symbol>, String> {
        let (_, uri, text) = self.open_document(path)?;
        byte_offset(&text, position)?;
        let mut params = json!({"textDocument":{"uri":uri},"position":position});
        let method = if references {
            params["context"] = json!({"includeDeclaration":false});
            "textDocument/references"
        } else {
            "textDocument/definition"
        };
        let values = self.client()?.request(method, params)?;
        let locations = if values.is_null() {
            vec![]
        } else if let Some(values) = values.as_array() {
            values.clone()
        } else {
            vec![values]
        };
        let mut symbols = vec![];
        for value in locations {
            let (uri, range) = if value.get("targetUri").is_some() {
                (
                    string(&value, "targetUri")?,
                    decode::<SourceRange>(&value["targetSelectionRange"])?,
                )
            } else {
                (
                    string(&value, "uri")?,
                    decode::<SourceRange>(&value["range"])?,
                )
            };
            let path = uri_path(uri)?;
            symbols.push(self.symbol_at(&path, range, None)?);
        }
        deduplicate(&mut symbols);
        Ok(symbols)
    }
}

impl LanguageService for RustAnalyzer {
    fn project_crates(&mut self) -> Result<Vec<ProjectCrate>, String> {
        Ok(self
            .project
            .as_ref()
            .ok_or("no project open")?
            .crates
            .clone())
    }
    fn search(&mut self, query: &str) -> Result<Vec<Symbol>, String> {
        RustAnalyzer::search(self, query)
    }

    fn open_project(&mut self, root: &Path) -> Result<(), String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("cannot open {}: {e}", root.display()))?;
        if !root.join("Cargo.toml").is_file() {
            return Err(format!(
                "{} is not a Cargo project (Cargo.toml is missing)",
                root.display()
            ));
        }
        // Prepare the replacement independently so failed opens preserve the active project.
        let mut command = Command::new(&self.executable);
        command.current_dir(&root);
        let mut client = Transport::spawn(&mut command, self.timeout)?;
        let uri = path_uri(&root)?;
        let capabilities = client.request("initialize", json!({
            "processId":std::process::id(),"rootUri":uri,
            "clientInfo":{"name":"Refscape","version":env!("CARGO_PKG_VERSION")},
            "workspaceFolders":[{"uri":uri,"name":root.file_name().unwrap_or_default().to_string_lossy()}],
            "capabilities":{
                "general":{"positionEncodings":["utf-16"]},
                "window":{"workDoneProgress":true},
                "workspace":{"configuration":true,"workspaceFolders":true,"symbol":{"symbolKind":{"valueSet":(1..=26).collect::<Vec<_>>()}}},
                "textDocument":{
                    "documentSymbol":{"hierarchicalDocumentSymbolSupport":true},
                    "definition":{"linkSupport":true},
                    "semanticTokens":{"requests":{"full":true},"tokenTypes":["namespace","type","class","enum","interface","struct","typeParameter","parameter","variable","property","enumMember","event","function","method","macro","keyword","modifier","comment","string","number","regexp","operator","decorator"],"tokenModifiers":["declaration","definition","readonly","static","deprecated","abstract","async","modification","documentation","defaultLibrary"],"formats":["relative"],"overlappingTokenSupport":false,"multilineTokenSupport":false}
                },
                "experimental":{"serverStatusNotification":true}
            },
            "initializationOptions":{"checkOnSave":false}
        }))?;
        let encoding = capabilities["capabilities"]["positionEncoding"]
            .as_str()
            .unwrap_or("utf-16");
        if encoding != "utf-16" {
            return Err(format!(
                "unsupported rust-analyzer position encoding: {encoding}"
            ));
        }
        let provider = &capabilities["capabilities"]["semanticTokensProvider"];
        let semantic_tokens = !provider.is_null();
        let token_types = strings(&provider["legend"]["tokenTypes"]);
        let token_modifiers = strings(&provider["legend"]["tokenModifiers"]);
        client.notify("initialized", json!({}))?;
        client.wait_for_index()?;
        let project = project::Project::discover(&root, self.timeout)?;
        self.root = Some(root);
        self.project = Some(project);
        self.transport = Some(client);
        self.semantic_tokens = semantic_tokens;
        self.token_types = token_types;
        self.token_modifiers = token_modifiers;
        self.opened.clear();
        self.symbol_cache.clear();
        self.token_cache.clear();
        Ok(())
    }

    fn files(&mut self) -> Result<Vec<PathBuf>, String> {
        let project = self.project.as_ref().ok_or("no project open")?;
        let mut files = vec![];
        for package in &project.crates {
            collect_files(&package.root, &mut files)?;
        }
        files.extend(project.targets.iter().cloned());
        files.sort();
        files.dedup();
        Ok(files)
    }

    fn symbols(&mut self, path: &Path) -> Result<Vec<Symbol>, String> {
        let (path, uri, _) = self.open_document(path)?;
        if let Some(symbols) = self.symbol_cache.get(&path) {
            return Ok(symbols.clone());
        }
        let value = self.client()?.request(
            "textDocument/documentSymbol",
            json!({"textDocument":{"uri":uri}}),
        )?;
        let symbols = value
            .as_array()
            .into_iter()
            .flatten()
            .map(|value| document_symbol(value, &path))
            .collect::<Result<Vec<_>, _>>()?;
        self.symbol_cache.insert(path, symbols.clone());
        Ok(symbols)
    }

    fn source(&mut self, symbol: &Symbol) -> Result<SourceDocument, String> {
        let (path, uri, text) = self.open_document(&symbol.path)?;
        let mut symbol = symbol.clone();
        symbol.path = path.clone();
        if symbol.kind == "file" {
            symbol.range = full_range(&text);
            symbol.selection_range = symbol.range;
        }
        let code = slice(&text, symbol.range)?.to_string();
        let mut tokens = vec![];
        if self.semantic_tokens {
            if let Some(cached) = self.token_cache.get(&path) {
                tokens = cached.clone();
            } else {
                let result = self.client()?.request(
                    "textDocument/semanticTokens/full",
                    json!({"textDocument":{"uri":uri}}),
                )?;
                tokens =
                    semantic_tokens(&result["data"], &self.token_types, &self.token_modifiers)?;
                self.token_cache.insert(path, tokens.clone());
            }
            tokens.retain(|token| token_intersects(token, symbol.range));
        }
        Ok(SourceDocument {
            symbol,
            code,
            tokens,
        })
    }

    fn definitions(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.navigate(path, position, false)
    }

    fn references(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.navigate(path, position, true)
    }
}

fn collect_files(root: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|e| format!("cannot list {}: {e}", root.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if !matches!(
                entry.file_name().to_str(),
                Some("target" | ".git" | ".hg" | ".svn" | "node_modules")
            ) {
                collect_files(&path, output)?;
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
    Ok(())
}

fn decode<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|e| format!("invalid LSP response: {e}"))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("LSP response is missing {key}"))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn document_symbol(value: &Value, path: &Path) -> Result<Symbol, String> {
    let range: SourceRange = decode(value.get("range").unwrap_or(&value["location"]["range"]))?;
    let selection_range = value
        .get("selectionRange")
        .map(decode)
        .transpose()?
        .unwrap_or(range);
    let kind = match value["kind"].as_u64().unwrap_or(0) {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        19 => "object",
        20 => "key",
        21 => "null",
        22 => "enumMember",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "typeParameter",
        _ => "symbol",
    };
    let children = value["children"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|child| document_symbol(child, path))
        .collect::<Result<_, _>>()?;
    Ok(Symbol {
        id: symbol_id(path, range),
        name: string(value, "name")?.into(),
        kind: kind.into(),
        path: path.to_path_buf(),
        range,
        selection_range,
        children,
    })
}

fn symbol_id(path: &Path, range: SourceRange) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        path.display(),
        range.start.line,
        range.start.character,
        range.end.line,
        range.end.character
    )
}

fn before(left: Position, right: Position) -> bool {
    (left.line, left.character) < (right.line, right.character)
}

fn enclosing(symbols: &[Symbol], position: Position) -> Option<&Symbol> {
    symbols
        .iter()
        .filter(|symbol| {
            !before(position, symbol.range.start) && before(position, symbol.range.end)
        })
        .find_map(|symbol| enclosing(&symbol.children, position).or(Some(symbol)))
}

fn deduplicate(symbols: &mut Vec<Symbol>) {
    let mut seen = std::collections::BTreeSet::new();
    symbols.retain(|symbol| seen.insert(symbol.id.clone()));
}

fn token_intersects(token: &SemanticToken, range: SourceRange) -> bool {
    before(
        Position {
            line: token.line,
            character: token.start,
        },
        range.end,
    ) && before(
        range.start,
        Position {
            line: token.line,
            character: token.start.saturating_add(token.length),
        },
    )
}

fn semantic_tokens(
    value: &Value,
    types: &[String],
    modifiers: &[String],
) -> Result<Vec<SemanticToken>, String> {
    if value.is_null() {
        return Ok(vec![]);
    }
    let data: Vec<u32> = decode(value)?;
    if !data.len().is_multiple_of(5) {
        return Err("invalid semantic token data length".into());
    }
    let (mut line, mut start) = (0_u32, 0_u32);
    let mut tokens = vec![];
    for item in data.as_chunks::<5>().0 {
        line = line
            .checked_add(item[0])
            .ok_or("semantic token line overflow")?;
        start = if item[0] == 0 {
            start
                .checked_add(item[1])
                .ok_or("semantic token column overflow")?
        } else {
            item[1]
        };
        let kind = types
            .get(item[3] as usize)
            .ok_or("invalid semantic token type index")?
            .clone();
        let modifiers = modifiers
            .iter()
            .enumerate()
            .filter(|(index, _)| *index < 32 && item[4] & (1_u32 << index) != 0)
            .map(|(_, name)| name.clone())
            .collect();
        tokens.push(SemanticToken {
            line,
            start,
            length: item[2],
            kind,
            modifiers,
        });
    }
    Ok(tokens)
}

/// Convert the server's UTF-16 offset to a byte boundary without splitting surrogate pairs.
pub fn byte_offset(text: &str, position: Position) -> Result<usize, String> {
    let mut line_start = 0;
    for _ in 0..position.line {
        let newline = text[line_start..]
            .find('\n')
            .ok_or("source line is outside the document")?;
        line_start += newline + 1;
    }
    let end = text[line_start..]
        .find('\n')
        .map(|index| line_start + index)
        .unwrap_or(text.len());
    let content_end = if end > line_start && text.as_bytes()[end - 1] == b'\r' {
        end - 1
    } else {
        end
    };
    let mut utf16 = 0;
    for (index, ch) in text[line_start..content_end].char_indices() {
        if utf16 == position.character {
            return Ok(line_start + index);
        }
        utf16 += ch.len_utf16() as u32;
        if utf16 > position.character {
            return Err("UTF-16 position splits a surrogate pair".into());
        }
    }
    if utf16 == position.character {
        Ok(content_end)
    } else {
        Err("source column is outside the line".into())
    }
}

fn slice(text: &str, range: SourceRange) -> Result<&str, String> {
    let start = byte_offset(text, range.start)?;
    let end = byte_offset(text, range.end)?;
    text.get(start..end)
        .ok_or_else(|| "invalid source range".into())
}

/// The complete range of an unmodified UTF-8 source file, measured in LSP UTF-16 units.
pub fn full_range(text: &str) -> SourceRange {
    let line = text.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let last = text
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .trim_end_matches('\r');
    SourceRange {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line,
            character: last.encode_utf16().count() as u32,
        },
    }
}

fn path_uri(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or("LSP requires a Unicode file path")?
        .replace('\\', "/");
    let text = text
        .strip_prefix("//?/UNC/")
        .map(|rest| format!("//{rest}"))
        .unwrap_or_else(|| text.strip_prefix("//?/").unwrap_or(&text).to_string());
    let prefix = if text.starts_with("//") {
        "file:"
    } else if text.starts_with('/') {
        "file://"
    } else {
        "file:///"
    };
    let mut uri = prefix.to_string();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'.' | b'_' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(uri)
}

fn uri_path(uri: &str) -> Result<PathBuf, String> {
    let encoded = uri
        .strip_prefix("file://")
        .ok_or_else(|| format!("unsupported source URI: {uri}"))?;
    let mut bytes = vec![];
    let mut index = 0;
    while index < encoded.len() {
        if encoded.as_bytes()[index] == b'%' {
            let hex = encoded
                .get(index + 1..index + 3)
                .ok_or("invalid URI escape")?;
            bytes.push(u8::from_str_radix(hex, 16).map_err(|_| "invalid URI escape")?);
            index += 3;
        } else {
            bytes.push(encoded.as_bytes()[index]);
            index += 1;
        }
    }
    let text = String::from_utf8(bytes).map_err(|_| "non UTF-8 file URI")?;
    #[cfg(windows)]
    let text = if text.as_bytes().get(2) == Some(&b':') && text.starts_with('/') {
        text[1..].to_string()
    } else if !text.starts_with('/') {
        format!("//{text}")
    } else {
        text
    };
    #[cfg(not(windows))]
    let text = if !text.starts_with('/') {
        return Err("remote file URI is unsupported on this platform".into());
    } else {
        text
    };
    Ok(PathBuf::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_positions_and_crlf_preserve_exact_source() {
        let text = "fn 日本🦀() {\r\n  hi();\r\n}\n";
        assert_eq!(
            byte_offset(
                text,
                Position {
                    line: 0,
                    character: 7
                }
            )
            .unwrap(),
            13
        );
        assert!(
            byte_offset(
                text,
                Position {
                    line: 0,
                    character: 6
                }
            )
            .is_err()
        );
        assert!(
            byte_offset(
                text,
                Position {
                    line: 1,
                    character: 99
                }
            )
            .is_err()
        );
        assert_eq!(slice(text, full_range(text)).unwrap(), text);
    }

    #[test]
    fn source_paths_roundtrip_reserved_and_unicode_characters() {
        #[cfg(windows)]
        let path = Path::new("C:/日本語/a #%.rs");
        #[cfg(not(windows))]
        let path = Path::new("/日本語/a #%.rs");
        assert_eq!(uri_path(&path_uri(path).unwrap()).unwrap(), path);
        assert!(uri_path("file:///bad%GG").is_err());
    }

    #[test]
    fn semantic_token_deltas_reset_columns_on_new_lines() {
        let data = json!([1, 5, 3, 0, 1, 0, 4, 2, 1, 0, 2, 1, 4, 0, 0]);
        let tokens = semantic_tokens(
            &data,
            &["function".into(), "variable".into()],
            &["definition".into()],
        )
        .unwrap();
        assert_eq!((tokens[0].line, tokens[0].start), (1, 5));
        assert_eq!((tokens[1].line, tokens[1].start), (1, 9));
        assert_eq!((tokens[2].line, tokens[2].start), (3, 1));
        assert_eq!(tokens[0].modifiers, ["definition"]);
        assert!(semantic_tokens(&json!([0]), &[], &[]).is_err());
    }

    #[test]
    fn hierarchical_symbols_are_preserved_without_source_parsing() {
        let range = json!({"start":{"line":0,"character":0},"end":{"line":4,"character":1}});
        let symbol = document_symbol(&json!({"name":"impl Sample","kind":3,"range":range,"selectionRange":range,"children":[{"name":"new","kind":6,"range":range,"selectionRange":range}]}),Path::new("sample.rs")).unwrap();
        assert_eq!(symbol.children[0].kind, "method");
        assert_eq!(
            enclosing(
                &[symbol],
                Position {
                    line: 1,
                    character: 0
                }
            )
            .unwrap()
            .name,
            "new"
        );
    }
}
