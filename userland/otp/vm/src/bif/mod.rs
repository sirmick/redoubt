//! Native functions (BIFs): the functions of `erlang` and friends that are implemented in Rust.
//!
//! Every native is listed in [`TABLE`], so the complete set of things BEAM code can ask the VM to
//! do natively is in one place. A call to a function missing from the table (and from the
//! loaded modules) raises `undef`.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::atom::{Atom, Atoms};
use crate::process::{Exception, Process};
use crate::sched::{Sched, SysGuard};
use crate::term::{Bits, Heap, OwnedTerm, Term};

mod arith;
mod atomics;
mod binary;
mod code;
mod erlang;
mod ets;
mod file;
pub(crate) use file::read_whole_file;
pub(crate) use info::load_binary;
mod info;
mod lists;
mod maps;
mod math;
mod phash;
pub(crate) mod port;
mod proc;
mod unicode;
mod zlib;

pub use proc::send;

/// A resource a native holds while it works: the `Arc` keeps it alive, so the caller's heap is
/// free for building the result.
pub struct Held<T> {
    r: alloc::sync::Arc<crate::term::Resource>,
    _t: core::marker::PhantomData<T>,
}

impl<T: 'static> core::ops::Deref for Held<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.r.get::<T>().expect("checked when held")
    }
}

impl<T> Held<T> {
    /// The resource's id (the reference it is to Erlang code).
    pub fn id(&self) -> u64 {
        self.r.id
    }
}

/// Context for a native call: the calling process, and the VM.
pub struct Ctx<'a> {
    pub p: &'a mut Process,
    /// The atoms the VM names. Reading them takes no lock.
    pub atoms: &'a Atoms,
    sched: &'a Sched<'a>,
}

impl<'a> Ctx<'a> {
    pub fn new(sched: &'a Sched<'a>, p: &'a mut Process) -> Ctx<'a> {
        Ctx {
            p,
            atoms: &sched.atoms,
            sched,
        }
    }

    /// The VM's shared state, locked until the guard is dropped (for a temporary, the end of
    /// the statement). A native that never calls this runs without the lock, in parallel with
    /// other schedulers. Never call it while holding its guard: that is a bug, and panics.
    pub fn sys(&self) -> SysGuard<'a> {
        self.sched.lock()
    }

    /// The platform, locked until the guard is dropped. Never take the system lock
    /// (`c.sys()`) while holding it.
    pub fn platform(
        &self,
    ) -> crate::sync::Guard<'a, alloc::boxed::Box<dyn crate::platform::Platform>> {
        self.sched.platform()
    }
}

pub type Native = fn(&mut Ctx, &[Term]) -> Result<Term, Exception>;

/// A native an embedder adds: `(module, function, arity, implementation)`.
pub type NativeSpec = (&'static str, &'static str, u32, Native);

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
    (
        "erlang",
        "list_to_existing_atom",
        1,
        erlang::list_to_existing_atom,
    ),
    ("erlang", "binary_to_atom", 1, erlang::binary_to_atom),
    ("erlang", "binary_to_atom", 2, erlang::binary_to_atom),
    (
        "erlang",
        "binary_to_existing_atom",
        1,
        erlang::binary_to_existing_atom,
    ),
    (
        "erlang",
        "binary_to_existing_atom",
        2,
        erlang::binary_to_existing_atom,
    ),
    ("erlang", "integer_to_list", 1, erlang::integer_to_list),
    ("erlang", "integer_to_list", 2, erlang::integer_to_list),
    ("erlang", "integer_to_binary", 1, erlang::integer_to_binary),
    ("erlang", "integer_to_binary", 2, erlang::integer_to_binary),
    ("erlang", "list_to_integer", 1, erlang::list_to_integer),
    ("erlang", "list_to_integer", 2, erlang::list_to_integer),
    (
        "erts_internal",
        "list_to_integer",
        2,
        erlang::internal_list_to_integer,
    ),
    (
        "erts_internal",
        "binary_to_integer",
        2,
        erlang::internal_binary_to_integer,
    ),
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
    ("erlang", "iolist_to_iovec", 1, erlang::iolist_to_iovec),
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
    (
        "binary",
        "longest_common_prefix",
        1,
        binary::longest_common_prefix,
    ),
    (
        "binary",
        "longest_common_suffix",
        1,
        binary::longest_common_suffix,
    ),
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
    (
        "unicode",
        "characters_to_list",
        2,
        unicode::characters_to_list,
    ),
    (
        "unicode",
        "characters_to_binary",
        2,
        unicode::characters_to_binary,
    ),
    ("unicode", "bin_is_7bit", 1, unicode::bin_is_7bit),
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
    ("erlang", "send", 3, proc::send3),
    ("erlang", "!", 2, proc::send),
    ("erlang", "link", 1, proc::link),
    ("erlang", "unlink", 1, proc::unlink),
    ("erlang", "monitor", 2, proc::monitor),
    ("erlang", "monitor", 3, proc::monitor3),
    ("erlang", "alias", 0, proc::alias),
    ("erlang", "alias", 1, proc::alias),
    ("erlang", "unalias", 1, proc::unalias),
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
    ("erlang", "group_leader", 2, proc::set_group_leader),
    ("net_kernel", "dflag_unicode_io", 1, proc::dflag_unicode_io),
    // There is no distribution: this node is never alive and no other node ever connects, so
    // subscriptions to node events are accepted and never fire.
    ("net_kernel", "monitor_nodes", 1, proc::ok_any),
    ("net_kernel", "monitor_nodes", 2, proc::ok_any),
    ("erlang", "nodes", 0, proc::nil_any),
    ("erlang", "nodes", 1, proc::nil_any),
    ("erlang", "nodes", 2, proc::nil_any),
    ("erlang", "is_alive", 0, proc::false_1),
    // There are no ports: the VM runs no OS processes or drivers.
    ("erlang", "ports", 0, port::ports),
    ("erlang", "open_port", 2, port::open_port),
    ("erlang", "port_command", 2, port::port_command),
    ("erlang", "port_command", 3, port::port_command3),
    ("erlang", "port_close", 1, port::port_close),
    ("erlang", "port_connect", 2, port::port_connect),
    ("erlang", "port_info", 1, port::port_info1),
    ("erlang", "port_info", 2, port::port_info2),
    ("erlang", "port_control", 3, port::no_driver),
    ("erlang", "port_call", 2, port::no_driver),
    ("erlang", "port_call", 3, port::no_driver),
    ("erlang", "port_to_list", 1, port::port_to_list),
    ("erlang", "list_to_port", 1, port::list_to_port),
    ("erlang", "monitor_node", 2, proc::monitor_node),
    ("erlang", "monitor_node", 3, proc::monitor_node),
    ("io", "printable_range", 0, proc::printable_range),
    ("erlang", "start_timer", 3, proc::start_timer),
    ("erlang", "start_timer", 4, proc::start_timer),
    ("erlang", "send_after", 3, proc::send_after),
    ("erlang", "send_after", 4, proc::send_after),
    ("erlang", "cancel_timer", 1, proc::cancel_timer),
    ("erlang", "cancel_timer", 2, proc::cancel_timer),
    ("erlang", "read_timer", 1, proc::read_timer),
    ("erlang", "read_timer", 2, proc::read_timer),
    ("persistent_term", "put", 2, proc::pt_put),
    ("persistent_term", "get", 1, proc::pt_get),
    ("persistent_term", "get", 2, proc::pt_get),
    ("persistent_term", "get", 0, proc::pt_get_all),
    ("persistent_term", "erase", 1, proc::pt_erase),
    ("erlang", "monotonic_time", 0, proc::monotonic_time),
    ("erlang", "monotonic_time", 1, proc::monotonic_time1),
    ("erlang", "system_time", 0, proc::system_time),
    ("erlang", "system_time", 1, proc::system_time1),
    ("erlang", "yield", 0, proc::yield_),
    ("erlang", "function_exported", 3, proc::function_exported),
    ("erlang", "module_loaded", 1, proc::module_loaded),
    ("erlang", "spawn_opt", 2, proc::spawn_opt2),
    ("erlang", "spawn_monitor", 1, proc::spawn_monitor1),
    ("erlang", "spawn_monitor", 3, proc::spawn_monitor3),
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
    ("erlang", "fun_info_mfa", 1, proc::fun_info_mfa),
    ("erts_internal", "cmp_term", 2, proc::cmp_term),
    ("erts_internal", "time_unit", 0, proc::time_unit),
    ("erts_internal", "perf_counter_unit", 0, proc::time_unit),
    // ets (the native half of the OTP module).
    ("ets", "new", 2, ets::new),
    ("ets", "insert", 2, ets::insert),
    ("ets", "insert_new", 2, ets::insert_new),
    ("ets", "lookup", 2, ets::lookup),
    ("ets", "lookup_element", 3, ets::lookup_element),
    ("ets", "lookup_element", 4, ets::lookup_element),
    ("ets", "member", 2, ets::member),
    ("ets", "delete", 1, ets::delete),
    ("ets", "delete", 2, ets::delete),
    ("ets", "delete_object", 2, ets::delete_object),
    ("ets", "take", 2, ets::take),
    ("ets", "internal_delete_all", 2, ets::internal_delete_all),
    ("ets", "update_counter", 3, ets::update_counter),
    ("ets", "update_counter", 4, ets::update_counter),
    ("ets", "update_element", 3, ets::update_element),
    ("ets", "first", 1, ets::first),
    ("ets", "last", 1, ets::last),
    ("ets", "next", 2, ets::next),
    ("ets", "prev", 2, ets::prev),
    ("ets", "first_lookup", 1, ets::first_lookup),
    ("ets", "last_lookup", 1, ets::last_lookup),
    ("ets", "next_lookup", 2, ets::next_lookup),
    ("ets", "prev_lookup", 2, ets::prev_lookup),
    ("ets", "match", 2, ets::match_),
    ("ets", "match_object", 2, ets::match_object),
    ("ets", "select", 1, ets::select1),
    ("ets", "select", 2, ets::select),
    ("ets", "select", 3, ets::select3),
    ("ets", "select_reverse", 2, ets::select_reverse),
    ("ets", "select_reverse", 3, ets::select_reverse3),
    ("ets", "select_reverse", 1, ets::select1),
    ("ets", "match", 3, ets::match3),
    ("ets", "match", 1, ets::select1),
    ("ets", "match_object", 3, ets::match_object3),
    ("ets", "match_object", 1, ets::select1),
    ("ets", "select_count", 2, ets::select_count),
    ("ets", "select_replace", 2, ets::select_replace),
    (
        "ets",
        "internal_select_delete",
        2,
        ets::internal_select_delete,
    ),
    ("ets", "match_spec_compile", 1, ets::match_spec_compile),
    ("ets", "is_compiled_ms", 1, ets::is_compiled_ms),
    ("ets", "match_spec_run_r", 3, ets::match_spec_run_r),
    ("ets", "info", 1, ets::info),
    ("ets", "info", 2, ets::info),
    ("ets", "whereis", 1, ets::whereis),
    ("ets", "rename", 2, ets::rename),
    ("ets", "give_away", 3, ets::give_away),
    ("ets", "setopts", 2, ets::setopts),
    ("ets", "safe_fixtable", 2, ets::safe_fixtable),
    ("ets", "all", 0, ets::all),
    // Introspection.
    ("erlang", "processes", 0, info::processes),
    ("erlang", "process_info", 1, info::process_info1),
    ("erlang", "process_info", 2, info::process_info),
    (
        "file",
        "native_name_encoding",
        0,
        file::native_name_encoding,
    ),
    // The console is not a terminal (no line editing, no ANSI colours) until a platform says so.
    ("prim_tty", "isatty", 1, proc::false_1),
    // Dynamic-trace tags, as BEAM built without VM probes has them.
    ("erlang", "dt_spread_tag", 1, erlang::dt_true),
    ("erlang", "dt_restore_tag", 1, erlang::dt_true),
    ("erlang", "dt_get_tag", 0, erlang::dt_undefined),
    ("erlang", "dt_get_tag_data", 0, erlang::dt_undefined),
    ("erlang", "dt_put_tag", 1, erlang::dt_undefined),
    ("erlang", "dt_prepend_vm_tag_data", 1, erlang::dt_same),
    ("erlang", "dt_append_vm_tag_data", 1, erlang::dt_same),
    ("prim_file", "internal_name2native", 1, file::name2native),
    ("prim_file", "internal_native2name", 1, file::native2name),
    (
        "prim_file",
        "internal_normalize_utf8",
        1,
        file::normalize_utf8,
    ),
    ("prim_file", "is_translatable", 1, file::is_translatable),
    ("prim_file", "open_nif", 2, file::open),
    ("prim_file", "close_nif", 1, file::close),
    ("prim_file", "delayed_close_nif", 1, file::close),
    ("prim_file", "read_nif", 2, file::read),
    ("prim_file", "write_nif", 2, file::write),
    ("prim_file", "pread_nif", 3, file::pread),
    ("prim_file", "pwrite_nif", 3, file::pwrite),
    ("prim_file", "seek_nif", 3, file::seek),
    ("prim_file", "sync_nif", 2, file::sync),
    ("prim_file", "truncate_nif", 1, file::truncate),
    ("prim_file", "advise_nif", 4, file::advise),
    ("prim_file", "allocate_nif", 3, file::not_supported),
    (
        "prim_file",
        "read_handle_info_nif",
        1,
        file::read_handle_info,
    ),
    ("prim_file", "read_file_nif", 1, file::read_file),
    ("prim_file", "read_info_nif", 2, file::read_info),
    ("prim_file", "list_dir_nif", 1, file::list_dir),
    ("prim_file", "make_dir_nif", 1, file::make_dir),
    ("prim_file", "del_file_nif", 1, file::del_file),
    ("prim_file", "del_dir_nif", 1, file::del_dir),
    ("prim_file", "rename_nif", 2, file::rename),
    ("prim_file", "read_link_nif", 1, file::read_link),
    ("prim_file", "get_cwd_nif", 0, file::get_cwd),
    ("prim_file", "set_cwd_nif", 1, file::set_cwd),
    ("prim_file", "get_device_cwd_nif", 1, file::not_supported),
    ("prim_file", "make_hard_link_nif", 2, file::make_link),
    ("prim_file", "make_soft_link_nif", 2, file::make_symlink),
    ("prim_file", "set_owner_nif", 3, file::not_supported),
    ("prim_file", "set_permissions_nif", 2, file::set_permissions),
    ("prim_file", "set_time_nif", 4, file::set_time),
    ("prim_file", "altname_nif", 1, file::not_supported),
    ("prim_file", "get_handle_nif", 1, file::not_supported),
    ("prim_file", "file_desc_to_ref_nif", 1, file::not_supported),
    (
        "prim_file",
        "ipread_s32bu_p32bu_nif",
        3,
        file::not_supported,
    ),
    ("prim_buffer", "new", 0, file::buffer_new),
    ("prim_buffer", "size", 1, file::buffer_size),
    ("prim_buffer", "peek_head", 1, file::buffer_peek_head),
    ("prim_buffer", "copying_read", 2, file::buffer_copying_read),
    ("prim_buffer", "write", 2, file::buffer_write),
    ("prim_buffer", "skip", 2, file::buffer_skip),
    (
        "prim_buffer",
        "find_byte_index",
        2,
        file::buffer_find_byte_index,
    ),
    ("prim_buffer", "try_lock", 1, file::buffer_try_lock),
    ("prim_buffer", "unlock", 1, file::buffer_unlock),
    ("erlang", "phash2", 1, phash::phash2_1),
    ("erlang", "phash2", 2, phash::phash2_2),
    ("erts_internal", "atomics_new", 2, atomics::atomics_new),
    ("atomics", "put", 3, atomics::put),
    ("atomics", "get", 2, atomics::get),
    ("atomics", "add", 3, atomics::add),
    ("atomics", "add_get", 3, atomics::add_get),
    ("atomics", "exchange", 3, atomics::exchange),
    ("atomics", "compare_exchange", 4, atomics::compare_exchange),
    ("atomics", "info", 1, atomics::atomics_info),
    ("erts_internal", "counters_new", 1, atomics::counters_new),
    ("erts_internal", "counters_get", 2, atomics::get),
    ("erts_internal", "counters_add", 3, atomics::add),
    ("erts_internal", "counters_put", 3, atomics::put),
    ("erts_internal", "counters_info", 1, atomics::counters_info),
    ("erlang", "halt", 0, proc::halt),
    ("erlang", "halt", 1, proc::halt),
    ("erlang", "halt", 2, proc::halt),
    ("erlang", "statistics", 1, proc::statistics),
    ("erlang", "system_flag", 2, proc::system_flag),
    ("erlang", "registered", 0, proc::registered),
    ("erlang", "get_keys", 0, proc::get_keys),
    ("erlang", "get_keys", 1, proc::get_keys),
    ("erlang", "now", 0, proc::now),
    ("erlang", "time_offset", 0, proc::time_offset),
    ("erlang", "time_offset", 1, proc::time_offset),
    ("erlang", "bump_reductions", 1, proc::bump_reductions),
    ("erlang", "link", 2, proc::link2),
    ("erlang", "pre_loaded", 0, proc::pre_loaded),
    ("erlang", "append", 2, erlang::append),
    ("erlang", "subtract", 2, erlang::subtract),
    ("erlang", "binary_part", 2, erlang::binary_part2),
    ("erlang", "make_tuple", 3, erlang::make_tuple3),
    ("erlang", "term_to_iovec", 1, erlang::term_to_iovec),
    ("erlang", "term_to_iovec", 2, erlang::term_to_iovec),
    ("erlang", "external_size", 1, erlang::external_size),
    ("erlang", "external_size", 2, erlang::external_size),
    ("erlang", "md5", 1, info::md5),
    ("erlang", "md5_init", 0, info::md5_init),
    ("erlang", "md5_update", 2, info::md5_update),
    ("erlang", "md5_final", 1, info::md5_final),
    ("erlang", "adler32", 1, info::adler32),
    ("erlang", "adler32", 2, info::adler32),
    ("erlang", "adler32_combine", 3, info::adler32_combine),
    ("erlang", "crc32_combine", 3, info::crc32_combine),
    ("erts_internal", "garbage_collect", 1, proc::garbage_collect),
    ("persistent_term", "put_new", 2, proc::pt_put_new),
    ("persistent_term", "info", 0, proc::pt_info),
    ("os", "getpid", 0, info::os_getpid),
    // The platform delivers no OS signals; handlers can be registered and never fire.
    ("os", "set_signal", 2, proc::ok_2),
    ("os", "env", 0, info::os_env),
    ("os", "perf_counter", 0, proc::monotonic_time),
    ("string", "list_to_float", 1, erlang::string_list_to_float),
    (
        "binary",
        "referenced_byte_size",
        1,
        info::referenced_byte_size,
    ),
    ("zlib", "open_nif", 0, zlib::open),
    ("zlib", "close_nif", 1, zlib::close),
    ("zlib", "set_controller_nif", 2, zlib::set_controller),
    ("zlib", "deflateInit_nif", 6, zlib::deflate_init),
    ("zlib", "deflateSetDictionary_nif", 2, zlib::not_supported),
    ("zlib", "deflateReset_nif", 1, zlib::reset),
    ("zlib", "deflateEnd_nif", 1, zlib::deflate_end),
    ("zlib", "deflateParams_nif", 3, zlib::deflate_params),
    ("zlib", "deflate_nif", 4, zlib::deflate),
    ("zlib", "inflateInit_nif", 3, zlib::inflate_init),
    ("zlib", "inflateSetDictionary_nif", 2, zlib::not_supported),
    ("zlib", "inflateGetDictionary_nif", 1, zlib::not_supported),
    ("zlib", "inflateReset_nif", 1, zlib::reset),
    ("zlib", "inflateEnd_nif", 1, zlib::inflate_end),
    ("zlib", "inflate_nif", 4, zlib::inflate_nif),
    ("zlib", "getStash_nif", 1, zlib::get_stash),
    ("zlib", "clearStash_nif", 1, zlib::clear_stash),
    ("zlib", "setStash_nif", 2, zlib::set_stash),
    ("zlib", "enqueue_nif", 2, zlib::enqueue),
    ("erlang", "memory", 0, info::memory0),
    ("erlang", "memory", 1, info::memory1),
    ("erlang", "loaded", 0, info::loaded),
    ("erlang", "get_module_info", 1, info::get_module_info),
    ("erlang", "get_module_info", 2, info::get_module_info),
    ("code", "ensure_loaded", 1, info::ensure_loaded),
    ("code", "is_loaded", 1, info::is_loaded),
    ("code", "all_loaded", 0, info::all_loaded),
    ("code", "load_binary", 3, info::load_binary),
    ("code", "delete", 1, info::code_delete),
    ("code", "get_object_code", 1, info::get_object_code),
    ("code", "purge", 1, info::purge),
    ("code", "soft_purge", 1, info::soft_purge),
    ("erlang", "delete_module", 1, info::delete_module),
    ("erlang", "check_old_code", 1, proc::false_1),
    (
        "code",
        "ensure_modules_loaded",
        1,
        info::ensure_modules_loaded,
    ),
    ("code", "add_patha", 1, code::add_patha),
    ("code", "add_pathz", 1, code::add_pathz),
    ("code", "add_path", 1, code::add_pathz),
    ("code", "add_patha", 2, code::add_patha),
    ("code", "add_pathz", 2, code::add_pathz),
    ("code", "add_path", 2, code::add_pathz),
    ("code", "add_pathsa", 1, code::add_pathsa),
    ("code", "add_pathsz", 1, code::add_pathsz),
    ("code", "add_paths", 1, code::add_pathsz),
    ("code", "add_pathsa", 2, code::add_pathsa),
    ("code", "add_pathsz", 2, code::add_pathsz),
    ("code", "add_paths", 2, code::add_pathsz),
    ("code", "del_path", 1, code::del_path),
    ("code", "del_paths", 1, code::del_paths),
    ("code", "get_path", 0, code::get_path),
    ("code", "set_path", 1, code::set_path),
    ("code", "set_path", 2, code::set_path),
    ("code", "which", 1, code::which),
    ("code", "all_available", 0, code::all_available),
    ("code", "load_file", 1, code::load_file),
    ("code", "load_abs", 1, code::load_abs),
    ("code", "lib_dir", 1, code::lib_dir),
    ("code", "lib_dir", 2, code::lib_dir),
    ("code", "priv_dir", 1, code::priv_dir),
    ("code", "root_dir", 0, code::root_dir),
    ("erlang", "pid_to_list", 1, info::pid_to_list),
    ("erlang", "list_to_pid", 1, info::list_to_pid),
    ("erlang", "ref_to_list", 1, info::ref_to_list),
    ("erlang", "fun_to_list", 1, info::fun_to_list),
    ("erlang", "display_string", 1, info::display_string),
    ("erlang", "display_string", 2, info::display_string),
    ("erlang", "universaltime", 0, info::universaltime),
    (
        "erlang",
        "posixtime_to_universaltime",
        1,
        info::posixtime_to_universaltime,
    ),
    (
        "erlang",
        "universaltime_to_posixtime",
        1,
        info::universaltime_to_posixtime,
    ),
    ("erlang", "localtime", 0, info::universaltime),
    ("erlang", "date", 0, info::date),
    ("erlang", "time", 0, info::time),
    (
        "erlang",
        "universaltime_to_localtime",
        1,
        info::same_datetime,
    ),
    (
        "erlang",
        "localtime_to_universaltime",
        1,
        info::same_datetime,
    ),
    (
        "erlang",
        "localtime_to_universaltime",
        2,
        info::same_datetime,
    ),
    ("erlang", "crc32", 1, info::crc32),
    ("erlang", "crc32", 2, info::crc32),
    ("beamlet", "app_spec", 1, info::app_spec),
    ("beamlet", "console_subscribe", 0, info::console_subscribe),
    ("inet", "gethostname", 0, info::gethostname),
    ("net_adm", "localhost", 0, info::localhost),
    ("init", "get_arguments", 0, info::init_get_arguments),
    ("init", "get_plain_arguments", 0, info::init_get_arguments),
    ("init", "get_argument", 1, info::init_get_argument),
    // OTP's error_handler asks init to load modules when there is no code server.
    ("init", "ensure_loaded", 1, info::ensure_loaded),
    ("init", "get_status", 0, info::init_get_status),
    ("os", "getenv", 0, info::getenv_all),
    ("os", "getenv", 1, info::getenv),
    ("os", "getenv", 2, info::getenv),
    ("os", "putenv", 2, info::putenv),
    ("os", "unsetenv", 1, info::unsetenv),
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
    /// The built-in natives plus the embedder's `extra` ones (such as `beamlet-crypto`'s).
    pub fn new(extra: &[NativeSpec]) -> Registry {
        let mut by_module: BTreeMap<&'static str, Functions> = BTreeMap::new();
        for &(m, f, a, n) in TABLE.iter().chain(extra) {
            let arities = by_module.entry(m).or_default().entry(f).or_default();
            assert!(
                arities.iter().all(|(x, _)| *x != a),
                "{m}:{f}/{a} is listed twice"
            );
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
        Self::new(&[])
    }
}

// ---- helpers shared by the natives ----

impl Ctx<'_> {
    pub fn badarg(&self) -> Exception {
        Exception::error(Term::Atom(self.atoms.badarg))
    }

    /// `badarg`, with the `cause` BEAM gives in its `error_info` (see [`Exception::cause`]).
    pub fn badarg_because(&mut self, cause: &str) -> Exception {
        let mut e = self.badarg();
        e.cause = Some(self.atom(cause));
        e
    }

    pub fn badarith(&self) -> Exception {
        Exception::error(Term::Atom(self.atoms.badarith))
    }

    pub fn system_limit(&self) -> Exception {
        Exception::error(Term::Atom(self.atoms.system_limit))
    }

    pub fn bool(&self, b: bool) -> Term {
        Term::Atom(if b {
            self.atoms.true_
        } else {
            self.atoms.false_
        })
    }

    pub fn atom(&mut self, name: &str) -> Term {
        Term::Atom(self.sys().atom(name))
    }

    /// An error `{Tag, Value}`, e.g. `{badkey, K}`.
    pub fn error_with(&mut self, tag: &Atom, value: Term) -> Exception {
        let t = self.p.heap.tuple(&[Term::Atom(*tag), value]);
        Exception::error(t)
    }

    pub fn ok(&self) -> Term {
        Term::Atom(self.atoms.ok)
    }

    // ---- terms on the calling process's heap ----

    /// The calling process's heap: every argument is a term of it, and results go on it.
    pub fn heap(&self) -> &Heap {
        &self.p.heap
    }

    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.p.heap
    }

    pub fn tuple(&mut self, elems: &[Term]) -> Term {
        self.p.heap.tuple(elems)
    }

    pub fn cons(&mut self, head: Term, tail: Term) -> Term {
        self.p.heap.cons(head, tail)
    }

    pub fn list(
        &mut self,
        items: impl IntoIterator<Item = Term, IntoIter: DoubleEndedIterator>,
    ) -> Term {
        self.p.heap.list(items)
    }

    pub fn list_with_tail(
        &mut self,
        items: impl IntoIterator<Item = Term, IntoIter: DoubleEndedIterator>,
        tail: Term,
    ) -> Term {
        self.p.heap.list_with_tail(items, tail)
    }

    /// A string as a list of characters.
    pub fn string(&mut self, s: &str) -> Term {
        self.p.heap.string(s)
    }

    pub fn binary(&mut self, bytes: &[u8]) -> Term {
        self.p.heap.binary(bytes)
    }

    pub fn bits(&mut self, b: Bits) -> Term {
        self.p.heap.bits(b)
    }

    /// An integer, as a bignum only if it needs one.
    pub fn big(&mut self, b: num_bigint::BigInt) -> Term {
        self.p.heap.big(b)
    }

    pub fn from_i128(&mut self, i: i128) -> Term {
        self.p.heap.from_i128(i)
    }

    pub fn map_from(&mut self, pairs: impl IntoIterator<Item = (Term, Term)>) -> Term {
        self.p.heap.map_from(pairs)
    }

    /// `{ok, V}`.
    pub fn ok_tuple(&mut self, v: Term) -> Term {
        let ok = self.ok();
        self.p.heap.tuple(&[ok, v])
    }

    /// `{error, Reason}`.
    pub fn error_tuple(&mut self, reason: Term) -> Term {
        let e = Term::Atom(self.atoms.error);
        self.p.heap.tuple(&[e, reason])
    }

    /// The value of resource `t`, if it is one holding a `T`.
    pub fn resource<T: 'static>(&self, t: Term) -> Option<Held<T>> {
        let r = self.p.heap.as_resource(t)?;
        r.get::<T>()?;
        Some(Held {
            r: r.clone(),
            _t: core::marker::PhantomData,
        })
    }

    /// Finish this call later: the process yields and the native is called again, with the
    /// same arguments, when it next runs. For a native that needs a process another scheduler
    /// is running just now. Only for natives called as functions (not guard BIFs).
    pub fn retry(&mut self) -> Result<Term, Exception> {
        self.p.retry = true;
        Ok(Term::Nil)
    }

    /// A new resource holding `value`, with a fresh id.
    pub fn new_resource<T: core::any::Any + crate::sync::Shared>(&mut self, value: T) -> Term {
        let id = self.sys().make_ref().0;
        self.p.heap.resource(crate::term::Resource {
            id,
            value: alloc::boxed::Box::new(value),
        })
    }

    /// A copy of a term kept outside the process.
    pub fn copy_in(&mut self, t: &OwnedTerm) -> Term {
        t.copy_into(&mut self.p.heap)
    }

    /// A copy of `t` to keep outside the process.
    pub fn own(&self, t: Term) -> OwnedTerm {
        OwnedTerm::new(&self.p.heap, t)
    }

    /// The elements of a proper list, or `badarg`.
    pub fn list_arg(&self, t: Term) -> Result<Vec<Term>, Exception> {
        self.p.heap.to_vec(t).ok_or_else(|| self.badarg())
    }

    /// The elements of a tuple (copied out, so the heap is free for building), or `None`.
    pub fn tuple_elems(&self, t: Term) -> Option<Vec<Term>> {
        self.p.heap.as_tuple(t).map(<[Term]>::to_vec)
    }

    /// Print `t` as `~w` does.
    pub fn show(&self, t: Term) -> alloc::string::String {
        alloc::string::ToString::to_string(&self.p.heap.show(t))
    }
}
