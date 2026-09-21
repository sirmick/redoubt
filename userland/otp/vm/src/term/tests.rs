use alloc::string::ToString;
use alloc::vec::Vec;
use core::cmp::Ordering;

use super::*;

const DEEP: usize = 1_000_000;

fn heap() -> Heap {
    Heap::new(&Literals::default())
}

fn nested_tuples(h: &mut Heap, depth: usize) -> Term {
    (0..depth).fold(Term::Nil, |acc, _| h.tuple(&[acc]))
}

fn nested_heads(h: &mut Heap, depth: usize) -> Term {
    (0..depth).fold(Term::Nil, |acc, _| h.cons(acc, Term::Nil))
}

fn nested_maps(h: &mut Heap, depth: usize) -> Term {
    (0..depth).fold(Term::Nil, |acc, _| h.map_from([(Term::Int(1), acc)]))
}

#[test]
fn term_is_two_words() {
    assert_eq!(core::mem::size_of::<Term>(), 16, "Term grew");
}

#[test]
fn printing_matches_otp() {
    let mut h = heap();
    let l = h.list([Term::Int(1), Term::Int(2)]);
    let improper = h.list_with_tail([Term::Int(1)], Term::Int(2));
    let empty = h.tuple(&[]);
    let m = h.empty_map();
    let b = h.binary(b"ab");
    let t = h.tuple(&[l, improper, empty, Term::Nil, m, b, Term::Float(1.5)]);
    assert_eq!(
        h.show(t).to_string(),
        "{[1,2],[1|2],{},[],#{},<<97,98>>,1.5}"
    );
}

/// A million levels of nesting, which is legal Erlang, must compare, print, copy and collect
/// without touching the Rust stack more than a few frames deep.
#[test]
fn deep_terms_are_handled_iteratively() {
    for make in [nested_tuples, nested_heads, nested_maps] {
        let mut h = heap();
        let a = make(&mut h, DEEP);
        let b = make(&mut h, DEEP);
        let shorter = make(&mut h, DEEP - 1);
        assert!(h.eq_exact(a, b));
        assert_ne!(h.cmp_term(a, shorter), Ordering::Equal);
        assert!(h.show(a).to_string().len() > DEEP);
        let owned = OwnedTerm::new(&h, a);
        assert!(compare(owned.heap(), owned.term(), &h, b, true) == Ordering::Equal);
        let mut roots = [a, b];
        let mut gc = h.collect(0);
        for r in roots.iter_mut() {
            gc.root(r);
        }
        gc.finish();
        assert!(h.eq_exact(roots[0], roots[1]));
    }
}

#[test]
fn collection_keeps_what_is_reachable_and_sharing() {
    let mut h = heap();
    let shared = h.list((0..100).map(Term::Int));
    let _garbage = h.list((0..10_000).map(Term::Int));
    let pair = h.tuple(&[shared, shared]);
    let bin = h.binary(&[7; 1000]);
    let _dead_bin = h.binary(&[1; 5000]);
    let mut roots = [pair, bin];
    let before = h.len();
    let mut gc = h.collect(0);
    for r in roots.iter_mut() {
        gc.root(r);
    }
    gc.finish();
    // The shared list is copied once: 100 cells of two, plus the tuple.
    assert_eq!(h.len(), 200 + 3 + 4);
    assert!(h.len() < before);
    assert_eq!(h.offheap_bytes(), 1000);
    let elems = h.as_tuple(roots[0]).unwrap();
    assert_eq!(elems[0].ptr(), elems[1].ptr());
    assert_eq!(h.to_vec(elems[0]).unwrap().len(), 100);
    assert_eq!(
        h.as_bits(roots[1]).unwrap().to_bytes().as_ref(),
        &[7u8; 1000][..]
    );
}

#[test]
fn copying_between_heaps() {
    let mut a = heap();
    let s = a.string("hello");
    let big = a.big(num_bigint::BigInt::from(u64::MAX) * 3u32);
    let m = a.map_from([(Term::Int(1), s), (Term::Int(2), big)]);
    let f = a.fun_local(
        Atom::test("m"),
        0,
        1,
        99,
        Atom::test("-f/0-fun-0-"),
        &[m, s],
    );
    let t = a.tuple(&[s, s, m, f]);
    let mut b = heap();
    let u = copy(&a, t, &mut b);
    assert_eq!(compare(&a, t, &b, u, true), Ordering::Equal);
    assert_eq!(a.show(t).to_string(), b.show(u).to_string());
    // Sharing survives the copy.
    let e = b.as_tuple(u).unwrap();
    assert_eq!(e[0].ptr(), e[1].ptr());
    // Copies are independent of the source.
    drop(a);
    assert!(b.show(u).to_string().starts_with("{[104,101,108,108,111]"));
}

#[test]
fn literals_are_shared_not_copied() {
    let mut lits = Literals::default();
    let mut chunk = heap();
    let big = chunk.list((0..1000).map(Term::Int));
    let bin = chunk.binary(b"literal");
    let mut roots = [chunk.tuple(&[big, bin])];
    lits.add(chunk, &mut roots);
    let lit = roots[0];
    let mut a = Heap::new(&lits);
    let t = a.tuple(&[lit, Term::Int(1)]);
    let mut b = heap();
    let u = copy(&a, t, &mut b);
    // Only the new tuple was copied; the literal is read from the chunk.
    assert_eq!(b.len(), 3);
    assert_eq!(compare(&a, t, &b, u, true), Ordering::Equal);
    let elems = b.as_tuple(u).unwrap();
    let inner = b.as_tuple(elems[0]).unwrap();
    assert_eq!(b.as_bits(inner[1]).unwrap().to_bytes().as_ref(), b"literal");
    // A collection leaves literals where they are.
    let mut roots = [u];
    let mut gc = b.collect(0);
    gc.root(&mut roots[0]);
    gc.finish();
    assert_eq!(b.len(), 3);
}

#[test]
fn owned_terms_order_exactly() {
    let mut h = heap();
    let one = h.tuple(&[Term::Int(1)]);
    let one_f = h.tuple(&[Term::Float(1.0)]);
    let a = OwnedTerm::new(&h, one);
    let b = OwnedTerm::new(&h, one_f);
    assert!(a < b);
    assert_eq!(a.clone(), a);
    let mut v: Vec<OwnedTerm> = alloc::vec![b.clone(), a.clone()];
    v.sort();
    assert_eq!(v, alloc::vec![a, b]);
}

#[test]
fn iodata() {
    let mut h = heap();
    let bin = h.binary(b"cd");
    let inner = h.list([Term::Int(b'b' as i64)]);
    let l = h.list([Term::Int(b'a' as i64), inner, bin]);
    assert_eq!(h.iodata_bytes(l).as_deref(), Some(&b"abcd"[..]));
    let bad = h.list([Term::Int(300)]);
    assert_eq!(h.iodata_bytes(bad), None);
}

/// Sub-binaries of one buffer share its entry: its bytes count once, through copying and GC.
#[test]
fn slices_share_one_offheap_entry() {
    let mut h = heap();
    let whole = h.binary(&[7; 100_000]);
    let b = h.as_bits(whole).unwrap();
    let other = h.binary(&[1; 10]);
    let mut slices: Vec<Term> = (0..1000).map(|i| h.bits(b.slice(i * 8, 800))).collect();
    slices.push(other);
    slices.push(h.bits(b.slice(0, 8)));
    assert_eq!(h.offheap_bytes(), 100_010);
    let list = h.list(slices);
    let mut dst = heap();
    let copied = copy(&h, list, &mut dst);
    assert_eq!(dst.offheap_bytes(), 100_010);
    let mut root = copied;
    let mut gc = dst.collect(0);
    gc.root(&mut root);
    gc.finish();
    assert_eq!(dst.offheap_bytes(), 100_010);
    assert_eq!(dst.list_iter(root).count(), 1002);
}

/// A fragment moved onto a heap that already holds terms reads the same, shares its binary
/// entries with the heap's, and keeps its sharing.
#[test]
fn absorbed_fragments_read_the_same() {
    let mut h = heap();
    let big = h.binary(&[3; 1000]);
    let before = h.tuple(&[Term::Int(1), big]);
    let bytes = h.as_bits(big).unwrap();
    let frag = OwnedTerm::build(&Literals::default(), |f| {
        let shared = f.list([Term::Int(1), Term::Int(2)]);
        let slice = f.bits(bytes.slice(8, 16));
        let m = f.map_from([(Term::Int(1), shared)]);
        let n = f.from_i128(1 << 100);
        f.tuple(&[shared, shared, slice, m, n])
    });
    let text = frag.to_string();
    let before_text = h.show(before).to_string();
    let t = frag.absorb_into(&mut h);
    assert_eq!(h.show(t).to_string(), text);
    assert_eq!(
        h.show(before).to_string(),
        before_text,
        "what was there is untouched"
    );
    let e = h.as_tuple(t).unwrap();
    assert_eq!(e[0].ptr(), e[1].ptr(), "sharing kept");
    assert_eq!(
        h.offheap_bytes(),
        1000 + 13,
        "the buffer counts once; the bignum's 13 bytes"
    );
}
