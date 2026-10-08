//! Audit records (servers/steward.md, "The audit log"). A record has one constructor, which
//! takes the domain it is read under, so no record can lack the labels it must be read under.

use alloc::string::String;
use alloc::vec::Vec;

use crate::domain::Domain;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Record {
    Login {
        session: u64,
        principal: usize,
        key: u64,
        /// The context, `None` for the console's session.
        context: Option<String>,
    },
    AgentStarted {
        lease: u64,
        sponsor: usize,
        parent: Option<u64>,
        deadline: u64,
    },
    /// A granted lease that could not start.
    StartFailed {
        lease: u64,
    },
    LeaseEnded {
        lease: u64,
    },
    Submitted {
        request: u64,
    },
    Approved {
        request: u64,
        principal: usize,
        key: u64,
        hash: [u8; 32],
    },
    Denied {
        request: u64,
        principal: usize,
    },
    /// `reader` is the kernel's id of the budget the snapshot was read through, 0 for none.
    Declassified {
        request: u64,
        bytes: Vec<u8>,
        reader: u64,
    },
    CopyFailed {
        request: u64,
    },
    /// `writer` is the kernel's id of the budget the item was written through, 0 for none.
    Pushed {
        request: u64,
        source: u64,
        item: u64,
        bytes: Vec<u8>,
        writer: u64,
    },
    PushFailed {
        request: u64,
    },
    Blamed,
    LockedOut {
        until: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Audit {
    domain: Domain,
    at: u64,
    record: Record,
}

impl Audit {
    /// The one constructor: stamped with `domain`'s account and labels.
    pub(crate) fn new(domain: &Domain, at: u64, record: Record) -> Audit {
        Audit { domain: domain.clone(), at, record }
    }

    /// The domain it is read under.
    pub fn domain(&self) -> &Domain { &self.domain }

    pub fn at(&self) -> u64 { self.at }

    pub fn record(&self) -> &Record { &self.record }
}
