//! The executable security model of Redoubt (WP-M0): KERNEL-SPEC.md's objects, system calls,
//! errors, rules R1-R12 and invariants I1-I14, with the same names and arguments; the steward's
//! milestone 1 policy above it; property tests over random operation sequences; and the trace
//! format the kernel's conformance test replays.
//!
//! Read [`kernel`] next to KERNEL-SPEC.md: one method per system call, in the spec's order.
//! README.md says how to run the tests, what is abstracted, the order in which checks return
//! their errors, the trace format, the interpretation choices, and the spec problems found.
//!
//! `no_std` + `alloc` so that a kernel-side replayer can use [`trace`] as it is.

#![no_std]
#![forbid(unsafe_code)]
extern crate alloc;

pub mod check;
pub mod gen;
pub mod ghost;
pub mod invariants;
pub mod kernel;
pub mod mutation;
pub mod policy;
pub mod sched;
pub mod spec;
pub mod steward;
pub mod syscall;
pub mod trace;

/// I14's tests run in release builds; the workspace keeps overflow checks on for this crate there
/// (root Cargo.toml), so an arithmetic overflow in the model is a panic the runner catches.
#[cfg(test)]
mod tests {
    extern crate std;

    #[test]
    fn overflow_checks_are_on() {
        let x: u64 = core::hint::black_box(u64::MAX);
        let r = std::panic::catch_unwind(|| x + 1);
        assert!(r.is_err(), "overflow checks are off for redoubt-model");
    }
}
