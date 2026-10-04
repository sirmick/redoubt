//! The one error type of every call.

use redoubt_rt::abi;
use redoubt_rt::client::ClientError;
use redoubt_rt::startup::StartupError;
use redoubt_rt::wire;

/// Why a call failed: the kernel's refusal, a server gone, a reply that does not decode or answer
/// the request, the server's own refusal, or a refusal made here before any call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The kernel refused the call (anything but `Dead`).
    Sys(abi::Error),
    /// The server has gone (`Dead`). The library never reconnects: a new connection is its
    /// launcher's to grant (servers/init.md, "Restarts and reboots").
    Disconnected,
    /// A request that does not encode, or a reply that does not decode.
    Wire(wire::Error),
    /// A reply that decodes but does not answer the request (words, handles, tag, type or count).
    Unexpected,
    /// The server's typed error code (the protocol's `ErrorCode::from_code` names it; 1 is
    /// `Malformed` in every protocol).
    Server(u32),
    /// The server answered a 9P request with `Rerror` (its text is not kept), or walked only part
    /// of a path.
    Rerror,
    /// Refused here, before any call was made.
    Refused(Refusal),
}

/// What the library refuses before making a call: what the call could not carry, or what would
/// name something that is not the caller's. Never a check a server makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A path that does not clean, is not absolute where it must be, or has more than 16
    /// components (one `Twalk`).
    BadPath,
    /// The connection's fids are all in use (at most the skeleton's `MAX_FIDS` per connection).
    NoFid,
    /// A multiplexed connection's tags are all in use (at most the skeleton's `MAX_TAGS`).
    NoTag,
    /// Two open files on different connections, in one `fsd` operation: their fids name files on
    /// different servers, or other files entirely.
    OtherConnection,
    /// A launch naming more than `MAX_START_HANDLES` handles.
    TooManyHandles,
    /// A launch with no image.
    EmptyImage,
    /// A launch whose stack does not fit below `STACK_TOP`.
    StackTooLarge,
    /// A startup block its parser refuses (a bad name, a path given twice, too long).
    Startup(StartupError),
}

impl From<ClientError> for Error {
    fn from(e: ClientError) -> Error {
        match e {
            ClientError::Sys(e) => e.into(),
            ClientError::Wire(e) | ClientError::Encode(e) => Error::Wire(e),
            ClientError::Pages(e) => e.into(),
            ClientError::Remote => Error::Rerror,
            ClientError::Unexpected => Error::Unexpected,
            ClientError::BadPath => Error::Refused(Refusal::BadPath),
        }
    }
}

impl From<abi::Error> for Error {
    fn from(e: abi::Error) -> Error {
        match e {
            abi::Error::Dead => Error::Disconnected,
            e => Error::Sys(e),
        }
    }
}

impl From<wire::Error> for Error {
    fn from(e: wire::Error) -> Error { Error::Wire(e) }
}

impl From<Refusal> for Error {
    fn from(refusal: Refusal) -> Error { Error::Refused(refusal) }
}
