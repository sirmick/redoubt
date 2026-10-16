//! What a server's fault costs the caller it was serving (servers/init.md, "Restarts and
//! reboots", "Blame"): the exit notice names the account and label set of the faulting thread's
//! current call ([R21](../../../docs/kernel/processes.md)), and `init` passes them to the steward
//! in one `blame` call, after the restart and within [`TIMEOUT`], so a wedged steward cannot
//! stall a restart; a blame lost to the timeout is said on the console and counts toward no
//! lockout. Pure: the decision and the lines, which the boot prints.

use alloc::format;
use alloc::string::String;

use redoubt_client::Error;
use redoubt_rt::abi;

/// How long `init` waits for the steward to take a blame, in µs: one second, as every release
/// of a dead child's grants waits ([`redoubt_client::grants::RELEASE_TIMEOUT`]).
pub const TIMEOUT: u64 = 1_000_000;

/// Whether a fault's blame goes to the steward: a crash with no current call blames nobody
/// (account 0), and before the steward runs, or when the steward itself is the server that
/// faulted, there is nobody to tell.
pub fn goes_to_the_steward(account: u64, steward_runs: bool) -> bool { account != 0 && steward_runs }

/// How the fault line ends when the blame goes nowhere: `init` says whom the notice named, and
/// why nothing counts it.
pub fn nowhere(account: u64, steward_runs: bool) -> &'static str {
    match (account, steward_runs) {
        (_, false) => "; blamed on nobody: no steward",
        (0, true) => "; blamed on nobody",
        _ => "",
    }
}

/// How the steward took a blame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    /// Answered within the timeout.
    Told,
    /// Not answered within [`TIMEOUT`]: lost, and the crash counts toward no lockout.
    Lost,
    /// The steward ended under the call (`Dead`): its restart empties its tables, so the blame
    /// would count toward nothing anyway.
    Gone,
    /// Refused or failed otherwise: a bug in one of the two.
    Refused,
}

/// The call's result, read as [`Taken`].
pub fn taken(result: Result<(), Error>) -> Taken {
    match result {
        Ok(()) => Taken::Told,
        Err(Error::Sys(abi::Error::Timeout)) => Taken::Lost,
        Err(Error::Disconnected) => Taken::Gone,
        Err(_) => Taken::Refused,
    }
}

/// The line said once the blame was offered: whom the notice named and what became of it.
pub fn said(server: &str, account: u64, labels: &str, taken: Taken) -> String {
    let what = match taken {
        Taken::Told => "the steward took it",
        Taken::Lost => "lost: the steward did not answer within 1 s",
        Taken::Gone => "lost: the steward ended",
        Taken::Refused => "lost: the steward refused it",
    };
    format!("init: {server}'s fault blamed on account {account}, labels {labels}: {what}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blame_goes_to_the_steward_only_for_a_named_account_while_the_steward_runs() {
        assert!(goes_to_the_steward(1002, true));
        assert!(!goes_to_the_steward(0, true));
        assert!(!goes_to_the_steward(1002, false));
        assert_eq!(nowhere(1002, false), "; blamed on nobody: no steward");
        assert_eq!(nowhere(0, true), "; blamed on nobody");
        assert_eq!(nowhere(1002, true), "");
    }

    /// The steward's timeout: the call's `Timeout` is a lost blame, said as such; the restart
    /// has gone ahead already, and nothing is retried.
    #[test]
    fn a_blame_the_steward_does_not_take_in_time_is_lost_and_said() {
        assert_eq!(taken(Err(Error::Sys(abi::Error::Timeout))), Taken::Lost);
        assert_eq!(
            said("walfsd:data", 1002, "none", Taken::Lost),
            "init: walfsd:data's fault blamed on account 1002, labels none: lost: the steward did not answer within 1 s"
        );
        assert_eq!(TIMEOUT, redoubt_client::grants::RELEASE_TIMEOUT);
    }

    #[test]
    fn a_blame_taken_a_dead_steward_and_a_refusal_are_told_apart() {
        assert_eq!(taken(Ok(())), Taken::Told);
        assert_eq!(taken(Err(Error::Disconnected)), Taken::Gone);
        assert_eq!(taken(Err(Error::Server(1))), Taken::Refused);
        assert_eq!(
            said("walfsd:data", 1002, "secrets", Taken::Told),
            "init: walfsd:data's fault blamed on account 1002, labels secrets: the steward took it"
        );
        assert!(said("ipd", 7, "none", Taken::Gone).ends_with("lost: the steward ended"));
        assert!(said("ipd", 7, "none", Taken::Refused).ends_with("lost: the steward refused it"));
    }
}
