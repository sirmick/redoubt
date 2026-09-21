//! Printing terms in the `~w` format.

use core::fmt;

use super::{FunView, Heap, Term};

/// A term with its heap, for printing: `heap.show(t).to_string()`.
pub struct Show<'h> {
    heap: &'h Heap,
    term: Term,
}

impl Heap {
    pub fn show(&self, t: Term) -> Show<'_> {
        Show {
            heap: self,
            term: t,
        }
    }
}

impl fmt::Display for Show<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_term(f, self.heap, self.term)
    }
}

impl fmt::Debug for Show<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_term(f, self.heap, self.term)
    }
}

/// What is left to print: terms, and the punctuation between them.
enum Out {
    Term(Term),
    Text(&'static str),
}

fn write_term(f: &mut fmt::Formatter<'_>, h: &Heap, t: Term) -> fmt::Result {
    let mut work = alloc::vec![Out::Term(t)];
    while let Some(item) = work.pop() {
        let t = match item {
            Out::Text(s) => {
                f.write_str(s)?;
                continue;
            }
            Out::Term(t) => t,
        };
        // Compound terms push their parts in reverse, so they come off the stack in order.
        match t {
            Term::Cons(_) => {
                let mut parts = alloc::vec![Out::Text("[")];
                for (i, item) in h.list_iter(t).enumerate() {
                    match item {
                        Ok(x) => {
                            if i > 0 {
                                parts.push(Out::Text(","));
                            }
                            parts.push(Out::Term(x));
                        }
                        Err(tail) => {
                            parts.push(Out::Text("|"));
                            parts.push(Out::Term(tail));
                        }
                    }
                }
                parts.push(Out::Text("]"));
                work.extend(parts.into_iter().rev());
            }
            Term::Tuple(_) => {
                let elems = h.as_tuple(t).expect("a tuple");
                work.push(Out::Text("}"));
                for (i, e) in elems.iter().enumerate().rev() {
                    work.push(Out::Term(*e));
                    if i > 0 {
                        work.push(Out::Text(","));
                    }
                }
                work.push(Out::Text("{"));
            }
            Term::Map(_) => {
                work.push(Out::Text("}"));
                for (i, (k, v)) in h
                    .map_entries(t)
                    .expect("a map")
                    .into_iter()
                    .enumerate()
                    .rev()
                {
                    work.push(Out::Term(v));
                    work.push(Out::Text(" => "));
                    work.push(Out::Term(k));
                    if i > 0 {
                        work.push(Out::Text(","));
                    }
                }
                work.push(Out::Text("#{"));
            }
            _ => write_leaf(f, h, t)?,
        }
    }
    Ok(())
}

/// Print a term that contains no other terms.
fn write_leaf(f: &mut fmt::Formatter<'_>, h: &Heap, t: Term) -> fmt::Result {
    match t {
        Term::Int(i) => write!(f, "{i}"),
        Term::Big(_) => write!(f, "{}", h.as_big(t).expect("a bignum")),
        Term::Float(x) => f.write_str(&crate::float::format_short(x)),
        Term::Atom(a) => write_atom(f, a.as_str()),
        Term::Nil => f.write_str("[]"),
        Term::Bits(_) => {
            let b = h.as_bits(t).expect("bits");
            f.write_str("<<")?;
            let whole = b.len / 8;
            for i in 0..whole {
                if i > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{}", b.byte(i))?;
            }
            let rest = b.len % 8;
            if rest != 0 {
                let mut v = 0u8;
                for k in 0..rest {
                    v = (v << 1) | b.bit(whole * 8 + k) as u8;
                }
                if whole > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{v}:{rest}")?;
            }
            f.write_str(">>")
        }
        Term::Fun(_) => match h.as_fun(t).expect("a fun") {
            FunView::Export {
                module,
                function,
                arity,
            } => {
                f.write_str("fun ")?;
                write_atom(f, module.as_str())?;
                f.write_str(":")?;
                write_atom(f, function.as_str())?;
                write!(f, "/{arity}")
            }
            FunView::Local {
                module,
                index,
                uniq,
                ..
            } => write!(f, "#Fun<{}.{}.{}>", module.as_str(), index, uniq),
        },
        Term::Pid(p) if p.port => write!(f, "#Port<0.{}>", p.serial),
        Term::Pid(p) => write!(f, "<0.{}.{}>", p.index, p.serial),
        Term::Ref(r) => write!(f, "#Ref<0.0.0.{}>", r.0),
        Term::Resource(_) => write!(f, "#Ref<0.0.0.{}>", h.as_resource(t).map_or(0, |r| r.id)),
        Term::Match(_) => f.write_str("#MatchState<>"),
        Term::Node(_) | Term::Header(_) | Term::OffHeap(_) => f.write_str("#Internal<>"),
        Term::Cons(_) | Term::Tuple(_) | Term::Map(_) => {
            unreachable!("containers are handled by write_term")
        }
    }
}

pub(crate) fn atom_needs_quotes(s: &str) -> bool {
    const RESERVED: &[&str] = &[
        "after", "and", "andalso", "band", "begin", "bnot", "bor", "bsl", "bsr", "bxor", "case",
        "catch", "cond", "div", "else", "end", "fun", "if", "let", "maybe", "not", "of", "or",
        "orelse", "receive", "rem", "try", "when", "xor",
    ];
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || ('ß'..='ÿ').contains(&c) && c != '÷' => {}
        _ => return true,
    }
    if !chars.all(|c| {
        c.is_ascii_alphanumeric()
            || c == '_'
            || c == '@'
            || (('À'..='ÿ').contains(&c) && c != '×' && c != '÷')
    }) {
        return true;
    }
    RESERVED.contains(&s)
}

pub(crate) fn write_atom(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    if !atom_needs_quotes(s) {
        return f.write_str(s);
    }
    f.write_str("'")?;
    for c in s.chars() {
        match c {
            '\'' => f.write_str("\\'")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\t' => f.write_str("\\t")?,
            '\r' => f.write_str("\\r")?,
            '\x08' => f.write_str("\\b")?,
            '\x0c' => f.write_str("\\f")?,
            '\x0b' => f.write_str("\\v")?,
            '\x1b' => f.write_str("\\e")?,
            '\x7f' => f.write_str("\\d")?,
            c if (c as u32) < 0x20 => write!(f, "\\^{}", ((c as u8) + 0x40) as char)?,
            c => write!(f, "{c}")?,
        }
    }
    f.write_str("'")
}
