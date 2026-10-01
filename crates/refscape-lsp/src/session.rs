use crate::{
    conversion::*,
    transport::{ServerBehavior, Transport},
};
use refscape_model::{Position, SemanticToken, SourceDocument, SourceRange, Symbol};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

/// Server-owned startup and document policy, without coupling protocol code to languages.
pub struct ServerConfiguration {
    pub name: String,
    pub installation_hint: String,
    pub initialization_options: Value,
    pub experimental_capabilities: Value,
    pub language_id: fn(&Path) -> &'static str,
    pub behavior: Box<dyn ServerBehavior>,
}

/// An initialized server session and the documents analyzed through it.
pub struct LspSession {
    root: PathBuf,
    transport: Transport,
    language_id: fn(&Path) -> &'static str,
    opened: BTreeMap<PathBuf, String>,
    symbol_cache: BTreeMap<PathBuf, Vec<Symbol>>,
    token_cache: BTreeMap<PathBuf, Vec<SemanticToken>>,
    token_types: Vec<String>,
    token_modifiers: Vec<String>,
    semantic_tokens: bool,
}

impl LspSession {
    pub fn start(
        root: PathBuf,
        command: &mut Command,
        timeout: Duration,
        configuration: ServerConfiguration,
    ) -> Result<Self, String> {
        let server = &configuration.name;
        let mut client = Transport::spawn(
            command,
            timeout,
            server.clone(),
            configuration.installation_hint,
            configuration.behavior,
        )?;
        let uri = path_uri(&root)?;
        let capabilities = client.request("initialize", json!({
            "processId":std::process::id(),"rootUri":uri,
            "clientInfo":{"name":"Refscape","version":env!("CARGO_PKG_VERSION")},
            "workspaceFolders":[{"uri":uri,"name":root.file_name().unwrap_or_default().to_string_lossy()}],
            "capabilities":{
                "general":{"positionEncodings":["utf-16"]},
                "offsetEncoding":["utf-16"],
                "window":{"workDoneProgress":true},
                "workspace":{"configuration":true,"workspaceFolders":true,"symbol":{"symbolKind":{"valueSet":(1..=26).collect::<Vec<_>>()}}},
                "textDocument":{
                    "documentSymbol":{"hierarchicalDocumentSymbolSupport":true},
                    "definition":{"linkSupport":true},
                    "typeDefinition":{"linkSupport":true},
                    "documentHighlight":{},
                    "hover":{"contentFormat":["plaintext"]},
                    "semanticTokens":{"requests":{"full":true},"tokenTypes":["namespace","type","class","enum","interface","struct","typeParameter","parameter","variable","property","enumMember","event","function","method","macro","keyword","modifier","comment","string","number","regexp","operator","decorator"],"tokenModifiers":["declaration","definition","readonly","static","deprecated","abstract","async","modification","documentation","defaultLibrary"],"formats":["relative"],"overlappingTokenSupport":false,"multilineTokenSupport":false}
                },
                "experimental":configuration.experimental_capabilities
            },
            "initializationOptions":configuration.initialization_options
        }))?;
        let encoding = capabilities["capabilities"]["positionEncoding"]
            .as_str()
            .or_else(|| capabilities["offsetEncoding"].as_str())
            .unwrap_or("utf-16");
        if encoding != "utf-16" {
            return Err(format!(
                "unsupported {server} position encoding: {encoding}"
            ));
        }
        let provider = &capabilities["capabilities"]["semanticTokensProvider"];
        let semantic_tokens = !provider.is_null();
        let token_types = strings(&provider["legend"]["tokenTypes"]);
        let token_modifiers = strings(&provider["legend"]["tokenModifiers"]);
        client.notify("initialized", json!({}))?;
        client.wait_until_ready()?;
        Ok(Self {
            root,
            transport: client,
            language_id: configuration.language_id,
            opened: BTreeMap::new(),
            symbol_cache: BTreeMap::new(),
            token_cache: BTreeMap::new(),
            semantic_tokens,
            token_types,
            token_modifiers,
        })
    }

    fn resolve(&self, path: &Path) -> Result<PathBuf, String> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
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
            self.transport
                .notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}))?;
            self.opened.remove(&path);
            self.symbol_cache.remove(&path);
            self.token_cache.remove(&path);
        }
        if !self.opened.contains_key(&path) {
            let language_id = (self.language_id)(&path);
            self.transport.notify(
                "textDocument/didOpen",
                json!({"textDocument":{"uri":uri,"languageId":language_id,"version":1,"text":text}}),
            )?;
            self.opened.insert(path.clone(), text.clone());
        }
        Ok((path, uri, text))
    }

    /// Search the server's symbol index, then resolve results to complete source ranges.
    pub fn search(&mut self, query: &str) -> Result<Vec<Symbol>, String> {
        let values = self
            .transport
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
        method: &str,
    ) -> Result<Vec<Symbol>, String> {
        let (_, uri, text) = self.open_document(path)?;
        byte_offset(&text, position)?;
        let mut params = json!({"textDocument":{"uri":uri},"position":position});
        if method == "textDocument/references" {
            params["context"] = json!({"includeDeclaration":false});
        }
        let values = self.transport.request(method, params)?;
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
    pub fn hover(&mut self, path: &Path, position: Position) -> Result<Option<String>, String> {
        let (_, uri, text) = self.open_document(path)?;
        byte_offset(&text, position)?;
        let value = self.transport.request(
            "textDocument/hover",
            json!({"textDocument":{"uri":uri},"position":position}),
        )?;
        hover_contents(&value)
    }
    pub fn symbols(&mut self, path: &Path) -> Result<Vec<Symbol>, String> {
        let (path, uri, _) = self.open_document(path)?;
        if let Some(symbols) = self.symbol_cache.get(&path) {
            return Ok(symbols.clone());
        }
        let value = self.transport.request(
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

    pub fn source(&mut self, symbol: &Symbol) -> Result<SourceDocument, String> {
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
                let result = self.transport.request(
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

    pub fn definitions(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.navigate(path, position, "textDocument/definition")
    }

    pub fn references(&mut self, path: &Path, position: Position) -> Result<Vec<Symbol>, String> {
        self.navigate(path, position, "textDocument/references")
    }

    pub fn type_definitions(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<Symbol>, String> {
        self.navigate(path, position, "textDocument/typeDefinition")
    }

    pub fn document_highlights(
        &mut self,
        path: &Path,
        position: Position,
    ) -> Result<Vec<SourceRange>, String> {
        let (_, uri, text) = self.open_document(path)?;
        byte_offset(&text, position)?;
        let value = self.transport.request(
            "textDocument/documentHighlight",
            json!({"textDocument":{"uri":uri},"position":position}),
        )?;
        if value.is_null() {
            return Ok(Vec::new());
        }
        value
            .as_array()
            .ok_or("Invalid document highlights from language server")?
            .iter()
            .map(|highlight| {
                let range = decode::<SourceRange>(&highlight["range"])?;
                range.validate()?;
                Ok(range)
            })
            .collect()
    }
}
