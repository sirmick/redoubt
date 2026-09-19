//! Term order, for two terms that may live on different heaps.

use alloc::vec::Vec;
use core::cmp::Ordering;

use num_bigint::BigInt;
use num_traits::float::FloatCore;
use num_traits::{FromPrimitive, Zero};

use super::{Bits, FunView, Heap, Term};

/// Rank of each type in the standard term order:
/// number < atom < reference < fun < port < pid < tuple < map < nil < list < bitstring.
fn type_rank(t: &Term) -> u8 {
    match t {
        Term::Int(_) | Term::Big(_) | Term::Float(_) => 0,
        Term::Atom(_) => 1,
        Term::Ref(_) | Term::Resource(_) => 2,
        Term::Fun(_) => 3,
        Term::Pid(p) if p.port => 4,
        Term::Pid(_) => 5,
        Term::Tuple(_) => 6,
        Term::Map(_) => 7,
        Term::Nil => 8,
        Term::Cons(_) => 9,
        Term::Bits(_) | Term::Match(_) => 10,
        Term::Node(_) | Term::Header(_) | Term::OffHeap(_) => 11,
    }
}

impl Heap {
    /// Standard term order, comparing numbers by value (`1 == 1.0`), as `<`, `==`, `lists:sort/1`.
    pub fn cmp_term(&self, a: Term, b: Term) -> Ordering {
        compare(self, a, self, b, false)
    }

    /// Exact term order (`=:=`, map keys): an integer never equals a float, and when their values
    /// are equal the integer sorts first.
    pub fn cmp_exact(&self, a: Term, b: Term) -> Ordering {
        compare(self, a, self, b, true)
    }

    /// `==`
    pub fn eq_arith(&self, a: Term, b: Term) -> bool {
        self.cmp_term(a, b) == Ordering::Equal
    }

    /// `=:=`
    pub fn eq_exact(&self, a: Term, b: Term) -> bool {
        self.cmp_exact(a, b) == Ordering::Equal
    }
}

/// Compare `a` (a term of heap `ha`) with `b` (of `hb`). Pairs still to compare wait on an
/// explicit stack, in the order Erlang compares them, so deep or long terms cannot exhaust the
/// Rust stack.
pub fn compare(ha: &Heap, a: Term, hb: &Heap, b: Term, exact: bool) -> Ordering {
    // Fast path, no allocation: most comparisons are between atomic values.
    if !a.is_container() || !b.is_container() {
        if let (Term::Int(x), Term::Int(y)) = (a, b) {
            return x.cmp(&y);
        }
        let (ra, rb) = (type_rank(&a), type_rank(&b));
        if ra != rb {
            return ra.cmp(&rb);
        }
        if !a.is_container() && !b.is_container() {
            return compare_one(ha, a, hb, b, exact);
        }
    }
    let same_heap = core::ptr::eq(ha, hb);
    let mut work: Vec<(Term, Term, bool)> = alloc::vec![(a, b, exact)];
    while let Some((a, b, exact)) = work.pop() {
        let (ra, rb) = (type_rank(&a), type_rank(&b));
        if ra != rb {
            return ra.cmp(&rb);
        }
        // The same object on the same heap is equal to itself.
        if same_heap && a.ptr().is_some() && a.ptr() == b.ptr() {
            continue;
        }
        let o = match (a, b) {
            (Term::Cons(_), Term::Cons(_)) => {
                let (h1, t1) = ha.as_cons(a).expect("a list cell");
                let (h2, t2) = hb.as_cons(b).expect("a list cell");
                // Head first, then the tail.
                work.push((t1, t2, exact));
                work.push((h1, h2, exact));
                Ordering::Equal
            }
            (Term::Tuple(_), Term::Tuple(_)) => {
                let (x, y) = (ha.as_tuple(a).expect("a tuple"), hb.as_tuple(b).expect("a tuple"));
                if x.len() == y.len() {
                    push_pairs(&mut work, x.iter().copied(), y.iter().copied(), exact);
                }
                x.len().cmp(&y.len())
            }
            (Term::Map(_), Term::Map(_)) => {
                // Size first, then all keys in key order (always exactly), then the values.
                let (x, y) = (ha.map_entries(a).expect("a map"), hb.map_entries(b).expect("a map"));
                if x.len() == y.len() {
                    push_pairs(&mut work, x.iter().map(|e| e.1), y.iter().map(|e| e.1), exact);
                    push_pairs(&mut work, x.iter().map(|e| e.0), y.iter().map(|e| e.0), true);
                }
                x.len().cmp(&y.len())
            }
            (Term::Fun(_), Term::Fun(_)) => {
                let (x, y) = (ha.as_fun(a).expect("a fun"), hb.as_fun(b).expect("a fun"));
                let o = compare_fun_heads(&x, &y);
                if o == Ordering::Equal {
                    if let (FunView::Local { env: e1, .. }, FunView::Local { env: e2, .. }) = (x, y) {
                        push_pairs(&mut work, e1.iter().copied(), e2.iter().copied(), exact);
                    }
                }
                o
            }
            _ => compare_one(ha, a, hb, b, exact),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

/// Push element pairs so that the first pair is compared first.
fn push_pairs(
    work: &mut Vec<(Term, Term, bool)>,
    xs: impl DoubleEndedIterator<Item = Term> + ExactSizeIterator,
    ys: impl DoubleEndedIterator<Item = Term> + ExactSizeIterator,
    exact: bool,
) {
    for (x, y) in xs.zip(ys).rev() {
        work.push((x, y, exact));
    }
}

/// Compare two terms of the same type that contain no other terms.
fn compare_one(ha: &Heap, a: Term, hb: &Heap, b: Term, exact: bool) -> Ordering {
    match (a, b) {
        (Term::Atom(x), Term::Atom(y)) => {
            if x == y {
                Ordering::Equal
            } else {
                x.as_str().cmp(y.as_str())
            }
        }
        (Term::Ref(x), Term::Ref(y)) => x.cmp(&y),
        // References and resources share one counter, so a reference with a resource's id is
        // that resource written out and read back (`term_to_binary`): the same reference, as
        // BEAM's magic references are.
        (Term::Ref(_) | Term::Resource(_), Term::Ref(_) | Term::Resource(_)) => ref_id(ha, a).cmp(&ref_id(hb, b)),
        // Creation order, as BEAM's pids compare (the serial is one counter for the VM).
        (Term::Pid(x), Term::Pid(y)) => (x.serial, x.index).cmp(&(y.serial, y.index)),
        (Term::Nil, Term::Nil) => Ordering::Equal,
        (Term::Bits(_), Term::Bits(_)) => compare_bits(&ha.as_bits(a).expect("bits"), &hb.as_bits(b).expect("bits")),
        (Term::Match(_), Term::Match(_)) => ha.as_match(a).map(|m| m.1).cmp(&hb.as_match(b).map(|m| m.1)),
        (Term::Match(_), _) => Ordering::Less,
        (_, Term::Match(_)) => Ordering::Greater,
        _ => compare_numbers(ha, a, hb, b, exact),
    }
}

fn ref_id(h: &Heap, t: Term) -> u64 {
    match t {
        Term::Ref(r) => r.0,
        _ => h.as_resource(t).map_or(0, |r| r.id),
    }
}

fn compare_bits(x: &Bits, y: &Bits) -> Ordering {
    // Bit by bit; a prefix sorts first.
    let n = x.len.min(y.len);
    let bytes = n / 8;
    if x.offset.is_multiple_of(8) && y.offset.is_multiple_of(8) {
        let (xs, ys) = (&x.data[x.offset / 8..x.offset / 8 + bytes], &y.data[y.offset / 8..y.offset / 8 + bytes]);
        let o = xs.cmp(ys);
        if o != Ordering::Equal {
            return o;
        }
    } else {
        for i in 0..bytes {
            let o = x.byte(i).cmp(&y.byte(i));
            if o != Ordering::Equal {
                return o;
            }
        }
    }
    for i in bytes * 8..n {
        let o = x.bit(i).cmp(&y.bit(i));
        if o != Ordering::Equal {
            return o;
        }
    }
    x.len.cmp(&y.len)
}

/// Compare funs by everything except their captured environments.
fn compare_fun_heads(x: &FunView, y: &FunView) -> Ordering {
    match (x, y) {
        (
            FunView::Export { module: m1, function: f1, arity: a1 },
            FunView::Export { module: m2, function: f2, arity: a2 },
        ) => m1.as_str().cmp(m2.as_str()).then_with(|| f1.as_str().cmp(f2.as_str())).then(a1.cmp(a2)),
        (FunView::Local { .. }, FunView::Export { .. }) => Ordering::Less,
        (FunView::Export { .. }, FunView::Local { .. }) => Ordering::Greater,
        (
            FunView::Local { module: m1, index: i1, env: e1, .. },
            FunView::Local { module: m2, index: i2, env: e2, .. },
        ) => m1.as_str().cmp(m2.as_str()).then(i1.cmp(i2)).then(e1.len().cmp(&e2.len())),
    }
}

fn compare_numbers(ha: &Heap, a: Term, hb: &Heap, b: Term, exact: bool) -> Ordering {
    let int = |h: &Heap, t: Term| h.as_bigint(t).unwrap_or_else(BigInt::zero);
    match (a, b) {
        (Term::Int(x), Term::Int(y)) => x.cmp(&y),
        (Term::Float(x), Term::Float(y)) => {
            // Erlang has no NaN. Under exact comparison 0.0 and -0.0 differ (-0.0 first).
            if exact {
                x.total_cmp(&y)
            } else {
                x.partial_cmp(&y).unwrap_or(Ordering::Equal)
            }
        }
        (Term::Float(x), _) => {
            let o = cmp_float_int(x, int(hb, b));
            if o == Ordering::Equal && exact {
                Ordering::Greater // integers sort before equal floats
            } else {
                o
            }
        }
        (_, Term::Float(y)) => {
            let o = cmp_float_int(y, int(ha, a)).reverse();
            if o == Ordering::Equal && exact {
                Ordering::Less
            } else {
                o
            }
        }
        // A bignum is outside the i64 range: its sign decides.
        (Term::Int(_), Term::Big(_)) => match hb.as_big(b).map(|y| y.sign()) {
            Some(num_bigint::Sign::Minus) => Ordering::Greater,
            _ => Ordering::Less,
        },
        (Term::Big(_), Term::Int(_)) => compare_numbers(hb, b, ha, a, exact).reverse(),
        _ => int(ha, a).cmp(&int(hb, b)),
    }
}

/// Compare a finite float with an integer exactly (no rounding of either side).
fn cmp_float_int(f: f64, i: BigInt) -> Ordering {
    let whole = FloatCore::trunc(f);
    // A finite double's integer part is exactly representable as a BigInt.
    let w = BigInt::from_f64(whole).unwrap_or_else(BigInt::zero);
    match w.cmp(&i) {
        Ordering::Equal => {
            let frac = f - whole;
            frac.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
        }
        o => o,
    }
}
