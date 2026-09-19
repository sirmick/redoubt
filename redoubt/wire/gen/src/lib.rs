//! Generates the typed-message codecs from the tables in the design notes (WIRE.md: "one
//! layout per message type, defined by a table in the owning server's note ... the Rust and
//! Elixir codecs are generated from those tables, so sender and receiver cannot disagree").
//!
//! The notes are the single source of truth. This crate reads every `planning/redoubt/*.md`
//! and `redoubt/wire/tables/*.md`, finds each table marked `<!-- wire: NAME -->`, and emits
//! `redoubt/wire/src/proto/NAME.rs` and `redoubt/wire/elixir/proto/NAME.ex`. The
//! generated files are checked in, and a test fails if they differ from what the notes
//! produce now. The table format is described in `redoubt/wire/tables/example.md`.
//!
//! Host-only (`std`); nothing here runs on Redoubt.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Bytes an inline message carries (redoubt-wire `typed::INLINE_BYTES`).
const INLINE_BYTES: usize = 12;
/// Handle slots in a message (KERNEL-SPEC.md `MAX_MSG_HANDLES`).
const MAX_MSG_HANDLES: usize = 4;
const HEADER: [&str; 3] = ["Opcode", "Message", "Fields"];
const MARKER_PREFIX: &str = "<!-- wire:";

/// A field's type, as written in a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    U8,
    U16,
    U32,
    U64,
    Str,
    Bytes,
    /// The handle in this slot; carries no bytes.
    Handle(usize),
}

impl Ty {
    /// Encoded size, if fixed; `None` for strings and byte arrays; `Some(0)` for handles.
    fn fixed_size(self) -> Option<usize> {
        match self {
            Ty::U8 => Some(1),
            Ty::U16 => Some(2),
            Ty::U32 => Some(4),
            Ty::U64 => Some(8),
            Ty::Str | Ty::Bytes => None,
            Ty::Handle(_) => Some(0),
        }
    }

    fn bits(self) -> usize {
        self.fixed_size().unwrap_or(0) * 8
    }

    /// Method name on the Rust `Reader`/`Writer`, and the Elixir type atom.
    fn codec_name(self) -> &'static str {
        match self {
            Ty::U8 => "u8",
            Ty::U16 => "u16",
            Ty::U32 => "u32",
            Ty::U64 => "u64",
            Ty::Str => "string",
            Ty::Bytes => "bytes",
            Ty::Handle(_) => "handle",
        }
    }

    fn rust_type(self) -> &'static str {
        match self {
            Ty::U8 => "u8",
            Ty::U16 => "u16",
            Ty::U32 => "u32",
            Ty::U64 => "u64",
            Ty::Str => "&'a str",
            Ty::Bytes => "&'a [u8]",
            Ty::Handle(_) => "",
        }
    }

    fn value_variant(self) -> &'static str {
        match self {
            Ty::U8 => "U8",
            Ty::U16 => "U16",
            Ty::U32 => "U32",
            Ty::U64 => "U64",
            Ty::Str => "Str",
            Ty::Bytes => "Bytes",
            Ty::Handle(_) => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageDef {
    pub opcode: u32,
    pub name: String,
    /// In table order, handles included.
    pub fields: Vec<Field>,
}

impl MessageDef {
    /// The fields that are encoded (everything but handles).
    fn data(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(|f| !matches!(f.ty, Ty::Handle(_)))
    }

    fn handles(&self) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(|f| matches!(f.ty, Ty::Handle(_)))
    }

    /// Inline if every field has a fixed size and they fit in the words (WIRE.md).
    pub fn inline(&self) -> bool {
        let sizes: Option<Vec<usize>> = self.fields.iter().map(|f| f.ty.fixed_size()).collect();
        sizes.is_some_and(|s| s.iter().sum::<usize>() <= INLINE_BYTES)
    }

    fn borrows(&self) -> bool {
        self.data().any(|f| matches!(f.ty, Ty::Str | Ty::Bytes))
    }

    fn type_name(&self) -> String {
        camel(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Protocol {
    pub name: String,
    /// The note the table came from, relative to the repository root.
    pub source: String,
    pub messages: Vec<MessageDef>,
}

/// Rust keywords (struct fields and modules) and Elixir reserved words (map keys, atoms) a
/// name may not be. Generated code never binds a bare field name, so no local can clash.
const RESERVED: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if",
    "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "static",
    "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "async", "await", "dyn",
    "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof", "unsized", "virtual",
    "yield", "try", "gen", "and", "or", "not", "when", "end", "nil", "catch", "rescue", "after",
];

fn check_ident(what: &str, name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let ok = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !name.ends_with('_')
        && !name.contains("__");
    if !ok {
        return Err(format!("{what} `{name}` must be snake_case ([a-z][a-z0-9_]*)"));
    }
    if RESERVED.contains(&name) {
        return Err(format!("{what} `{name}` is reserved in generated code"));
    }
    Ok(())
}

fn camel(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut c = part.chars();
            c.next().map(|first| first.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect()
}

fn cells(line: &str) -> Option<Vec<&str>> {
    let inner = line.trim().strip_prefix('|')?.strip_suffix('|')?;
    Some(inner.split('|').map(str::trim).collect())
}

fn backticked(s: &str) -> Option<&str> {
    s.strip_prefix('`')?.strip_suffix('`')
}

fn parse_type(s: &str, handles_so_far: usize) -> Result<Ty, String> {
    Ok(match s {
        "u8" => Ty::U8,
        "u16" => Ty::U16,
        "u32" => Ty::U32,
        "u64" => Ty::U64,
        "string" => Ty::Str,
        "bytes" => Ty::Bytes,
        _ => {
            let slot = s
                .strip_prefix("handle[")
                .and_then(|r| r.strip_suffix(']'))
                .and_then(|n| n.parse::<usize>().ok())
                .ok_or_else(|| format!("unknown type `{s}`"))?;
            if slot != handles_so_far {
                return Err(format!("`{s}`: handle slots must be numbered 0, 1, ... in order"));
            }
            if slot >= MAX_MSG_HANDLES {
                return Err(format!("`{s}`: a message carries at most {MAX_MSG_HANDLES} handles"));
            }
            Ty::Handle(slot)
        }
    })
}

fn parse_row(row: &[&str]) -> Result<MessageDef, String> {
    let [opcode, name, fields] = row else {
        return Err(format!("a row has {} cells, expected 3", row.len()));
    };
    let decimal = !opcode.is_empty() && opcode.bytes().all(|b| b.is_ascii_digit());
    let opcode: u32 = opcode
        .parse()
        .ok()
        .filter(|_| decimal)
        .ok_or_else(|| format!("opcode `{opcode}` is not a decimal u32"))?;
    let name = backticked(name).ok_or_else(|| format!("message name `{name}` must be in backticks"))?;
    check_ident("message", name)?;
    let mut parsed = Vec::new();
    if *fields != "-" {
        for item in fields.split(',') {
            let item = item.trim();
            let inner = backticked(item).ok_or_else(|| format!("field `{item}` must be `name: type` in backticks"))?;
            let (fname, ty) = inner.split_once(':').ok_or_else(|| format!("field `{inner}` has no `: type`"))?;
            let fname = fname.trim();
            check_ident("field", fname)?;
            if parsed.iter().any(|f: &Field| f.name == fname) {
                return Err(format!("field `{fname}` appears twice"));
            }
            let handles = parsed.iter().filter(|f| matches!(f.ty, Ty::Handle(_))).count();
            parsed.push(Field { name: fname.to_string(), ty: parse_type(ty.trim(), handles)? });
        }
    }
    Ok(MessageDef { opcode, name: name.to_string(), fields: parsed })
}

/// Finds and parses every wire table in one markdown file.
pub fn parse(source: &str, text: &str) -> Result<Vec<Protocol>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut protocols = Vec::new();
    let mut i = 0;
    while let Some(line) = lines.get(i) {
        let at = |n: usize, msg: String| format!("{source}:{}: {msg}", n + 1);
        let trimmed = line.trim();
        // A table with our header but no marker would be silently ignored: refuse it.
        if cells(trimmed).is_some_and(|c| c == HEADER) {
            return Err(at(i, "a wire table needs a `<!-- wire: NAME -->` line before it".into()));
        }
        let Some(rest) = trimmed.strip_prefix(MARKER_PREFIX) else {
            i += 1;
            continue;
        };
        let name = rest.strip_suffix("-->").map(str::trim).ok_or_else(|| at(i, "malformed wire marker".into()))?;
        check_ident("protocol", name).map_err(|e| at(i, e))?;
        let marker = i;
        i += 1;
        while lines.get(i).is_some_and(|l| l.trim().is_empty()) {
            i += 1;
        }
        if lines.get(i).and_then(|l| cells(l)).is_none_or(|c| c != HEADER) {
            return Err(at(i, "expected the header `| Opcode | Message | Fields |`".into()));
        }
        i += 1;
        let separator = lines.get(i).and_then(|l| cells(l));
        if separator.is_none_or(|c| c.len() != 3 || c.iter().any(|s| s.len() < 3 || !s.chars().all(|ch| ch == '-'))) {
            return Err(at(i, "expected the separator `| --- | --- | --- |`".into()));
        }
        i += 1;
        let mut messages: Vec<MessageDef> = Vec::new();
        while let Some(row) = lines.get(i).and_then(|l| cells(l)) {
            let m = parse_row(&row).map_err(|e| at(i, e))?;
            if messages.iter().any(|o| o.opcode == m.opcode) {
                return Err(at(i, format!("opcode {} used twice", m.opcode)));
            }
            if messages.iter().any(|o| o.name == m.name || o.type_name() == m.type_name()) {
                return Err(at(i, format!("message `{}` defined twice", m.name)));
            }
            messages.push(m);
            i += 1;
        }
        if messages.is_empty() {
            return Err(at(marker, format!("protocol `{name}` has no messages")));
        }
        protocols.push(Protocol { name: name.to_string(), source: source.to_string(), messages });
    }
    Ok(protocols)
}

/// The generated Rust module for one protocol.
pub fn rust(p: &Protocol) -> String {
    let lt = if p.messages.iter().any(MessageDef::borrows) { "<'a>" } else { "" };
    let any_buffer = p.messages.iter().any(|m| !m.inline());
    let mut s = String::new();
    let _ = writeln!(s, "//! Typed messages of the `{}` protocol.", p.name);
    let _ = writeln!(s, "//!");
    let _ = writeln!(s, "//! Generated by `redoubt-wire-gen` from the table in `{}`.", p.source);
    let _ = writeln!(s, "//! Do not edit: change the table and run `cargo run -p redoubt-wire-gen`.");
    s.push('\n');
    s.push_str("use crate::codec::{Error, Reader};\nuse crate::typed::{self, Value, Words};\n");
    for m in &p.messages {
        let shape = if m.inline() { "inline" } else { "buffer" };
        let _ = writeln!(s, "\n/// `{}`: opcode {}, {shape}.", m.name, m.opcode);
        let handles: Vec<String> = m.handles().map(|f| format!("`{}`", f.name)).collect();
        if !handles.is_empty() {
            let _ = writeln!(s, "/// Handle slots: {}.", handles.join(", "));
        }
        let mlt = if m.borrows() { "<'a>" } else { "" };
        let _ = writeln!(s, "#[derive(Debug, Clone, Copy, PartialEq, Eq)]");
        if m.data().next().is_none() {
            let _ = writeln!(s, "pub struct {} {{}}", m.type_name());
            continue;
        }
        let _ = writeln!(s, "pub struct {}{mlt} {{", m.type_name());
        for f in m.data() {
            let _ = writeln!(s, "    pub {}: {},", f.name, f.ty.rust_type());
        }
        s.push_str("}\n");
    }
    let _ = writeln!(s, "\n/// Every message of the protocol.");
    let _ = writeln!(s, "#[derive(Debug, Clone, Copy, PartialEq, Eq)]");
    let _ = writeln!(s, "pub enum Message{lt} {{");
    for m in &p.messages {
        let mlt = if m.borrows() { "<'a>" } else { "" };
        let _ = writeln!(s, "    {}({}{mlt}),", m.type_name(), m.type_name());
    }
    s.push_str("}\n");
    let a = if lt.is_empty() { "" } else { "'a " };
    let _ = writeln!(s, "\nimpl{lt} Message{lt} {{");
    let opcodes: Vec<String> = p.messages.iter().map(|m| m.opcode.to_string()).collect();
    let names: Vec<String> = p.messages.iter().map(|m| format!("{:?}", m.name)).collect();
    let handles: Vec<String> = p
        .messages
        .iter()
        .map(|m| format!("&[{}]", m.handles().map(|f| format!("{:?}", f.name)).collect::<Vec<_>>().join(", ")))
        .collect();
    accessor(&mut s, p, "The opcode in word 0.", "opcode", "u32", &opcodes);
    accessor(&mut s, p, "The message's name in the table.", "name", "&'static str", &names);
    accessor(&mut s, p, "The handles the message carries, by slot.", "handle_names", "&'static [&'static str]", &handles);
    // decode
    s.push_str("    /// Decodes a received message: its words, the buffer that came with it (empty if\n");
    s.push_str("    /// none), and how many handles it carried.\n");
    let _ = writeln!(s, "    pub fn decode(words: &Words, buf: &{a}[u8], handles: usize) -> Result<Self, Error> {{");
    s.push_str("        let message = match typed::opcode(words)? {\n");
    for m in &p.messages {
        let _ = writeln!(s, "            {} => {{", m.opcode);
        let has_data = m.data().next().is_some();
        let r = if has_data { "mut r" } else { "r" };
        if m.inline() {
            s.push_str("                let bytes = typed::inline_bytes(words, buf)?;\n");
            let _ = writeln!(s, "                let {r} = Reader::new(&bytes);");
        } else {
            let _ = writeln!(s, "                let {r} = Reader::new(typed::buffer_body(words, buf)?);");
        }
        let _ = write!(s, "                let m = {} {{", m.type_name());
        let inits: Vec<String> = m.data().map(|f| format!(" {}: r.{}()?", f.name, f.ty.codec_name())).collect();
        s.push_str(&inits.join(","));
        s.push_str(if inits.is_empty() { "};\n" } else { " };\n" });
        let finish = if m.inline() { "finish_padding" } else { "finish" };
        let _ = writeln!(s, "                r.{finish}()?;");
        let _ = writeln!(s, "                Message::{}(m)", m.type_name());
        s.push_str("            }\n");
    }
    s.push_str("            _ => return Err(Error::BadOpcode),\n        };\n");
    s.push_str("        typed::check_handles(handles, message.handle_names().len())?;\n");
    s.push_str("        Ok(message)\n    }\n\n");
    // encode
    s.push_str("    /// Encodes the message: returns its words, and writes the fields into `buf` if it\n");
    s.push_str("    /// is a buffer message (the buffer's length is then in word 1).\n");
    let buf = if any_buffer { "buf" } else { "_buf" };
    let _ = writeln!(s, "    pub fn encode(&self, {buf}: &mut [u8]) -> Result<Words, Error> {{");
    s.push_str("        match self {\n");
    for m in &p.messages {
        let data: Vec<&Field> = m.data().collect();
        let binding = if data.is_empty() { "_" } else { "m" };
        let w = if data.is_empty() { "_" } else { "w" };
        let call = if m.inline() {
            format!("typed::encode_inline({}, |{w}| ", m.opcode)
        } else {
            format!("typed::encode_buffer({}, buf, |{w}| ", m.opcode)
        };
        let _ = write!(s, "            Message::{}({binding}) => {call}", m.type_name());
        match data.as_slice() {
            [] => s.push_str("Ok(())),\n"),
            [f] => {
                let _ = writeln!(s, "w.{}(m.{})),", f.ty.codec_name(), f.name);
            }
            _ => {
                s.push_str("{\n");
                for (i, f) in data.iter().enumerate() {
                    let q = if i + 1 == data.len() { "" } else { "?;" };
                    let _ = writeln!(s, "                w.{}(m.{}){q}", f.ty.codec_name(), f.name);
                }
                s.push_str("            }),\n");
            }
        }
    }
    s.push_str("        }\n    }\n\n");
    // fields
    s.push_str("    /// Calls `f` with each field's name and value, in table order (handles excluded).\n");
    s.push_str("    pub fn fields(&self, f: &mut dyn FnMut(&'static str, Value<'_>)) {\n");
    s.push_str("        match self {\n");
    for m in &p.messages {
        let data: Vec<&Field> = m.data().collect();
        if data.is_empty() {
            let _ = writeln!(s, "            Message::{}(_) => {{}}", m.type_name());
            continue;
        }
        let _ = writeln!(s, "            Message::{}(m) => {{", m.type_name());
        for fd in data {
            let _ = writeln!(s, "                f({:?}, Value::{}(m.{}));", fd.name, fd.ty.value_variant(), fd.name);
        }
        s.push_str("            }\n");
    }
    s.push_str("        }\n    }\n}\n");
    s
}

/// A `match self` method returning one constant per message.
fn accessor(s: &mut String, p: &Protocol, doc: &str, name: &str, ret: &str, values: &[String]) {
    let _ = writeln!(s, "    /// {doc}");
    let _ = writeln!(s, "    pub fn {name}(&self) -> {ret} {{");
    s.push_str("        match self {\n");
    for (m, value) in p.messages.iter().zip(values) {
        let _ = writeln!(s, "            Message::{}(_) => {value},", m.type_name());
    }
    s.push_str("        }\n    }\n\n");
}

/// `redoubt/wire/src/proto/mod.rs`.
pub fn rust_mod(protocols: &[Protocol]) -> String {
    let mut s = String::from(
        "//! Generated typed-message codecs, one module per protocol table.\n//!\n\
         //! Generated by `redoubt-wire-gen`. Do not edit: change the tables and run\n\
         //! `cargo run -p redoubt-wire-gen`.\n\n",
    );
    for p in protocols {
        let _ = writeln!(s, "pub mod {};", p.name);
    }
    s
}

/// The generated Elixir module for one protocol.
pub fn elixir(p: &Protocol) -> String {
    let module = format!("Redoubt.Wire.Proto.{}", camel(&p.name));
    let mut s = String::new();
    let _ = writeln!(s, "# Typed messages of the `{}` protocol.", p.name);
    let _ = writeln!(s, "#");
    let _ = writeln!(s, "# Generated by `redoubt-wire-gen` from the table in `{}`.", p.source);
    let _ = writeln!(s, "# Do not edit: change the table and run `cargo run -p redoubt-wire-gen`.");
    let _ = writeln!(s, "defmodule {module} do");
    let _ = writeln!(s, "  @moduledoc \"\"\"");
    let _ = writeln!(s, "  Codec for the `{}` protocol: messages are `{{name, fields}}` with `fields` a map", p.name);
    let _ = writeln!(s, "  holding exactly the table's non-handle fields. See `Redoubt.Wire`.");
    let _ = writeln!(s, "  \"\"\"");
    s.push_str("  alias Redoubt.Wire, as: W\n\n");
    // layout
    s.push_str("  @doc \"The layout of a message: opcode, shape, fields in order with their types, handle names.\"\n");
    for m in &p.messages {
        let shape = if m.inline() { ":inline" } else { ":buffer" };
        let fields: Vec<String> = m.data().map(|f| format!("{{:{}, :{}}}", f.name, f.ty.codec_name())).collect();
        let handles: Vec<String> = m.handles().map(|f| format!(":{}", f.name)).collect();
        let _ = writeln!(
            s,
            "  def layout(:{}), do: {{{}, {shape}, [{}], [{}]}}",
            m.name,
            m.opcode,
            fields.join(", "),
            handles.join(", ")
        );
    }
    s.push_str("  def layout(_), do: nil\n\n");
    // encode
    s.push_str("  @doc \"Encodes `{name, fields}`: `{:ok, words, buffer}` or `{:error, reason}`.\"\n");
    s.push_str("  def encode({name, fields}) when is_atom(name) and is_map(fields), do: W.encoding(fn -> enc(name, fields) end)\n");
    s.push_str("  def encode(_), do: {:error, :bad_message}\n\n");
    for m in &p.messages {
        let data: Vec<&Field> = m.data().collect();
        let pat: Vec<String> = data.iter().map(|f| format!("{}: v_{}", f.name, f.name)).collect();
        let parts: Vec<String> = data
            .iter()
            .map(|f| match f.ty {
                Ty::Str => format!("W.str(v_{})", f.name),
                Ty::Bytes => format!("W.bytes(v_{})", f.name),
                ty => format!("W.u(v_{}, {})", f.name, ty.bits()),
            })
            .collect();
        let shape = if m.inline() { "inline" } else { "buffer" };
        let _ = writeln!(
            s,
            "  defp enc(:{}, %{{{}}} = f) when map_size(f) == {}, do: W.{shape}({}, [{}])",
            m.name,
            pat.join(", "),
            data.len(),
            m.opcode,
            parts.join(", ")
        );
    }
    s.push_str("  defp enc(_, _), do: throw({:wire, :bad_message})\n\n");
    // decode
    s.push_str("  @doc \"\"\"\n  Decodes a received message from its four words, its buffer (`<<>>` if none) and the\n");
    s.push_str("  number of handles it carried: `{:ok, {name, fields}}` or `{:error, reason}`.\n  \"\"\"\n");
    s.push_str("  def decode(words, buffer, handles) when is_binary(buffer) and is_integer(handles) do\n");
    s.push_str("    with {:ok, op} <- W.opcode(words),\n");
    s.push_str("         {:ok, name, fields, count} <- dec(op, words, buffer),\n");
    s.push_str("         :ok <- W.check_handles(handles, count) do\n");
    s.push_str("      {:ok, {name, fields}}\n    end\n  end\n\n");
    s.push_str("  def decode(_, _, _), do: {:error, :bad_message}\n\n");
    for m in &p.messages {
        let data: Vec<&Field> = m.data().collect();
        let (body, finish) = if m.inline() {
            ("W.inline_body(words, buffer)", true)
        } else {
            ("W.buffer_body(words, buffer)", false)
        };
        let mut segs: Vec<String> = Vec::new();
        for f in &data {
            match f.ty {
                Ty::Str | Ty::Bytes => {
                    let len_bits = if f.ty == Ty::Str { 16 } else { 32 };
                    segs.push(format!("n_{}::little-{len_bits}", f.name));
                    segs.push(format!("v_{}::binary-size(n_{})", f.name, f.name));
                }
                ty => segs.push(format!("v_{}::little-{}", f.name, ty.bits())),
            }
        }
        if finish {
            segs.push("pad::binary".into());
        }
        let _ = writeln!(s, "  defp dec({}, words, buffer) do", m.opcode);
        let _ = writeln!(s, "    with {{:ok, body}} <- {body},");
        let _ = write!(s, "         <<{}>> <- body", segs.join(", "));
        if finish {
            s.push_str(",\n         true <- W.zero?(pad)");
        }
        for f in data.iter().filter(|f| f.ty == Ty::Str) {
            let _ = write!(s, ",\n         true <- String.valid?(v_{})", f.name);
        }
        let map: Vec<String> = data.iter().map(|f| format!("{}: v_{}", f.name, f.name)).collect();
        let _ = writeln!(s, " do");
        let _ = writeln!(s, "      {{:ok, :{}, %{{{}}}, {}}}", m.name, map.join(", "), m.handles().count());
        s.push_str("    else\n      {:error, _} = e -> e\n      _ -> {:error, :malformed}\n    end\n  end\n\n");
    }
    s.push_str("  defp dec(_, _, _), do: {:error, :bad_opcode}\nend\n");
    s
}

/// The notes that may hold tables, relative to the repository root, sorted.
pub fn sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for dir in ["planning/redoubt", "redoubt/wire/tables"] {
        let entries = std::fs::read_dir(root.join(dir)).map_err(|e| format!("{dir}: {e}"))?;
        for entry in entries {
            let path = entry.map_err(|e| format!("{dir}: {e}"))?.path();
            if path.extension().is_some_and(|x| x == "md") {
                let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
                found.push(rel.to_path_buf());
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Every generated file (path relative to the repository root, contents), from the notes.
pub fn generate(root: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let mut protocols: Vec<Protocol> = Vec::new();
    for src in sources(root)? {
        let text = std::fs::read_to_string(root.join(&src)).map_err(|e| format!("{}: {e}", src.display()))?;
        let name = src.to_str().ok_or("non-UTF-8 path")?.replace('\\', "/");
        for p in parse(&name, &text)? {
            if let Some(other) = protocols.iter().find(|o| o.name == p.name) {
                return Err(format!("protocol `{}` is defined in both {} and {}", p.name, other.source, p.source));
            }
            protocols.push(p);
        }
    }
    protocols.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out = vec![(PathBuf::from("redoubt/wire/src/proto/mod.rs"), rust_mod(&protocols))];
    for p in &protocols {
        out.push((PathBuf::from(format!("redoubt/wire/src/proto/{}.rs", p.name)), rust(p)));
        out.push((PathBuf::from(format!("redoubt/wire/elixir/proto/{}.ex", p.name)), elixir(p)));
    }
    Ok(out)
}

/// Generated files that are missing or differ, and files in the output directories that
/// no table produces any more.
pub fn stale(root: &Path) -> Result<Vec<PathBuf>, String> {
    let expected = generate(root)?;
    let mut stale = Vec::new();
    for (path, contents) in &expected {
        if std::fs::read_to_string(root.join(path)).ok().as_deref() != Some(contents.as_str()) {
            stale.push(path.clone());
        }
    }
    for dir in ["redoubt/wire/src/proto", "redoubt/wire/elixir/proto"] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else { continue };
        for entry in entries.flatten() {
            let rel = Path::new(dir).join(entry.file_name());
            if !expected.iter().any(|(p, _)| *p == rel) {
                stale.push(rel);
            }
        }
    }
    Ok(stale)
}

/// The repository root, from this crate's location (`redoubt/wire/gen`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "intro\n\n<!-- wire: demo -->\n| Opcode | Message | Fields |\n| --- | --- | --- |\n\
                         | 1 | `a` | - |\n| 2 | `b_c` | `x: u64`, `h: handle[0]`, `s: string` |\n\nafter\n";

    #[test]
    fn parses_a_table() {
        let p = parse("n.md", TABLE).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].name, "demo");
        assert_eq!(p[0].messages[1].name, "b_c");
        assert_eq!(p[0].messages[1].type_name(), "BC");
        assert_eq!(p[0].messages[1].fields[1].ty, Ty::Handle(0));
        assert!(p[0].messages[0].inline());
        assert!(!p[0].messages[1].inline());
    }

    fn err(text: &str) -> String {
        parse("n.md", text).unwrap_err()
    }

    #[test]
    fn refuses_bad_tables() {
        let head = "<!-- wire: demo -->\n| Opcode | Message | Fields |\n| --- | --- | --- |\n";
        assert!(err("| Opcode | Message | Fields |\n| --- | --- | --- |\n| 1 | `a` | - |\n").contains("needs a"));
        assert!(err(&format!("{head}| 1 | `a` | - |\n| 1 | `b` | - |\n")).contains("opcode 1 used twice"));
        assert!(err(&format!("{head}| 1 | `a` | - |\n| 2 | `a` | - |\n")).contains("defined twice"));
        assert!(err(&format!("{head}| 1 | `a` | `x: u128` |\n")).contains("unknown type"));
        assert!(err(&format!("{head}| 1 | `a` | `x: handle[1]` |\n")).contains("in order"));
        let five = "`a: handle[0]`, `b: handle[1]`, `c: handle[2]`, `d: handle[3]`, `e: handle[4]`";
        assert!(err(&format!("{head}| 1 | `a` | {five} |\n")).contains("at most 4"));
        assert!(err(&format!("{head}| 1 | `a` | `x: u8`, `x: u8` |\n")).contains("twice"));
        assert!(err(&format!("{head}| 1 | `a` | `type: u8` |\n")).contains("reserved"));
        assert!(err(&format!("{head}| 1 | `A` | - |\n")).contains("snake_case"));
        assert!(err(&format!("{head}| -1 | `a` | - |\n")).contains("decimal u32"));
        assert!(err(&format!("{head}| 4294967296 | `a` | - |\n")).contains("decimal u32"));
        assert!(err(&format!("{head}| 1 | a | - |\n")).contains("backticks"));
        assert!(err(&format!("{head}\n")).contains("no messages"));
        assert!(err("<!-- wire: demo -->\n| Op | Message | Fields |\n").contains("header"));
    }

    #[test]
    fn inline_boundary_is_twelve_bytes() {
        let t = "<!-- wire: d -->\n| Opcode | Message | Fields |\n| --- | --- | --- |\n\
                 | 1 | `a` | `x: u64`, `y: u32`, `h: handle[0]` |\n| 2 | `b` | `x: u64`, `y: u32`, `z: u8` |\n";
        let p = parse("n.md", t).unwrap();
        assert!(p[0].messages[0].inline());
        assert!(!p[0].messages[1].inline());
    }

    /// The checked-in generated code is exactly what the notes produce: the notes stay the
    /// single source of truth.
    #[test]
    fn generated_files_are_current() {
        let stale = stale(&repo_root()).unwrap();
        assert!(stale.is_empty(), "stale generated files {stale:?}: run `cargo run -p redoubt-wire-gen`");
    }
}
