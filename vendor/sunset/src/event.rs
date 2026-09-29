#[allow(unused_imports)]
use {
    crate::error::{Error, Result, TrapBug},
    log::{debug, error, info, log, trace, warn},
    subtle::ConstantTimeEq,
};

use core::fmt::Debug;

use crate::*;
use channel::{CliSessionExit, CliSessionOpener};
use packets::Packet;
use runner::{CliRunner, ServRunner};
use sshwire::TextString;

#[derive(Debug)]
pub enum Event<'g, 'a> {
    Cli(CliEvent<'g, 'a>),
    Serv(ServEvent<'g, 'a>),

    /// Connection state has progressed
    ///
    /// Should poll `Runner::progress()` again
    Progressed,

    /// No event
    ///
    /// No progress, may idle waiting for external events.
    None,
}

/// Client events.
///
/// These events are returned by the `progress()` function
/// which is polled during the course of the connection.
/// The application can call response functions on the associated
/// enum item, for example providing a username or password
/// for authentication.
pub enum CliEvent<'g, 'a> {
    Hostkey(CheckHostkey<'g, 'a>),
    Banner(Banner<'g>),
    Username(RequestUsername<'g, 'a>),
    Password(RequestPassword<'g, 'a>),
    Pubkey(RequestPubkey<'g, 'a>),
    AgentSign(RequestSign<'g, 'a>),
    Authenticated,
    SessionOpened(CliSessionOpener<'g, 'a>),
    /// Remote process exited
    SessionExit(CliSessionExit<'g>),

    // ChanRequest(ChanRequest<'g, 'a>),
    // Banner { banner: TextString<'a>, language: TextString<'a> },
    /// The SSH connection is no longer running
    Defunct,

    /// No event was returned.
    ///
    /// The caller should poll `progress()` again.
    // TODO: remove this after polonius lands in rustc.
    // [#70255](https://github.com/rust-lang/rust/issues/70255), eventually
    PollAgain,
}

impl Debug for CliEvent<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let e = match self {
            Self::Hostkey(_) => "Hostkey",
            Self::Username(_) => "Username",
            Self::Password(_) => "Password",
            Self::Pubkey(_) => "Pubkey",
            Self::Authenticated => "Authenticated",
            Self::SessionOpened(_) => "SessionOpened",
            Self::SessionExit(_) => "SessionExit",
            Self::AgentSign(_) => "AgentSign",
            Self::Banner(_) => "Banner",
            Self::Defunct => "Defunct",
            Self::PollAgain => "PollAgain",
        };
        write!(f, "CliEvent({e})")
    }
}

pub struct RequestUsername<'g, 'a> {
    runner: &'g mut CliRunner<'a>,
}

impl RequestUsername<'_, '_> {
    pub fn username(self, username: impl AsRef<str>) -> Result<()> {
        self.runner.resume_cliusername(username.as_ref())
    }
}

pub struct RequestPassword<'g, 'a> {
    runner: &'g mut CliRunner<'a>,
}

impl RequestPassword<'_, '_> {
    /// Provide a password to try.
    pub fn password(self, password: impl AsRef<str>) -> Result<()> {
        self.runner.resume_clipassword(Some(password.as_ref()))
    }

    /// Don't provide a password.
    ///
    /// `RequestPassword` will not be returned again.
    pub fn skip(self) -> Result<()> {
        self.runner.resume_clipassword(None)
    }
}

pub struct RequestPubkey<'g, 'a> {
    runner: &'g mut CliRunner<'a>,
}

impl<'g, 'a> RequestPubkey<'g, 'a> {
    /// Provide a public key to try.
    pub fn pubkey(self, signkey: SignKey) -> Result<()> {
        self.runner.resume_clipubkey(Some(signkey))
    }

    /// Don't provide a public key.
    ///
    /// `RequestPubkey` will not be returned again.
    pub fn skip(self) -> Result<()> {
        self.runner.resume_clipubkey(None)
    }
}

pub struct RequestSign<'g, 'a> {
    runner: &'g mut CliRunner<'a>,
}

impl RequestSign<'_, '_> {
    pub fn key(&self) -> Result<&SignKey> {
        self.runner.fetch_agentsign_key()
    }
    pub fn message(&self) -> Result<AuthSigMsg<'_>> {
        self.runner.fetch_agentsign_msg()
    }
    pub fn signed(self, sig: &OwnedSig) -> Result<()> {
        self.runner.resume_agentsign(Some(sig))
    }
    pub fn skip(self) -> Result<()> {
        self.runner.resume_agentsign(None)
    }
}

pub struct CheckHostkey<'g, 'a> {
    runner: &'g mut CliRunner<'a>,
}

impl CheckHostkey<'_, '_> {
    pub fn hostkey(&self) -> Result<PubKey<'_>> {
        self.runner.fetch_checkhostkey()
    }

    pub fn accept(self) -> Result<()> {
        self.runner.resume_checkhostkey(true)
    }

    pub fn reject(self) -> Result<()> {
        self.runner.resume_checkhostkey(false)
    }
}

pub struct Banner<'a>(pub(crate) packets::UserauthBanner<'a>);

impl Banner<'_> {
    pub fn banner(&self) -> Result<&str> {
        self.0.message.to_str()
    }

    pub fn raw_banner(&self) -> TextString<'_> {
        self.0.message
    }
}

// Only small values should be stored inline.
// Larger state is retrieved from the current packet via Runner::fetch_*()
#[derive(Debug, Clone)]
pub(crate) enum CliEventId {
    Hostkey,
    Username,
    Password,
    Pubkey,
    AgentSign,
    Authenticated,
    SessionOpened(ChanNum),
    SessionExit,
    Banner,
    #[expect(unused)]
    Defunct,
    // TODO:
    // Disconnected
    // OpenTCPForwarded (new session)
    // TCPDirectOpened (response)
}

impl CliEventId {
    pub fn event<'g, 'a>(
        self,
        runner: &'g mut CliRunner<'a>,
    ) -> Result<CliEvent<'g, 'a>> {
        let pk = runner.packet()?;

        match self {
            Self::Username => Ok(CliEvent::Username(RequestUsername { runner })),
            Self::Password => Ok(CliEvent::Password(RequestPassword { runner })),
            Self::Pubkey => Ok(CliEvent::Pubkey(RequestPubkey { runner })),
            Self::AgentSign => Ok(CliEvent::AgentSign(RequestSign { runner })),
            Self::Hostkey => {
                debug_assert!(matches!(pk, Some(Packet::KexDHReply(_))));
                Ok(CliEvent::Hostkey(CheckHostkey { runner }))
            }
            Self::Authenticated => Ok(CliEvent::Authenticated),
            Self::SessionOpened(h) => {
                Ok(CliEvent::SessionOpened(runner.cli_session_opener(h)?))
            }
            // (Self::Banner, Packet::UserauthBanner(p)) => {
            //     CliEvent::Banner { banner: p.message, language: p.lang }
            // }
            Self::SessionExit => {
                Ok(CliEvent::SessionExit(runner.fetch_cli_session_exit()?))
            }
            Self::Banner => Ok(CliEvent::Banner(runner.fetch_cli_banner()?)),
            Self::Defunct => error::BadUsage.fail(),
        }
    }

    // Whether the event must have called an appropriate `resume_` method.
    // Used for internal correctness checks.
    //
    // Note that some of these are called by the event's Drop handler
    // with a default (eg reject auth).
    pub(crate) fn needs_resume(&self) -> bool {
        match self {
            Self::Authenticated
            | Self::SessionOpened(_)
            | Self::SessionExit
            | Self::Banner
            | Self::Defunct => false,
            Self::Hostkey
            | Self::Username
            | Self::Password
            | Self::Pubkey
            | Self::AgentSign => true,
        }
    }
}

/// Server events.
///
/// These events are returned by the `progress()` function
/// which is polled during the course of the connection.
/// The application can call response functions on the associated
/// enum item, for example accepting or requesting a client request.
pub enum ServEvent<'g, 'a> {
    /// Request hostkeys to use for the session
    Hostkeys(ServHostkeys<'g, 'a>),
    /// Sign the exchange with a host key held outside sunset.
    ///
    /// Follows `Hostkeys` when the key chosen is public-only
    /// ([`SignKey::from_agent_pubkey`]).
    SignExchange(ServSignExchange<'g, 'a>),
    /// Client's first authentication attempt.
    ///
    /// This can be used to capture the username.
    /// The application can accept a login without any
    /// authentication by calling ServerFirstAuth::allow().
    FirstAuth(ServFirstAuth<'g, 'a>),
    /// Client's password authentication attempt.
    ///
    /// `ServerPasswordAuth::allow()` will allow the user to log in.
    PasswordAuth(ServPasswordAuth<'g, 'a>),
    /// Client's public key authentication attempt.
    ///
    /// `ServerPubkeyAuth::allow()` will allow the user to log in.
    ///
    /// Note that this event may be emitted multiple times,
    /// since the client first queries acceptable public keys,
    /// and then later sends an actual signature. The application
    /// should give consistent `allow()`/`deny()` (default) responses
    /// for requests of the same key.
    PubkeyAuth(ServPubkeyAuth<'g, 'a>),
    /// Authentication success.
    ///
    /// Emitted when a client first successfully authenticates.
    Authenticated,
    /// Client's request for a session channel.
    ///
    /// After accepting a channel the [`ChanHandle`] will be returned.
    OpenSession(ServOpenSession<'g, 'a>),
    /// Client requested to run a shell on a channel.
    SessionShell(ServShellRequest<'g, 'a>),
    /// Client requested to execute a command on a channel.
    SessionExec(ServExecRequest<'g, 'a>),
    /// Client requested to execute a subsystem on a channel.
    // Exec and Subsystem are similar enough they can use
    // the same ServExecRequest.
    SessionSubsystem(ServExecRequest<'g, 'a>),
    /// Client requested a PTY for the channel.
    ///
    /// TODO details
    SessionPty(ServPtyRequest<'g, 'a>),
    /// Server has received one environment variable.
    /// Note: input strings are not sanitised.
    SessionEnv(ServEnvironmentRequest<'g, 'a>),
    /// Client's terminal window changed size.
    SessionWinChange(ServWinChangeRequest<'g, 'a>),
    /// Client sent a signal to the channel's process.
    SessionSignal(ServSignalRequest<'g, 'a>),
    /// Client sent a break (RFC 4335).
    SessionBreak(ServBreakRequest<'g, 'a>),

    /// The SSH session is no longer running
    Defunct,

    /// No event was returned.
    ///
    /// The caller should poll `progress()` again.
    // TODO: remove this after polonius lands in rustc.
    // [#70255](https://github.com/rust-lang/rust/issues/70255), eventually
    PollAgain,
}

impl Debug for ServEvent<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let e = match self {
            Self::Hostkeys(_) => "Hostkeys",
            Self::SignExchange(_) => "SignExchange",
            Self::PasswordAuth(_) => "PasswordAuth",
            Self::PubkeyAuth(_) => "PubkeyAuth",
            Self::FirstAuth(_) => "FirstAuth",
            Self::Authenticated => "Authenticated",
            Self::OpenSession(_) => "OpenSession",
            Self::SessionShell(_) => "SessionShell",
            Self::SessionExec(_) => "SessionExec",
            Self::SessionSubsystem(_) => "SessionSubsystem",
            Self::SessionPty(_) => "SessionPty",
            Self::SessionEnv(_) => "Environment",
            Self::SessionWinChange(_) => "SessionWinChange",
            Self::SessionSignal(_) => "SessionSignal",
            Self::SessionBreak(_) => "SessionBreak",
            Self::Defunct => "Defunct",
            Self::PollAgain => "PollAgain",
        };
        write!(f, "ServEvent({e})")
    }
}

/// Provide server hostkeys.
///
/// `hostkeys()` must be called.
pub struct ServHostkeys<'g, 'a> {
    runner: &'g mut Runner<'a, server::Server>,
}

impl<'g, 'a> ServHostkeys<'g, 'a> {
    /// Provide the list of hostkeys to use.
    ///
    /// This function must be called, with at least one hostkey.
    pub fn hostkeys(self, keys: &[&SignKey]) -> Result<()> {
        self.runner.resume_servhostkeys(keys)
    }
}

/// The parts of an exchange hash (RFC 4253 section 8) but the host key
/// `K_S`, which the signer takes from its own key.
#[derive(Debug, Clone, Copy)]
pub struct ExchangeTranscript<'a> {
    /// The client's identification string.
    pub v_c: &'a [u8],
    /// The server's identification string.
    pub v_s: &'a [u8],
    /// The client's `SSH_MSG_KEXINIT` payload.
    pub i_c: &'a [u8],
    /// The server's `SSH_MSG_KEXINIT` payload.
    pub i_s: &'a [u8],
    /// The client's ephemeral public key.
    pub q_c: &'a [u8],
    /// The server's ephemeral public key.
    pub q_s: &'a [u8],
    /// The shared secret as an `mpint` body: no leading zeros, and a zero
    /// byte before a set top bit.
    pub k: &'a [u8],
}

/// Sign the exchange hash with a host key held outside sunset.
///
/// `signed()` must be called: dropping this fails the next `progress()`,
/// which ends the connection. The signature is checked against sunset's
/// own exchange hash before it is sent.
pub struct ServSignExchange<'g, 'a> {
    runner: &'g mut Runner<'a, server::Server>,
}

impl ServSignExchange<'_, '_> {
    /// The parts to sign from.
    pub fn transcript(&self) -> Result<ExchangeTranscript<'_>> {
        self.runner.fetch_exchange_transcript()
    }

    /// Provide the signature, which sends `KEXDH_REPLY`.
    pub fn signed(self, sig: &OwnedSig) -> Result<()> {
        self.runner.resume_signexchange(sig)
    }
}

pub struct ServPasswordAuth<'g, 'a> {
    runner: &'g mut ServRunner<'a>,
    done: bool,
}

impl<'g, 'a> ServPasswordAuth<'g, 'a> {
    fn new(runner: &'g mut ServRunner<'a>) -> Self {
        Self { runner, done: false }
    }

    /// Retrieve the username.
    ///
    /// The username will be the same between all
    /// `ServFirstAuth`, `ServPasswordAuth`, and `ServPubkeyAuth`
    /// events in a session.
    pub fn username(&self) -> Result<&str> {
        self.raw_username()?.to_str()
    }

    /// Perform a constant-time comparison of the user-presented username against a passed string.
    pub fn matches_username(
        &self,
        username: impl core::convert::AsRef<str>,
    ) -> bool {
        match self.username() {
            Ok(u) => u.as_bytes().ct_eq(username.as_ref().as_bytes()).into(),
            _ => false,
        }
    }

    /// Retrieve the password presented by the user.
    ///
    /// When comparing with an expected password or hash, take
    /// care to use timing-insensitive comparison, for example
    /// by using [`subtle`](https://docs.rs/subtle/latest/subtle/) crate.
    pub fn password(&self) -> Result<&str> {
        self.raw_password()?.to_str()
    }

    /// Perform a constant-time comparison of the user-presented password against a passed string.
    /// # Caution
    /// This is better than a naive comparison, but passwords should be hashed and stored using a
    /// platform-appropriate password hashing function. Consider bcrypt, argon2, or pbkdf2.
    pub fn matches_password(
        &self,
        password: impl core::convert::AsRef<str>,
    ) -> bool {
        match self.password() {
            Ok(p) => p.as_bytes().ct_eq(password.as_ref().as_bytes()).into(),
            _ => false,
        }
    }

    /// Accept the presented password.
    pub fn allow(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_servauth(true)
    }

    /// Does not need to be called explicitly, also occurs on drop without `allow()`
    pub fn reject(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_servauth(false)
    }

    /// Enable or disable password authentication for subsequent attempts.
    ///
    /// # Caution
    /// Enabling or disabling authentication methods based on username can
    /// unintentionally enable user enumeration attacks.
    pub fn enable_password_auth(&mut self, enabled: bool) -> Result<()> {
        let (_, pubkey) = self.runner.auth_methods()?;
        self.runner.set_auth_methods(enabled, pubkey)
    }

    /// Enable or disable public key authentication for subsequent attempts.
    ///
    /// # Caution
    /// Enabling or disabling authentication methods based on username can
    /// unintentionally enable user enumeration attacks.
    pub fn enable_pubkey_auth(&mut self, enabled: bool) -> Result<()> {
        let (password, _) = self.runner.auth_methods()?;
        self.runner.set_auth_methods(password, enabled)
    }

    /// Configure which authentication methods are allowed for subsequent attempts.
    ///
    /// # Caution
    /// Enabling or disabling authentication methods based on username can
    /// unintentionally enable user enumeration attacks.
    pub fn set_auth_methods(&mut self, password: bool, pubkey: bool) -> Result<()> {
        self.runner.set_auth_methods(password, pubkey)
    }

    pub fn raw_username(&self) -> Result<TextString<'_>> {
        self.runner.fetch_servusername()
    }

    pub fn raw_password(&self) -> Result<TextString<'_>> {
        self.runner.fetch_servpassword()
    }
}

// implement Drop to be the same as .reject()
impl Drop for ServPasswordAuth<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = self.runner.resume_servauth(false) {
                trace!("Error for pw auth: {e}")
            }
        }
    }
}

pub struct ServPubkeyAuth<'g, 'a> {
    runner: &'g mut ServRunner<'a>,
    done: bool,
    // Indicates whether this was a real auth request (signature already
    // verified) or a query for public key suitability.
    real_sig: bool,
}

impl<'g, 'a> ServPubkeyAuth<'g, 'a> {
    fn new(runner: &'g mut ServRunner<'a>, real_sig: bool) -> Self {
        Self { runner, done: false, real_sig }
    }

    /// Retrieve the username.
    ///
    /// The username will be the same between all
    /// `ServFirstAuth`, `ServPasswordAuth`, and `ServPubkeyAuth`
    /// events in a session.
    pub fn username(&self) -> Result<&str> {
        self.raw_username()?.to_str()
    }

    /// Retrieve the public key presented by a client.
    pub fn pubkey(&self) -> Result<PubKey<'_>> {
        self.runner.fetch_servpubkey()
    }

    /// Whether the client signed with the key (the signature is already
    /// verified), rather than only asking whether the key would do.
    pub fn signed(&self) -> bool {
        self.real_sig
    }

    /// Accept the presented public key.
    pub fn allow(mut self) -> Result<()> {
        self.done = true;
        if self.real_sig {
            self.runner.resume_servauth(true)
        } else {
            self.runner.resume_servauth_pkok()
        }
    }

    /// Reject the public key.
    ///
    /// Does not need to be called explicitly, also occurs on drop without `allow()`
    pub fn reject(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_servauth(false)
    }

    /// Enable or disable password authentication for subsequent attempts.
    ///
    /// # Caution
    /// Enabling or disabling authentication methods based on username can
    /// unintentionally enable user enumeration attacks.
    pub fn enable_password_auth(&mut self, enabled: bool) -> Result<()> {
        let (_, pubkey) = self.runner.auth_methods()?;
        self.runner.set_auth_methods(enabled, pubkey)
    }

    /// Enable or disable public key authentication for subsequent attempts.
    ///
    /// # Caution
    /// Enabling or disabling authentication methods based on username can
    /// unintentionally enable user enumeration attacks.
    pub fn enable_pubkey_auth(&mut self, enabled: bool) -> Result<()> {
        let (password, _) = self.runner.auth_methods()?;
        self.runner.set_auth_methods(password, enabled)
    }

    /// Configure which authentication methods are allowed for subsequent attempts.
    ///
    /// # Caution
    /// Enabling or disabling authentication methods based on username can
    /// unintentionally enable user enumeration attacks.
    pub fn set_auth_methods(&mut self, password: bool, pubkey: bool) -> Result<()> {
        self.runner.set_auth_methods(password, pubkey)
    }

    pub fn raw_username(&self) -> Result<TextString<'_>> {
        self.runner.fetch_servusername()
    }
}

// implement Drop to be the same as .reject()
impl Drop for ServPubkeyAuth<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = self.runner.resume_servauth(false) {
                trace!("Error for pw auth: {e}")
            }
        }
    }
}

pub struct ServFirstAuth<'g, 'a> {
    runner: &'g mut ServRunner<'a>,
    done: bool,
}

impl<'g, 'a> ServFirstAuth<'g, 'a> {
    fn new(runner: &'g mut ServRunner<'a>) -> Self {
        Self { runner, done: false }
    }

    /// Retrieve the username presented by the client.
    ///
    /// The username will be the same between all
    /// `ServFirstAuth`, `ServPasswordAuth`, and `ServPubkeyAuth`
    /// events in a session.
    pub fn username(&self) -> Result<&str> {
        self.raw_username()?.to_str()
    }

    /// Perform a constant-time comparison of the user-presented username against a passed string.
    pub fn matches_username(
        &self,
        username: impl core::convert::AsRef<str>,
    ) -> bool {
        match self.username() {
            Ok(u) => u.as_bytes().ct_eq(username.as_ref().as_bytes()).into(),
            _ => false,
        }
    }

    /// Allow the user to log in.
    ///
    /// No further authentication challenges will be requested.
    pub fn allow(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_servauth(true)
    }

    /// Don't allow the user to log in immediately.
    ///
    /// Subsequent authentication requests (eg password or pubkey)
    /// may still succeed.
    /// Does not need to be called explicitly, also occurs on drop without `allow()`
    pub fn reject(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_servauth(false)
    }

    /// Enable or disable password authentication for this session.
    ///
    /// # Caution
    /// Enabling or disabling authentication methods based on username can
    /// unintentionally enable user enumeration attacks.
    pub fn enable_password_auth(&mut self, enabled: bool) -> Result<()> {
        let (_, pubkey) = self.runner.auth_methods()?;
        self.runner.set_auth_methods(enabled, pubkey)
    }

    /// Enable or disable public key authentication for this session.
    ///
    /// # Caution
    /// Enabling or disabling authentication methods based on username can
    /// unintentionally enable user enumeration attacks.
    pub fn enable_pubkey_auth(&mut self, enabled: bool) -> Result<()> {
        let (password, _) = self.runner.auth_methods()?;
        self.runner.set_auth_methods(password, enabled)
    }

    /// Configure which authentication methods are allowed.
    ///
    /// # Caution
    /// Enabling or disabling authentication methods based on username can
    /// unintentionally enable user enumeration attacks.
    pub fn set_auth_methods(&mut self, password: bool, pubkey: bool) -> Result<()> {
        self.runner.set_auth_methods(password, pubkey)
    }

    pub fn raw_username(&self) -> Result<TextString<'_>> {
        self.runner.fetch_servusername()
    }
}

// implement Drop to be the same as .reject()
impl Drop for ServFirstAuth<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = self.runner.resume_servauth(false) {
                trace!("Error for first auth: {e}")
            }
        }
    }
}

pub struct ServOpenSession<'g, 'a> {
    runner: &'g mut ServRunner<'a>,
    done: bool,
    num: ChanNum,
}

impl<'g, 'a> ServOpenSession<'g, 'a> {
    fn new(runner: &'g mut ServRunner<'a>, num: ChanNum) -> Self {
        Self { runner, done: false, num }
    }
    pub fn accept(mut self) -> Result<ChanHandle> {
        self.done = true;
        self.runner.resume_chanopen(self.num, None)?;
        Ok(ChanHandle(self.num))
    }

    /// Does not need to be called explicitly, also occurs on drop without `accept()`
    pub fn reject(mut self, reason: ChanFail) -> Result<()> {
        self.done = true;
        self.runner.resume_chanopen(self.num, Some(reason))
    }
}

// implement Drop to be the same as .reject()
impl Drop for ServOpenSession<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = self.runner.resume_chanopen(
                self.num,
                Some(ChanFail::SSH_OPEN_ADMINISTRATIVELY_PROHIBITED),
            ) {
                trace!("Error for chanopen: {e}")
            }
        }
    }
}

pub struct ServShellRequest<'g, 'a> {
    runner: &'g mut Runner<'a, Server>,
    num: ChanNum,
    done: bool,
}

impl<'g, 'a> ServShellRequest<'g, 'a> {
    fn new(runner: &'g mut Runner<'a, Server>, num: ChanNum) -> Self {
        Self { runner, num, done: false }
    }
    /// Indicate that the request succeeded.
    ///
    /// Note that if the peer didn't request a reply, this call
    /// will not do anything.
    pub fn succeed(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(true)
    }

    /// Indicate that the request failed.
    ///
    /// Note that if the peer didn't request a reply, this call
    /// will not do anything.
    /// Does not need to be called explicitly, also occurs on drop without `accept()`
    pub fn fail(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(false)
    }

    /// Return the associated channel number.
    ///
    /// This will correspond to a `ChanHandle::num()`
    /// from a previous [`ServOpenSession`] event.
    pub fn channel(&self) -> ChanNum {
        self.num
    }

    // TODO: does the app care about wantreply?
}

// implement Drop to be the same as .fail()
impl Drop for ServShellRequest<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = self.runner.resume_chanreq(false) {
                trace!("Error for shellreq: {e}")
            }
        }
    }
}

/// A channel `exec` or `subsystem` request.
pub struct ServExecRequest<'g, 'a> {
    runner: &'g mut Runner<'a, Server>,
    num: ChanNum,
    done: bool,
}

impl<'g, 'a> ServExecRequest<'g, 'a> {
    fn new(runner: &'g mut Runner<'a, Server>, num: ChanNum) -> Self {
        Self { runner, num, done: false }
    }

    /// Retrieve the command presented by the client.
    pub fn command(&self) -> Result<&str> {
        self.raw_command()?.to_str()
    }

    pub fn raw_command(&self) -> Result<TextString<'_>> {
        self.runner.fetch_servcommand()
    }

    /// Indicate that the request succeeded.
    ///
    /// Note that if the peer didn't request a reply, this call
    /// will not do anything.
    pub fn succeed(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(true)
    }

    /// Indicate that the request failed.
    ///
    /// Note that if the peer didn't request a reply, this call
    /// will not do anything.
    /// Does not need to be called explicitly, also occurs on drop without `accept()`
    pub fn fail(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(false)
    }

    /// Return the associated channel number.
    ///
    /// This will correspond to a `ChanHandle::num()`
    /// from a previous [`ServOpenSession`] event.
    pub fn channel(&self) -> ChanNum {
        self.num
    }

    // TODO: does the app care about wantreply?
}

// implement Drop to be the same as .fail()
impl Drop for ServExecRequest<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = self.runner.resume_chanreq(false) {
                trace!("Error for shellreq: {e}")
            }
        }
    }
}

/// A PTY request
///
/// Terminal modes are not yet parsed.
pub struct ServPtyRequest<'g, 'a> {
    runner: &'g mut Runner<'a, Server>,
    num: ChanNum,
    done: bool,
}

impl<'g, 'a> ServPtyRequest<'g, 'a> {
    fn new(runner: &'g mut Runner<'a, Server>, num: ChanNum) -> Self {
        Self { runner, num, done: false }
    }

    /// The terminal's name and starting size, with no modes. Fails for a
    /// name longer than `config::MAX_TERM` or not ASCII.
    pub fn pty(&self) -> Result<Pty> {
        self.runner.fetch_pty()
    }

    /// Indicate that the request succeeded.
    ///
    /// Note that if the peer didn't request a reply, this call
    /// will not do anything.
    pub fn succeed(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(true)
    }

    /// Indicate that the request failed.
    ///
    /// Note that if the peer didn't request a reply, this call
    /// will not do anything.
    /// Does not need to be called explicitly, also occurs on drop without `accept()`
    pub fn fail(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(false)
    }

    /// Return the associated channel number.
    ///
    /// This will correspond to a `ChanHandle::num()`
    /// from a previous [`ServOpenSession`] event.
    pub fn channel(&self) -> ChanNum {
        self.num
    }

    // TODO: does the app care about wantreply?
}

// implement Drop to be the same as .fail()
impl Drop for ServPtyRequest<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = self.runner.resume_chanreq(false) {
                trace!("Error for shellreq: {e}")
            }
        }
    }
}

/// An environment variable request
///
pub struct ServEnvironmentRequest<'g, 'a> {
    runner: &'g mut Runner<'a, Server>,
    num: ChanNum,
    done: bool,
}

impl<'g, 'a> ServEnvironmentRequest<'g, 'a> {
    fn new(runner: &'g mut Runner<'a, Server>, num: ChanNum) -> Self {
        Self { runner, num, done: false }
    }

    /// Indicate that the request succeeded.
    ///
    /// Note that if the peer didn't request a reply, this call
    /// will not do anything.
    pub fn succeed(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(true)
    }

    /// Indicate that the request failed.
    ///
    /// Note that if the peer didn't request a reply, this call
    /// will not do anything.
    /// Does not need to be called explicitly, also occurs on drop without `accept()`
    pub fn fail(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(false)
    }

    /// Return the associated channel number.
    ///
    /// This will correspond to a `ChanHandle::num()`
    /// from a previous [`ServOpenSession`] event.
    pub fn channel(&self) -> ChanNum {
        self.num
    }

    /// Retrieve the name of the environment variable (from NAME=VALUE pair).
    pub fn name(&self) -> Result<&str> {
        self.raw_name()?.to_str()
    }

    /// Retrieve the raw name of the environment variable.
    fn raw_name(&self) -> Result<TextString<'_>> {
        self.runner.fetch_env_name()
    }

    /// Retrieve the value of the environment variable (from NAME=VALUE pair).
    pub fn value(&self) -> Result<&str> {
        self.raw_value()?.to_str()
    }

    /// Retrieve the raw value of the environment variable.
    fn raw_value(&self) -> Result<TextString<'_>> {
        self.runner.fetch_env_value()
    }

    // TODO: does the app care about wantreply?
}

/// A `window-change` request.
pub struct ServWinChangeRequest<'g, 'a> {
    runner: &'g mut Runner<'a, Server>,
    num: ChanNum,
    done: bool,
}

impl<'g, 'a> ServWinChangeRequest<'g, 'a> {
    fn new(runner: &'g mut Runner<'a, Server>, num: ChanNum) -> Self {
        Self { runner, num, done: false }
    }

    /// The new size, in characters and in pixels.
    pub fn size(&self) -> Result<packets::WinChange> {
        self.runner.fetch_winchange()
    }

    /// Indicate that the request succeeded.
    pub fn succeed(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(true)
    }

    /// Indicate that the request failed. Also occurs on drop.
    pub fn fail(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(false)
    }

    /// Return the associated channel number.
    pub fn channel(&self) -> ChanNum {
        self.num
    }
}

impl Drop for ServWinChangeRequest<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = self.runner.resume_chanreq(false) {
                trace!("Error for winchange: {e}")
            }
        }
    }
}

/// A `signal` request.
pub struct ServSignalRequest<'g, 'a> {
    runner: &'g mut Runner<'a, Server>,
    num: ChanNum,
    done: bool,
}

impl<'g, 'a> ServSignalRequest<'g, 'a> {
    fn new(runner: &'g mut Runner<'a, Server>, num: ChanNum) -> Self {
        Self { runner, num, done: false }
    }

    /// The signal's name without "SIG", such as "INT". Not sanitised.
    pub fn signal(&self) -> Result<&str> {
        self.runner.fetch_signal()
    }

    /// Indicate that the request succeeded.
    pub fn succeed(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(true)
    }

    /// Indicate that the request failed. Also occurs on drop.
    pub fn fail(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(false)
    }

    /// Return the associated channel number.
    pub fn channel(&self) -> ChanNum {
        self.num
    }
}

impl Drop for ServSignalRequest<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = self.runner.resume_chanreq(false) {
                trace!("Error for signal: {e}")
            }
        }
    }
}

/// A `break` request (RFC 4335).
pub struct ServBreakRequest<'g, 'a> {
    runner: &'g mut Runner<'a, Server>,
    num: ChanNum,
    done: bool,
}

impl<'g, 'a> ServBreakRequest<'g, 'a> {
    fn new(runner: &'g mut Runner<'a, Server>, num: ChanNum) -> Self {
        Self { runner, num, done: false }
    }

    /// The break's length in milliseconds, as the client sent it.
    pub fn length(&self) -> Result<u32> {
        self.runner.fetch_break()
    }

    /// Indicate that the break was performed.
    pub fn succeed(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(true)
    }

    /// Indicate that the break was not performed. Also occurs on drop.
    pub fn fail(mut self) -> Result<()> {
        self.done = true;
        self.runner.resume_chanreq(false)
    }

    /// Return the associated channel number.
    pub fn channel(&self) -> ChanNum {
        self.num
    }
}

impl Drop for ServBreakRequest<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            if let Err(e) = self.runner.resume_chanreq(false) {
                trace!("Error for break: {e}")
            }
        }
    }
}

// Only small values should be stored inline.
// Larger state is retrieved from the current packet via Runner::fetch_*()
#[derive(Debug, Clone)]
pub(crate) enum ServEventId {
    Hostkeys,
    SignExchange,
    PasswordAuth,
    PubkeyAuth {
        real_sig: bool,
    },
    FirstAuth,
    Authenticated,
    OpenSession {
        num: ChanNum,
    },
    SessionShell {
        num: ChanNum,
    },
    SessionExec {
        num: ChanNum,
    },
    SessionSubsystem {
        num: ChanNum,
    },
    SessionPty {
        num: ChanNum,
    },
    Environment {
        num: ChanNum,
    },
    SessionWinChange {
        num: ChanNum,
    },
    SessionSignal {
        num: ChanNum,
    },
    SessionBreak {
        num: ChanNum,
    },
    #[expect(unused)]
    Defunct,
    // TODO:
    // Disconnected
    // OpenTCPForwarded (new session)
    // TCPDirectOpened (response)
    // Banner
}

impl ServEventId {
    pub fn event<'g, 'a>(
        self,
        runner: &'g mut ServRunner<'a>,
    ) -> Result<ServEvent<'g, 'a>> {
        let p = if cfg!(debug_assertions) { runner.packet()? } else { None };

        match self {
            Self::Hostkeys => {
                debug_assert!(matches!(p, Some(Packet::KexDHInit(_))));
                Ok(ServEvent::Hostkeys(ServHostkeys { runner }))
            }
            Self::SignExchange => {
                // Emitted by the Runner after Hostkeys, not from a packet.
                Ok(ServEvent::SignExchange(ServSignExchange { runner }))
            }
            Self::PasswordAuth => {
                debug_assert!(matches!(p, Some(Packet::UserauthRequest(_))));
                Ok(ServEvent::PasswordAuth(ServPasswordAuth::new(runner)))
            }
            Self::PubkeyAuth { real_sig } => {
                debug_assert!(matches!(p, Some(Packet::UserauthRequest(_))));
                Ok(ServEvent::PubkeyAuth(ServPubkeyAuth::new(runner, real_sig)))
            }
            Self::FirstAuth => {
                debug_assert!(matches!(p, Some(Packet::UserauthRequest(_))));
                Ok(ServEvent::FirstAuth(ServFirstAuth::new(runner)))
            }
            Self::Authenticated => {
                // Emitted by the Runner, not from a packet.
                Ok(ServEvent::Authenticated)
            }
            Self::OpenSession { num } => {
                debug_assert!(matches!(p, Some(Packet::ChannelOpen(_))));
                Ok(ServEvent::OpenSession(ServOpenSession::new(runner, num)))
            }
            Self::SessionShell { num } => {
                debug_assert!(matches!(p, Some(Packet::ChannelRequest(_))));
                Ok(ServEvent::SessionShell(ServShellRequest::new(runner, num)))
            }
            Self::SessionExec { num } => {
                debug_assert!(matches!(p, Some(Packet::ChannelRequest(_))));
                Ok(ServEvent::SessionExec(ServExecRequest::new(runner, num)))
            }
            Self::SessionSubsystem { num } => {
                debug_assert!(matches!(p, Some(Packet::ChannelRequest(_))));
                Ok(ServEvent::SessionSubsystem(ServExecRequest::new(runner, num)))
            }
            Self::SessionPty { num } => {
                debug_assert!(matches!(p, Some(Packet::ChannelRequest(_))));
                Ok(ServEvent::SessionPty(ServPtyRequest::new(runner, num)))
            }
            Self::Environment { num } => {
                debug_assert!(matches!(p, Some(Packet::ChannelRequest(_))));
                Ok(ServEvent::SessionEnv(ServEnvironmentRequest::new(runner, num)))
            }
            Self::SessionWinChange { num } => {
                debug_assert!(matches!(p, Some(Packet::ChannelRequest(_))));
                Ok(ServEvent::SessionWinChange(ServWinChangeRequest::new(
                    runner, num,
                )))
            }
            Self::SessionSignal { num } => {
                debug_assert!(matches!(p, Some(Packet::ChannelRequest(_))));
                Ok(ServEvent::SessionSignal(ServSignalRequest::new(runner, num)))
            }
            Self::SessionBreak { num } => {
                debug_assert!(matches!(p, Some(Packet::ChannelRequest(_))));
                Ok(ServEvent::SessionBreak(ServBreakRequest::new(runner, num)))
            }
            Self::Defunct => Ok(ServEvent::Defunct),
        }
    }

    // Whether the event must have called an appropriate `resume_` method.
    // Used for internal correctness checks.
    pub(crate) fn needs_resume(&self) -> bool {
        match self {
            Self::Defunct | Self::Authenticated => false,
            Self::Hostkeys
            | Self::SignExchange
            | Self::FirstAuth
            | Self::PasswordAuth
            | Self::PubkeyAuth { .. }
            | Self::OpenSession { .. }
            | Self::SessionShell { .. }
            | Self::SessionExec { .. }
            | Self::SessionSubsystem { .. }
            | Self::Environment { .. }
            | Self::SessionWinChange { .. }
            | Self::SessionSignal { .. }
            | Self::SessionBreak { .. }
            | Self::SessionPty { .. } => true,
        }
    }
}
