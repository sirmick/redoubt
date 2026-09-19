//! Native functions (BIFs): the functions of `erlang` and friends that are implemented in Rust.
//!
//! Every native is listed in [`TABLE`], so the complete set of things BEAM code can ask the VM to
//! do natively is in one place. A call to a function missing from the table (and from the
//! loaded modules) raises `undef`.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::atom::Atom;
use crate::process::{Exception, Process};
use crate::term::Term;
use crate::vm::System;

mod arith;
mod erlang;
mod lists;
mod maps;
mod proc;

pub use proc::send;

/// Context for a native call: the VM and the calling process.
pub struct Ctx<'a> {
    pub sys: &'a mut System,
    pub p: &'a mut Process,
}

pub type Native = fn(&mut Ctx, &[Term]) -> Result<Term, Exception>;

/// `(module, function, arity, implementation)`.
const TABLE: &[(&str, &str, u32, Native)] = &[
    // Arithmetic and comparison (the operators).
    ("erlang", "+", 2, arith::add),
    ("erlang", "-", 2, arith::sub),
    ("erlang", "*", 2, arith::mul),
    ("erlang", "/", 2, arith::fdiv),
    ("erlang", "div", 2, arith::idiv),
    ("erlang", "rem", 2, arith::rem),
    ("erlang", "-", 1, arith::neg),
    ("erlang", "+", 1, arith::plus),
    ("erlang", "band", 2, arith::band),
    ("erlang", "bor", 2, arith::bor),
    ("erlang", "bxor", 2, arith::bxor),
    ("erlang", "bnot", 1, arith::bnot),
    ("erlang", "bsl", 2, arith::bsl),
    ("erlang", "bsr", 2, arith::bsr),
    ("erlang", "abs", 1, arith::abs),
    ("erlang", "float", 1, arith::float),
    ("erlang", "trunc", 1, arith::trunc),
    ("erlang", "round", 1, arith::round),
    ("erlang", "floor", 1, arith::floor),
    ("erlang", "ceil", 1, arith::ceil),
    ("erlang", "max", 2, arith::max),
    ("erlang", "min", 2, arith::min),
    ("erlang", "==", 2, arith::eq),
    ("erlang", "/=", 2, arith::ne),
    ("erlang", "=:=", 2, arith::eq_exact),
    ("erlang", "=/=", 2, arith::ne_exact),
    ("erlang", "<", 2, arith::lt),
    ("erlang", ">", 2, arith::gt),
    ("erlang", "=<", 2, arith::le),
    ("erlang", ">=", 2, arith::ge),
    ("erlang", "and", 2, arith::and),
    ("erlang", "or", 2, arith::or),
    ("erlang", "xor", 2, arith::xor),
    ("erlang", "not", 1, arith::not),
    // Type tests.
    ("erlang", "is_atom", 1, erlang::is_atom),
    ("erlang", "is_binary", 1, erlang::is_binary),
    ("erlang", "is_bitstring", 1, erlang::is_bitstring),
    ("erlang", "is_boolean", 1, erlang::is_boolean),
    ("erlang", "is_float", 1, erlang::is_float),
    ("erlang", "is_function", 1, erlang::is_function),
    ("erlang", "is_function", 2, erlang::is_function2),
    ("erlang", "is_integer", 1, erlang::is_integer),
    ("erlang", "is_list", 1, erlang::is_list),
    ("erlang", "is_map", 1, erlang::is_map),
    ("erlang", "is_number", 1, erlang::is_number),
    ("erlang", "is_pid", 1, erlang::is_pid),
    ("erlang", "is_port", 1, erlang::is_port),
    ("erlang", "is_reference", 1, erlang::is_reference),
    ("erlang", "is_tuple", 1, erlang::is_tuple),
    // Tuples, lists, binaries, conversions.
    ("erlang", "element", 2, erlang::element),
    ("erlang", "setelement", 3, erlang::setelement),
    ("erlang", "tuple_size", 1, erlang::tuple_size),
    ("erlang", "size", 1, erlang::size),
    ("erlang", "make_tuple", 2, erlang::make_tuple),
    ("erlang", "append_element", 2, erlang::append_element),
    ("erlang", "tuple_to_list", 1, erlang::tuple_to_list),
    ("erlang", "list_to_tuple", 1, erlang::list_to_tuple),
    ("erlang", "hd", 1, erlang::hd),
    ("erlang", "tl", 1, erlang::tl),
    ("erlang", "length", 1, erlang::length),
    ("erlang", "++", 2, erlang::append),
    ("erlang", "--", 2, erlang::subtract),
    ("erlang", "byte_size", 1, erlang::byte_size),
    ("erlang", "bit_size", 1, erlang::bit_size),
    ("erlang", "binary_part", 3, erlang::binary_part),
    ("erlang", "split_binary", 2, erlang::split_binary),
    ("erlang", "atom_to_list", 1, erlang::atom_to_list),
    ("erlang", "atom_to_binary", 1, erlang::atom_to_binary),
    ("erlang", "atom_to_binary", 2, erlang::atom_to_binary),
    ("erlang", "list_to_atom", 1, erlang::list_to_atom),
    ("erlang", "list_to_existing_atom", 1, erlang::list_to_existing_atom),
    ("erlang", "binary_to_atom", 1, erlang::binary_to_atom),
    ("erlang", "binary_to_atom", 2, erlang::binary_to_atom),
    ("erlang", "binary_to_existing_atom", 1, erlang::binary_to_existing_atom),
    ("erlang", "binary_to_existing_atom", 2, erlang::binary_to_existing_atom),
    ("erlang", "integer_to_list", 1, erlang::integer_to_list),
    ("erlang", "integer_to_list", 2, erlang::integer_to_list),
    ("erlang", "integer_to_binary", 1, erlang::integer_to_binary),
    ("erlang", "integer_to_binary", 2, erlang::integer_to_binary),
    ("erlang", "list_to_integer", 1, erlang::list_to_integer),
    ("erlang", "list_to_integer", 2, erlang::list_to_integer),
    ("erlang", "binary_to_integer", 1, erlang::binary_to_integer),
    ("erlang", "binary_to_integer", 2, erlang::binary_to_integer),
    ("erlang", "float_to_list", 1, erlang::float_to_list),
    ("erlang", "float_to_binary", 1, erlang::float_to_binary),
    ("erlang", "binary_to_list", 1, erlang::binary_to_list),
    ("erlang", "binary_to_list", 3, erlang::binary_to_list3),
    ("erlang", "bitstring_to_list", 1, erlang::bitstring_to_list),
    ("erlang", "list_to_binary", 1, erlang::list_to_binary),
    ("erlang", "list_to_bitstring", 1, erlang::list_to_bitstring),
    ("erlang", "iolist_to_binary", 1, erlang::list_to_binary),
    ("erlang", "iolist_size", 1, erlang::iolist_size),
    ("erlang", "display", 1, erlang::display),
    // Maps.
    ("erlang", "map_size", 1, maps::map_size),
    ("erlang", "map_get", 2, maps::get),
    ("erlang", "is_map_key", 2, maps::is_key_rev),
    ("maps", "get", 2, maps::get_rev),
    ("maps", "find", 2, maps::find),
    ("maps", "is_key", 2, maps::is_key),
    ("maps", "put", 3, maps::put),
    ("maps", "remove", 2, maps::remove),
    ("maps", "take", 2, maps::take),
    ("maps", "update", 3, maps::update),
    ("maps", "merge", 2, maps::merge),
    ("maps", "keys", 1, maps::keys),
    ("maps", "values", 1, maps::values),
    ("maps", "to_list", 1, maps::to_list),
    ("maps", "from_list", 1, maps::from_list),
    ("maps", "from_keys", 2, maps::from_keys),
    ("erts_internal", "map_next", 3, maps::map_next),
    // Lists (the ones BEAM implements natively).
    ("lists", "reverse", 2, lists::reverse),
    ("lists", "member", 2, lists::member),
    ("lists", "keyfind", 3, lists::keyfind),
    ("lists", "keymember", 3, lists::keymember),
    ("lists", "keysearch", 3, lists::keysearch),
    // Exceptions.
    ("erlang", "error", 1, proc::error),
    ("erlang", "error", 2, proc::error2),
    ("erlang", "error", 3, proc::error2),
    ("erlang", "exit", 1, proc::exit),
    ("erlang", "throw", 1, proc::throw),
    ("erlang", "raise", 3, proc::raise),
    // Processes.
    ("erlang", "self", 0, proc::self_),
    ("erlang", "node", 0, proc::node),
    ("erlang", "node", 1, proc::node1),
    ("erlang", "make_ref", 0, proc::make_ref),
    ("erlang", "spawn", 3, proc::spawn),
    ("erlang", "spawn_link", 3, proc::spawn_link),
    ("erlang", "spawn", 1, proc::spawn_fun),
    ("erlang", "spawn_link", 1, proc::spawn_link_fun),
    ("erlang", "send", 2, proc::send),
    ("erlang", "!", 2, proc::send),
    ("erlang", "link", 1, proc::link),
    ("erlang", "unlink", 1, proc::unlink),
    ("erlang", "monitor", 2, proc::monitor),
    ("erlang", "demonitor", 1, proc::demonitor),
    ("erlang", "demonitor", 2, proc::demonitor2),
    ("erlang", "exit", 2, proc::exit2),
    ("erlang", "process_flag", 2, proc::process_flag),
    ("erlang", "is_process_alive", 1, proc::is_process_alive),
    ("erlang", "register", 2, proc::register),
    ("erlang", "unregister", 1, proc::unregister),
    ("erlang", "whereis", 1, proc::whereis),
    ("erlang", "put", 2, proc::put),
    ("erlang", "get", 1, proc::get),
    ("erlang", "get", 0, proc::get_all),
    ("erlang", "erase", 1, proc::erase),
    ("erlang", "group_leader", 0, proc::group_leader),
    ("erlang", "monotonic_time", 0, proc::monotonic_time),
    ("erlang", "monotonic_time", 1, proc::monotonic_time1),
    ("erlang", "system_time", 0, proc::system_time),
    ("erlang", "system_time", 1, proc::system_time1),
    ("erlang", "yield", 0, proc::yield_),
    ("erlang", "function_exported", 3, proc::function_exported),
    ("erlang", "module_loaded", 1, proc::module_loaded),
];

/// Function name to the natives of that name, by arity.
type Functions = BTreeMap<&'static str, Vec<(u32, Native)>>;

pub struct Registry {
    by_module: BTreeMap<&'static str, Functions>,
}

impl Registry {
    pub fn new() -> Registry {
        let mut by_module: BTreeMap<&'static str, Functions> = BTreeMap::new();
        for &(m, f, a, n) in TABLE {
            let arities = by_module.entry(m).or_default().entry(f).or_default();
            assert!(arities.iter().all(|(x, _)| *x != a), "{m}:{f}/{a} is listed twice");
            arities.push((a, n));
        }
        Registry { by_module }
    }

    pub fn get(&self, module: &Atom, function: &Atom, arity: u32) -> Option<Native> {
        self.by_module
            .get(module.as_str())?
            .get(function.as_str())?
            .iter()
            .find(|(a, _)| *a == arity)
            .map(|(_, n)| *n)
    }

    /// Every native, for documentation and coverage reports.
    pub fn all() -> impl Iterator<Item = (&'static str, &'static str, u32)> {
        TABLE.iter().map(|&(m, f, a, _)| (m, f, a))
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

// ---- helpers shared by the natives ----

impl Ctx<'_> {
    pub fn badarg(&self) -> Exception {
        Exception::error(Term::Atom(self.sys.atoms.badarg.clone()))
    }

    pub fn badarith(&self) -> Exception {
        Exception::error(Term::Atom(self.sys.atoms.badarith.clone()))
    }

    pub fn system_limit(&self) -> Exception {
        Exception::error(Term::Atom(self.sys.atoms.system_limit.clone()))
    }

    pub fn bool(&self, b: bool) -> Term {
        Term::Atom(if b { self.sys.atoms.true_.clone() } else { self.sys.atoms.false_.clone() })
    }

    pub fn atom(&mut self, name: &str) -> Term {
        Term::Atom(self.sys.atom(name))
    }

    /// An error `{Tag, Value}`, e.g. `{badkey, K}`.
    pub fn error_with(&self, tag: &Atom, value: Term) -> Exception {
        Exception::error(Term::tuple(alloc::vec![Term::Atom(tag.clone()), value]))
    }

    pub fn ok(&self) -> Term {
        Term::Atom(self.sys.atoms.ok.clone())
    }
}
