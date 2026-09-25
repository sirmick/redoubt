//! Deliberate rule-breaking, so that the property tests are shown not to be vacuous.
//!
//! Each [`Mutation`] breaks one rule in exactly one place, marked in the code with
//! `self.broken(Mutation::...)`: a numbered rule of KERNEL-SPEC.md (R1-R12), another statement of
//! the spec (its Messages, Process, Budget or Handle paragraphs, a call's checks), an owner's
//! answer to docs/QUESTIONS.md, or the steward's policy. `tests/mutations.rs` runs the
//! property tests against every mutation and requires each to be caught; every rule R1-R12 has
//! at least one. Several came from the red team's reviews, which named the breaks the tests missed.

/// One deliberate break. [`Mutation::rule`] names what it breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mutation {
    // R1. Flow.
    /// Messages between user budgets are delivered whatever their labels.
    R1SkipLabelCheck,
    /// Exit notices are delivered whatever the receiver's labels.
    R1ExitNoticeIgnoresLabels,
    /// `budget_usage` skips the label check for user-class callers too.
    R1UsageIgnoresLabels,
    /// `budget_usage` is exempt when the *target* is system class, not the caller.
    R1UsageExemptBySystemTarget,
    /// An exit notice is exempt when the *exiting* budget is system class, not the owner.
    R1ExitExemptBySystemExiting,
    /// R1 compares the sender with the budget of the thread waiting in `receive`, not with the
    /// endpoint's owner.
    R1ChecksReceiverNotOwner,
    /// R1 takes the sender's class from the stamp of the handle it sends through: a system sender
    /// is refused by a labelled user endpoint.
    R1SenderClassFromStamp,
    // R2. Fair waiting.
    /// Blocked senders are served oldest first across all senders.
    R2FifoAcrossAccounts,
    /// No `WAIT_CAP`.
    R2NoWaitCap,
    /// Groups are keyed by account alone, not (account, label set) (QUESTIONS 17).
    R2KeyByAccountOnly,
    /// Groups take the labels of the handle's stamp budget, not the sender's.
    R2KeyByStampLabels,
    /// Every account-0 sender shares one group (QUESTIONS 87).
    R2SystemCallersShareGroup,
    // R3. Lends and abandoned calls.
    /// An abandoned lend is unmapped from the server at once.
    R3UnmapAbandonedLend,
    /// An abandoned lend stays charged to the caller, and the server's charge ends.
    R3ChargeStaysWithCaller,
    /// An abandoned call's holder is never told (QUESTIONS 81).
    AbandonNoticeMissing,
    /// An abandoned-call notice is delivered again on every `receive` (QUESTIONS 81; I15).
    AbandonNoticeRepeated,
    // R4. Delivery.
    /// Transfers are delivered whatever `max_transfer` says.
    R4IgnoreMaxTransfer,
    /// A delivery the receiver cannot pay for goes through anyway, over its limit (QUESTIONS 72).
    R4OverdrawOnDelivery,
    /// R4a: `MAX_OPEN_CALLS` is counted per thread, not per process.
    R4aOpenCallsPerThread,
    /// R4a: a process at `MAX_OPEN_CALLS` takes nothing, sends and notices included (QUESTIONS 81).
    R4aFullTakesNothing,
    /// R4b: the caller of a dead server gets an empty reply instead of `Dead`.
    R4bDeadServerFakesReply,
    // R5. Interrupts.
    /// A firing IRQ source is not masked.
    R5NoMaskOnFire,
    /// `receive` on an IRQ handle does not unmask the source.
    R5NoUnmaskOnReceive,
    // R6. Charging.
    /// A parent's usage also counts its children's live usage (not only their limits).
    R6ChargeAncestors,
    /// A budget's own page is charged to itself, not its parent (QUESTIONS 76).
    R6OwnPageChargedToItself,
    /// Endpoints cost nothing.
    R6EndpointsFree,
    /// Page-table pages cost nothing (QUESTIONS 13).
    R6PageTablesFree,
    /// Open calls cost nothing (QUESTIONS 2).
    R6OpenCallsFree,
    /// Process objects cost nothing (QUESTIONS 74).
    R6ProcessObjectFree,
    /// The process object is charged to the budget the process runs in, not its creator's.
    R6ProcessObjectChargedToBudget,
    /// A lend is charged to its caller only, not to the receiver as well (QUESTIONS 70).
    R6LendChargedOnce,
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
    /// Pending exit notices whose process object's payer is destroyed stay queued.
    R10ExitNoticesOutlivePayer,
    /// Queued messages sent through a revoked handle, or to a destroyed endpoint, are not failed
    /// (QUESTIONS 30). The two cases are one: every handle to an endpoint is stamped with its
    /// owner or a descendant (R9), and an endpoint is destroyed only with its owner.
    R10RevokedMessageDelivered,
    /// Taken calls sent through a revoked handle, or to a destroyed endpoint, keep their caller
    /// waiting for the reply (QUESTIONS 30).
    R10RevokedCallAnswered,
    /// A revoked handle in a queued message is dropped from the list instead of arriving as 0
    /// (QUESTIONS 86).
    R10SweptHandlesDropped,
    /// Destroying a creator's budget leaves the processes it created running (QUESTIONS 74).
    R10CreatorDeathSparesProcess,
    // R11. Memory.
    /// Reused pages are not zeroed.
    R11NoZeroing,
    /// `set_flags` accepts writable and executable together (the decoder's refusal included).
    R11SetFlagsAllowsWx,
    /// `set_flags` and `process_map` accept writable without readable.
    R11AllowsWriteOnly,
    /// A lent page stays mapped in the lender during the call.
    R11LendStaysMapped,
    /// `map_fixed` skips the overlap check, so it can map over an existing mapping.
    R11MapFixedSkipsOverlap,
    /// Publishes no lend despite a supplied buffer.
    IpcWrongLend,
    /// Hides a committed partial reply behind its error.
    IpcDropPartial,
    /// Reports delivery after failed output commit.
    IpcFalseDelivery,
    /// Commits through an invalid completion-time output record.
    IpcSkipOutputCheck,
    /// Leaks newly installed reply handles after failed output commit.
    IpcLeakRollback,
    // R12. Scheduling.
    /// Budget id creates a priority tier ahead of stride pass (violates answer 103).
    R12PriorityById,
    /// Pass advances by runtime, whatever the weight.
    R12IgnoreWeight,
    /// A waking budget keeps its old pass (banks credit while asleep).
    R12WakeBanksCredit,
    /// A waking budget ranks behind queued budgets of equal pass (wake-first ties broken).
    R12TieQueuedFirst,
    /// A descheduled, still-runnable budget ranks ahead of equal passes, like a waker.
    R12RequeueAhead,
    /// Requeued budgets of equal pass run last-in first-out instead of FIFO.
    R12RequeueLifo,
    /// A wake at a better rank than the running budget preempts it.
    R12PreemptOnWake,
    /// A timeout expiring in a tick re-picks, preempting the running thread.
    R12TimeoutWakePreempts,
    /// A budget waking into an empty queue keeps its own pass (no floor across an idle gap).
    R12NoFloorWhenIdle,
    /// Runs shorter than a slice are charged nothing.
    R12ShortRunsFree,
    /// The division remainder of charging is dropped.
    R12DropRemainder,
    /// Exiting, faulting or being killed on the CPU charges nothing for the partial slice.
    R12ExitRunsFree,
    /// A destroyed budget's unpaid work is dropped instead of moving to its parent.
    R12DestroyDropsDebt,
    /// A new budget enters at the floor, ignoring its parent's pass.
    R12CreateAtFloorOnly,
    /// Destroy lifts the parent to max(parent, floor + work) instead of adding the work to its
    /// lead.
    R12LiftByMax,
    /// Stride weight is the weight limit, not the free weight (carving duplicates share).
    R12StrideWeightIsLimit,
    /// Destroy lifts the parent to the child's raw pass, not its work normalized by weight.
    R12UnnormalizedLift,
    /// Destroy counts the child's inherited entry wait as its work (debt measured from the floor).
    R12LiftCountsEntryWait,
    /// Pending runtime is not folded before a weight change (charged at the new weight).
    R12FoldAtNewWeight,
    /// A deschedule charges only what the clock saw: a run shorter than one unit is free.
    R12NoMinimumCharge,
    // KERNEL-SPEC.md, Messages: what the kernel attaches, and notices.
    /// Messages carry no labels.
    MsgNoLabels,
    /// Every delivered message has badge 0.
    MsgBadgeZero,
    /// Messages carry account 0.
    MsgAccountZero,
    /// Message ids come from one counter shared by every process (QUESTIONS 88).
    MsgIdsGlobal,
    /// An exit notice is dropped when no receiver waits on the exit endpoint.
    ExitNoticeDroppedIfNoReceiver,
    /// A fault blames nobody.
    BlameNobody,
    /// A fault blames the thread's most recently taken open call, not its current call
    /// (QUESTIONS 82).
    BlameNewestCall,
    /// `process_exit` while holding open calls is reported `exited`, blaming nobody (QUESTIONS 55).
    ExitWithOpenCallsNotFaulted,
    // KERNEL-SPEC.md, Process: the current call.
    /// Taking a call does not make it current.
    CurrentNeverSet,
    /// `receive` leaves the current call in place when it returns something else.
    ReceiveKeepsCurrent,
    /// `serve` does not change the current call.
    ServeIgnored,
    // KERNEL-SPEC.md, Budget: deadlines and inherited class.
    /// Budget deadlines never fire.
    BudgetDeadlineIgnored,
    /// At an equal instant, budget deadlines are processed before timeouts.
    ExpireBudgetsFirst,
    /// Timeouts expire only while nothing runs (a timer armed only when idle).
    TimeoutIgnoredWhileOthersRun,
    /// A child takes its creator's class, not its parent's (QUESTIONS 73).
    ClassNotInherited,
    /// "Adding labels needs a system-class caller" checks the parent's class.
    LabelsAddedByParentClass,
    // KERNEL-SPEC.md, Handle: badge 0 is the receive right.
    /// `receive` accepts a minted (badge != 0) endpoint handle.
    ReceiveWithBadgedHandle,
    /// `process_create` accepts a badged exit endpoint (QUESTIONS 93).
    ExitEndpointBadged,
    // `mint`: a message source must be an open call of the caller's thread.
    /// `mint` accepts an open call of any thread of the process.
    MintFromUnservedMessage,
    // Open calls (QUESTIONS 2).
    /// No `MAX_OPEN_CALLS` limit.
    OpenCallsUnlimited,
    /// A new `receive` forgets the thread's open calls.
    ReceiveDropsOpenCalls,
    // QUESTIONS 12.
    /// A budget with free weight 0 may hold a process.
    ProcessInWeightlessBudget,
    /// A carve may leave a process-holding budget with free weight 0.
    R7CarveToZeroFree,
    // The steward's policy (CONTAINMENT.md, CAPABILITIES.md, INIT.md).
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
    /// One session may take a whole bucket's pending requests (no fair share; QUESTIONS 90).
    PolicyNoFairShare,
    /// Ending a lease is subject to its sponsor's pending cap (QUESTIONS 90).
    PolicyEndLeaseAdmitted,
    /// Declassification copies the item as it is now, not the snapshot.
    PolicyDeclassifyLive,
    /// Declassification reads the item as the steward, with no reader budget (QUESTIONS 54).
    PolicyDeclassifyWithoutReader,
    /// Crashes are blamed without the 10-minute window.
    PolicyBlameNoWindow,
    /// Crash blame is counted per account, not per (account, label set) (QUESTIONS 48).
    PolicyBlamePerAccount,
    /// A logout does not refuse new sessions for the window (QUESTIONS 91).
    PolicyNoLockout,
    /// Request ids are a global counter (visible to unlabelled observers).
    PolicySequentialIds,
    /// Login accepts a key `keyd` holds.
    PolicyLoginWithKeydKey,
    /// An agent's sub-agent is carved from the sponsor, with a lease outliving the agent's.
    PolicySubAgentOutlivesAgent,
    /// No `MAX_LEASE`.
    PolicyUnboundedLease,
    /// Requests of dead sessions stay pending and count against the cap.
    PolicyDeadSessionRequestsKept,
    /// Rendered text passes non-ASCII (bidi and format) characters (QUESTIONS 34).
    PolicyRenderNotWhitelisted,
    /// A labelled request's free text (reason, note) is shown on the approval screen (QUESTIONS 35).
    PolicyLabelledFreeTextShown,
    /// A session may write an item whose labels contain its own (write up; QUESTIONS 51).
    PolicyWriteUp,
    /// The server is started holding a system-class budget handle (QUESTIONS 79).
    PolicyServerHoldsSystemBudget,
    /// A session's connection is narrowed to its session budget, not to a revocation scope
    /// (QUESTIONS 80).
    PolicyNarrowToSessionBudget,
    /// Sessions of every label set are carved from the principal's unlabelled sub-budget
    /// (QUESTIONS 89).
    PolicyCarveFromUnlabelled,
    /// Audit records are read without their labels (QUESTIONS 92).
    PolicyAuditUnfiltered,
}

impl Mutation {
    pub const ALL: [Mutation; 121] = {
        use Mutation::*;
        [
            R1SkipLabelCheck,
            R1ExitNoticeIgnoresLabels,
            R1UsageIgnoresLabels,
            R1UsageExemptBySystemTarget,
            R1ExitExemptBySystemExiting,
            R1ChecksReceiverNotOwner,
            R1SenderClassFromStamp,
            R2FifoAcrossAccounts,
            R2NoWaitCap,
            R2KeyByAccountOnly,
            R2KeyByStampLabels,
            R2SystemCallersShareGroup,
            R3UnmapAbandonedLend,
            R3ChargeStaysWithCaller,
            AbandonNoticeMissing,
            AbandonNoticeRepeated,
            R4IgnoreMaxTransfer,
            R4OverdrawOnDelivery,
            R4aOpenCallsPerThread,
            R4aFullTakesNothing,
            R4bDeadServerFakesReply,
            R5NoMaskOnFire,
            R5NoUnmaskOnReceive,
            R6ChargeAncestors,
            R6OwnPageChargedToItself,
            R6EndpointsFree,
            R6PageTablesFree,
            R6OpenCallsFree,
            R6ProcessObjectFree,
            R6ProcessObjectChargedToBudget,
            R6LendChargedOnce,
            R7NoCarveCheck,
            R8AccountFromArgument,
            R9ReceivedHandleRestamped,
            R9MintStampsCaller,
            R9MsgStampIsSenderBudget,
            R10KeepForeignHandles,
            R10KeepCarvedLimits,
            R10SpareDescendantProcesses,
            R10ExitNoticesOutlivePayer,
            R10RevokedMessageDelivered,
            R10RevokedCallAnswered,
            R10SweptHandlesDropped,
            R10CreatorDeathSparesProcess,
            R11NoZeroing,
            R11SetFlagsAllowsWx,
            R11AllowsWriteOnly,
            R11LendStaysMapped,
            R11MapFixedSkipsOverlap,
            IpcWrongLend,
            IpcDropPartial,
            IpcFalseDelivery,
            IpcSkipOutputCheck,
            IpcLeakRollback,
            R12PriorityById,
            R12IgnoreWeight,
            R12WakeBanksCredit,
            R12TieQueuedFirst,
            R12RequeueAhead,
            R12RequeueLifo,
            R12PreemptOnWake,
            R12TimeoutWakePreempts,
            R12NoFloorWhenIdle,
            R12ShortRunsFree,
            R12DropRemainder,
            R12ExitRunsFree,
            R12DestroyDropsDebt,
            R12CreateAtFloorOnly,
            R12LiftByMax,
            R12StrideWeightIsLimit,
            R12UnnormalizedLift,
            R12LiftCountsEntryWait,
            R12FoldAtNewWeight,
            R12NoMinimumCharge,
            MsgNoLabels,
            MsgBadgeZero,
            MsgAccountZero,
            MsgIdsGlobal,
            ExitNoticeDroppedIfNoReceiver,
            BlameNobody,
            BlameNewestCall,
            ExitWithOpenCallsNotFaulted,
            CurrentNeverSet,
            ReceiveKeepsCurrent,
            ServeIgnored,
            BudgetDeadlineIgnored,
            ExpireBudgetsFirst,
            TimeoutIgnoredWhileOthersRun,
            ClassNotInherited,
            LabelsAddedByParentClass,
            ReceiveWithBadgedHandle,
            ExitEndpointBadged,
            MintFromUnservedMessage,
            OpenCallsUnlimited,
            ReceiveDropsOpenCalls,
            ProcessInWeightlessBudget,
            R7CarveToZeroFree,
            PolicyVaultWithoutOwnership,
            PolicyApproveIgnoresHash,
            PolicyShowLabelledToAll,
            PolicyNoPendingCap,
            PolicyCapPerAccount,
            PolicyNoFairShare,
            PolicyEndLeaseAdmitted,
            PolicyDeclassifyLive,
            PolicyDeclassifyWithoutReader,
            PolicyBlameNoWindow,
            PolicyBlamePerAccount,
            PolicyNoLockout,
            PolicySequentialIds,
            PolicyLoginWithKeydKey,
            PolicySubAgentOutlivesAgent,
            PolicyUnboundedLease,
            PolicyDeadSessionRequestsKept,
            PolicyRenderNotWhitelisted,
            PolicyLabelledFreeTextShown,
            PolicyWriteUp,
            PolicyServerHoldsSystemBudget,
            PolicyNarrowToSessionBudget,
            PolicyCarveFromUnlabelled,
            PolicyAuditUnfiltered,
        ]
    };

    /// What it breaks: "R1".."R12" (R4a and R4b count as R4), a spec paragraph or call,
    /// "QUESTIONS n", or "policy".
    pub fn rule(self) -> &'static str {
        use Mutation::*;
        match self {
            R1SkipLabelCheck
            | R1ExitNoticeIgnoresLabels
            | R1UsageIgnoresLabels
            | R1UsageExemptBySystemTarget
            | R1ExitExemptBySystemExiting
            | R1ChecksReceiverNotOwner
            | R1SenderClassFromStamp => "R1",
            R2FifoAcrossAccounts
            | R2NoWaitCap
            | R2KeyByAccountOnly
            | R2KeyByStampLabels
            | R2SystemCallersShareGroup => "R2",
            R3UnmapAbandonedLend | R3ChargeStaysWithCaller | AbandonNoticeMissing | AbandonNoticeRepeated => {
                "R3"
            }
            R4IgnoreMaxTransfer
            | R4OverdrawOnDelivery
            | R4aOpenCallsPerThread
            | R4aFullTakesNothing
            | R4bDeadServerFakesReply => "R4",
            R5NoMaskOnFire | R5NoUnmaskOnReceive => "R5",
            R6ChargeAncestors
            | R6OwnPageChargedToItself
            | R6EndpointsFree
            | R6PageTablesFree
            | R6OpenCallsFree
            | R6ProcessObjectFree
            | R6ProcessObjectChargedToBudget
            | R6LendChargedOnce => "R6",
            R7NoCarveCheck | R7CarveToZeroFree => "R7",
            R8AccountFromArgument => "R8",
            R9ReceivedHandleRestamped | R9MintStampsCaller | R9MsgStampIsSenderBudget => "R9",
            R10KeepForeignHandles
            | R10KeepCarvedLimits
            | R10SpareDescendantProcesses
            | R10ExitNoticesOutlivePayer
            | R10RevokedMessageDelivered
            | R10RevokedCallAnswered
            | R10SweptHandlesDropped
            | R10CreatorDeathSparesProcess => "R10",
            R11NoZeroing
            | R11SetFlagsAllowsWx
            | R11AllowsWriteOnly
            | R11LendStaysMapped
            | R11MapFixedSkipsOverlap => "R11",
            IpcWrongLend | IpcDropPartial | IpcFalseDelivery | IpcSkipOutputCheck | IpcLeakRollback => "IPC",
            R12PriorityById
            | R12IgnoreWeight
            | R12WakeBanksCredit
            | R12TieQueuedFirst
            | R12RequeueAhead
            | R12RequeueLifo
            | R12PreemptOnWake
            | R12TimeoutWakePreempts
            | R12NoFloorWhenIdle
            | R12ShortRunsFree
            | R12DropRemainder
            | R12ExitRunsFree
            | R12DestroyDropsDebt
            | R12CreateAtFloorOnly
            | R12LiftByMax
            | R12StrideWeightIsLimit
            | R12UnnormalizedLift
            | R12LiftCountsEntryWait
            | R12FoldAtNewWeight
            | R12NoMinimumCharge => "R12",
            MsgNoLabels
            | MsgBadgeZero
            | MsgAccountZero
            | MsgIdsGlobal
            | ExitNoticeDroppedIfNoReceiver
            | BlameNobody
            | BlameNewestCall
            | ExitWithOpenCallsNotFaulted => "Messages",
            CurrentNeverSet | ReceiveKeepsCurrent => "Process",
            ServeIgnored => "serve",
            BudgetDeadlineIgnored | ClassNotInherited | ExpireBudgetsFirst => "Budget",
            TimeoutIgnoredWhileOthersRun => "I13",
            LabelsAddedByParentClass => "budget_create",
            ReceiveWithBadgedHandle => "Handle",
            ExitEndpointBadged => "process_create",
            MintFromUnservedMessage => "mint",
            OpenCallsUnlimited | ReceiveDropsOpenCalls => "QUESTIONS 2",
            ProcessInWeightlessBudget => "QUESTIONS 12",
            _ => "policy",
        }
    }
}
