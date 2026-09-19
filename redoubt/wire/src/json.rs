//! Strict JSON for files people write (WIRE.md): RFC 8259 syntax under the I-JSON profile
//! (RFC 7493), one parser for the boot manifest, package manifests and configuration.
//!
//! WIRE.md's rules, as enforced here:
//! - UTF-8 only;
//! - no object has two members with the same name. Names are compared byte for byte after
//!   unescaping (so `"a"` and `"\u0061"` are the same name), with no Unicode normalisation;
//! - each field's JSON type is fixed by the schema: a 64-bit quantity is a decimal string,
//!   read with [`Value::u64_string`] (up to `u64::MAX`), and a small count is a number, read
//!   with [`Value::int`]; the wrong JSON type is a `WrongType` error. A number must lie in
//!   `-(2^53 - 1)..=2^53 - 1` ([`MAX_SAFE_INT`], I-JSON's safe range; 2^53 itself is refused);
//! - nesting at most [`MAX_DEPTH`] (32) deep, counting the top-level container as depth 1;
//! - a file at most [`MAX_LEN`] (64 KiB);
//! - unknown members are errors: objects are decoded only through [`Value::object`], which
//!   refuses any member the decoder did not take.
//!
//! Stricter than WIRE.md states (each is also I-JSON's advice or removes a second spelling):
//! - no byte-order mark;
//! - no surrogates (so no unpaired `\uD800` escape) and no Unicode noncharacters
//!   (U+FDD0..U+FDEF, U+xFFFE, U+xFFFF), escaped or raw (RFC 7493 section 2.1);
//! - numbers are integers: fractions, exponents and `-0` are refused, so the parser has no
//!   floating point and 0 has one spelling;
//! - a decimal string is canonical: digits only, no sign, spaces or leading zeros.
//!
//! Errors from typed decoding ([`SchemaError`]) name where they happened, e.g.
//! `servers[2].budget.pages`.
//!
//! Cost, for a caller that must size memory before parsing (`init` parses the boot manifest
//! before anything else runs; tests/json_limits.rs holds these bounds):
//! - time is linear in the input: every loop consumes input, plus a sort per object to find
//!   duplicate names in O(n log n);
//! - heap: at most 32 bytes per input byte, so 2 MiB for a 64 KiB file. The worst case is a
//!   flat array of one-byte values (`[0,0,...]`): each 2 input bytes become a 32-byte
//!   [`Value`], and growing the `Vec` briefly holds the old and new buffers (measured peak
//!   24x, 1.5 MiB);
//! - stack: the parser recurses once per container level, bounded by `MAX_DEPTH`. The
//!   deepest file needs about 56 KiB unoptimised (roughly 1.7 KiB per level) and under
//!   16 KiB optimised. Input nested deeper fails with `TooDeep` at level 33, before using
//!   more.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

/// Largest file accepted.
pub const MAX_LEN: usize = 64 * 1024;
/// Deepest nesting accepted.
pub const MAX_DEPTH: usize = 32;
/// Largest integer magnitude a JSON number may have: 2^53 - 1.
pub const MAX_SAFE_INT: i64 = (1 << 53) - 1;

/// A parsed JSON value. Strings borrow from the input unless they contained escapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value<'a> {
    Null,
    Bool(bool),
    /// Within `-MAX_SAFE_INT..=MAX_SAFE_INT`.
    Int(i64),
    Str(Cow<'a, str>),
    Array(Vec<Value<'a>>),
    /// Members in file order; names are unique.
    Object(Vec<(Cow<'a, str>, Value<'a>)>),
}

/// Why a file was refused, and the byte offset where it was noticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Longer than [`MAX_LEN`].
    TooLong,
    /// Not UTF-8, or starts with a byte-order mark.
    BadUtf8,
    /// Not JSON: an unexpected byte or the end of input.
    Syntax,
    /// A raw control character in a string.
    ControlCharacter,
    /// A malformed `\` escape.
    BadEscape,
    /// A surrogate or noncharacter code point.
    BadCodePoint,
    /// A number with a fraction or exponent, a leading zero, no digits, or `-0`.
    NotInteger,
    /// An integer beyond [`MAX_SAFE_INT`] in magnitude.
    OutOfRange,
    /// Nested deeper than [`MAX_DEPTH`].
    TooDeep,
    /// Two members of one object with the same name.
    DuplicateMember,
    /// Bytes after the value.
    Trailing,
}

/// Parses one strict JSON document.
pub fn parse(input: &[u8]) -> Result<Value<'_>, Error> {
    if input.len() > MAX_LEN {
        return Err(Error { offset: MAX_LEN, kind: ErrorKind::TooLong });
    }
    let text = core::str::from_utf8(input)
        .map_err(|e| Error { offset: e.valid_up_to(), kind: ErrorKind::BadUtf8 })?;
    if text.starts_with('\u{feff}') {
        return Err(Error { offset: 0, kind: ErrorKind::BadUtf8 });
    }
    let mut p = Parser { text, pos: 0 };
    p.skip_ws();
    let value = p.value(0)?;
    p.skip_ws();
    if p.pos != text.len() {
        return Err(p.err(ErrorKind::Trailing));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, kind: ErrorKind) -> Error {
        Error { offset: self.pos, kind }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn next(&mut self) -> Result<u8, Error> {
        let b = self.peek().ok_or(self.err(ErrorKind::Syntax))?;
        self.pos += 1;
        Ok(b)
    }

    fn expect(&mut self, b: u8) -> Result<(), Error> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.err(ErrorKind::Syntax))
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// `depth` is the number of containers around this value.
    fn value(&mut self, depth: usize) -> Result<Value<'a>, Error> {
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            _ => Err(self.err(ErrorKind::Syntax)),
        }
    }

    fn literal(&mut self, word: &str, value: Value<'a>) -> Result<Value<'a>, Error> {
        if self.text.get(self.pos..).is_some_and(|rest| rest.starts_with(word)) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.err(ErrorKind::Syntax))
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value<'a>, Error> {
        if depth > MAX_DEPTH {
            return Err(self.err(ErrorKind::TooDeep));
        }
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth)?);
            self.skip_ws();
            match self.next()? {
                b',' => continue,
                b']' => return Ok(Value::Array(items)),
                _ => return Err(Error { offset: self.pos - 1, kind: ErrorKind::Syntax }),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value<'a>, Error> {
        if depth > MAX_DEPTH {
            return Err(self.err(ErrorKind::TooDeep));
        }
        let start = self.pos;
        self.expect(b'{')?;
        let mut members = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err(ErrorKind::Syntax));
            }
            let name = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let value = self.value(depth)?;
            members.push((name, value));
            self.skip_ws();
            match self.next()? {
                b',' => continue,
                b'}' => break,
                _ => return Err(Error { offset: self.pos - 1, kind: ErrorKind::Syntax }),
            }
        }
        // Sorting the names finds duplicates in O(n log n); a pairwise check would let a
        // 64 KiB object of short names cost ~10^8 comparisons.
        let mut names: Vec<&str> = members.iter().map(|(n, _)| n.as_ref()).collect();
        names.sort_unstable();
        if names.windows(2).any(|w| w.first() == w.get(1)) {
            return Err(Error { offset: start, kind: ErrorKind::DuplicateMember });
        }
        Ok(Value::Object(members))
    }

    /// A string starting at the opening quote. Borrowed if it has no escapes.
    fn string(&mut self) -> Result<Cow<'a, str>, Error> {
        self.expect(b'"')?;
        let mut owned: Option<String> = None;
        let mut run = self.pos; // start of the unescaped run not yet copied into `owned`
        loop {
            let at = self.pos;
            match self.next()? {
                b'"' => {
                    let tail = self.slice(run, at)?;
                    let s = match owned {
                        None => Cow::Borrowed(tail),
                        Some(mut s) => {
                            s.push_str(tail);
                            Cow::Owned(s)
                        }
                    };
                    return Ok(s);
                }
                b'\\' => {
                    let s = owned.get_or_insert_with(String::new);
                    s.push_str(self.slice(run, at)?);
                    let c = self.escape()?;
                    s.push(c);
                    run = self.pos;
                }
                0x00..=0x1f => return Err(Error { offset: at, kind: ErrorKind::ControlCharacter }),
                b if b < 0x80 => {}
                _ => {
                    // A multi-byte character: the input is UTF-8, so decode it from the
                    // text to check for noncharacters and step over its continuation bytes.
                    let c = self.text.get(at..).and_then(|s| s.chars().next());
                    let c = c.ok_or(Error { offset: at, kind: ErrorKind::BadUtf8 })?;
                    if is_noncharacter(c) {
                        return Err(Error { offset: at, kind: ErrorKind::BadCodePoint });
                    }
                    self.pos = at + c.len_utf8();
                }
            }
        }
    }

    fn slice(&self, from: usize, to: usize) -> Result<&'a str, Error> {
        // Both ends are at ASCII bytes, so on character boundaries; `get` checks anyway.
        self.text.get(from..to).ok_or(Error { offset: from, kind: ErrorKind::BadUtf8 })
    }

    /// The character an escape after `\` stands for.
    fn escape(&mut self) -> Result<char, Error> {
        let at = self.pos;
        let c = match self.next()? {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let unit = self.hex4()?;
                let code = if (0xd800..0xdc00).contains(&unit) {
                    // A high surrogate must be followed by an escaped low surrogate.
                    if self.next()? != b'\\' || self.next()? != b'u' {
                        return Err(Error { offset: at, kind: ErrorKind::BadCodePoint });
                    }
                    let low = self.hex4()?;
                    if !(0xdc00..0xe000).contains(&low) {
                        return Err(Error { offset: at, kind: ErrorKind::BadCodePoint });
                    }
                    0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00)
                } else {
                    unit
                };
                // char::from_u32 refuses lone surrogates (0xdc00..0xe000 here).
                let c = char::from_u32(code).ok_or(Error { offset: at, kind: ErrorKind::BadCodePoint })?;
                if is_noncharacter(c) {
                    return Err(Error { offset: at, kind: ErrorKind::BadCodePoint });
                }
                c
            }
            _ => return Err(Error { offset: at, kind: ErrorKind::BadEscape }),
        };
        Ok(c)
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let mut v = 0;
        for _ in 0..4 {
            let at = self.pos;
            let d = char::from(self.next()?).to_digit(16);
            v = v * 16 + d.ok_or(Error { offset: at, kind: ErrorKind::BadEscape })?;
        }
        Ok(v)
    }

    fn number(&mut self) -> Result<Value<'a>, Error> {
        let start = self.pos;
        let negative = self.peek() == Some(b'-');
        if negative {
            self.pos += 1;
        }
        let digits = self.pos;
        let mut magnitude: i64 = 0;
        while let Some(d @ b'0'..=b'9') = self.peek() {
            self.pos += 1;
            magnitude = magnitude
                .checked_mul(10)
                .and_then(|m| m.checked_add(i64::from(d - b'0')))
                .filter(|m| *m <= MAX_SAFE_INT)
                .ok_or(Error { offset: start, kind: ErrorKind::OutOfRange })?;
        }
        let count = self.pos - digits;
        let leading_zero = count > 1 && self.text.as_bytes().get(digits) == Some(&b'0');
        // `-0` is JSON, but a second spelling of 0 (and a float to most parsers): refused.
        let negative_zero = negative && magnitude == 0;
        let fraction = matches!(self.peek(), Some(b'.' | b'e' | b'E'));
        if count == 0 || leading_zero || negative_zero || fraction {
            return Err(Error { offset: start, kind: ErrorKind::NotInteger });
        }
        Ok(Value::Int(if negative { -magnitude } else { magnitude }))
    }
}

/// Unicode noncharacters: U+FDD0..=U+FDEF and the last two code points of every plane.
fn is_noncharacter(c: char) -> bool {
    let c = u32::from(c);
    (0xfdd0..=0xfdef).contains(&c) || c & 0xfffe == 0xfffe
}

/// Why a parsed value does not fit the structure a decoder expects, and where: `path` names
/// the value from the top, e.g. `servers[2].budget.pages` (empty for the top-level value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    pub path: String,
    pub kind: SchemaKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaKind {
    /// A required member is absent (`path` ends with its name).
    Missing,
    /// A member the decoder does not know (WIRE.md: unknown members are errors).
    Unknown,
    /// A value of the wrong type, or an integer out of range.
    WrongType,
}

impl SchemaError {
    fn new(kind: SchemaKind) -> Self {
        SchemaError { path: String::new(), kind }
    }

    /// Prefixes the path with the member or index the error happened inside.
    fn inside(mut self, segment: &str) -> Self {
        let sep = if self.path.is_empty() || self.path.starts_with('[') { "" } else { "." };
        self.path = alloc::format!("{segment}{sep}{}", self.path);
        self
    }
}

/// Typed decoding, for turning a parsed file into a structure. Every accessor fails with
/// `WrongType` on a value of another type; objects are read only through [`Value::object`],
/// which refuses members the decoder did not take, so an unknown member cannot be ignored
/// by forgetting a check.
impl<'a> Value<'a> {
    pub fn str(&self) -> Result<&str, SchemaError> {
        match self {
            Value::Str(s) => Ok(s),
            _ => Err(SchemaError::new(SchemaKind::WrongType)),
        }
    }

    pub fn bool(&self) -> Result<bool, SchemaError> {
        match self {
            Value::Bool(b) => Ok(*b),
            _ => Err(SchemaError::new(SchemaKind::WrongType)),
        }
    }

    /// A small count: a JSON number (within `MAX_SAFE_INT`, which the parser enforced).
    pub fn int(&self) -> Result<i64, SchemaError> {
        match self {
            Value::Int(n) => Ok(*n),
            _ => Err(SchemaError::new(SchemaKind::WrongType)),
        }
    }

    /// A 64-bit quantity: a decimal string of digits only, with no sign, spaces or leading
    /// zeros, up to `u64::MAX`. A JSON number is the wrong type.
    pub fn u64_string(&self) -> Result<u64, SchemaError> {
        let Value::Str(s) = self else { return Err(SchemaError::new(SchemaKind::WrongType)) };
        let canonical = s == "0" || (!s.starts_with('0') && !s.is_empty());
        // Digits only, so the only failure left for `parse` is overflow.
        let n = (canonical && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten();
        n.ok_or(SchemaError::new(SchemaKind::WrongType))
    }

    /// Decodes each item of an array with `decode`.
    pub fn items<T>(&self, mut decode: impl FnMut(&Value<'a>) -> Result<T, SchemaError>) -> Result<Vec<T>, SchemaError> {
        let Value::Array(items) = self else { return Err(SchemaError::new(SchemaKind::WrongType)) };
        let mut out = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            out.push(decode(item).map_err(|e| e.inside(&alloc::format!("[{i}]")))?);
        }
        Ok(out)
    }

    /// Decodes an object: `decode` takes the members it knows from [`Members`]; afterwards
    /// any member it did not take is an `Unknown` error.
    pub fn object<'v, T>(&'v self, decode: impl FnOnce(&mut Members<'v, 'a>) -> Result<T, SchemaError>) -> Result<T, SchemaError> {
        let Value::Object(members) = self else { return Err(SchemaError::new(SchemaKind::WrongType)) };
        let mut m = Members { members, taken: alloc::vec![false; members.len()] };
        let value = decode(&mut m)?;
        match m.members.iter().zip(&m.taken).find(|(_, taken)| !**taken) {
            Some(((name, _), _)) => Err(SchemaError::new(SchemaKind::Unknown).inside(name)),
            None => Ok(value),
        }
    }
}

/// The members of an object being decoded by [`Value::object`].
#[derive(Debug)]
pub struct Members<'v, 'a> {
    members: &'v [(Cow<'a, str>, Value<'a>)],
    taken: Vec<bool>,
}

impl<'v, 'a> Members<'v, 'a> {
    /// Decodes the member `name` with `decode`, if present.
    pub fn optional<T>(&mut self, name: &str, decode: impl FnOnce(&'v Value<'a>) -> Result<T, SchemaError>) -> Result<Option<T>, SchemaError> {
        let members = self.members;
        let Some((i, (_, value))) = members.iter().enumerate().find(|(_, (n, _))| n == name) else { return Ok(None) };
        if let Some(t) = self.taken.get_mut(i) {
            *t = true;
        }
        decode(value).map(Some).map_err(|e| e.inside(name))
    }

    /// Decodes the member `name`, which must be present, with `decode`.
    pub fn required<T>(&mut self, name: &str, decode: impl FnOnce(&'v Value<'a>) -> Result<T, SchemaError>) -> Result<T, SchemaError> {
        self.optional(name, decode)?.ok_or_else(|| SchemaError::new(SchemaKind::Missing).inside(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn kind(input: &str) -> ErrorKind {
        parse(input.as_bytes()).unwrap_err().kind
    }

    #[test]
    fn accepts_json() {
        let v = parse(r#" { "a": [1, -2, true, false, null, "x\n\u00e9\ud83d\ude00"], "b": {} } "#.as_bytes()).unwrap();
        assert_eq!(
            v,
            Value::Object(vec![
                (
                    "a".into(),
                    Value::Array(vec![
                        Value::Int(1),
                        Value::Int(-2),
                        Value::Bool(true),
                        Value::Bool(false),
                        Value::Null,
                        Value::Str("x\n\u{e9}\u{1f600}".into()),
                    ])
                ),
                ("b".into(), Value::Object(vec![])),
            ])
        );
        assert_eq!(parse(b"0").unwrap(), Value::Int(0));
        // Found by the json fuzz target (serde_json reads it as the float -0.0): one spelling of 0.
        assert_eq!(kind("-0"), ErrorKind::NotInteger);
        assert_eq!(kind("[1, -0]"), ErrorKind::NotInteger);
        assert_eq!(parse("\"\u{e9}\"".as_bytes()).unwrap(), Value::Str(Cow::Borrowed("\u{e9}")));
    }

    #[test]
    fn strings_borrow_unless_escaped() {
        assert!(matches!(parse(br#""plain""#).unwrap(), Value::Str(Cow::Borrowed("plain"))));
        assert!(matches!(parse(br#""a\/b""#).unwrap(), Value::Str(Cow::Owned(s)) if s == "a/b"));
    }

    #[test]
    fn integers_only_within_2_to_53() {
        assert_eq!(parse(b"9007199254740991").unwrap(), Value::Int(MAX_SAFE_INT));
        assert_eq!(parse(b"-9007199254740991").unwrap(), Value::Int(-MAX_SAFE_INT));
        assert_eq!(kind("9007199254740992"), ErrorKind::OutOfRange);
        assert_eq!(kind("99999999999999999999999999"), ErrorKind::OutOfRange);
        assert_eq!(kind("1.0"), ErrorKind::NotInteger);
        assert_eq!(kind("1e3"), ErrorKind::NotInteger);
        assert_eq!(kind("01"), ErrorKind::NotInteger);
        assert_eq!(kind("-"), ErrorKind::NotInteger);
        assert_eq!(kind("+1"), ErrorKind::Syntax);
    }

    #[test]
    fn big_integers_as_strings() {
        let v = parse(br#"["18446744073709551615", "0", 7, "007", "-1", " 1", "18446744073709551616", -1, true, "7"]"#).unwrap();
        let Value::Array(items) = &v else { panic!() };
        let strings: Vec<Option<u64>> = items.iter().map(|v| v.u64_string().ok()).collect();
        assert_eq!(strings, [Some(u64::MAX), Some(0), None, None, None, None, None, None, None, Some(7)]);
        // Each type has one reader: a number is not a string and a string is not a number.
        let numbers: Vec<Option<i64>> = items.iter().map(|v| v.int().ok()).collect();
        assert_eq!(numbers, [None, None, Some(7), None, None, None, None, Some(-1), None, None]);
    }

    #[test]
    fn duplicate_members_are_refused() {
        assert_eq!(kind(r#"{"a":1,"b":2,"a":3}"#), ErrorKind::DuplicateMember);
        // Compared after unescaping.
        assert_eq!(kind(r#"{"a":1,"\u0061":2}"#), ErrorKind::DuplicateMember);
        assert!(parse(br#"[{"a":1},{"a":2}]"#).is_ok());
    }

    #[test]
    fn depth_is_bounded() {
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(ok.as_bytes()).is_ok());
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert_eq!(kind(&deep), ErrorKind::TooDeep);
        let deep_obj = r#"{"a":"#.repeat(MAX_DEPTH + 1) + "1" + &"}".repeat(MAX_DEPTH + 1);
        assert_eq!(kind(&deep_obj), ErrorKind::TooDeep);
        // Far deeper input fails the same way, without exhausting the stack.
        assert_eq!(kind(&"[".repeat(60_000)), ErrorKind::TooDeep);
    }

    #[test]
    fn size_is_bounded() {
        let big = alloc::format!("\"{}\"", "a".repeat(MAX_LEN - 2));
        assert!(parse(big.as_bytes()).is_ok());
        let too_big = alloc::format!("\"{}\"", "a".repeat(MAX_LEN - 1));
        assert_eq!(kind(&too_big), ErrorKind::TooLong);
    }

    #[test]
    fn text_rules() {
        assert_eq!(parse(b"\"\xff\"").unwrap_err().kind, ErrorKind::BadUtf8);
        assert_eq!(kind("\u{feff}{}"), ErrorKind::BadUtf8);
        assert_eq!(kind("\"a\u{1}\""), ErrorKind::ControlCharacter);
        assert_eq!(kind("\"a\tb\""), ErrorKind::ControlCharacter);
        assert_eq!(kind(r#""\ud800""#), ErrorKind::BadCodePoint);
        assert_eq!(kind(r#""\ud800\u0041""#), ErrorKind::BadCodePoint);
        assert_eq!(kind(r#""\udc00""#), ErrorKind::BadCodePoint);
        assert_eq!(kind(r#""\ufdd0""#), ErrorKind::BadCodePoint);
        assert_eq!(kind(r#""\uffff""#), ErrorKind::BadCodePoint);
        assert_eq!(kind(r#""\ud83f\udffe""#), ErrorKind::BadCodePoint); // U+1FFFE
        assert_eq!(kind("\"\u{fffe}\""), ErrorKind::BadCodePoint);
        assert_eq!(kind("\"\u{10ffff}\""), ErrorKind::BadCodePoint);
        assert_eq!(kind(r#""\x""#), ErrorKind::BadEscape);
        assert_eq!(kind(r#""\u12g4""#), ErrorKind::BadEscape);
        assert_eq!(kind("\"abc"), ErrorKind::Syntax);
    }

    #[test]
    fn syntax_errors() {
        for bad in ["", " ", "[1,]", "{\"a\":1,}", "[1 2]", "{\"a\" 1}", "{1:2}", "tru", "nul", "[", "{}}", "{} {}", "'a'"] {
            assert!(parse(bad.as_bytes()).is_err(), "{bad:?}");
        }
        assert_eq!(kind("{} x"), ErrorKind::Trailing);
    }

    fn err(path: &str, kind: SchemaKind) -> SchemaError {
        SchemaError { path: path.into(), kind }
    }

    /// INIT.md's budget: pages is a 64-bit quantity (a string), the rest small counts.
    #[derive(Debug, PartialEq)]
    struct Budget {
        pages: u64,
        processes: i64,
        weight: i64,
    }

    fn budget(v: &Value<'_>) -> Result<Budget, SchemaError> {
        v.object(|m| {
            Ok(Budget {
                pages: m.required("pages", Value::u64_string)?,
                processes: m.required("processes", Value::int)?,
                weight: m.required("weight", Value::int)?,
            })
        })
    }

    /// Decodes a server entry the way `init` would, taking the members in `known`.
    fn server(v: &Value<'_>, known: &[&str]) -> Result<Budget, SchemaError> {
        v.object(|m| {
            for name in known {
                m.optional(name, |_| Ok(()))?;
            }
            m.required("budget", budget)
        })
    }

    #[test]
    fn unknown_members_are_errors() {
        // The manifest example from INIT.md.
        let text = br#"{ "name": "fsd:data", "program": "fsd", "volume": "data",
                         "budget": { "pages": "4096", "processes": 1, "weight": 100 },
                         "receives": ["fsd:data"], "handed": ["blkd"] }"#;
        let v = parse(text).unwrap();
        let all = ["name", "program", "volume", "receives", "handed"];
        assert_eq!(server(&v, &all), Ok(Budget { pages: 4096, processes: 1, weight: 100 }));
        // A member the decoder does not take is refused.
        assert_eq!(server(&v, &all[1..]), Err(err("name", SchemaKind::Unknown)));
    }

    #[test]
    fn schema_errors_name_the_path() {
        let v = parse(br#"{"servers": [{"budget": {"pages": "1", "processes": 1, "weight": "1"}}]}"#).unwrap();
        let servers = |v: &Value<'_>| v.object(|m| m.required("servers", |s| s.items(|s| server(s, &[]))));
        assert_eq!(servers(&v), Err(err("servers[0].budget.weight", SchemaKind::WrongType)));
        let v = parse(br#"{"servers": [{"budget": {"pages": 1, "processes": 1, "weight": 1}}]}"#).unwrap();
        assert_eq!(servers(&v), Err(err("servers[0].budget.pages", SchemaKind::WrongType)));
        let v = parse(br#"{"servers": [{"budget": {"pages": "1", "processes": 1}}]}"#).unwrap();
        assert_eq!(servers(&v), Err(err("servers[0].budget.weight", SchemaKind::Missing)));
        let v = parse(br#"{"servers": [{"budget": {"pages": "1", "processes": 1, "weight": 1, "cpu": 2}}]}"#).unwrap();
        assert_eq!(servers(&v), Err(err("servers[0].budget.cpu", SchemaKind::Unknown)));
        assert_eq!(servers(&Value::Int(1)), Err(err("", SchemaKind::WrongType)));
        let v = parse(br#"{"servers": {}}"#).unwrap();
        assert_eq!(servers(&v), Err(err("servers", SchemaKind::WrongType)));
    }
}
