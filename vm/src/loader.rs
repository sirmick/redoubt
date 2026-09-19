//! Loading `.beam` files.
//!
//! Only the format written by the pinned compiler (OTP 28) is accepted: its atom table layout,
//! its uncompressed literal table and its opcodes. Everything else is refused with an error, never
//! guessed at. The input is treated as hostile, so every index is checked here, once, and the
//! interpreter can rely on the result: labels resolve to instructions, and import, fun, literal
//! and atom indices are all in range.

use alloc::string::String;
use alloc::vec::Vec;

use crate::atom::{Atom, AtomTable};
use crate::etf;
use crate::module::{Arg, Export, FunEntry, FunctionInfo, Import, Instr, Module};
use crate::opcodes::{self, MAX_OPCODE, OPCODES};
use crate::term::Term;

/// Number of X registers; operands naming a higher one are rejected.
pub const X_REGS: usize = 1024;
/// Largest stack frame (Y registers) a function may allocate.
pub const MAX_Y_REGS: usize = 1024;
/// Number of floating-point registers.
pub const FLOAT_REGS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// Not an IFF `FOR1`/`BEAM` container, or a chunk overruns the file.
    NotBeam,
    /// A required chunk is absent.
    MissingChunk(&'static str),
    /// Compiled by an older OTP (compressed literals or old atom table). Recompile with OTP 28.
    OldFormat,
    /// Uses an opcode newer than the pinned compiler knows. Built with a newer OTP?
    NewerOpcode(u8),
    /// An opcode that the pinned compiler no longer emits.
    Unsupported(&'static str),
    /// A structurally invalid chunk or operand, with a short reason.
    Malformed(&'static str),
    /// A literal failed to decode.
    Literal(etf::EtfError),
    /// Too many or too long atoms.
    AtomLimit,
}

impl From<etf::EtfError> for LoadError {
    fn from(e: etf::EtfError) -> Self {
        LoadError::Literal(e)
    }
}

type Result<T> = core::result::Result<T, LoadError>;

/// Parse and validate `bytes` as a module.
pub fn load(bytes: &[u8], atoms: &mut AtomTable) -> Result<Module> {
    let chunks = chunks(bytes)?;
    let chunk = |id: &[u8; 4]| chunks.iter().find(|(c, _)| c == id).map(|(_, d)| *d);
    let need = |id: &'static str| chunk(id.as_bytes().try_into().unwrap()).ok_or(LoadError::MissingChunk(id));

    let atom_table = atom_chunk(need("AtU8")?, atoms)?;
    let atom = |i: usize| -> Result<Atom> {
        // Atom indices in tables are 1-based.
        i.checked_sub(1)
            .and_then(|i| atom_table.get(i))
            .cloned()
            .ok_or(LoadError::Malformed("atom index out of range"))
    };
    let name = atom(1)?;

    let mut imports = Vec::new();
    for [m, f, a] in triples(need("ImpT")?)? {
        imports.push(Import { module: atom(m)?, function: atom(f)?, arity: arity(a)?, native: None });
    }

    let literals = match chunk(b"LitT") {
        Some(d) => literal_chunk(d, atoms)?,
        None => Vec::new(),
    };
    let strings = chunk(b"StrT").unwrap_or(&[]).to_vec();

    let ctx = Tables { atoms: &atom_table, literals: &literals };
    let (code, label_count, labels) = code_chunk(need("Code")?, &ctx)?;
    let resolve = |label: usize| -> Result<u32> {
        match labels.get(label) {
            Some(Some(pc)) => Ok(*pc),
            _ => Err(LoadError::Malformed("undefined label")),
        }
    };

    let mut exports = Vec::new();
    for [f, a, l] in triples(need("ExpT")?)? {
        exports.push(Export { function: atom(f)?, arity: arity(a)?, entry: resolve(l)? });
    }

    let mut funs = Vec::new();
    if let Some(d) = chunk(b"FunT") {
        let words = words(d)?;
        let (&count, rest) = words.split_first().ok_or(LoadError::Malformed("FunT"))?;
        if rest.len() != count as usize * 6 {
            return Err(LoadError::Malformed("FunT size"));
        }
        for e in rest.chunks_exact(6) {
            let (arity_total, num_free) = (arity(e[1] as usize)?, e[4]);
            if num_free > arity_total {
                return Err(LoadError::Malformed("fun has more free variables than arguments"));
            }
            funs.push(FunEntry {
                function: atom(e[0] as usize)?,
                arity: arity_total,
                entry: resolve(e[2] as usize)?,
                num_free,
                uniq: e[5],
            });
        }
    }

    let mut lines = match chunk(b"Line") {
        Some(d) => line_chunk(d, name.as_str())?,
        None => crate::module::Lines::default(),
    };
    let mut code = code;
    let mut functions = Vec::new();
    for (pc, ins) in code.iter_mut().enumerate() {
        // Rewrite label numbers into code indices.
        resolve_labels(&mut ins.args, &resolve, label_count)?;
        check_operands(ins, &imports, &funs, &literals, strings.len())?;
        if ins.op == opcodes::LINE {
            match ins.args.first() {
                Some(Arg::U(item)) if (*item as usize) < lines.items.len().max(1) => {
                    lines.marks.push((pc as u32, *item as u32));
                }
                _ => return Err(LoadError::Malformed("line item out of range")),
            }
        }
        if ins.op == opcodes::FUNC_INFO {
            if let [_, Arg::Const(Term::Atom(f)), Arg::U(a)] = &ins.args[..] {
                functions.push(FunctionInfo { start: pc as u32, name: f.clone(), arity: arity(*a as usize)? });
            } else {
                return Err(LoadError::Malformed("func_info"));
            }
        }
    }

    let attributes = chunk(b"Attr").unwrap_or(&[]).to_vec();
    let compile_info = chunk(b"CInf").unwrap_or(&[]).to_vec();
    let md5 = checksum(&chunk);
    Ok(Module {
        name,
        imports,
        exports,
        funs,
        literals,
        strings,
        code,
        functions,
        lines,
        body_natives: Vec::new(),
        attributes,
        compile_info,
        md5,
    })
}

/// BEAM's module checksum (`beam_file.c`): MD5 over the chunks that define the code, in a fixed
/// order, with each fun's `OldUniq` zeroed (it came from an old, endian-dependent hash).
fn checksum<'a>(chunk: &impl Fn(&[u8; 4]) -> Option<&'a [u8]>) -> [u8; 16] {
    use md5::Digest;
    let mut h = md5::Md5::new();
    for id in [b"AtU8", b"Code", b"StrT", b"ImpT", b"ExpT"] {
        h.update(chunk(id).unwrap_or(&[]));
    }
    if let Some(funt) = chunk(b"FunT").filter(|d| d.len() >= 4) {
        h.update(&funt[..4]);
        for entry in funt[4..].chunks_exact(24) {
            h.update(&entry[..20]);
            h.update([0u8; 4]);
        }
    }
    for id in [b"LitT", b"Meta", b"Recs", b"DbgB"] {
        h.update(chunk(id).unwrap_or(&[]));
    }
    h.finalize().into()
}

/// Parse the `Line` chunk (see `parse_line_chunk` in BEAM's `beam_file.c`).
fn line_chunk(d: &[u8], module: &str) -> Result<crate::module::Lines> {
    let mut r = Compact { bytes: d, pos: 0 };
    let version = r.word()?;
    if version != 0 {
        // A newer format: stack traces lose their locations, nothing else is affected.
        return Ok(crate::module::Lines::default());
    }
    let _flags = r.word()?;
    let _instruction_count = r.word()?;
    let item_count = r.word()? as usize;
    let name_count = r.word()? as usize;
    if item_count > d.len() || name_count > d.len() {
        return Err(LoadError::Malformed("Line counts"));
    }
    let mut items = alloc::vec![(0u32, 0u32)];
    let mut file = 0u32;
    while items.len() <= item_count {
        match r.tag_and_value()? {
            (TAG_A, f) => {
                file = u32::try_from(f).ok().filter(|f| *f as usize <= name_count).ok_or(LoadError::Malformed("Line file"))?;
            }
            (TAG_I, line) => {
                items.push((file, u32::try_from(line).map_err(|_| LoadError::Malformed("Line number"))?));
            }
            _ => return Err(LoadError::Malformed("Line item")),
        }
    }
    let as_list = |s: &str| Term::list(s.chars().map(|ch| Term::Int(ch as i64)).collect::<Vec<_>>());
    let mut files = alloc::vec![as_list(&alloc::format!("{module}.erl"))];
    for _ in 0..name_count {
        let len = u16::from_be_bytes(r.take(2)?.try_into().unwrap()) as usize;
        let name = core::str::from_utf8(r.take(len)?).map_err(|_| LoadError::Malformed("Line file name"))?;
        files.push(as_list(name));
    }
    Ok(crate::module::Lines { files, items, marks: Vec::new() })
}

/// Split the IFF container into `(id, data)` chunks.
fn chunks(bytes: &[u8]) -> Result<Vec<([u8; 4], &[u8])>> {
    if bytes.len() < 12 || &bytes[0..4] != b"FOR1" || &bytes[8..12] != b"BEAM" {
        return Err(LoadError::NotBeam);
    }
    let size = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let body = bytes.get(8..8usize.checked_add(size).ok_or(LoadError::NotBeam)?).ok_or(LoadError::NotBeam)?;
    let mut pos = 4; // past "BEAM"
    let mut out = Vec::new();
    while pos < body.len() {
        let header = body.get(pos..pos + 8).ok_or(LoadError::NotBeam)?;
        let id: [u8; 4] = header[0..4].try_into().unwrap();
        let len = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
        let start = pos + 8;
        let end = start.checked_add(len).ok_or(LoadError::NotBeam)?;
        out.push((id, body.get(start..end).ok_or(LoadError::NotBeam)?));
        pos = end.checked_add(3).ok_or(LoadError::NotBeam)? & !3; // chunks are 4-byte aligned
    }
    Ok(out)
}

fn words(d: &[u8]) -> Result<Vec<u32>> {
    if !d.len().is_multiple_of(4) {
        return Err(LoadError::Malformed("chunk is not a whole number of words"));
    }
    Ok(d.chunks_exact(4).map(|w| u32::from_be_bytes(w.try_into().unwrap())).collect())
}

/// A table of `count` entries of three words (ImpT, ExpT).
fn triples(d: &[u8]) -> Result<Vec<[usize; 3]>> {
    let w = words(d)?;
    let (&count, rest) = w.split_first().ok_or(LoadError::Malformed("empty table"))?;
    if rest.len() != count as usize * 3 {
        return Err(LoadError::Malformed("table size"));
    }
    Ok(rest.chunks_exact(3).map(|t| [t[0] as usize, t[1] as usize, t[2] as usize]).collect())
}

fn arity(a: usize) -> Result<u32> {
    if a <= 255 {
        Ok(a as u32)
    } else {
        Err(LoadError::Malformed("arity above 255"))
    }
}

fn atom_chunk(d: &[u8], atoms: &mut AtomTable) -> Result<Vec<Atom>> {
    let count = i32::from_be_bytes(d.get(0..4).ok_or(LoadError::Malformed("AtU8"))?.try_into().unwrap());
    // OTP 28 writes a negative count and compact-encoded lengths; older compilers do not.
    if count >= 0 {
        return Err(LoadError::OldFormat);
    }
    let count = count.unsigned_abs() as usize;
    let mut r = Compact { bytes: d, pos: 4 };
    if count > d.len() {
        return Err(LoadError::Malformed("atom count"));
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let (tag, len) = r.tag_and_value()?;
        if tag != TAG_U {
            return Err(LoadError::Malformed("atom length"));
        }
        let len = usize::try_from(len).map_err(|_| LoadError::Malformed("atom length"))?;
        let text = r.take(len)?;
        let text = core::str::from_utf8(text).map_err(|_| LoadError::Malformed("atom is not UTF-8"))?;
        out.push(atoms.intern(text).map_err(|_| LoadError::AtomLimit)?);
    }
    Ok(out)
}

fn literal_chunk(d: &[u8], atoms: &mut AtomTable) -> Result<Vec<Term>> {
    let mut r = Compact { bytes: d, pos: 0 };
    // OTP 28 writes a zero word here; older compilers wrote the size before zlib compression.
    if r.word()? != 0 {
        return Err(LoadError::OldFormat);
    }
    let count = r.word()? as usize;
    if count > d.len() {
        return Err(LoadError::Malformed("literal count"));
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let size = r.word()? as usize;
        out.push(etf::decode(r.take(size)?, atoms)?);
    }
    if r.pos != d.len() {
        return Err(LoadError::Malformed("trailing bytes in LitT"));
    }
    Ok(out)
}

struct Tables<'a> {
    atoms: &'a [Atom],
    literals: &'a [Term],
}

/// Decode the code chunk. Returns the instructions (with label *numbers* in `Arg::Label`), the
/// number of labels, and for each label number the index of its `label` instruction.
#[allow(clippy::type_complexity)]
fn code_chunk(d: &[u8], t: &Tables) -> Result<(Vec<Instr>, usize, Vec<Option<u32>>)> {
    let mut r = Compact { bytes: d, pos: 0 };
    let header_size = r.word()? as usize;
    let header_start = r.pos;
    let instruction_set = r.word()?;
    let max_opcode = r.word()?;
    let label_count = r.word()? as usize;
    let _function_count = r.word()?;
    if instruction_set != 0 {
        return Err(LoadError::Malformed("instruction set"));
    }
    if max_opcode > MAX_OPCODE as u32 {
        return Err(LoadError::NewerOpcode(max_opcode.min(255) as u8));
    }
    r.pos = header_start.checked_add(header_size).ok_or(LoadError::Malformed("Code header"))?;
    if r.pos > d.len() || label_count > d.len() {
        return Err(LoadError::Malformed("Code header"));
    }

    let mut code = Vec::new();
    let mut labels = alloc::vec![None; label_count];
    loop {
        let op = r.byte()?;
        if op == 0 || op > MAX_OPCODE {
            return Err(LoadError::NewerOpcode(op));
        }
        if op == opcodes::INT_CODE_END {
            break;
        }
        let info = &OPCODES[op as usize];
        // Deprecated opcodes are ones the pinned compiler never emits. Refusing them keeps the
        // interpreter to the instruction set that is actually in use.
        if info.deprecated {
            return Err(LoadError::Unsupported(info.name));
        }
        let mut args = Vec::with_capacity(info.arity as usize);
        for _ in 0..info.arity {
            args.push(r.operand(t, 0)?);
        }
        if op == opcodes::LABEL {
            let n = match args[0] {
                Arg::U(n) => n as usize,
                _ => return Err(LoadError::Malformed("label operand")),
            };
            match labels.get_mut(n) {
                Some(slot @ None) if n != 0 => *slot = Some(code.len() as u32),
                _ => return Err(LoadError::Malformed("label out of range or defined twice")),
            }
        }
        code.push(Instr { op, args });
    }
    Ok((code, label_count, labels))
}

fn resolve_labels(args: &mut [Arg], resolve: &impl Fn(usize) -> Result<u32>, count: usize) -> Result<()> {
    for a in args {
        match a {
            Arg::Label(Some(n)) => {
                if *n as usize >= count {
                    return Err(LoadError::Malformed("label out of range"));
                }
                *a = Arg::Label(Some(resolve(*n as usize)?));
            }
            Arg::List(items) => resolve_labels(items, resolve, count)?,
            _ => {}
        }
    }
    Ok(())
}

/// Checks that need to know what an operand means: table indices.
fn check_operands(ins: &Instr, imports: &[Import], funs: &[FunEntry], _lits: &[Term], strings: usize) -> Result<()> {
    use opcodes::*;
    let index_at = |i: usize| match ins.args.get(i) {
        Some(Arg::U(n)) => Ok(*n as usize),
        _ => Err(LoadError::Malformed("expected an index operand")),
    };
    let import_at = |i: usize| -> Result<()> {
        if index_at(i)? < imports.len() {
            Ok(())
        } else {
            Err(LoadError::Malformed("import index out of range"))
        }
    };
    // A BIF instruction supplies a fixed number of arguments; the import it names must take
    // exactly that many.
    let bif_arity = |i: usize, n: u32| -> Result<()> {
        import_at(i)?;
        if imports[index_at(i)?].arity == n {
            Ok(())
        } else {
            Err(LoadError::Malformed("BIF import arity does not match the instruction"))
        }
    };
    match ins.op {
        // The call's arity must be the import's: natives are resolved per import.
        CALL_EXT | CALL_EXT_LAST | CALL_EXT_ONLY => {
            import_at(1)?;
            if imports[index_at(1)?].arity as usize != index_at(0)? {
                return Err(LoadError::Malformed("call arity does not match the import"));
            }
        }
        BIF0 => bif_arity(0, 0)?,
        BIF1 => bif_arity(1, 1)?,
        BIF2 => bif_arity(1, 2)?,
        GC_BIF1 => bif_arity(2, 1)?,
        GC_BIF2 => bif_arity(2, 2)?,
        GC_BIF3 => bif_arity(2, 3)?,
        MAKE_FUN3
            if index_at(0)? >= funs.len() => {
                return Err(LoadError::Malformed("fun index out of range"));
            }
        _ => {}
    }
    // String-table references in bs_match `string` commands are checked where they are decoded.
    let _ = strings;
    Ok(())
}

// ---- the compact term encoding ----

const TAG_U: u8 = 0;
const TAG_I: u8 = 1;
const TAG_A: u8 = 2;
const TAG_X: u8 = 3;
const TAG_Y: u8 = 4;
const TAG_F: u8 = 5;
const TAG_H: u8 = 6;
const TAG_Z: u8 = 7;

/// How deeply extended lists may nest in one operand (the compiler never nests them).
const MAX_OPERAND_DEPTH: usize = 2;

struct Compact<'a> {
    bytes: &'a [u8],
    pos: usize,
}

/// A decoded compact value: small ones as `i64`, anything larger (the compiler inlines integers
/// up to 2^128) as a bignum.
enum Value {
    Small(i64),
    Wide(num_bigint::BigInt),
}

impl Compact<'_> {
    fn byte(&mut self) -> Result<u8> {
        let b = *self.bytes.get(self.pos).ok_or(LoadError::Malformed("truncated"))?;
        self.pos += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.pos.checked_add(n).ok_or(LoadError::Malformed("truncated"))?;
        let s = self.bytes.get(self.pos..end).ok_or(LoadError::Malformed("truncated"))?;
        self.pos = end;
        Ok(s)
    }

    fn word(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    /// Read a tag and a value that must fit in `i64`.
    fn tag_and_value(&mut self) -> Result<(u8, i64)> {
        match self.tag_and_wide()? {
            (tag, Value::Small(v)) => Ok((tag, v)),
            _ => Err(LoadError::Malformed("operand too large")),
        }
    }

    /// Read a tag and value. See `encode/2` in the compiler's `beam_asm.erl`: the low three bits
    /// are the tag; values below 16 sit in the high nibble, below 2048 in 3 bits plus a byte,
    /// and larger ones as 2 to 8 (or more) big-endian two's-complement bytes.
    fn tag_and_wide(&mut self) -> Result<(u8, Value)> {
        let b = self.byte()?;
        let tag = b & 7;
        if b & 0x08 == 0 {
            return Ok((tag, Value::Small((b >> 4) as i64)));
        }
        if b & 0x10 == 0 {
            let lo = self.byte()?;
            return Ok((tag, Value::Small((((b as i64) >> 5) << 8) | lo as i64)));
        }
        let n = match b >> 5 {
            7 => {
                let (t, extra) = self.tag_and_value()?;
                if t != TAG_U || !(0..=8).contains(&extra) {
                    // More than 17 bytes is more than the compiler ever writes inline.
                    return Err(LoadError::Malformed("operand too large"));
                }
                extra as usize + 9
            }
            k => k as usize + 2,
        };
        let bytes = self.take(n)?;
        // Big-endian two's complement.
        let v = num_bigint::BigInt::from_signed_bytes_be(bytes);
        Ok((tag, match num_traits::ToPrimitive::to_i64(&v) {
            Some(v) => Value::Small(v),
            None => Value::Wide(v),
        }))
    }

    fn unsigned(&mut self) -> Result<u64> {
        match self.tag_and_value()? {
            (TAG_U, v) if v >= 0 => Ok(v as u64),
            _ => Err(LoadError::Malformed("expected an unsigned operand")),
        }
    }

    fn operand(&mut self, t: &Tables, depth: usize) -> Result<Arg> {
        let (tag, value) = self.tag_and_wide()?;
        let small = |v: &Value| match v {
            Value::Small(v) => Ok(*v),
            Value::Wide(_) => Err(LoadError::Malformed("operand too large")),
        };
        let index = |v: &Value, limit: usize| -> Result<usize> {
            let v = small(v)?;
            usize::try_from(v).ok().filter(|v| *v < limit).ok_or(LoadError::Malformed("register or index out of range"))
        };
        Ok(match tag {
            TAG_U => match value {
                Value::Small(v) => Arg::U(u64::try_from(v).map_err(|_| LoadError::Malformed("negative"))?),
                // Only bs_match patterns carry unsigned values this wide; they are read as numbers.
                Value::Wide(v) if v.sign() != num_bigint::Sign::Minus => Arg::Const(Term::big(v)),
                Value::Wide(_) => return Err(LoadError::Malformed("negative")),
            },
            TAG_I | TAG_H => match value {
                Value::Small(v) => Arg::Const(Term::Int(v)),
                Value::Wide(v) => Arg::Const(Term::big(v)),
            },
            TAG_A => match small(&value)? {
                0 => Arg::Const(Term::Nil),
                i => Arg::Const(Term::Atom(
                    t.atoms.get(i as usize - 1).cloned().ok_or(LoadError::Malformed("atom index out of range"))?,
                )),
            },
            TAG_X => Arg::X(index(&value, X_REGS)? as u16),
            TAG_Y => Arg::Y(index(&value, MAX_Y_REGS)? as u16),
            TAG_F => match small(&value)? {
                0 => Arg::Label(None),
                n => Arg::Label(Some(u32::try_from(n).map_err(|_| LoadError::Malformed("label"))?)),
            },
            TAG_Z => match small(&value)? {
                1 => {
                    if depth >= MAX_OPERAND_DEPTH {
                        return Err(LoadError::Malformed("nested list operand"));
                    }
                    let n = self.unsigned()? as usize;
                    if n > self.bytes.len() - self.pos {
                        return Err(LoadError::Malformed("list operand length"));
                    }
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n {
                        items.push(self.operand(t, depth + 1)?);
                    }
                    Arg::List(items)
                }
                2 => Arg::FloatReg(
                    usize::try_from(self.unsigned()?).ok().filter(|r| *r < FLOAT_REGS).ok_or(LoadError::Malformed("float register"))? as u16,
                ),
                3 => {
                    let n = self.unsigned()?;
                    if n > 3 {
                        return Err(LoadError::Malformed("allocation list"));
                    }
                    for _ in 0..n {
                        let kind = self.unsigned()?;
                        let _amount = self.unsigned()?;
                        if kind > 2 {
                            return Err(LoadError::Malformed("allocation kind"));
                        }
                    }
                    Arg::Alloc
                }
                4 => {
                    let i = self.unsigned()? as usize;
                    Arg::Const(t.literals.get(i).cloned().ok_or(LoadError::Malformed("literal index out of range"))?)
                }
                5 => {
                    // A register annotated with a type for the JIT. The type is only a hint.
                    let reg = self.operand(t, MAX_OPERAND_DEPTH)?;
                    let _type_index = self.unsigned()?;
                    match reg {
                        Arg::X(_) | Arg::Y(_) => reg,
                        _ => return Err(LoadError::Malformed("typed register")),
                    }
                }
                _ => return Err(LoadError::Malformed("extended operand")),
            },
            _ => unreachable!("three-bit tag"),
        })
    }
}

#[allow(dead_code)]
fn describe(e: &LoadError) -> String {
    alloc::format!("{e:?}")
}
