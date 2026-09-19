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
mod binary;
mod erlang;
mod info;
mod lists;
mod maps;
mod math;
mod proc;
mod unicode;

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
    ("erlang", "iolist_to_binary", 1, erlang::iolist_to_binary),
    ("erlang", "iolist_size", 1, erlang::iolist_size),
    ("erlang", "display", 1, erlang::display),
    ("erts_debug", "flat_size", 1, erlang::flat_size),
    ("erlang", "is_record", 2, erlang::is_record),
    ("erlang", "is_record", 3, erlang::is_record),
    ("erlang", "insert_element", 3, erlang::insert_element),
    ("erlang", "delete_element", 2, erlang::delete_element),
    ("erlang", "float_to_list", 2, erlang::float_to_list2),
    ("erlang", "float_to_binary", 2, erlang::float_to_binary2),
    ("erlang", "list_to_float", 1, erlang::list_to_float),
    ("erlang", "binary_to_float", 1, erlang::binary_to_float),
    ("erlang", "term_to_binary", 1, erlang::term_to_binary),
    ("erlang", "term_to_binary", 2, erlang::term_to_binary),
    ("erlang", "binary_to_term", 1, erlang::binary_to_term),
    ("erlang", "binary_to_term", 2, erlang::binary_to_term),
    // binary (the native half of the OTP module).
    ("binary", "at", 2, binary::at),
    ("binary", "first", 1, binary::first),
    ("binary", "last", 1, binary::last),
    ("binary", "part", 2, binary::part),
    ("binary", "part", 3, binary::part),
    ("binary", "copy", 1, binary::copy),
    ("binary", "copy", 2, binary::copy),
    ("binary", "bin_to_list", 1, binary::bin_to_list),
    ("binary", "bin_to_list", 2, binary::bin_to_list),
    ("binary", "bin_to_list", 3, binary::bin_to_list),
    ("binary", "list_to_bin", 1, binary::list_to_bin),
    ("binary", "encode_unsigned", 1, binary::encode_unsigned),
    ("binary", "encode_unsigned", 2, binary::encode_unsigned),
    ("binary", "decode_unsigned", 1, binary::decode_unsigned),
    ("binary", "decode_unsigned", 2, binary::decode_unsigned),
    ("binary", "compile_pattern", 1, binary::compile_pattern),
    ("binary", "match", 2, binary::match_),
    ("binary", "match", 3, binary::match_),
    ("binary", "matches", 2, binary::matches),
    ("binary", "matches", 3, binary::matches),
    ("binary", "split", 2, binary::split),
    ("binary", "split", 3, binary::split),
    ("binary", "longest_common_prefix", 1, binary::longest_common_prefix),
    ("binary", "longest_common_suffix", 1, binary::longest_common_suffix),
    // math.
    ("math", "sin", 1, math::sin),
    ("math", "cos", 1, math::cos),
    ("math", "tan", 1, math::tan),
    ("math", "asin", 1, math::asin),
    ("math", "acos", 1, math::acos),
    ("math", "atan", 1, math::atan),
    ("math", "atan2", 2, math::atan2),
    ("math", "sinh", 1, math::sinh),
    ("math", "cosh", 1, math::cosh),
    ("math", "tanh", 1, math::tanh),
    ("math", "asinh", 1, math::asinh),
    ("math", "acosh", 1, math::acosh),
    ("math", "atanh", 1, math::atanh),
    ("math", "exp", 1, math::exp),
    ("math", "log", 1, math::log),
    ("math", "log2", 1, math::log2),
    ("math", "log10", 1, math::log10),
    ("math", "pow", 2, math::pow),
    ("math", "sqrt", 1, math::sqrt),
    ("math", "erf", 1, math::erf),
    ("math", "erfc", 1, math::erfc),
    ("math", "floor", 1, math::floor),
    ("math", "ceil", 1, math::ceil),
    ("math", "fmod", 2, math::fmod),
    // unicode (the native half of the OTP module).
    ("unicode", "characters_to_list", 2, unicode::characters_to_list),
    ("unicode", "characters_to_binary", 2, unicode::characters_to_binary),
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
    ("erlang", "spawn_opt", 2, proc::spawn_opt2),
    ("erlang", "spawn_opt", 4, proc::spawn_opt4),
    ("erlang", "system_info", 1, proc::system_info),
    ("erlang", "nif_error", 1, proc::nif_error),
    ("erlang", "nif_error", 2, proc::nif_error),
    ("erlang", "garbage_collect", 0, proc::garbage_collect),
    ("erlang", "erase", 0, proc::erase_all),
    ("erlang", "unique_integer", 0, proc::unique_integer),
    ("erlang", "unique_integer", 1, proc::unique_integer),
    ("erlang", "timestamp", 0, proc::timestamp),
    ("erlang", "make_fun", 3, proc::make_fun),
    ("erlang", "fun_info", 2, proc::fun_info),
    ("erts_internal", "cmp_term", 2, proc::cmp_term),
    // Introspection.
    ("erlang", "processes", 0, info::processes),
    ("erlang", "process_info", 1, info::process_info1),
    ("erlang", "process_info", 2, info::process_info),
    ("erlang", "loaded", 0, info::loaded),
    ("erlang", "get_module_info", 1, info::get_module_info),
    ("erlang", "get_module_info", 2, info::get_module_info),
    ("code", "ensure_loaded", 1, info::ensure_loaded),
    ("code", "is_loaded", 1, info::is_loaded),
    ("code", "all_loaded", 0, info::all_loaded),
    ("erlang", "pid_to_list", 1, info::pid_to_list),
    ("erlang", "list_to_pid", 1, info::list_to_pid),
    ("erlang", "ref_to_list", 1, info::ref_to_list),
    ("erlang", "fun_to_list", 1, info::fun_to_list),
    ("erlang", "display_string", 1, info::display_string),
    ("erlang", "display_string", 2, info::display_string),
    ("erlang", "universaltime", 0, info::universaltime),
    ("erlang", "localtime", 0, info::universaltime),
    ("erlang", "crc32", 1, info::crc32),
    ("erlang", "crc32", 2, info::crc32),
    ("os", "system_time", 0, proc::system_time),
    ("os", "system_time", 1, proc::system_time1),
    ("os", "timestamp", 0, proc::timestamp),
    ("erlang", "garbage_collect", 1, proc::garbage_collect),
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
