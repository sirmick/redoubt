//! `redoubt-client`, the one client API every userland binds to (userland/native.md, "The client
//! library"): native programs link it, beamlet's platform and natives are thin adapters over it,
//! and `init` launches and asks its servers through it.
//!
//! What it holds to:
//! - **Blocking, one call per thread.** Every call takes the caller's [`Lend`], one per thread and reused, so
//!   nothing is mapped behind the caller's back. Or many requests at once on one thread: [`aio`]'s hub owns
//!   their buffers, which go in and come back by value.
//! - **No policy.** It makes no check a server does not make; every refusal is the server's, or the kernel's,
//!   except where a call is refused before it is made ([`Refusal`]).
//! - **Nothing buffered, cached or retried.** One read or write is one 9P request; every open walks from the
//!   connection's root; a connection whose server has gone is [`Error::Disconnected`] on every call, and is
//!   never reconnected.
//! - **No error path drops a handle** (kernel/ipc.md R13): what a reply brought is the caller's or is closed.
//! - **No second copy** of the ABI, the startup encoder or a wire format: [`launch`] writes its block with
//!   the runtime's `StartupBuilder`, and [`typed`] calls the generated codecs.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod aio;
pub mod console;
mod error;
pub mod file;
pub mod grants;
pub mod launch;
pub mod littlefsd;
pub mod ns;
pub mod typed;

pub use error::{Error, Name, Refusal};
pub use redoubt_rt::client::Lend;
