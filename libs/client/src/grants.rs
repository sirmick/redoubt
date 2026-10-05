//! A launcher's ledger of what servers granted one child (servers/wire.md, "A launcher releases
//! its child's grants"): the fresh connections it minted with `new_connection`, and the typed
//! grants it asked for. When the child's exit notice arrives ([`crate::launch::Job::wait`]) every
//! one is released at its server, so a dead child's grants do not hold their servers' admission.
//!
//! Each release waits at most [`RELEASE_TIMEOUT`], so one hung server cannot stop a launcher
//! reaping its children.

use alloc::vec::Vec;

use redoubt_rt::abi::Handle;
use redoubt_rt::client::Lend;
use redoubt_rt::handle::Endpoint;
use redoubt_rt::wire::Error as WireError;
use redoubt_rt::wire::typed::{self, Protocol, Words};

use crate::error::Error;
use crate::file::Connection;

/// How long each release may take, in µs: a second. A release is one short call that a live
/// server answers at once, even loaded; one that has not answered in a second is hung, and the
/// launcher has other children to reap. A timed-out release is reported, not retried.
pub const RELEASE_TIMEOUT: u64 = 1_000_000;

enum Grant {
    /// Minted through `parent`, and disconnected on it by `id`.
    Connection { parent: Connection, id: u64 },
    /// A typed grant, released by calling `at` with the protocol's release request. `at` is the
    /// caller's handle, which must outlive the job: a closed one is a stale slot.
    Typed { at: Handle, release: Words },
}

/// The grants made for one child.
#[derive(Default)]
pub struct Grants {
    entries: Vec<Grant>,
}

impl Grants {
    pub fn new() -> Grants { Grants::default() }

    /// A fresh connection for the child from `parent`'s server, rooted at `root` below
    /// `parent`'s, with `quota` bytes of its own (0 shares `parent`'s), recorded to disconnect.
    pub fn connection(
        &mut self,
        lend: &mut Lend,
        parent: &Connection,
        root: &str,
        quota: u64,
    ) -> Result<Endpoint, Error> {
        let (conn, id) = parent.new_connection(lend, root, quota)?;
        self.entries.push(Grant::Connection { parent: parent.clone(), id });
        Ok(conn)
    }

    /// Records a typed grant the caller made at `at`, to be released with `release`, its
    /// protocol's release request for the grant's id. A release is inline (it names an id), so
    /// it is kept as its words; one that is not is refused (`TooLarge`). `at` is kept as its
    /// handle, not a copy, so it must stay open until the job has ended: closed sooner, its slot
    /// is stale, and the release goes to whatever the slot holds then, or nowhere.
    pub fn record<P: Protocol>(&mut self, at: &Endpoint, release: &P::Message<'_>) -> Result<(), Error> {
        if !P::layout(release)?.inline {
            return Err(WireError::TooLarge.into());
        }
        let release = P::encode(release, &mut [])?;
        self.entries.push(Grant::Typed { at: at.handle(), release });
        Ok(())
    }

    /// Releases every grant at its server, each once and each within [`RELEASE_TIMEOUT`], whatever
    /// happened to the others, and forgets them all. A server's refusal (`not_yours`, or a typed
    /// error) means the grant was already gone, which is not an error and is not retried; the
    /// first failure of a call itself (a timeout among them) is returned, and not retried either.
    pub fn release_all(&mut self) -> Result<(), Error> {
        let mut first = Ok(());
        for grant in self.entries.drain(..) {
            let released = match grant {
                Grant::Connection { parent, id } => match parent.disconnect(id, RELEASE_TIMEOUT) {
                    Err(Error::Rerror(_)) => Ok(()),
                    done => done,
                },
                Grant::Typed { at, release } => release_typed(at, &release),
            };
            if first.is_ok() {
                first = released;
            }
        }
        first
    }
}

/// One typed release: any status is an answer, and a reply's handles are closed.
fn release_typed(at: Handle, release: &Words) -> Result<(), Error> {
    let (reply, _) = Endpoint::from_handle(at).call(release, &[], None, RELEASE_TIMEOUT).into_result()?;
    let handles = reply.handles.as_slice();
    for handle in handles.iter().flatten() {
        let _ = redoubt_rt::handle::close(*handle);
    }
    typed::reply_status(&reply.words, handles.len())?;
    Ok(())
}
