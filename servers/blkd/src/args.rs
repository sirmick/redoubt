//! `blkd`'s arguments: each labelled range's label set (servers/blkd.md, "Ranges and badges").
//!
//! `init` passes one argument per labelled volume on the disk, `labels.P=ID[,ID...]`, P the
//! volume's GPT entry number, so the range badge P + 1 names carries that set. A range named by
//! none has no labels. `blkd` still remembers nothing: a restarted `blkd` gets the same arguments
//! and reads the same table.
//!
//! Parsed strictly, before anything is served: a malformed argument, a P named twice, or a P that
//! names no partition is refused whole, so `blkd` never serves a range under labels it misread.

use alloc::vec::Vec;

use redoubt_rt::abi::{Labels, MAX_LABELS};

use crate::range::Range;

/// An argument `blkd` refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadArgs;

const PREFIX: &str = "labels.";

/// A decimal number without leading zeros, the form `init` writes.
fn number(s: &str) -> Result<u64, BadArgs> {
    let canonical =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    s.parse().ok().filter(|_| canonical).ok_or(BadArgs)
}

/// Each slot's label set, one per GPT entry as `roots` has them, from `args`: every argument
/// must be `labels.P=ID[,ID...]` with P a used entry, named once, and its IDs distinct, at most
/// [`MAX_LABELS`].
pub fn range_labels<'a>(
    args: impl Iterator<Item = &'a str>,
    roots: &[Option<Range>],
) -> Result<Vec<Labels>, BadArgs> {
    let mut labels: Vec<Option<Labels>> = Vec::new();
    labels.try_reserve(roots.len()).map_err(|_| BadArgs)?;
    labels.resize(roots.len(), None);
    for arg in args {
        let (entry, list) = arg.strip_prefix(PREFIX).and_then(|rest| rest.split_once('=')).ok_or(BadArgs)?;
        let entry = usize::try_from(number(entry)?).map_err(|_| BadArgs)?;
        if !matches!(roots.get(entry), Some(Some(_))) {
            return Err(BadArgs);
        }
        let mut set: Vec<u64> = Vec::new();
        for id in list.split(',') {
            let id = number(id)?;
            if set.contains(&id) || set.len() >= MAX_LABELS {
                return Err(BadArgs);
            }
            set.try_reserve(1).map_err(|_| BadArgs)?;
            set.push(id);
        }
        let slot = labels.get_mut(entry).ok_or(BadArgs)?;
        if slot.is_some() {
            return Err(BadArgs);
        }
        *slot = Some(Labels::from_slice(&set).map_err(|_| BadArgs)?);
    }
    Ok(labels.into_iter().map(Option::unwrap_or_default).collect())
}

#[cfg(test)]
mod tests {
    use alloc::string::String;
    use alloc::vec;

    use super::*;

    fn roots() -> Vec<Option<Range>> {
        let range = |first| Some(Range::new(first, 100, 10_000).unwrap());
        vec![range(64), None, range(1000)]
    }

    fn parse(args: &[&str]) -> Result<Vec<Vec<u64>>, BadArgs> {
        range_labels(args.iter().copied(), &roots())
            .map(|l| l.iter().map(|l| l.as_slice().to_vec()).collect())
    }

    #[test]
    fn each_range_gets_the_labels_its_argument_names_and_the_rest_none() {
        assert_eq!(parse(&[]), Ok(vec![vec![], vec![], vec![]]));
        assert_eq!(parse(&["labels.2=7,42"]), Ok(vec![vec![], vec![], vec![7, 42]]));
        assert_eq!(parse(&["labels.0=1", "labels.2=0"]), Ok(vec![vec![1], vec![], vec![0]]));
    }

    #[test]
    fn a_malformed_argument_a_partition_named_twice_or_none_is_refused() {
        for bad in [
            &["buckets=4"][..],
            &["labels=1"],
            &["labels.0"],
            &["labels.0="],
            &["labels.0=1,"],
            &["labels.0=01"],
            &["labels.00=1"],
            &["labels.+0=1"],
            &["labels.0=1,1"],
            &["labels.0=18446744073709551616"],
            &["labels.0=1", "labels.0=2"],
            // An unused entry, and one past the array.
            &["labels.1=1"],
            &["labels.3=1"],
        ] {
            assert_eq!(parse(bad), Err(BadArgs), "{bad:?}");
        }
        let too_many: Vec<String> = (0..=MAX_LABELS as u64).map(|i| alloc::format!("{i}")).collect();
        let arg = alloc::format!("labels.0={}", too_many.join(","));
        assert_eq!(parse(&[&arg]), Err(BadArgs));
    }
}
