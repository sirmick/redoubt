//! The trace text's lexical layer: a line's tokens, and the values a field holds
//! (servers/steward.md, "The trace encoding").

/// The tokens of a line, split at spaces outside a quoted string. `#` outside a string starts a
/// comment.
pub fn tokens(line: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let (mut start, mut quoted, mut escaped) = (None, false, false);
    for (i, c) in line.char_indices() {
        if quoted {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => {}
            }
            continue;
        }
        match c {
            ' ' => {
                if let Some(s) = start.take() {
                    out.push(&line[s..i]);
                }
            }
            '#' if start.is_none() => break,
            _ => {
                start.get_or_insert(i);
                quoted = c == '"';
            }
        }
    }
    if quoted {
        return Err(format!("unclosed string in `{line}`"));
    }
    if let Some(s) = start {
        out.push(&line[s..]);
    }
    Ok(out)
}

/// A token `key=value`.
pub fn field<'a>(token: &'a str, key: &str) -> Option<&'a str> { token.strip_prefix(key)?.strip_prefix('=') }

pub fn u64_of(s: &str) -> Result<u64, String> { s.parse().map_err(|_| format!("`{s}` is not a number")) }

/// `[1,2,3]`, or `[]`.
pub fn list(s: &str) -> Result<Vec<u64>, String> {
    let inner =
        s.strip_prefix('[').and_then(|s| s.strip_suffix(']')).ok_or(format!("`{s}` is not a list"))?;
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    inner.split(',').map(u64_of).collect()
}

/// `[[],[7],[7,9]]`.
pub fn lists(s: &str) -> Result<Vec<Vec<u64>>, String> {
    let inner = s
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or(format!("`{s}` is not a list of lists"))?;
    let mut out = Vec::new();
    let mut rest = inner;
    while !rest.is_empty() {
        let end = rest.find(']').ok_or(format!("`{s}` is not a list of lists"))?;
        out.push(list(&rest[..=end])?);
        rest = rest[end + 1..].strip_prefix(',').unwrap_or(&rest[end + 1..]);
    }
    Ok(out)
}

/// `a,b,c`: a budget's pages, processes and weight.
pub fn triple(s: &str) -> Result<[u64; 3], String> {
    let v: Vec<u64> = s.split(',').map(u64_of).collect::<Result<_, _>>()?;
    v.try_into().map_err(|_| format!("`{s}` is not pages,processes,weight"))
}

/// A quoted string's bytes: `\"`, `\\`, `\n` and `\xNN` are the escapes.
pub fn bytes(s: &str) -> Result<Vec<u8>, String> {
    let inner =
        s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).ok_or(format!("`{s}` is not a string"))?;
    let mut out = Vec::new();
    let mut it = inner.bytes();
    while let Some(b) = it.next() {
        if b != b'\\' {
            out.push(b);
            continue;
        }
        match it.next() {
            Some(b'n') => out.push(b'\n'),
            Some(b'x') => {
                let hex: Vec<u8> = it.by_ref().take(2).collect();
                let hex = std::str::from_utf8(&hex).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                out.push(hex.ok_or(format!("bad \\x escape in {s}"))?);
            }
            Some(c @ (b'"' | b'\\')) => out.push(c),
            _ => return Err(format!("bad escape in {s}")),
        }
    }
    Ok(out)
}

/// A quoted string that is UTF-8.
pub fn string(s: &str) -> Result<String, String> {
    String::from_utf8(bytes(s)?).map_err(|_| format!("{s} is not UTF-8"))
}

/// Bytes as a quoted string: printable ASCII as itself, `"` and `\` escaped, a newline as `\n`,
/// and every other byte as `\xNN`.
pub fn quote(b: &[u8]) -> String {
    let mut s = String::from("\"");
    for &c in b {
        match c {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            0x20..=0x7e => s.push(c as char),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s.push('"');
    s
}

pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

/// 64 hex digits.
pub fn hash(s: &str) -> Result<[u8; 32], String> {
    let bad = || format!("`{s}` is not 64 hex digits");
    if s.len() != 64 {
        return Err(bad());
    }
    let mut h = [0; 32];
    for (i, x) in h.iter_mut().enumerate() {
        *x = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).map_err(|_| bad())?;
    }
    Ok(h)
}

pub fn show_list(l: &[u64]) -> String {
    let items: Vec<String> = l.iter().map(u64::to_string).collect();
    format!("[{}]", items.join(","))
}
