//! A loaded module: its tables and its code, decoded into [`Instr`]s.

use alloc::vec::Vec;

use crate::atom::Atom;
use crate::term::Term;

/// An external function a module calls: `module:function/arity`.
#[derive(Clone, Debug)]
pub struct Import {
    pub module: Atom,
    pub function: Atom,
    pub arity: u32,
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
#[derive(Clone, Debug)]
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
    List(Vec<Arg>),
    /// A heap allocation hint. This VM has no heap to reserve, so it is only validated.
    Alloc,
}

/// A decoded instruction. Operands are in the order of the compiler's `genop.tab`.
#[derive(Clone, Debug)]
pub struct Instr {
    pub op: u8,
    pub args: Vec<Arg>,
}

pub struct Module {
    pub name: Atom,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub funs: Vec<FunEntry>,
    pub literals: Vec<Term>,
    pub strings: Vec<u8>,
    pub code: Vec<Instr>,
    /// Functions in code order.
    pub functions: Vec<FunctionInfo>,
}

impl Module {
    pub fn export(&self, function: &Atom, arity: u32) -> Option<u32> {
        self.exports
            .iter()
            .find(|e| &e.function == function && e.arity == arity)
            .map(|e| e.entry)
    }

    /// The function containing code index `pc`.
    pub fn function_at(&self, pc: u32) -> Option<&FunctionInfo> {
        let i = self.functions.partition_point(|f| f.start <= pc);
        i.checked_sub(1).map(|i| &self.functions[i])
    }
}
