/// Mask comments and string tokens while preserving newlines, then count code-bearing lines.
pub(super) fn count_code_lines(source: &str, rust: bool) -> Result<usize, String> {
    let bytes = source.as_bytes();
    let mut code = bytes.to_vec();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let rest = &bytes[index..];
        let excluded_end = if rest.starts_with(b"//") {
            Some(line_comment_end(bytes, index + 2, rust))
        } else if rest.starts_with(b"/*") {
            Some(block_comment_end(bytes, index + 2, rust)?)
        } else if let Some((quote, hashes)) = rust.then(|| rust_raw_start(bytes, index)).flatten() {
            Some(raw_end(bytes, quote + 1, hashes)?)
        } else if let Some((contents, terminator)) =
            (!rust).then(|| cpp_raw_start(bytes, index)).flatten()
        {
            Some(find_terminator(bytes, contents, &terminator)?)
        } else if let Some(quote) = string_start(bytes, index, rust) {
            Some(quoted_end(bytes, quote, b'"')?)
        } else {
            None
        };
        if let Some(end) = excluded_end {
            for byte in &mut code[start..end] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            index = end;
        } else if bytes[index] == b'\'' && !(!rust && cpp_digit_separator(bytes, index)) {
            // Character literals are code; skip their contents so '/' and '"'
            // cannot start comments or strings. Rust lifetimes are not literals.
            index = if rust {
                rust_char_end(bytes, index).unwrap_or(index + 1)
            } else {
                quoted_end(bytes, index, b'\'')?
            };
        } else {
            index += 1;
        }
    }
    Ok(code
        .split(|byte| *byte == b'\n')
        .filter(|line| line.iter().any(|byte| !byte.is_ascii_whitespace()))
        .count())
}

fn cpp_digit_separator(bytes: &[u8], index: usize) -> bool {
    if !bytes.get(index + 1).is_some_and(u8::is_ascii_alphanumeric) {
        return false;
    }
    let mut start = index;
    while start > 0
        && (bytes[start - 1].is_ascii_alphanumeric()
            || matches!(bytes[start - 1], b'_' | b'.' | b'\''))
    {
        start -= 1;
    }
    start < index && bytes[start].is_ascii_digit()
}

fn line_comment_end(bytes: &[u8], mut index: usize, rust: bool) -> usize {
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            let before = if index > 0 && bytes[index - 1] == b'\r' {
                index - 1
            } else {
                index
            };
            if rust || before == 0 || bytes[before - 1] != b'\\' {
                break;
            }
        }
        index += 1;
    }
    index
}

fn block_comment_end(bytes: &[u8], mut index: usize, rust: bool) -> Result<usize, String> {
    let mut depth = 1;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"*/") {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return Ok(index);
            }
        } else if rust && bytes[index..].starts_with(b"/*") {
            depth += 1;
            index += 2;
        } else {
            index += 1;
        }
    }
    Err("unterminated block comment in line-count check".into())
}

fn token_boundary(bytes: &[u8], index: usize) -> bool {
    index == 0
        || !(bytes[index - 1].is_ascii_alphanumeric()
            || bytes[index - 1] == b'_'
            || bytes[index - 1] >= 128)
}

fn rust_raw_start(bytes: &[u8], index: usize) -> Option<(usize, usize)> {
    if !token_boundary(bytes, index) {
        return None;
    }
    let mut quote = index;
    if matches!(bytes[quote], b'b' | b'c') {
        quote += 1;
    }
    if bytes.get(quote) != Some(&b'r') {
        return None;
    }
    quote += 1;
    let hashes_start = quote;
    while bytes.get(quote) == Some(&b'#') {
        quote += 1;
    }
    (bytes.get(quote) == Some(&b'"')).then_some((quote, quote - hashes_start))
}

fn raw_end(bytes: &[u8], mut index: usize, hashes: usize) -> Result<usize, String> {
    while index < bytes.len() {
        if bytes[index] == b'"'
            && bytes
                .get(index + 1..index + 1 + hashes)
                .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
        {
            return Ok(index + 1 + hashes);
        }
        index += 1;
    }
    Err("unterminated raw string in line-count check".into())
}

fn cpp_raw_start(bytes: &[u8], index: usize) -> Option<(usize, Vec<u8>)> {
    if !token_boundary(bytes, index) {
        return None;
    }
    for prefix in [b"R\"".as_slice(), b"u8R\"", b"uR\"", b"UR\"", b"LR\""] {
        if !bytes[index..].starts_with(prefix) {
            continue;
        }
        let delimiter_start = index + prefix.len();
        let mut end = delimiter_start;
        while let Some(byte) = bytes.get(end) {
            if *byte == b'(' {
                let mut terminator = vec![b')'];
                terminator.extend_from_slice(&bytes[delimiter_start..end]);
                terminator.push(b'"');
                return Some((end + 1, terminator));
            }
            if end - delimiter_start == 16
                || byte.is_ascii_whitespace()
                || matches!(byte, b')' | b'\\')
            {
                break;
            }
            end += 1;
        }
    }
    None
}

fn find_terminator(bytes: &[u8], start: usize, terminator: &[u8]) -> Result<usize, String> {
    bytes[start..]
        .windows(terminator.len())
        .position(|window| window == terminator)
        .map(|offset| start + offset + terminator.len())
        .ok_or_else(|| "unterminated raw string in line-count check".into())
}

fn string_start(bytes: &[u8], index: usize, rust: bool) -> Option<usize> {
    if bytes[index] == b'"' {
        return Some(index);
    }
    if !token_boundary(bytes, index) {
        return None;
    }
    let prefixes: &[&[u8]] = if rust {
        &[b"b\"", b"c\""]
    } else {
        &[b"u8\"", b"u\"", b"U\"", b"L\""]
    };
    prefixes
        .iter()
        .find(|prefix| bytes[index..].starts_with(prefix))
        .map(|prefix| index + prefix.len() - 1)
}

fn quoted_end(bytes: &[u8], quote: usize, delimiter: u8) -> Result<usize, String> {
    let mut index = quote + 1;
    while index < bytes.len() {
        if bytes[index] == delimiter {
            return Ok(index + 1);
        }
        index += if bytes[index] == b'\\' { 2 } else { 1 };
    }
    Err("unterminated literal in line-count check".into())
}

fn rust_char_end(bytes: &[u8], quote: usize) -> Option<usize> {
    let mut index = quote + 1;
    if bytes.get(index) == Some(&b'\\') {
        index += 1;
        match bytes.get(index)? {
            b'x' => index += 3,
            b'u' if bytes.get(index + 1) == Some(&b'{') => {
                index += 2;
                while bytes.get(index).is_some_and(|byte| *byte != b'}') {
                    index += 1;
                }
                index += 1;
            }
            _ => index += 1,
        }
    } else {
        let character = std::str::from_utf8(bytes.get(index..)?)
            .ok()?
            .chars()
            .next()?;
        if matches!(character, '\'' | '\n' | '\r') {
            return None;
        }
        index += character.len_utf8();
    }
    (bytes.get(index) == Some(&b'\'')).then_some(index + 1)
}

#[cfg(test)]
mod tests;
