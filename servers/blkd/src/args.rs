//! `blkd`'s arguments: the endpoint it receives on, and each labelled range's label set
//! (servers/blkd.md, "Its endpoint" and "Ranges and badges").
//!
//! `endpoint=NAME` names the endpoint, the manifest's name for it (`blkd`, `blkd:system`): one
//! `blkd` runs per disk, so it is told which is its own, never defaulted.
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
use redoubt_rt::startup::valid_name;

use crate::range::Range;

/// An argument `blkd` refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadArgs;

const PREFIX: &str = "labels.";
const ENDPOINT: &str = "endpoint=";

/// `blkd`'s arguments, read once.
#[derive(Debug, PartialEq, Eq)]
pub struct Args<'a> {
    /// The manifest's name of the endpoint it receives on (`blkd:system`): its startup block
    /// holds that endpoint under this name.
    pub endpoint: &'a str,
    /// Each labelled entry P and its set, as `labels.P=` gave them.
    labels: Vec<(usize, Labels)>,
}

/// A decimal number without leading zeros, the form `init` writes.
fn number(s: &str) -> Result<u64, BadArgs> {
    let canonical =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    s.parse().ok().filter(|_| canonical).ok_or(BadArgs)
}

/// The arguments: `endpoint=NAME` exactly once, a name under the manifest's rule, never
/// defaulted; and `labels.P=ID[,ID...]` per labelled entry, each P named once, its IDs distinct,
/// at most [`MAX_LABELS`]. Anything else is refused.
pub fn parse_args<'a>(args: impl Iterator<Item = &'a str>) -> Result<Args<'a>, BadArgs> {
    let (mut endpoint, mut labels) = (None, Vec::new());
    for arg in args {
        if let Some(name) = arg.strip_prefix(ENDPOINT) {
            if endpoint.is_some() || !valid_name(name) {
                return Err(BadArgs);
            }
            endpoint = Some(name);
            continue;
        }
        let (entry, list) = arg.strip_prefix(PREFIX).and_then(|rest| rest.split_once('=')).ok_or(BadArgs)?;
        let entry = usize::try_from(number(entry)?).map_err(|_| BadArgs)?;
        if labels.iter().any(|(e, _)| *e == entry) {
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
        labels.try_reserve(1).map_err(|_| BadArgs)?;
        labels.push((entry, Labels::from_slice(&set).map_err(|_| BadArgs)?));
    }
    Ok(Args { endpoint: endpoint.ok_or(BadArgs)?, labels })
}

impl Args<'_> {
    /// Each slot's label set, one per GPT entry as `roots` has them: every P named must be a used
    /// entry, and a range named by none has no labels.
    pub fn range_labels(&self, roots: &[Option<Range>]) -> Result<Vec<Labels>, BadArgs> {
        let mut labels: Vec<Labels> = Vec::new();
        labels.try_reserve(roots.len()).map_err(|_| BadArgs)?;
        labels.resize(roots.len(), Labels::default());
        for (entry, set) in &self.labels {
            if !matches!(roots.get(*entry), Some(Some(_))) {
                return Err(BadArgs);
            }
            labels[*entry] = *set;
        }
        Ok(labels)
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::String;
    use alloc::vec;

    use redoubt_rt::abi::Handle;
    use redoubt_rt::startup::{Startup, StartupBuilder};

    use super::*;

    fn roots() -> Vec<Option<Range>> {
        let range = |first| Some(Range::new(first, 100, 10_000).unwrap());
        vec![range(64), None, range(1000)]
    }

    /// The label sets `labels` give, after the endpoint argument every `blkd` has.
    fn parse(labels: &[&str]) -> Result<Vec<Vec<u64>>, BadArgs> {
        let args = ["endpoint=blkd"].iter().chain(labels).copied();
        parse_args(args)?.range_labels(&roots()).map(|l| l.iter().map(|l| l.as_slice().to_vec()).collect())
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

    fn h(i: u32) -> Handle { Handle::new(i).unwrap() }

    /// A second `blkd` receives on the endpoint its `endpoint=` names, here `blkd:system`, found
    /// in its startup block; with none, two, or a name the manifest could not give, its arguments
    /// are refused and it does not start.
    #[test]
    fn blkd_receives_on_the_endpoint_its_argument_names_and_never_guesses() {
        let mut builder = StartupBuilder::new(2);
        builder
            .handle("disk", h(1))
            .handle("blkd:system", h(2))
            .arg("labels.0=7")
            .arg("endpoint=blkd:system");
        let block = builder.finish().unwrap();
        let startup = Startup::parse(&block).unwrap();
        let args = parse_args(startup.args()).unwrap();
        assert_eq!(args.endpoint, "blkd:system");
        assert_eq!(startup.handle(args.endpoint), Some(h(2)));
        for bad in [
            &[][..],
            &["labels.0=7"],
            &["endpoint="],
            &["endpoint=Blkd"],
            &["endpoint=blkd:system", "endpoint=blkd:system"],
        ] {
            assert_eq!(parse_args(bad.iter().copied()), Err(BadArgs), "{bad:?}");
        }
    }
}
