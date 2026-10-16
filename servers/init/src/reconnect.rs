//! A restarted shared server's new connection for the steward (servers/init.md, "Fresh
//! connections per child"; servers/steward.md, "Failure and restart"): the server's tables are
//! empty, so the fresh connection `init` made the steward there, and every session's connection
//! minted under it, are dead. After the restart and the fault's blame, `init` makes a new one
//! through the handed badge, as at the steward's start, waiting [`SERVER_TIMEOUT`] for the new
//! instance to serve, and hands it over with `reconnected` within [`crate::blame::TIMEOUT`]; the
//! steward tells its sessions, which re-bind. Pure: which of the steward's endpoints a restart
//! strands, the bounds, and the lines the boot prints.

use alloc::format;
use alloc::string::String;

use crate::blame::Taken;
use crate::manifest::Handed;

/// How long `init` waits for the restarted server's first answer, the new connection, in µs:
/// three seconds. A file server recovers its log, and under a test feature checks its volume,
/// before it serves; measured on the image's data volume (`steward-server-restart`, both
/// widths, the `after N ms` of init's line), the first restart's recovery took 589 to 953 ms
/// and one run passed a one-second bound before the instance served, unmeasured; the restarts
/// after it 8 to 71 ms. The rule: twice the measured maximum, plus a second's margin for the
/// run the old bound cut short, rounded up to the next whole second (2 × 0.953 s + 1 s = 2.9 s,
/// so 3 s). The steward's own answer is waited for [`crate::blame::TIMEOUT`], one second,
/// which stays.
pub const SERVER_TIMEOUT: u64 = 3 * crate::blame::TIMEOUT;

/// The steward's `handed` items at the endpoints a restarted server `receives` on: each is a
/// connection the restart stranded, made again for the new instance.
pub fn handed_at<'a>(handed: &'a [Handed], receives: &'a [String]) -> impl Iterator<Item = &'a Handed> + 'a {
    handed.iter().filter(move |h| receives.contains(&h.endpoint))
}

/// The line said once the new connection was offered: how long the new instance took to make
/// it (its recovery, the measurement [`SERVER_TIMEOUT`] is set from), and what became of it.
/// One the steward did not take is disconnected again; its sessions keep their dead connections
/// there, and the steward stays stranded at that server until its own restart (servers/init.md,
/// "Residual risks").
pub fn said(server: &str, waited_ms: u64, taken: Taken) -> String {
    let what = match taken {
        Taken::Told => "the steward took it",
        Taken::Lost => "lost: the steward did not answer within 1 s",
        Taken::Gone => "lost: the steward ended",
        Taken::Refused => "lost: the steward refused it",
    };
    format!("init: {server} reconnected for the steward after {waited_ms} ms: {what}")
}

/// The line said when the restarted server made the steward no new connection: it refused, or
/// did not answer within [`SERVER_TIMEOUT`], named.
pub fn none_made(server: &str) -> String {
    format!(
        "init: {server} made the steward no new connection within {} s of its restart",
        SERVER_TIMEOUT / 1_000_000
    )
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;
    use alloc::vec::Vec;

    use super::*;

    fn handed(endpoint: &str, badge: u64) -> Handed { Handed { endpoint: endpoint.into(), badge } }

    /// The image's steward is handed five endpoints; a restart of `walfsd:data` strands one of
    /// them, a restart of `keyd` (which the steward is not handed) none, and a server receiving
    /// on two endpoints strands each the steward is handed.
    #[test]
    fn a_restart_strands_the_stewards_connections_at_the_endpoints_the_server_receives_on() {
        let steward = [
            handed("bootfsd", 7),
            handed("erofsd:system", 7),
            handed("walfsd:data", 7),
            handed("walfsd:alice-secrets", 7),
            handed("ipd", 4),
        ];
        let at = |receives: &[&str]| -> Vec<String> {
            let receives: Vec<String> = receives.iter().map(|s| s.to_string()).collect();
            handed_at(&steward, &receives).map(|h| h.endpoint.clone()).collect()
        };
        assert_eq!(at(&["walfsd:data"]), ["walfsd:data"]);
        assert!(at(&["keyd"]).is_empty());
        assert_eq!(at(&["ipd", "walfsd:data"]), ["walfsd:data", "ipd"]);
    }

    #[test]
    fn the_outcome_of_the_hand_over_is_said_in_blames_words() {
        assert_eq!(
            said("walfsd:data", 812, Taken::Told),
            "init: walfsd:data reconnected for the steward after 812 ms: the steward took it"
        );
        assert_eq!(
            said("walfsd:data", 3, Taken::Lost),
            "init: walfsd:data reconnected for the steward after 3 ms: lost: the steward did not answer within 1 s"
        );
        assert!(said("ipd", 0, Taken::Gone).ends_with("lost: the steward ended"));
        assert!(said("ipd", 0, Taken::Refused).ends_with("lost: the steward refused it"));
        assert_eq!(
            none_made("ipd"),
            "init: ipd made the steward no new connection within 3 s of its restart"
        );
        assert_eq!(SERVER_TIMEOUT, 3_000_000);
    }
}
