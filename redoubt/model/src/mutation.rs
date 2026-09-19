//! Deliberate rule-breaking, so that the property tests are shown not to be vacuous.
//!
//! Each [`Mutation`] breaks one rule of KERNEL-SPEC.md (R1-R12) or one part of the steward's
//! policy in exactly one place, marked in the code with `self.broken(Mutation::...)`. The test
//! `mutations_are_caught` (tests/mutations.rs) runs the property tests against every mutation and
//! requires each one to be caught. Acceptance for WP-M0: every rule R1-R12 has at least one
//! mutation, and every mutation is caught.

/// One deliberate break. [`Mutation::rule`] names the rule it breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mutation {
    // R1. Label check.
    /// Messages between user budgets are delivered whatever their labels.
    R1SkipLabelCheck,
    /// Exit notices are delivered whatever the receiver's labels.
    R1ExitNoticeIgnoresLabels,
    // R2. Fair waiting.
    /// Blocked senders are served oldest first across all accounts.
    R2FifoAcrossAccounts,
    /// No `WAIT_CAP`.
    R2NoWaitCap,
    // R3. Lends outlive their lender.
    /// An abandoned lend is unmapped from the server at once.
    R3UnmapAbandonedLend,
    /// An abandoned lend stays charged to the (possibly dead) caller's budget.
    R3ChargeStaysWithCaller,
    // R4. Transfer opt-in.
    /// Transfers are delivered whatever `max_transfer` says.
    R4IgnoreMaxTransfer,
    // R5. Interrupts.
    /// A firing IRQ source is not masked.
    R5NoMaskOnFire,
    /// `receive` on an IRQ handle does not unmask the source.
    R5NoUnmaskOnReceive,
    // R6. Charging.
    /// A parent's usage also counts its children's live usage (not only their limits).
    R6ChargeAncestors,
    /// A revocation scope's own object is charged to itself.
    R6ScopeChargedToItself,
    /// Endpoints cost nothing.
    R6EndpointsFree,
    // R7. Carving.
    /// Children may be carved beyond the parent's free limits.
    R7NoCarveCheck,
    // R8. Accounts.
    /// A new budget takes the account argument even when the parent has one.
    R8AccountFromArgument,
    // R9. Stamps.
    /// A handle received in a message is restamped with the receiver's budget.
    R9ReceivedHandleRestamped,
    /// `mint` stamps with the caller's budget instead of the source's default stamp.
    R9MintStampsCaller,
    // R10. Destruction.
    /// Destroying a budget leaves handles stamped with it in other processes' tables.
    R10KeepForeignHandles,
    /// Destroying a budget does not return its carved limits to its parent.
    R10KeepCarvedLimits,
    /// Destroying a budget does not kill its descendants' processes.
    R10SpareDescendantProcesses,
    // R11. Memory.
    /// Reused pages are not zeroed.
    R11NoZeroing,
    /// `set_flags` accepts writable and executable together.
    R11SetFlagsAllowsWx,
    /// A lent page stays mapped in the lender during the call.
    R11LendStaysMapped,
    // R12. Scheduling.
    /// User-class budgets compete with system-class ones.
    R12NoClassOrder,
    /// Pass advances by runtime, whatever the weight.
    R12IgnoreWeight,
    /// A waking budget keeps its old pass (banks credit while asleep).
    R12WakeBanksCredit,
    // The steward's policy (CONTAINMENT.md, CAPABILITIES.md).
    /// A vault session may carry a label its principal does not own.
    PolicyVaultWithoutOwnership,
    /// `approve` does not check the request's content hash.
    PolicyApproveIgnoresHash,
    /// Labelled requests are shown to every approver.
    PolicyShowLabelledToAll,
    /// No per-account cap on pending requests.
    PolicyNoPendingCap,
    /// Declassification copies the item as it is now, not the snapshot.
    PolicyDeclassifyLive,
    /// Crashes are blamed without the 10-minute window.
    PolicyBlameNoWindow,
    /// Request ids are a global counter (visible to unlabelled observers).
    PolicySequentialIds,
    /// Login accepts a key `keyd` holds.
    PolicyLoginWithKeydKey,
}

impl Mutation {
    pub const ALL: [Mutation; 33] = [
        Mutation::R1SkipLabelCheck,
        Mutation::R1ExitNoticeIgnoresLabels,
        Mutation::R2FifoAcrossAccounts,
        Mutation::R2NoWaitCap,
        Mutation::R3UnmapAbandonedLend,
        Mutation::R3ChargeStaysWithCaller,
        Mutation::R4IgnoreMaxTransfer,
        Mutation::R5NoMaskOnFire,
        Mutation::R5NoUnmaskOnReceive,
        Mutation::R6ChargeAncestors,
        Mutation::R6ScopeChargedToItself,
        Mutation::R6EndpointsFree,
        Mutation::R7NoCarveCheck,
        Mutation::R8AccountFromArgument,
        Mutation::R9ReceivedHandleRestamped,
        Mutation::R9MintStampsCaller,
        Mutation::R10KeepForeignHandles,
        Mutation::R10KeepCarvedLimits,
        Mutation::R10SpareDescendantProcesses,
        Mutation::R11NoZeroing,
        Mutation::R11SetFlagsAllowsWx,
        Mutation::R11LendStaysMapped,
        Mutation::R12NoClassOrder,
        Mutation::R12IgnoreWeight,
        Mutation::R12WakeBanksCredit,
        Mutation::PolicyVaultWithoutOwnership,
        Mutation::PolicyApproveIgnoresHash,
        Mutation::PolicyShowLabelledToAll,
        Mutation::PolicyNoPendingCap,
        Mutation::PolicyDeclassifyLive,
        Mutation::PolicyBlameNoWindow,
        Mutation::PolicySequentialIds,
        Mutation::PolicyLoginWithKeydKey,
    ];

    /// The rule broken: "R1".."R12", or "policy".
    pub fn rule(self) -> &'static str {
        use Mutation::*;
        match self {
            R1SkipLabelCheck | R1ExitNoticeIgnoresLabels => "R1",
            R2FifoAcrossAccounts | R2NoWaitCap => "R2",
            R3UnmapAbandonedLend | R3ChargeStaysWithCaller => "R3",
            R4IgnoreMaxTransfer => "R4",
            R5NoMaskOnFire | R5NoUnmaskOnReceive => "R5",
            R6ChargeAncestors | R6ScopeChargedToItself | R6EndpointsFree => "R6",
            R7NoCarveCheck => "R7",
            R8AccountFromArgument => "R8",
            R9ReceivedHandleRestamped | R9MintStampsCaller => "R9",
            R10KeepForeignHandles | R10KeepCarvedLimits | R10SpareDescendantProcesses => "R10",
            R11NoZeroing | R11SetFlagsAllowsWx | R11LendStaysMapped => "R11",
            R12NoClassOrder | R12IgnoreWeight | R12WakeBanksCredit => "R12",
            _ => "policy",
        }
    }
}
