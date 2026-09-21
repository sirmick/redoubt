//! Atoms, interned per VM.
//!
//! An [`Atom`] is a handle to an interned name. Interning makes equality a pointer comparison;
//! ordering compares text, as Erlang's term order requires. Atoms are never freed (as in BEAM),
//! so a name is allocated once and leaked, which makes an atom a `Copy` value that can live in a
//! term anywhere, with no table at hand to read its name. The table is bounded: creating atoms
//! from untrusted data is a classic way to exhaust a BEAM node, and here it fails with
//! `system_limit` instead.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use core::fmt;

/// Most atoms any one VM may hold. Stock BEAM defaults to 1,048,576.
pub const MAX_ATOMS: usize = 1 << 20;
/// Longest atom in characters, as in stock BEAM.
pub const MAX_ATOM_CHARS: usize = 255;

/// A thin pointer (to a `String`, not the two-word `str`), which keeps `Term` two words.
#[derive(Clone, Copy)]
pub struct Atom(&'static String);

impl Atom {
    pub fn as_str(&self) -> &str {
        self.0
    }

    /// An atom outside any table, for unit tests.
    #[cfg(test)]
    pub fn test(name: &str) -> Atom {
        Atom(Box::leak(Box::new(String::from(name))))
    }

    /// A number that identifies this atom (its allocation: atoms are interned and never freed),
    /// for cheap keys.
    pub fn id(&self) -> usize {
        self.0 as *const String as usize
    }
}

impl PartialEq for Atom {
    fn eq(&self, other: &Self) -> bool {
        // Every Atom comes from one interner, so equal text means the same allocation.
        core::ptr::eq(self.0, other.0)
    }
}
impl Eq for Atom {}

impl fmt::Debug for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomError {
    /// Longer than [`MAX_ATOM_CHARS`].
    TooLong,
    /// The table already holds [`MAX_ATOMS`].
    TableFull,
}

pub struct AtomTable {
    by_name: BTreeMap<&'static str, Atom>,
}

impl AtomTable {
    pub fn new() -> AtomTable {
        AtomTable {
            by_name: BTreeMap::new(),
        }
    }

    /// The atom named `name`, creating it if needed.
    pub fn intern(&mut self, name: &str) -> Result<Atom, AtomError> {
        if let Some(a) = self.by_name.get(name) {
            return Ok(*a);
        }
        if name.chars().count() > MAX_ATOM_CHARS {
            return Err(AtomError::TooLong);
        }
        if self.by_name.len() >= MAX_ATOMS {
            return Err(AtomError::TableFull);
        }
        let name: &'static String = Box::leak(Box::new(String::from(name)));
        let atom = Atom(name);
        self.by_name.insert(name.as_str(), atom);
        Ok(atom)
    }

    /// The atom named `name` if it already exists (`list_to_existing_atom`).
    pub fn existing(&self, name: &str) -> Option<Atom> {
        self.by_name.get(name).copied()
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

impl Default for AtomTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Declares [`Atoms`]: the atoms the VM itself refers to, interned once at start-up.
macro_rules! common_atoms {
    ($($field:ident = $name:literal),* $(,)?) => {
        /// Atoms the VM refers to by name, so Rust code never interns in a hot path.
        #[derive(Clone)]
        pub struct Atoms {
            $(pub $field: Atom,)*
        }

        impl Atoms {
            pub fn new(table: &mut AtomTable) -> Atoms {
                Atoms {
                    $($field: table.intern($name).expect("built-in atoms fit"),)*
                }
            }
        }
    };
}

common_atoms! {
    true_ = "true",
    false_ = "false",
    undefined = "undefined",
    ok = "ok",
    error = "error",
    exit = "exit",
    throw = "throw",
    normal = "normal",
    kill = "kill",
    killed = "killed",
    noproc = "noproc",
    nocatch = "nocatch",
    badarg = "badarg",
    badarith = "badarith",
    badarity = "badarity",
    badfun = "badfun",
    badmatch = "badmatch",
    badmap = "badmap",
    badkey = "badkey",
    badrecord = "badrecord",
    case_clause = "case_clause",
    if_clause = "if_clause",
    try_clause = "try_clause",
    function_clause = "function_clause",
    undef = "undef",
    system_limit = "system_limit",
    timeout_value = "timeout_value",
    infinity = "infinity",
    exit_upper = "EXIT",
    down = "DOWN",
    process = "process",
    trap_exit = "trap_exit",
    erlang = "erlang",
    module = "module",
    all = "all",
    latin1 = "latin1",
    unicode = "unicode",
    utf8 = "utf8",
    big = "big",
    little = "little",
    native = "native",
    signed = "signed",
    unsigned = "unsigned",
    integer = "integer",
    float = "float",
    binary = "binary",
    bits = "bits",
    utf16 = "utf16",
    utf32 = "utf32",
    string = "string",
    skip = "skip",
    get_tail = "get_tail",
    ensure_at_least = "ensure_at_least",
    ensure_exactly = "ensure_exactly",
    append = "append",
    private_append = "private_append",
    info = "info",
    file = "file",
    line = "line",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_gives_identity() {
        let mut t = AtomTable::new();
        let a = t.intern("hello").unwrap();
        let b = t.intern("hello").unwrap();
        let c = t.intern("world").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(t.existing("world"), Some(c));
        assert_eq!(t.existing("nope"), None);
    }

    #[test]
    fn length_limit_counts_characters() {
        let mut t = AtomTable::new();
        assert!(t.intern(&"a".repeat(255)).is_ok());
        assert_eq!(t.intern(&"a".repeat(256)), Err(AtomError::TooLong));
        // 255 two-byte characters are still 255 characters.
        assert!(t.intern(&"é".repeat(255)).is_ok());
    }
}
