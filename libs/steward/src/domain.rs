//! Domains: the store's partition (servers/steward.md, "Domains").

use alloc::vec::Vec;
use core::num::NonZeroU64;

use crate::consts::MAX_LABELS;

/// A label set: sorted, without repeats, at most `MAX_LABELS` long.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Labels(Vec<u64>);

impl Labels {
    /// The set of `labels`, or `None` if it is too long.
    pub fn new(labels: &[u64]) -> Option<Labels> {
        let mut v = labels.to_vec();
        v.sort_unstable();
        v.dedup();
        (v.len() <= MAX_LABELS).then_some(Labels(v))
    }

    pub fn empty() -> Labels { Labels(Vec::new()) }

    pub fn as_slice(&self) -> &[u64] { &self.0 }

    pub fn is_empty(&self) -> bool { self.0.is_empty() }

    /// Every label of `other` is one of these.
    pub fn includes(&self, other: &Labels) -> bool { other.0.iter().all(|l| self.0.contains(l)) }
}

/// A principal's account and one of its label sets. The account is never 0.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Domain {
    account: NonZeroU64,
    labels: Labels,
}

impl Domain {
    pub fn new(account: NonZeroU64, labels: Labels) -> Domain { Domain { account, labels } }

    pub fn account(&self) -> NonZeroU64 { self.account }

    pub fn labels(&self) -> &Labels { &self.labels }

    /// The unlabelled domain of the same account.
    pub fn unlabelled(&self) -> Domain { Domain { account: self.account, labels: Labels::empty() } }

    /// The domain of the same account with `labels`.
    pub fn with(&self, labels: Labels) -> Domain { Domain { account: self.account, labels } }
}
