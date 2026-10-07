//! A loaded module: its tables and its code, decoded into [`Instr`]s over one array of operands.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::atom::Atom;
use crate::term::Term;

/// An external function a module calls: `module:function/arity`.
#[derive(Clone)]
pub struct Import {
    pub module: Atom,
    pub function: Atom,
    pub arity: u32,
    /// The native implementing it, if there is one; resolved once when the module is loaded
    /// (natives never change, so this cannot go stale).
    pub native: Option<crate::bif::Native>,
}

impl core::fmt::Debug for Import {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}/{}", self.module.as_str(), self.function.as_str(), self.arity)
    }
}

#[derive(Clone, Debug)]
pub struct Export {
    pub function: Atom,
    pub arity: u32,
    /// Index into [`Module::code`] of the function's entry.
    pub entry: u32,
}

/// An entry of the fun table: the code behind one `fun` expression.
#[derive(Clone, Debug)]
pub struct FunEntry {
    pub function: Atom,
    /// Arity of the implementing function: the fun's own arity plus its free variables.
    pub arity: u32,
    pub entry: u32,
    pub num_free: u32,
    /// The compiler's hash of the fun's code, shown in `#Fun<Module.Index.Uniq>`.
    pub uniq: u32,
}

/// Where a function starts, for error reports and stack traces.
#[derive(Clone, Debug)]
pub struct FunctionInfo {
    /// Index of its `func_info` instruction.
    pub start: u32,
    pub name: Atom,
    pub arity: u32,
}

/// An instruction operand.
#[derive(Clone, Copy, Debug)]
pub enum Arg {
    X(u16),
    Y(u16),
    FloatReg(u16),
    /// A constant: an integer, atom, `[]` or a literal from the literal table.
    Const(Term),
    /// An unsigned number (an arity, a count, a table index, flags).
    U(u64),
    /// A jump target as an index into the code, or `None` for "no label" (raise instead).
    Label(Option<u32>),
    /// The items at `start..start + len` of [`Module::operands`].
    List {
        start: u32,
        len: u32,
    },
    /// A heap allocation hint. This VM has no heap to reserve, so it is only validated.
    Alloc,
}

/// A pseudo-opcode no `.beam` file can contain (the loader accepts only up to
/// [`crate::opcodes::MAX_OPCODE`]): "this function's body is a native", operand 0 indexing
/// [`Module::body_natives`]. Put in place at load time, like `erlang:load_nif/2`.
pub const NATIVE_BODY: u8 = 255;

/// A decoded instruction: its operands are `count` entries of [`Module::operands`] from
/// `start`, in the order of the compiler's `genop.tab`.
#[derive(Clone, Copy, Debug)]
pub struct Instr {
    pub op: u8,
    /// At most 8 (`genop.tab`'s largest arity).
    pub count: u8,
    pub start: u32,
}

// The code's size, on either width (docs/userland/beamlet.md, "What the VM holds at its prompt").
const _: () = assert!(size_of::<Instr>() == 8 && size_of::<Arg>() == 16);

/// An instruction as the interpreter reads it: the one way to its operands.
#[derive(Clone, Copy)]
pub struct InstrView<'a> {
    pub op: u8,
    args: &'a [Arg],
    operands: &'a [Arg],
}

impl<'a> InstrView<'a> {
    /// `ins`, of the module whose operands are `operands`.
    pub fn new(ins: &Instr, operands: &'a [Arg]) -> Option<Self> {
        let args = operands.get(ins.start as usize..)?.get(..ins.count as usize)?;
        Some(InstrView { op: ins.op, args, operands })
    }

    /// Operand `i`.
    pub fn arg(&self, i: usize) -> Option<&'a Arg> { self.args.get(i) }

    pub fn count(&self) -> usize { self.args.len() }

    /// The items of an [`Arg::List`].
    pub fn items(&self, start: u32, len: u32) -> Option<&'a [Arg]> {
        self.operands.get(start as usize..)?.get(..len as usize)
    }
}

pub struct Module {
    pub name: Atom,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub funs: Vec<FunEntry>,
    pub literals: Vec<Term>,
    pub strings: Vec<u8>,
    pub code: Box<[Instr]>,
    /// Every instruction's operands, in code order; the items of an instruction's lists follow
    /// its own operands.
    pub operands: Box<[Arg]>,
    /// Functions in code order.
    pub functions: Vec<FunctionInfo>,
    /// Source locations, for stack traces. See [`Module::location`].
    pub lines: Lines,
    /// Natives that replace function bodies (see [`NATIVE_BODY`]): native, name, arity.
    pub body_natives: Vec<(crate::bif::Native, Atom, u32)>,
    /// The `Attr` and `CInf` chunks (external term format), for `module_info/1`.
    pub attributes: Vec<u8>,
    pub compile_info: Vec<u8>,
    /// BEAM's module checksum (`module_info(md5)`), which also identifies its funs in the
    /// external term format.
    pub md5: [u8; 16],
}

/// The `Line` chunk: which source line each `line` instruction marks.
#[derive(Default)]
pub struct Lines {
    /// File names; index 0 is the implicit `<module>.erl`.
    pub files: Vec<Term>,
    /// Location items: `(file index, line)`. Item 0 is "no location".
    pub items: Vec<(u32, u32)>,
    /// `(code index, item)` of every `line` instruction, in code order.
    pub marks: Vec<(u32, u32)>,
}

impl Module {
    /// The function named by `-on_load(F/0)`: the compiler marks its body with an `on_load`
    /// instruction.
    pub fn on_load(&self) -> Option<Atom> { self.on_load_entry().map(|(name, _)| name) }

    /// The on_load function's name and entry (the label after its `func_info`).
    pub fn on_load_entry(&self) -> Option<(Atom, u32)> {
        let pc = self.code.iter().position(|i| i.op == crate::opcodes::ON_LOAD)?;
        let f = self.function_at(pc as u32).filter(|f| f.arity == 0)?;
        Some((f.name, f.start + 1))
    }

    /// Instruction `pc` and its operands.
    pub fn instr(&self, pc: u32) -> Option<InstrView<'_>> {
        InstrView::new(self.code.get(pc as usize)?, &self.operands)
    }

    /// Replace the `label` at `entry` with [`NATIVE_BODY`], calling `native`; nothing if
    /// `entry` is not a `label`.
    pub fn replace_body(&mut self, entry: usize, native: (crate::bif::Native, Atom, u32)) {
        let Some(ins) = self.code.get_mut(entry).filter(|i| i.op == crate::opcodes::LABEL) else {
            return;
        };
        // A `label`'s one operand, its number, becomes the native's index.
        if let Some(arg) = self.operands.get_mut(ins.start as usize) {
            ins.op = NATIVE_BODY;
            *arg = Arg::U(self.body_natives.len() as u64);
            self.body_natives.push(native);
        }
    }

    pub fn export(&self, function: &Atom, arity: u32) -> Option<u32> {
        self.exports.iter().find(|e| &e.function == function && e.arity == arity).map(|e| e.entry)
    }

    /// The source location of code index `pc`: the last `line` instruction at or before it.
    pub fn location(&self, pc: u32) -> Option<(&Term, u32)> {
        let i = self.lines.marks.partition_point(|(at, _)| *at <= pc).checked_sub(1)?;
        let item = self.lines.marks[i].1 as usize;
        if item == 0 {
            return None;
        }
        let &(file, line) = self.lines.items.get(item)?;
        Some((self.lines.files.get(file as usize)?, line))
    }

    /// The function containing code index `pc`.
    pub fn function_at(&self, pc: u32) -> Option<&FunctionInfo> {
        let i = self.functions.partition_point(|f| f.start <= pc);
        i.checked_sub(1).map(|i| &self.functions[i])
    }
}
