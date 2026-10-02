//! Server admission (servers/serving.md R26): the self-minted connection share. Not the steward's:
//! any server that admits by bucket keeps one.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use crate::steward::Denied;

type Res<T> = Result<T, Denied>;

/// An authenticated server bucket. Account zero deliberately groups per badge, as specified
/// for server admission (not the kernel's separate R2 system-budget grouping).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AdmissionBucket {
    pub account: u64,
    pub labels: Vec<u64>,
    pub system_badge: Option<u64>,
}
#[derive(Clone, Debug)]
pub struct ConnectionShare {
    pub bucket: AdmissionBucket,
    pub root: u64,
    pub used: usize,
}
/// Observable admission abstraction for the self-minted share (servers/serving.md R26). `grant`
/// presupposes the server already checked mint authority; bucket metadata comes from authenticated
/// kernel metadata. It grants no authority to mint across accounts or labels and models no confined
/// placement topology.
#[derive(Clone, Debug)]
pub struct ConnectionShares {
    pub connections: BTreeMap<u64, ConnectionShare>,
    issued: BTreeSet<u64>,
    cap: usize,
}
impl ConnectionShares {
    pub fn new(cap: usize) -> Res<Self> {
        if cap < 2 {
            return Err(Denied::Cap);
        }
        Ok(Self { connections: BTreeMap::new(), issued: BTreeSet::new(), cap })
    }

    /// `through=None` is a separately authorized grant to a client. A self-minted descendant
    /// keeps its parent's share transitively while used within the same bucket.
    pub fn grant(&mut self, badge: u64, account: u64, mut labels: Vec<u64>, through: Option<u64>) -> Res<()> {
        if badge == 0 || self.issued.contains(&badge) {
            return Err(Denied::BadKey);
        }
        labels.sort_unstable();
        labels.dedup();
        let bucket = AdmissionBucket { account, labels, system_badge: (account == 0).then_some(badge) };
        let root = match through {
            Some(parent) => {
                let p = self.connections.get(&parent).ok_or(Denied::UnknownSession)?;
                if p.bucket == bucket { p.root } else { badge }
            }
            None => badge,
        };
        self.issued.insert(badge);
        self.connections.insert(badge, ConnectionShare { bucket, root, used: 0 });
        Ok(())
    }

    pub fn admit(&mut self, badge: u64) -> Res<()> {
        let c = self.connections.get(&badge).ok_or(Denied::UnknownSession)?.clone();
        let bucket: Vec<_> = self.connections.values().filter(|x| x.bucket == c.bucket).collect();
        let roots: BTreeSet<_> = bucket.iter().map(|x| x.root).collect();
        let total: usize = bucket.iter().map(|x| x.used).sum();
        let share: usize = bucket.iter().filter(|x| x.root == c.root).map(|x| x.used).sum();
        if total >= self.cap || share >= (self.cap / roots.len()).max(1) {
            return Err(Denied::Cap);
        }
        self.connections.get_mut(&badge).unwrap().used += 1;
        Ok(())
    }

    pub fn release(&mut self, badge: u64) -> Res<()> {
        let c = self.connections.get_mut(&badge).ok_or(Denied::UnknownSession)?;
        if c.used == 0 {
            return Err(Denied::Cap);
        }
        c.used -= 1;
        Ok(())
    }

    pub fn disconnect(&mut self, badge: u64) -> Res<()> {
        self.connections.remove(&badge).map(|_| ()).ok_or(Denied::UnknownSession)
    }
}
