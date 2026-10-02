use crate::{
    conversion::*,
    transport::{ServerBehavior, Transport},
};
use refscape_analysis::{
    AnalysisCapabilities, AnalysisResult, FeatureResult, NavigationLocation, NavigationTarget,
};
use refscape_model::{
    DocumentSnapshot, ErrorKind, JobId, OperationContext, Position, ProjectEpoch, RefscapeError,
    SemanticToken, SourceDocument, SourceRange, Symbol,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Instant,
};

pub struct ServerConfiguration {
    pub name: String,
    pub installation_hint: String,
    pub initialization_options: Value,
    pub experimental_capabilities: Value,
    pub language_id: fn(&Path) -> &'static str,
    pub behavior: Box<dyn ServerBehavior>,
}
struct Document {
    snapshot: Option<Arc<DocumentSnapshot>>,
    version: u64,
    symbol_index: Option<(u64, Arc<crate::context::SymbolIndex>)>,
    tokens: Option<(u64, Arc<Vec<SemanticToken>>)>,
    symbol_bytes: usize,
    token_bytes: usize,
}
impl Document {
    fn retained_bytes(&self) -> usize {
        // Conservative accounting includes UTF-16 boundary vectors, line-vector
        // capacity, and empty-line allocations. Server residency is independent.
        let source = self.snapshot.as_ref().map_or(0, |snapshot| {
            snapshot
                .text
                .len()
                .saturating_mul(33)
                .saturating_add(snapshot.index.line_count().saturating_mul(176))
        });
        source
            .saturating_add(self.symbol_bytes)
            .saturating_add(self.token_bytes)
    }
}
fn symbol_bytes(symbols: &[Symbol]) -> usize {
    let mut stack = symbols.iter().collect::<Vec<_>>();
    let mut bytes = 0usize;
    while let Some(symbol) = stack.pop() {
        bytes = bytes
            .saturating_add(std::mem::size_of::<Symbol>().saturating_mul(4))
            .saturating_add(symbol.id.len())
            .saturating_add(symbol.name.len())
            .saturating_add(symbol.kind.len())
            .saturating_add(symbol.path.as_os_str().len().saturating_mul(2));
        stack.extend(&symbol.children);
    }
    bytes
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentStatistics {
    pub opened: usize,
    pub snapshots: usize,
    pub symbol_caches: usize,
    pub token_caches: usize,
    pub retained_bytes: usize,
    pub disk_reads: u64,
    pub operation_snapshots: usize,
}
/// One initialized backend. All feature conversions in an operation share one immutable document version.
pub struct LspProjectSession {
    root: PathBuf,
    transport: Transport,
    language_id: fn(&Path) -> &'static str,
    documents: BTreeMap<PathBuf, Document>,
    capabilities: AnalysisCapabilities,
    cache_order: VecDeque<PathBuf>,
    token_types: Vec<String>,
    token_modifiers: Vec<String>,
    operation_stamp: Option<(JobId, ProjectEpoch, Instant)>,
    operation_snapshots: HashMap<PathBuf, Arc<DocumentSnapshot>>,
    operation_aliases: HashMap<PathBuf, PathBuf>,
    disk_reads: u64,
}
fn protocol(message: impl Into<String>) -> RefscapeError {
    RefscapeError::new(ErrorKind::Protocol, message)
}
fn convert<T>(value: std::result::Result<T, String>) -> AnalysisResult<T> {
    value.map_err(protocol)
}
fn enabled(value: &Value) -> bool {
    matches!(value, Value::Bool(true) | Value::Object(_))
}
fn capability(wire: &serde_json::Map<String, Value>, key: &str) -> AnalysisResult<bool> {
    match wire.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(value @ Value::Bool(_)) | Some(value @ Value::Object(_)) => Ok(enabled(value)),
        _ => Err(protocol(format!("invalid server capability: {key}"))),
    }
}
fn values<'a>(value: &'a Value, method: &str) -> AnalysisResult<&'a [Value]> {
    if value.is_null() {
        Ok(&[])
    } else {
        value
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| protocol(format!("invalid {method} response")))
    }
}
impl LspProjectSession {
    pub fn start(
        root: PathBuf,
        command: &mut Command,
        context: &OperationContext,
        configuration: ServerConfiguration,
    ) -> AnalysisResult<Self> {
        context.check()?;
        let client = Transport::spawn(
            command,
            configuration.name.clone(),
            configuration.installation_hint,
            configuration.behavior,
        )?;
        let uri = convert(path_uri(&root))?;
        let folders =
            json!([{"uri":uri,"name":root.file_name().unwrap_or_default().to_string_lossy()}]);
        client.set_folders(folders.clone(), context)?;
        let result=client.request("initialize",json!({
            "processId":std::process::id(),"rootUri":uri,
            "clientInfo":{"name":"Refscape","version":env!("CARGO_PKG_VERSION")},"workspaceFolders":folders,
            "capabilities":{
                "general":{"positionEncodings":["utf-16"]},"offsetEncoding":["utf-16"],"window":{"workDoneProgress":true},
                "workspace":{"configuration":true,"workspaceFolders":true,"semanticTokens":{"refreshSupport":true},"symbol":{"symbolKind":{"valueSet":(1..=26).collect::<Vec<_>>()}}},
                "textDocument":{"documentSymbol":{"hierarchicalDocumentSymbolSupport":true},"definition":{"linkSupport":true},"typeDefinition":{"linkSupport":true},"documentHighlight":{},"hover":{"contentFormat":["plaintext"]},
                    "semanticTokens":{"requests":{"full":true},"tokenTypes":["namespace","type","class","enum","interface","struct","typeParameter","parameter","variable","property","enumMember","event","function","method","macro","keyword","modifier","comment","string","number","regexp","operator","decorator"],"tokenModifiers":["declaration","definition","readonly","static","deprecated","abstract","async","modification","documentation","defaultLibrary"],"formats":["relative"],"overlappingTokenSupport":false,"multilineTokenSupport":false}},
                "experimental":configuration.experimental_capabilities},"initializationOptions":configuration.initialization_options
        }),context)?;
        let wire = result
            .get("capabilities")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol("initialize response has no capabilities"))?;
        let encoding = match wire
            .get("positionEncoding")
            .or_else(|| result.get("offsetEncoding"))
        {
            None => "utf-16",
            Some(value) => value
                .as_str()
                .ok_or_else(|| protocol("invalid position encoding"))?,
        };
        if encoding != "utf-16" {
            return Err(RefscapeError::new(
                ErrorKind::Unsupported,
                format!(
                    "unsupported {} position encoding: {encoding}",
                    configuration.name
                ),
            ));
        }
        let provider = wire.get("semanticTokensProvider").unwrap_or(&Value::Null);
        if !provider.is_null() && !provider.is_object() {
            return Err(protocol("invalid semantic tokens provider"));
        }
        let semantic = match provider.get("full") {
            None | Some(Value::Null) => false,
            Some(value @ Value::Bool(_)) | Some(value @ Value::Object(_)) => enabled(value),
            _ => return Err(protocol("invalid full semantic tokens capability")),
        };
        let legend = |key: &str| -> AnalysisResult<Vec<String>> {
            if !semantic {
                return Ok(vec![]);
            }
            provider["legend"][key]
                .as_array()
                .ok_or_else(|| protocol("invalid semantic token legend"))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| protocol("invalid semantic token legend entry"))
                })
                .collect()
        };
        let token_types = legend("tokenTypes")?;
        let token_modifiers = legend("tokenModifiers")?;
        let capabilities = AnalysisCapabilities {
            document_symbols: capability(wire, "documentSymbolProvider")?,
            workspace_symbols: capability(wire, "workspaceSymbolProvider")?,
            definitions: capability(wire, "definitionProvider")?,
            references: capability(wire, "referencesProvider")?,
            type_definitions: capability(wire, "typeDefinitionProvider")?,
            highlights: capability(wire, "documentHighlightProvider")?,
            hover: capability(wire, "hoverProvider")?,
            semantic_tokens: semantic,
        };
        client.notify("initialized", json!({}), context)?;
        client.wait_until_ready(context)?;
        Ok(Self {
            root,
            transport: client,
            language_id: configuration.language_id,
            documents: BTreeMap::new(),
            cache_order: VecDeque::new(),
            capabilities,
            token_types,
            token_modifiers,
            operation_stamp: None,
            operation_snapshots: HashMap::new(),
            operation_aliases: HashMap::new(),
            disk_reads: 0,
        })
    }
    pub fn capabilities(&self) -> AnalysisCapabilities {
        let mut capabilities = self.capabilities.clone();
        let (symbols, tokens) = self.transport.dynamic_capabilities();
        capabilities.document_symbols |= symbols;
        capabilities.semantic_tokens |= tokens.is_some();
        capabilities
    }
    pub fn disposal(&self) -> crate::transport::ProcessDisposal {
        self.transport.disposal()
    }
    pub fn analysis_epoch(&self) -> u64 {
        self.transport.analysis_epoch()
    }
    fn begin_operation(&mut self, context: &OperationContext) {
        let stamp = (context.id, context.project, context.deadline);
        if self.operation_stamp != Some(stamp) {
            self.operation_snapshots.clear();
            self.operation_aliases.clear();
            self.operation_stamp = Some(stamp);
        }
    }
    pub fn finish_operation(&mut self, context: &OperationContext) {
        if self.operation_stamp == Some((context.id, context.project, context.deadline)) {
            self.operation_snapshots.clear();
            self.operation_aliases.clear();
            self.operation_stamp = None;
        }
    }
    fn capture(&mut self, alias: &Path, snapshot: Arc<DocumentSnapshot>) -> Arc<DocumentSnapshot> {
        self.operation_aliases
            .insert(alias.to_path_buf(), snapshot.path.clone());
        self.operation_snapshots
            .insert(snapshot.path.clone(), snapshot.clone());
        snapshot
    }
    /// A committed metadata generation invalidates analyses even when document bytes did not change.
    pub fn invalidate_analysis(&mut self) -> u64 {
        self.transport.invalidate_analysis()
    }
    pub fn document_statistics(&self) -> DocumentStatistics {
        DocumentStatistics {
            opened: self.documents.len(),
            snapshots: self
                .documents
                .values()
                .filter(|doc| doc.snapshot.is_some())
                .count(),
            symbol_caches: self
                .documents
                .values()
                .filter(|doc| doc.symbol_index.is_some())
                .count(),
            token_caches: self
                .documents
                .values()
                .filter(|doc| doc.tokens.is_some())
                .count(),
            retained_bytes: self.documents.values().fold(0usize, |bytes, doc| {
                bytes.saturating_add(doc.retained_bytes())
            }),
            disk_reads: self.disk_reads,
            operation_snapshots: self.operation_snapshots.len(),
        }
    }
    fn resolve(&self, path: &Path) -> AnalysisResult<PathBuf> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        path.canonicalize().map_err(|e| {
            RefscapeError::new(
                ErrorKind::Io,
                format!("cannot read {}: {e}", path.display()),
            )
            .with_path(path)
        })
    }
    fn open_document(
        &mut self,
        path: &Path,
        context: &OperationContext,
    ) -> AnalysisResult<Arc<DocumentSnapshot>> {
        context.check()?;
        self.begin_operation(context);
        if let Some(identity) = self.operation_aliases.get(path)
            && let Some(snapshot) = self.operation_snapshots.get(identity)
        {
            return Ok(snapshot.clone());
        }
        if let Some(snapshot) = self.operation_snapshots.get(path) {
            return Ok(snapshot.clone());
        }
        let alias = path.to_path_buf();
        let path = self.resolve(path)?;
        if let Some(snapshot) = self.operation_snapshots.get(&path) {
            let snapshot = snapshot.clone();
            return Ok(self.capture(&alias, snapshot));
        }
        self.disk_reads = self.disk_reads.saturating_add(1);
        let text = fs::read_to_string(&path).map_err(|e| {
            RefscapeError::new(
                ErrorKind::Io,
                format!("cannot read {}: {e}", path.display()),
            )
            .with_path(path.clone())
        })?;
        context.check()?;
        if let Some(document) = self.documents.get(&path)
            && let Some(snapshot) = &document.snapshot
            && snapshot.text.as_ref() == text
        {
            let snapshot = snapshot.clone();
            self.bounded_caches(&path);
            return Ok(self.capture(&alias, snapshot));
        }
        let uri = convert(path_uri(&path))?;
        let version = self.documents.get(&path).map_or(Ok(1), |doc| {
            doc.version
                .checked_add(1)
                .ok_or_else(|| protocol("document version overflow"))
        })?;
        let snapshot = Arc::new(DocumentSnapshot::new(
            path.clone(),
            version,
            Arc::from(text),
        )?);
        let version =
            i32::try_from(version).map_err(|_| protocol("LSP document version overflow"))?;
        if self.documents.contains_key(&path) {
            self.transport.notify("textDocument/didChange",json!({"textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":snapshot.text.as_ref()}]}),context)?;
        } else {
            self.transport.notify("textDocument/didOpen",json!({"textDocument":{"uri":uri,"languageId":(self.language_id)(&path),"version":version,"text":snapshot.text.as_ref()}}),context)?;
        }
        self.documents.insert(
            path,
            Document {
                snapshot: Some(snapshot.clone()),
                version: snapshot.version,
                symbol_index: None,
                tokens: None,
                symbol_bytes: 0,
                token_bytes: 0,
            },
        );
        self.bounded_caches(&snapshot.path);
        Ok(self.capture(&alias, snapshot))
    }
    pub fn close_documents(
        &mut self,
        paths: &[PathBuf],
        context: &OperationContext,
    ) -> AnalysisResult<()> {
        for path in paths {
            context.check()?;
            if self.documents.contains_key(path) {
                self.transport.notify(
                    "textDocument/didClose",
                    json!({"textDocument":{"uri":convert(path_uri(path))?}}),
                    context,
                )?;
                self.documents.remove(path);
                self.cache_order.retain(|cached| cached != path);
            }
        }
        Ok(())
    }
    fn valid_snapshot(&self, snapshot: &DocumentSnapshot, epoch: u64) -> AnalysisResult<()> {
        if self.analysis_epoch() != epoch
            || self
                .documents
                .get(&snapshot.path)
                .is_none_or(|doc| doc.version != snapshot.version)
        {
            Err(RefscapeError::new(
                ErrorKind::Stale,
                "analysis snapshot or capability epoch changed",
            ))
        } else {
            Ok(())
        }
    }
    fn bounded_caches(&mut self, path: &Path) {
        self.cache_order.retain(|cached| cached != path);
        self.cache_order.push_back(path.to_path_buf());
        while self.cache_order.len() > 128
            || self
                .cache_order
                .iter()
                .filter_map(|path| self.documents.get(path))
                .fold(0usize, |bytes, doc| {
                    bytes.saturating_add(doc.retained_bytes())
                })
                > 32 * 1024 * 1024
        {
            if let Some(path) = self.cache_order.pop_front()
                && let Some(doc) = self.documents.get_mut(&path)
            {
                doc.symbol_index = None;
                doc.tokens = None;
                doc.snapshot = None;
                doc.symbol_bytes = 0;
                doc.token_bytes = 0;
            }
        }
    }
    fn index_snapshot(
        &mut self,
        snapshot: &Arc<DocumentSnapshot>,
        context: &OperationContext,
    ) -> AnalysisResult<Arc<crate::context::SymbolIndex>> {
        for _ in 0..4 {
            context.check()?;
            match self.index_once(snapshot, context) {
                Err(error) if error.kind == ErrorKind::Stale => continue,
                result => return result,
            }
        }
        Err(RefscapeError::new(
            ErrorKind::Stale,
            "server repeatedly refreshed document symbols",
        ))
    }
    fn index_once(
        &mut self,
        snapshot: &Arc<DocumentSnapshot>,
        context: &OperationContext,
    ) -> AnalysisResult<Arc<crate::context::SymbolIndex>> {
        if !self.capabilities().document_symbols {
            return Ok(Arc::new(crate::context::SymbolIndex::new(
                snapshot,
                Arc::new(vec![]),
            )?));
        }
        let epoch = self.analysis_epoch();
        if let Some((cached_epoch, symbols)) = &self.documents[&snapshot.path].symbol_index
            && *cached_epoch == epoch
        {
            let symbols = symbols.clone();
            self.bounded_caches(&snapshot.path);
            return Ok(symbols);
        }
        let result = self.transport.request(
            "textDocument/documentSymbol",
            json!({"textDocument":{"uri":convert(path_uri(&snapshot.path))?}}),
            context,
        )?;
        let symbols = values(&result, "document symbols")?
            .iter()
            .map(|value| convert(document_symbol(value, &snapshot.path)))
            .collect::<AnalysisResult<Vec<_>>>()?;
        let symbols = Arc::new(symbols);
        let index = Arc::new(crate::context::SymbolIndex::new(snapshot, symbols.clone())?);
        self.valid_snapshot(snapshot, epoch)?;
        let doc = self.documents.get_mut(&snapshot.path).unwrap();
        doc.symbol_bytes = symbol_bytes(&symbols);
        doc.symbol_index = Some((epoch, index.clone()));
        self.bounded_caches(&snapshot.path);
        Ok(index)
    }
    pub fn symbols(
        &mut self,
        path: &Path,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<Symbol>> {
        let snapshot = self.open_document(path, context)?;
        Ok(self
            .index_snapshot(&snapshot, context)?
            .symbols()
            .as_ref()
            .clone())
    }
    pub fn document_fingerprint(
        &mut self,
        path: &Path,
        context: &OperationContext,
    ) -> AnalysisResult<refscape_model::DocumentFingerprint> {
        Ok(self.open_document(path, context)?.fingerprint)
    }
    pub fn search(
        &mut self,
        query: &str,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<Symbol>> {
        context.check()?;
        if !self.capabilities.workspace_symbols {
            return Ok(vec![]);
        }
        let result = self
            .transport
            .request("workspace/symbol", json!({"query":query}), context)?;
        let mut snapshots = HashMap::new();
        let mut symbols = vec![];
        for value in values(&result, "workspace symbols")? {
            context.check()?;
            let location = &value["location"];
            if location.get("range").is_none() {
                continue;
            }
            let path = convert(uri_path(convert(string(location, "uri"))?))?;
            let range = convert(decode(&location["range"]))?;
            symbols.push(self.symbol_at(
                &path,
                range,
                Some(convert(string(value, "name"))?.into()),
                &mut snapshots,
                context,
            )?);
        }
        deduplicate(&mut symbols);
        Ok(symbols)
    }
    fn symbol_at(
        &mut self,
        path: &Path,
        range: SourceRange,
        name: Option<String>,
        snapshots: &mut HashMap<PathBuf, Arc<DocumentSnapshot>>,
        context: &OperationContext,
    ) -> AnalysisResult<Symbol> {
        let snapshot = if let Some(snapshot) = snapshots.get(path) {
            snapshot.clone()
        } else {
            let snapshot = self.open_document(path, context)?;
            snapshots.insert(path.to_path_buf(), snapshot.clone());
            snapshots.insert(snapshot.path.clone(), snapshot.clone());
            snapshot
        };
        snapshot.index.range_bytes(range)?;
        let index = self.index_snapshot(&snapshot, context)?;
        if let Some(symbol) = index.enclosing(range.start) {
            return Ok(symbol.clone());
        }
        let name = match name {
            Some(name) => name,
            None => snapshot
                .index
                .slice(&snapshot.text, range)?
                .trim()
                .to_string(),
        };
        Ok(Symbol {
            id: symbol_id(&snapshot.path, range),
            name,
            kind: "location".into(),
            path: snapshot.path.clone(),
            range,
            selection_range: range,
            children: vec![],
        })
    }
    pub fn navigation_locations(
        &mut self,
        path: &Path,
        position: Position,
        method: &str,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationLocation>> {
        let snapshot = self.open_document(path, context)?;
        self.locations_snapshot(&snapshot, position, method, context)
    }
    fn locations_snapshot(
        &mut self,
        snapshot: &Arc<DocumentSnapshot>,
        position: Position,
        method: &str,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationLocation>> {
        snapshot.index.byte_offset(position)?;
        let mut params = json!({"textDocument":{"uri":convert(path_uri(&snapshot.path))?},"position":position_wire(position)});
        if method == "textDocument/references" {
            params["context"] = json!({"includeDeclaration":false});
        }
        let result = self.transport.request(method, params, context)?;
        let locations = if result.is_null() {
            vec![]
        } else if let Some(values) = result.as_array() {
            values.clone()
        } else if result.is_object() {
            vec![result]
        } else {
            return Err(protocol("invalid navigation locations"));
        };
        let mut locations = locations
            .iter()
            .map(|value| {
                if value.get("targetUri").is_some() {
                    Ok(NavigationLocation {
                        document: convert(uri_path(convert(string(value, "targetUri"))?))?,
                        target_range: convert(decode(&value["targetRange"]))?,
                        selection_range: convert(decode(&value["targetSelectionRange"]))?,
                        origin_range: value
                            .get("originSelectionRange")
                            .map(|v| convert(decode(v)))
                            .transpose()?,
                    })
                } else {
                    let range = convert(decode(&value["range"]))?;
                    Ok(NavigationLocation {
                        document: convert(uri_path(convert(string(value, "uri"))?))?,
                        target_range: range,
                        selection_range: range,
                        origin_range: None,
                    })
                }
            })
            .collect::<AnalysisResult<Vec<_>>>()?;
        for location in &mut locations {
            context.check()?;
            if let Some(origin) = location.origin_range {
                snapshot
                    .index
                    .range_bytes(origin)
                    .map_err(|error| protocol(error.to_string()))?;
            }
            let target = self.open_document(&location.document, context)?;
            location.document = target.path.clone();
            target
                .index
                .range_bytes(location.target_range)
                .map_err(|error| protocol(error.to_string()))?;
            target
                .index
                .range_bytes(location.selection_range)
                .map_err(|error| protocol(error.to_string()))?;
            if location.selection_range.start < location.target_range.start
                || location.selection_range.end > location.target_range.end
            {
                return Err(protocol("navigation selection outside target range"));
            }
        }
        Ok(locations)
    }
    fn navigate(
        &mut self,
        path: &Path,
        position: Position,
        method: &str,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationTarget>> {
        let snapshot = self.open_document(path, context)?;
        let locations = self.locations_snapshot(&snapshot, position, method, context)?;
        let mut snapshots = HashMap::from([(snapshot.path.clone(), snapshot)]);
        let mut symbols = vec![];
        for location in locations {
            location.target_range.validate().map_err(protocol)?;
            location.selection_range.validate().map_err(protocol)?;
            if location.selection_range.start < location.target_range.start
                || location.selection_range.end > location.target_range.end
            {
                return Err(protocol("navigation selection outside target range"));
            }
            let symbol = self.symbol_at(
                &location.document,
                location.selection_range,
                None,
                &mut snapshots,
                context,
            )?;
            symbols.push(NavigationTarget { symbol, location });
        }
        // Preserve links whose official target/origin ranges differ, even when
        // the existing display policy resolves them to the same enclosing symbol.
        let mut seen = std::collections::HashSet::new();
        let range_key = |range: SourceRange| {
            (
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character,
            )
        };
        symbols.retain(|target| {
            seen.insert((
                target.symbol.id.clone(),
                target.location.document.clone(),
                range_key(target.location.target_range),
                range_key(target.location.selection_range),
                target.location.origin_range.map(range_key),
            ))
        });
        Ok(symbols)
    }
    pub fn definitions(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationTarget>> {
        if !self.capabilities.definitions {
            return Err(RefscapeError::new(
                ErrorKind::Unsupported,
                "definitions unsupported",
            ));
        }
        self.navigate(path, position, "textDocument/definition", context)
    }
    pub fn references(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<Vec<NavigationTarget>> {
        if !self.capabilities.references {
            return Err(RefscapeError::new(
                ErrorKind::Unsupported,
                "references unsupported",
            ));
        }
        self.navigate(path, position, "textDocument/references", context)
    }
    pub fn type_definitions(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Vec<NavigationTarget>>> {
        context.check()?;
        if !self.capabilities.type_definitions {
            return Ok(FeatureResult::Unsupported);
        }
        self.navigate(path, position, "textDocument/typeDefinition", context)
            .map(FeatureResult::Supported)
    }
    pub fn hover(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Option<String>>> {
        context.check()?;
        if !self.capabilities.hover {
            return Ok(FeatureResult::Unsupported);
        }
        let snapshot = self.open_document(path, context)?;
        snapshot.index.byte_offset(position)?;
        let result=self.transport.request("textDocument/hover",json!({"textDocument":{"uri":convert(path_uri(&snapshot.path))?},"position":position_wire(position)}),context)?;
        if let Some(range) = result.get("range") {
            snapshot
                .index
                .range_bytes(convert(decode(range))?)
                .map_err(|error| protocol(error.to_string()))?;
        }
        convert(hover_contents(&result)).map(FeatureResult::Supported)
    }
    pub fn document_highlights(
        &mut self,
        path: &Path,
        position: Position,
        context: &OperationContext,
    ) -> AnalysisResult<FeatureResult<Vec<SourceRange>>> {
        context.check()?;
        if !self.capabilities.highlights {
            return Ok(FeatureResult::Unsupported);
        }
        let snapshot = self.open_document(path, context)?;
        snapshot.index.byte_offset(position)?;
        let result=self.transport.request("textDocument/documentHighlight",json!({"textDocument":{"uri":convert(path_uri(&snapshot.path))?},"position":position_wire(position)}),context)?;
        let ranges = values(&result, "document highlights")?
            .iter()
            .map(|value| {
                let range = convert(decode(&value["range"]))?;
                snapshot
                    .index
                    .range_bytes(range)
                    .map_err(|error| protocol(error.to_string()))?;
                Ok(range)
            })
            .collect::<AnalysisResult<Vec<_>>>()?;
        Ok(FeatureResult::Supported(ranges))
    }
    pub fn source(
        &mut self,
        target: &Symbol,
        context: &OperationContext,
    ) -> AnalysisResult<SourceDocument> {
        let snapshot = self.open_document(&target.path, context)?;
        for _ in 0..4 {
            context.check()?;
            match self.source_snapshot(target, &snapshot, context) {
                Err(error) if error.kind == ErrorKind::Stale => continue,
                result => return result,
            }
        }
        Err(RefscapeError::new(
            ErrorKind::Stale,
            "server repeatedly refreshed source analysis",
        ))
    }
    fn source_snapshot(
        &mut self,
        target: &Symbol,
        snapshot: &Arc<DocumentSnapshot>,
        context: &OperationContext,
    ) -> AnalysisResult<SourceDocument> {
        let mut symbol = target.clone();
        symbol.path = snapshot.path.clone();
        if symbol.kind == "file" {
            symbol.range = full_range(&snapshot.text)?;
            symbol.selection_range = symbol.range;
        }
        let epoch = self.analysis_epoch();
        let index = if symbol.kind == "file" {
            Arc::new(crate::context::SymbolIndex::new(
                snapshot,
                Arc::new(vec![]),
            )?)
        } else {
            self.index_snapshot(snapshot, context)?
        };
        let builder = crate::context::ExcerptBuilder::new(snapshot, &index);
        let range = builder.range(symbol.range)?;
        let code = snapshot.index.slice(&snapshot.text, range)?.to_string();
        let headers = if symbol.kind == "file" {
            vec![]
        } else {
            builder.context(&symbol)?
        };
        let folded = builder.gaps(&headers, range)?;
        let mut tokens = vec![];
        if self.capabilities().semantic_tokens {
            let cached = self.documents[&snapshot.path]
                .tokens
                .as_ref()
                .filter(|(cached, _)| *cached == epoch)
                .map(|(_, tokens)| tokens.clone());
            let all = if let Some(tokens) = cached {
                tokens
            } else {
                let result = self.transport.request(
                    "textDocument/semanticTokens/full",
                    json!({"textDocument":{"uri":convert(path_uri(&snapshot.path))?}}),
                    context,
                )?;
                let tokens = if result.is_null() {
                    vec![]
                } else {
                    let data = result
                        .get("data")
                        .ok_or_else(|| protocol("semantic tokens response missing data"))?;
                    if let Some(legend) = self.transport.dynamic_capabilities().1 {
                        convert(semantic_tokens(data, &legend.types, &legend.modifiers))?
                    } else {
                        convert(semantic_tokens(
                            data,
                            &self.token_types,
                            &self.token_modifiers,
                        ))?
                    }
                };
                for token in &tokens {
                    let end = token
                        .start
                        .checked_add(token.length)
                        .ok_or_else(|| protocol("token column overflow"))?;
                    snapshot
                        .index
                        .byte_offset(Position::new(token.line, token.start))
                        .map_err(|error| protocol(error.to_string()))?;
                    snapshot
                        .index
                        .byte_offset(Position::new(token.line, end))
                        .map_err(|error| protocol(error.to_string()))?;
                }
                self.valid_snapshot(snapshot, epoch)?;
                let tokens = Arc::new(tokens);
                let doc = self.documents.get_mut(&snapshot.path).unwrap();
                doc.token_bytes = tokens.iter().fold(0usize, |bytes, token| {
                    bytes
                        .saturating_add(std::mem::size_of::<SemanticToken>() * 2)
                        .saturating_add(token.kind.len())
                        .saturating_add(token.modifiers.iter().map(String::len).sum::<usize>())
                });
                doc.tokens = Some((epoch, tokens.clone()));
                self.bounded_caches(&snapshot.path);
                tokens
            };
            tokens = all
                .iter()
                .filter(|token| {
                    token_intersects(token, range)
                        || headers.iter().chain(&folded).any(|header| {
                            token.line >= header.start_line
                                && (token.line as u64)
                                    < header.start_line as u64 + header.code.lines().count() as u64
                        })
                })
                .cloned()
                .collect();
        }
        self.bounded_caches(&snapshot.path);
        self.valid_snapshot(snapshot, epoch)?;
        Ok(SourceDocument {
            symbol,
            code,
            tokens,
            context: headers,
            code_start: Some(range.start),
            folded,
            expanded: vec![],
        })
    }
}
