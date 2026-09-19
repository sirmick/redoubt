//! Deliberate rule-breaking, so that the property tests are shown not to be vacuous.
//!
//! Each [`Mutation`] breaks one rule in exactly one place, marked in the code with
//! `self.broken(Mutation::...)`: a numbered rule of KERNEL-SPEC.md (R1-R12), another statement of
//! the spec (its Messages, Process, Budget or Handle paragraphs, a call's checks), an owner's
//! answer to planning/redoubt/QUESTIONS.md, or the steward's policy. `tests/mutations.rs` runs the
//! property tests against every mutation and requires each to be caught; every rule R1-R12 has
//! at least one.

/// One deliberate break. [`Mutation::rule`] names what it breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mutation {
    // R1. Label check.
    /// Messages between user budgets are delivered whatever their labels.
    R1SkipLabelCheck,
    /// Exit notices are delivered whatever the receiver's labels.
    R1ExitNoticeIgnoresLabels,
    /// `budget_usage` skips the label check for user-class callers too (the system-class
    /// exemption of QUESTIONS 8, extended to everyone).
    R1UsageIgnoresLabels,
    // R2. Fair waiting.
    /// Blocked senders are served oldest first across all senders.
    R2FifoAcrossAccounts,
    /// No `WAIT_CAP`.
    R2NoWaitCap,
    /// `WAIT_CAP` and the round-robin are keyed by account alone, not (account, label set)
    /// (QUESTIONS 17).
    R2KeyByAccountOnly,
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
    /// Page-table pages cost nothing (QUESTIONS 13).
    R6PageTablesFree,
    /// Open calls cost nothing (QUESTIONS 2).
    R6OpenCallsFree,
    /// The exit slot is not charged to the creator (QUESTIONS 7).
    R6ExitSlotFree,
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
    /// A message records its sender's budget as the stamp, not the stamp of the handle it was
    /// sent through (so `mint` from it gets the wrong default stamp).
    R9MsgStampIsSenderBudget,
    // R10. Destruction.
    /// Destroying a budget leaves handles stamped with it in other processes' tables.
    R10KeepForeignHandles,
    /// Destroying a budget does not return its carved limits to its parent.
    R10KeepCarvedLimits,
    /// Destroying a budget does not kill its descendants' processes.
    R10SpareDescendantProcesses,
    /// Destroying an endpoint leaves its queued senders blocked.
    R10QueuedSendersNotFailed,
    /// Destroying an endpoint leaves the calls in flight to it waiting.
    R10InFlightCallsNotFailed,
    // R11. Memory.
    /// Reused pages are not zeroed.
    R11NoZeroing,
    /// `set_flags` accepts writable and executable together (the decoder's refusal included).
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
    // KERNEL-SPEC.md, Messages: what the kernel attaches, and exit notices.
    /// Messages carry no labels.
    MsgNoLabels,
    /// Every delivered message has badge 0.
    MsgBadgeZero,
    /// Messages carry account 0.
    MsgAccountZero,
    /// An exit notice is dropped when no receiver waits on the exit endpoint.
    ExitNoticeDroppedIfNoReceiver,
    // KERNEL-SPEC.md, Process: the account a thread serves (crash blame).
    /// `receive` never records the account of the message it delivers.
    ServedAccountNeverSet,
    // KERNEL-SPEC.md, Budget: deadlines.
    /// Budget deadlines never fire.
    BudgetDeadlineIgnored,
    // KERNEL-SPEC.md, Handle: badge 0 is the receive right.
    /// `receive` accepts a minted (badge != 0) endpoint handle.
    ReceiveWithBadgedHandle,
    // `mint`: a message source must be one the caller serves.
    /// `mint` accepts any message id, served or not.
    MintFromUnservedMessage,
    // Open calls (QUESTIONS 2).
    /// No `MAX_OPEN_CALLS` limit.
    OpenCallsUnlimited,
    /// A new `receive` forgets the thread's open calls (the red team's X11).
    ReceiveDropsOpenCalls,
    // Budget and process creation rules from the owner's answers.
    /// A user-class caller may create a system-class child (QUESTIONS 9).
    SystemChildFromUserCaller,
    /// A budget with weight 0 may hold a process (QUESTIONS 12).
    ProcessInWeightlessBudget,
    // The steward's policy (CONTAINMENT.md, CAPABILITIES.md).
    /// A vault session may carry a label its principal does not own.
    PolicyVaultWithoutOwnership,
    /// `approve` does not check the request's content hash.
    PolicyApproveIgnoresHash,
    /// Labelled requests are shown to every approver.
    PolicyShowLabelledToAll,
    /// No cap on pending requests.
    PolicyNoPendingCap,
    /// The pending cap is per account, not per (account, label set) (QUESTIONS 17).
    PolicyCapPerAccount,
    /// Declassification copies the item as it is now, not the snapshot.
    PolicyDeclassifyLive,
    /// Crashes are blamed without the 10-minute window.
    PolicyBlameNoWindow,
    /// Request ids are a global counter (visible to unlabelled observers).
    PolicySequentialIds,
    /// Login accepts a key `keyd` holds.
    PolicyLoginWithKeydKey,
    /// An agent's sub-agent is carved from the sponsor, with a lease outliving the agent's
    /// (QUESTIONS 33).
    PolicySubAgentOutlivesAgent,
    /// No `MAX_LEASE`.
    PolicyUnboundedLease,
    /// Requests of dead sessions stay pending and count against the cap.
    PolicyDeadSessionRequestsKept,
    /// Rendered text passes non-ASCII (bidi and format) characters (QUESTIONS 34).
    PolicyRenderNotWhitelisted,
}

impl Mutation {
    pub const ALL: [Mutation; 58] = {
        use Mutation::*;
        [
            R1SkipLabelCheck,
            R1ExitNoticeIgnoresLabels,
            R1UsageIgnoresLabels,
            R2FifoAcrossAccounts,
            R2NoWaitCap,
            R2KeyByAccountOnly,
            R3UnmapAbandonedLend,
            R3ChargeStaysWithCaller,
            R4IgnoreMaxTransfer,
            R5NoMaskOnFire,
            R5NoUnmaskOnReceive,
            R6ChargeAncestors,
            R6ScopeChargedToItself,
            R6EndpointsFree,
            R6PageTablesFree,
            R6OpenCallsFree,
            R6ExitSlotFree,
            R7NoCarveCheck,
            R8AccountFromArgument,
            R9ReceivedHandleRestamped,
            R9MintStampsCaller,
            R9MsgStampIsSenderBudget,
            R10KeepForeignHandles,
            R10KeepCarvedLimits,
            R10SpareDescendantProcesses,
            R10QueuedSendersNotFailed,
            R10InFlightCallsNotFailed,
            R11NoZeroing,
            R11SetFlagsAllowsWx,
            R11LendStaysMapped,
            R12NoClassOrder,
            R12IgnoreWeight,
            R12WakeBanksCredit,
            MsgNoLabels,
            MsgBadgeZero,
            MsgAccountZero,
            ExitNoticeDroppedIfNoReceiver,
            ServedAccountNeverSet,
            BudgetDeadlineIgnored,
            ReceiveWithBadgedHandle,
            MintFromUnservedMessage,
            OpenCallsUnlimited,
            ReceiveDropsOpenCalls,
            SystemChildFromUserCaller,
            ProcessInWeightlessBudget,
            PolicyVaultWithoutOwnership,
            PolicyApproveIgnoresHash,
            PolicyShowLabelledToAll,
            PolicyNoPendingCap,
            PolicyCapPerAccount,
            PolicyDeclassifyLive,
            PolicyBlameNoWindow,
            PolicySequentialIds,
            PolicyLoginWithKeydKey,
            PolicySubAgentOutlivesAgent,
            PolicyUnboundedLease,
            PolicyDeadSessionRequestsKept,
            PolicyRenderNotWhitelisted,
        ]
    };

    /// What it breaks: "R1".."R12", a spec paragraph or call, "QUESTIONS n", or "policy".
    pub fn rule(self) -> &'static str {
        use Mutation::*;
        match self {
            R1SkipLabelCheck | R1ExitNoticeIgnoresLabels | R1UsageIgnoresLabels => "R1",
            R2FifoAcrossAccounts | R2NoWaitCap | R2KeyByAccountOnly => "R2",
            R3UnmapAbandonedLend | R3ChargeStaysWithCaller => "R3",
            R4IgnoreMaxTransfer => "R4",
            R5NoMaskOnFire | R5NoUnmaskOnReceive => "R5",
            R6ChargeAncestors | R6ScopeChargedToItself | R6EndpointsFree | R6PageTablesFree | R6OpenCallsFree
            | R6ExitSlotFree => "R6",
            R7NoCarveCheck => "R7",
            R8AccountFromArgument => "R8",
            R9ReceivedHandleRestamped | R9MintStampsCaller | R9MsgStampIsSenderBudget => "R9",
            R10KeepForeignHandles
            | R10KeepCarvedLimits
            | R10SpareDescendantProcesses
            | R10QueuedSendersNotFailed
            | R10InFlightCallsNotFailed => "R10",
            R11NoZeroing | R11SetFlagsAllowsWx | R11LendStaysMapped => "R11",
            R12NoClassOrder | R12IgnoreWeight | R12WakeBanksCredit => "R12",
            MsgNoLabels | MsgBadgeZero | MsgAccountZero | ExitNoticeDroppedIfNoReceiver => "Messages",
            ServedAccountNeverSet => "Process",
            BudgetDeadlineIgnored => "Budget",
            ReceiveWithBadgedHandle => "Handle",
            MintFromUnservedMessage => "mint",
            OpenCallsUnlimited | ReceiveDropsOpenCalls => "QUESTIONS 2",
            SystemChildFromUserCaller => "QUESTIONS 9",
            ProcessInWeightlessBudget => "QUESTIONS 12",
            _ => "policy",
        }
    }
}
