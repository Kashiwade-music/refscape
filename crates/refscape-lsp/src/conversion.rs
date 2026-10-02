use refscape_model::{ErrorKind, Position, RefscapeError, SemanticToken, SourceRange, Symbol};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use std::path::{Path, PathBuf};

pub(crate) fn hover_contents(value: &Value) -> Result<Option<String>, String> {
    if value.is_null() {
        return Ok(None);
    }
    fn content(value: &Value) -> Result<String, String> {
        if let Some(text) = value.as_str() {
            return Ok(text.into());
        }
        if let Some(values) = value.as_array() {
            return values
                .iter()
                .map(content)
                .collect::<Result<Vec<_>, _>>()
                .map(|parts| parts.join("\n\n"));
        }
        value
            .get("value")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| "Invalid hover contents from language server".into())
    }
    let text = content(&value["contents"])?;
    Ok((!text.trim().is_empty()).then_some(text))
}

#[derive(serde::Deserialize)]
struct WirePosition {
    line: u32,
    character: u32,
}
#[derive(serde::Deserialize)]
struct WireRange {
    start: WirePosition,
    end: WirePosition,
}
pub(crate) trait WireValue: Sized {
    fn from_wire(value: &Value) -> Result<Self, String>;
}
impl WireValue for SourceRange {
    fn from_wire(value: &Value) -> Result<Self, String> {
        let raw: WireRange =
            serde_json::from_value(value.clone()).map_err(|e| format!("invalid LSP range: {e}"))?;
        let range = SourceRange {
            start: Position::new(raw.start.line, raw.start.character),
            end: Position::new(raw.end.line, raw.end.character),
        };
        range.validate()?;
        Ok(range)
    }
}
impl WireValue for Vec<u32> {
    fn from_wire(value: &Value) -> Result<Self, String> {
        serde_json::from_value(value.clone()).map_err(|e| format!("invalid LSP token data: {e}"))
    }
}
pub(crate) fn decode<T: WireValue>(value: &Value) -> Result<T, String> {
    T::from_wire(value)
}
pub(crate) fn position_wire(position: Position) -> Value {
    serde_json::json!({"line":position.line,"character":position.character})
}

pub(crate) fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("LSP response is missing {key}"))
}

pub(crate) fn document_symbol(value: &Value, path: &Path) -> Result<Symbol, String> {
    let range: SourceRange = decode(value.get("range").unwrap_or(&value["location"]["range"]))?;
    let selection_range = value
        .get("selectionRange")
        .map(decode)
        .transpose()?
        .unwrap_or(range);
    let kind = match value["kind"].as_u64().ok_or("LSP symbol is missing kind")? {
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
    let children = match value.get("children") {
        None => vec![],
        Some(children) => children
            .as_array()
            .ok_or("invalid LSP symbol children")?
            .iter()
            .map(|child| document_symbol(child, path))
            .collect::<Result<_, _>>()?,
    };
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

pub(crate) fn symbol_id(path: &Path, range: SourceRange) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        path.display(),
        range.start.line,
        range.start.character,
        range.end.line,
        range.end.character
    )
}

pub(crate) fn before(left: Position, right: Position) -> bool {
    (left.line, left.character) < (right.line, right.character)
}

#[cfg(test)]
pub(crate) fn enclosing(symbols: &[Symbol], position: Position) -> Option<&Symbol> {
    symbols
        .iter()
        .filter(|symbol| {
            !before(position, symbol.range.start) && before(position, symbol.range.end)
        })
        .find_map(|symbol| enclosing(&symbol.children, position).or(Some(symbol)))
}

pub(crate) fn deduplicate(symbols: &mut Vec<Symbol>) {
    let mut seen = std::collections::BTreeSet::new();
    symbols.retain(|symbol| seen.insert(symbol.id.clone()));
}

pub(crate) fn token_intersects(token: &SemanticToken, range: SourceRange) -> bool {
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

pub(crate) fn semantic_tokens(
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
        if item[2] == 0 {
            return Err("semantic token length must be positive".into());
        }
        if modifiers.len() < 32 && (item[4] >> modifiers.len()) != 0 {
            return Err("invalid semantic token modifier index".into());
        }
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
        start
            .checked_add(item[2])
            .ok_or("semantic token column overflow")?;
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
pub fn byte_offset(text: &str, position: Position) -> Result<usize, RefscapeError> {
    refscape_model::TextIndex::new(text).and_then(|index| index.byte_offset(position))
}

#[cfg(test)]
pub(crate) fn slice(text: &str, range: SourceRange) -> Result<&str, String> {
    let start = byte_offset(text, range.start).map_err(|error| error.to_string())?;
    let end = byte_offset(text, range.end).map_err(|error| error.to_string())?;
    text.get(start..end)
        .ok_or_else(|| "invalid source range".into())
}

/// The complete range of an unmodified UTF-8 source file, measured in LSP UTF-16 units.
pub fn full_range(text: &str) -> Result<SourceRange, RefscapeError> {
    let line = u32::try_from(text.bytes().filter(|byte| *byte == b'\n').count())
        .map_err(|_| RefscapeError::new(ErrorKind::InvalidData, "source line overflow"))?;
    let last = text
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .trim_end_matches('\r');
    Ok(SourceRange {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line,
            character: u32::try_from(last.encode_utf16().count()).map_err(|_| {
                RefscapeError::new(ErrorKind::InvalidData, "source column overflow")
            })?,
        },
    })
}

pub(crate) fn path_uri(path: &Path) -> Result<String, String> {
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

pub(crate) fn uri_path(uri: &str) -> Result<PathBuf, String> {
    let encoded = uri
        .strip_prefix("file://")
        .ok_or_else(|| format!("unsupported source URI: {uri}"))?;
    if encoded.is_empty()
        || encoded
            .bytes()
            .any(|byte| byte <= 0x20 || matches!(byte, b'?' | b'#'))
    {
        return Err("invalid file URI".into());
    }
    let local;
    let encoded = if let Some((authority, path)) = encoded.split_once('/')
        && authority.eq_ignore_ascii_case("localhost")
    {
        local = format!("/{path}");
        local.as_str()
    } else {
        encoded
    };
    if !encoded.starts_with('/')
        && encoded
            .split('/')
            .next()
            .is_none_or(|authority| authority.is_empty() || authority.contains([':', '@']))
    {
        return Err("invalid file URI authority".into());
    }
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
    if text.contains('\0') {
        return Err("NUL in file URI".into());
    }
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
    fn hover_decodes_markup_and_legacy_contents_and_empty_results() {
        for contents in [
            json!({"kind":"plaintext", "value":"fn answer() -> u32\n\nReturns the answer."}),
            json!([{"language":"rust", "value":"fn answer() -> u32"}, "Returns the answer."]),
            json!("fn answer() -> u32\n\nReturns the answer."),
        ] {
            assert_eq!(
                hover_contents(&json!({"contents":contents}))
                    .unwrap()
                    .as_deref(),
                Some("fn answer() -> u32\n\nReturns the answer.")
            );
        }
        assert_eq!(hover_contents(&Value::Null).unwrap(), None);
        assert_eq!(hover_contents(&json!({"contents":[]})).unwrap(), None);
        assert_eq!(
            hover_contents(&json!({"contents":{"kind":"plaintext","value":" "}})).unwrap(),
            None
        );
        assert!(hover_contents(&json!({"contents":42})).is_err());
    }

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
        assert_eq!(slice(text, full_range(text).unwrap()).unwrap(), text);
    }

    #[test]
    fn source_paths_roundtrip_reserved_and_unicode_characters() {
        #[cfg(windows)]
        let path = Path::new("C:/日本語/a #%.rs");
        #[cfg(not(windows))]
        let path = Path::new("/日本語/a #%.rs");
        assert_eq!(uri_path(&path_uri(path).unwrap()).unwrap(), path);
        assert!(uri_path("file:///bad%GG").is_err());
        for uri in [
            "file://",
            "file:///bad%00",
            "file:///name#fragment",
            "untitled:///virtual",
            "file://user@host/share",
        ] {
            assert!(uri_path(uri).is_err());
        }
        #[cfg(windows)]
        {
            assert_eq!(
                uri_path("file://localhost/C:/source.rs").unwrap(),
                Path::new("C:/source.rs")
            );
            assert_eq!(
                uri_path("file://server/share/source.rs").unwrap(),
                Path::new("//server/share/source.rs")
            );
            assert_eq!(
                path_uri(Path::new(r"\\?\C:\source.rs")).unwrap(),
                "file:///C:/source.rs"
            );
            assert_eq!(
                path_uri(Path::new(r"\\?\UNC\server\share\source.rs")).unwrap(),
                "file://server/share/source.rs"
            );
        }
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
