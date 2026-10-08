//! Deliberate rule-breaking, so that the property tests are shown not to be vacuous.
//!
//! Each [`Mutation`] breaks one rule in exactly one place, marked in the code with
//! `self.broken(Mutation::...)`: a numbered rule of docs/kernel/, another statement of the kernel
//! pages (messages, the current call, budgets, handles, a call's checks), a design decision
//! recorded there, or the steward's embedder. A break of the steward's policy is one broken entry
//! of the core's `Policy` table instead ([`policy`]): the shipped crate has no mutation switch.
//! `tests/mutations.rs` runs the property tests against every mutation and requires each to be
//! caught; every kernel rule the model holds has at least one.

use redoubt_steward::Policy;

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
    /// Groups are keyed by account alone, not (account, label set).
    R2KeyByAccountOnly,
    /// Groups take the labels of the handle's stamp budget, not the sender's.
    R2KeyByStampLabels,
    /// Every account-0 sender shares one group.
    R2SystemCallersShareGroup,
    /// One round-robin cursor per endpoint over every group, of every label set: the group after
    /// the one served last is next, so one label set's takes move another's turns.
    R2OneCursor,
    // R3. Lends and abandoned calls.
    /// An abandoned lend is unmapped from the server at once.
    R3UnmapAbandonedLend,
    /// An abandoned lend stays charged to the caller, and the server's charge ends.
    R3ChargeStaysWithCaller,
    /// An abandoned call's holder is never told.
    AbandonNoticeMissing,
    /// An abandoned-call notice is delivered again on every `receive` (I15).
    AbandonNoticeRepeated,
    /// A receiver whose record went bad while it waited takes its abandoned-call notice anyway,
    /// and the notice is lost (I15).
    BadRecordConsumesNotice,
    /// An endpoint's destruction leaves the calls taken through it owing a notice there.
    EndpointDestroyNoticeKept,
    // R4. Delivery.
    /// Transfers are delivered whatever `max_transfer` says.
    R4IgnoreMaxTransfer,
    /// A delivery the receiver cannot pay for goes through anyway, over its limit.
    R4OverdrawOnDelivery,
    /// R4a: `MAX_OPEN_CALLS` is counted per thread, not per process.
    R4aOpenCallsPerThread,
    /// R4a: a process at `MAX_OPEN_CALLS` takes nothing, sends and notices included.
    R4aFullTakesNothing,
    /// R4b: the caller of a dead server gets an empty reply instead of `Dead`.
    R4bDeadServerFakesReply,
    // R5. Interrupts.
    /// A firing IRQ source is not masked.
    R5NoMaskOnFire,
    /// `receive` on an IRQ handle does not unmask the source.
    R5NoUnmaskOnReceive,
    /// A receiver whose record went bad while it waited clears `fired` anyway, and the interrupt
    /// is lost.
    R5BadRecordConsumesInterrupt,
    // R6. Charging.
    /// A parent's usage also counts its children's live usage (not only their limits).
    R6ChargeAncestors,
    /// A budget's own page is charged to itself, not its parent.
    R6OwnPageChargedToItself,
    /// Endpoints cost nothing.
    R6EndpointsFree,
    /// Page-table pages cost nothing.
    R6PageTablesFree,
    /// A page table left mapping nothing stays, and stays charged, until its process ends.
    R6EmptyTableKept,
    /// Open calls cost nothing.
    R6OpenCallsFree,
    /// Process objects cost nothing.
    R6ProcessObjectFree,
    /// The process object is charged to the budget the process runs in, not its creator's.
    R6ProcessObjectChargedToBudget,
    /// A lend is charged to its caller only, not to the receiver as well.
    R6LendChargedOnce,
    /// `root`'s limit is every free frame, so its own page is charged to no one.
    R6RootPageUncounted,
    /// A PID stops counting when its process ends, not when its object is freed.
    R6PidUncountedAtEnd,
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
    /// Queued messages sent through a revoked handle, or to a destroyed endpoint, are not failed.
    /// The two cases are one: every handle to an endpoint is stamped with its owner or a
    /// descendant (R9), and an endpoint is destroyed only with its owner.
    R10RevokedMessageDelivered,
    /// Taken calls sent through a revoked handle, or to a destroyed endpoint, keep their caller
    /// waiting for the reply.
    R10RevokedCallAnswered,
    /// A revoked handle in a queued message is dropped from the list instead of arriving as 0.
    R10SweptHandlesDropped,
    /// Destroying a creator's budget leaves the processes it created running.
    R10CreatorDeathSparesProcess,
    /// Destroying a budget drops the count of the PIDs still held for its processes instead of
    /// moving it to the parent.
    R10HeldPidsDropped,
    /// `budget_reap` destroys the budget it names too, not only its first child.
    R10ReapDestroysParent,
    /// `budget_reap` does not return the reaped child's carve to the budget it keeps.
    R10ReapKeepsCarve,
    /// `budget_reap` spares the reaped child's own children.
    R10ReapSkipsGrandchildren,
    /// A destruction delivers as it kills: what each kill makes deliverable is delivered at once,
    /// so a receiver the destruction has yet to end takes what was owed to a survivor.
    R10DeliveredMidDestruction,
    // R11. Memory.
    /// Reused pages are not zeroed.
    R11NoZeroing,
    /// `set_flags` accepts writable and executable together (the decoder's refusal included).
    R11SetFlagsAllowsWx,
    /// `set_flags` accepts writable without readable.
    R11SetFlagsAllowsWriteOnly,
    /// A lent page stays mapped in the lender during the call.
    R11LendStaysMapped,
    /// `map_fixed` skips the overlap check, so it can map over an existing mapping.
    R11MapFixedSkipsOverlap,
    /// `set_flags` grants `EXECUTE` on device registers and `dma_alloc` frames.
    R11ExecOnDeviceMemory,
    /// `process_map` skips its own flag check (empty flags, and writable without readable).
    R11ProcessMapSkipsFlags,
    // R18. Device authority.
    /// A device call through a number that is not one of the caller's handles reaches the device
    /// object of that number.
    R18DeviceByNumber,
    /// `device_info` reports an interrupt as an MMIO region: the wrong kind of device.
    DeviceInfoWrongKind,
    // R20. PID reuse.
    /// A PID held only by an exit notice is handed to a new process.
    R20NoticePidReused,
    // R22. Range cost.
    /// `map_fixed` walks the page tables of its range before refusing pages its budget cannot pay
    /// for.
    R22MapFixedWalksFirst,
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
    /// Budget id creates a priority tier ahead of stride pass (kernel/scheduling.md: one flat
    /// stride queue).
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
    /// A carve rescales only the remainder; only a carve's return converts the lead.
    R12RescaleOnlyOnReturn,
    /// A deadline's destruction is billed to no budget.
    R12DeadlineWorkUnbilled,
    /// The kernel's work between a pick and the thread's return to user mode comes out of its
    /// slice.
    R12SliceCountsExitWork,
    /// A timer interrupt's work for the timeouts it expires is billed to no budget.
    R12TimerWorkUnbilled,
    /// The pick and switch into a budget are billed to the budget that ran before it, not the
    /// one picked.
    R12SwitchBilledToPrevious,
    // kernel/ipc.md, Messages: what the kernel attaches, and notices.
    /// Messages carry no labels.
    MsgNoLabels,
    /// Every delivered message has badge 0.
    MsgBadgeZero,
    /// Messages carry account 0.
    MsgAccountZero,
    /// Message ids come from one counter shared by every process.
    MsgIdsGlobal,
    /// An exit notice is dropped when no receiver waits on the exit endpoint.
    ExitNoticeDroppedIfNoReceiver,
    /// A fault blames nobody.
    BlameNobody,
    /// A fault blames the thread's most recently taken open call, not its current call.
    BlameNewestCall,
    /// `process_exit` while holding open calls is reported `exited`, blaming nobody.
    ExitWithOpenCallsNotFaulted,
    // kernel/ipc.md: the current call.
    /// Taking a call does not make it current.
    CurrentNeverSet,
    /// `receive` leaves the current call in place when it returns something else.
    ReceiveKeepsCurrent,
    /// `serve` does not change the current call.
    ServeIgnored,
    // kernel/budgets.md: deadlines and inherited class.
    /// Budget deadlines never fire.
    BudgetDeadlineIgnored,
    /// At an equal instant, budget deadlines are processed before timeouts.
    ExpireBudgetsFirst,
    /// Timeouts expire only while nothing runs (a timer armed only when idle).
    TimeoutIgnoredWhileOthersRun,
    /// A child takes its creator's class, not its parent's.
    ClassNotInherited,
    /// "Adding labels needs a system-class caller" checks the parent's class.
    LabelsAddedByParentClass,
    // kernel/objects.md, Handles: badge 0 is the receive right.
    /// `receive` accepts a minted (badge != 0) endpoint handle.
    ReceiveWithBadgedHandle,
    /// `process_create` accepts a badged exit endpoint.
    ExitEndpointBadged,
    // `mint`: a message source must be an open call of the caller's thread.
    /// `mint` accepts an open call of any thread of the process.
    MintFromUnservedMessage,
    // Open calls.
    /// No `MAX_OPEN_CALLS` limit.
    OpenCallsUnlimited,
    /// A new `receive` forgets the thread's open calls.
    ReceiveDropsOpenCalls,
    // kernel/budgets.md: a budget with free weight 0 cannot hold a process.
    /// A budget with free weight 0 may hold a process.
    ProcessInWeightlessBudget,
    /// A carve may leave a process-holding budget with free weight 0.
    R7CarveToZeroFree,
    // The steward's policy (servers/steward.md): each but the embedder's four is a broken entry
    // of the core's `Policy` table (`policy`, below).
    /// A vault session may carry a label its principal does not own (`owns_labels`).
    PolicyVaultWithoutOwnership,
    /// `approve` does not check the request's content hash (`hash_matches`).
    PolicyApproveIgnoresHash,
    /// An approval channel reaches every account's requests, labelled or not (`reaches`).
    PolicyShowLabelledToAll,
    /// No cap on pending requests (`pending_cap`).
    PolicyNoPendingCap,
    /// One session may take a whole domain's pending requests: no fair share (`fair_share`).
    PolicyNoFairShare,
    /// Ending a lease waits behind its sponsor's admission (the embedder ignores the tables'
    /// ahead mark).
    PolicyEndLeaseAdmitted,
    /// The copy out reads the item as it is now, not the snapshot (`copy_out`).
    PolicyDeclassifyLive,
    /// A crossing carves no reader or writer budget: the steward reads and writes the item itself
    /// (`carve_crossing`).
    PolicyDeclassifyWithoutReader,
    /// Crashes are blamed without the 10-minute window (`blame_window`).
    PolicyBlameNoWindow,
    /// A lockout does not refuse new sessions for the window (`not_locked`).
    PolicyNoLockout,
    /// The event's fresh words are a global counter (ids visible to unlabelled observers): the
    /// embedder's entropy source.
    PolicySequentialIds,
    /// Login accepts a key `keyd` holds (`login_key`).
    PolicyLoginWithKeydKey,
    /// An approval channel opens with a login key (`approval_key`).
    PolicyApproveWithLoginKey,
    /// A sub-agent is carved from its domain's sub-budget, with a lease outliving its agent's
    /// (`carve_lease`).
    PolicySubAgentOutlivesAgent,
    /// No `MAX_LEASE` (`lease_bounded`).
    PolicyUnboundedLease,
    /// A session's or agent's end leaves its requests pending, counted against the cap
    /// (`drop_requests`).
    PolicyDeadSessionRequestsKept,
    /// Rendered text passes non-ASCII (bidi and format) characters (`render`).
    PolicyRenderNotWhitelisted,
    /// A labelled request's free text (reason, note) is shown on the approval screen (`render`).
    PolicyLabelledFreeTextShown,
    /// An approval-waiting notice reaches every session of the account, whatever its labels
    /// (`notify`).
    PolicyNotifyLabelledToAll,
    /// A request is answered on a channel that did not render it last (`rendered_here`).
    PolicyApproveOtherChannel,
    /// A labelled session starts an agent (`caller_unlabelled`).
    PolicyLabelledStartsAgent,
    /// A labelled caller's agent request names another label set, so its work lands on that
    /// domain's record (`agent_own_set`).
    PolicyAgentOtherSet,
    /// A declassification is submitted from a session without exactly the item's labels, or a
    /// push from a labelled one (`exact_labels`).
    PolicyDeclassifyFromUnlabelled,
    /// A declassified item may be over `DECLASSIFY_MAX` or not printable text (`item_fits`).
    PolicyDeclassifyUnfit,
    /// A lease is ended from a labelled session of its sponsor (`sponsor_session`).
    PolicyEndLeaseFromVault,
    /// A session may write an item whose labels contain its own (write up): the volume's check, in
    /// the embedder.
    PolicyWriteUp,
    /// The server is started holding a system-class budget handle: the embedder's.
    PolicyServerHoldsSystemBudget,
    /// Audit records are read without their labels (`audit_visible`).
    PolicyAuditUnfiltered,
    /// A login to a live context makes a second session of it (`context_free`).
    PolicyContextTwice,
    // kernel/devices.md, I16: DMA device reset and frame quarantine.
    /// A dying process's DMA frames are pooled even when a device in its reset set did not
    /// confirm: the acceptance mutation.
    DmaFreeBeforeReset,
    /// A device already quarantined counts as reset: a co-holder's healthy runs are
    /// pooled after their shared device was quarantined by another death.
    DmaQuarantinedSlotCountsAsReset,
    /// `unmap` frees a DMA frame instead of only dropping its mapping (kernel/devices.md).
    DmaUnmapFrees,
    /// A quarantined frame's charge is dropped instead of moving to its budget's destroyed
    /// parent (kernel/devices.md, "Quarantine").
    DmaQuarantineChargeDropped,
    /// A quarantined device's handles are not swept, so it can be mapped and allocated through
    /// again (kernel/devices.md, "Quarantine").
    DmaQuarantinedDeviceUsable,
    /// A confirmed reset at one death drops the device from every live co-holder's reset set, so
    /// a co-holder's later death pools frames the device can still write (kernel/devices.md,
    /// "Reset before reuse").
    DmaResetClearsCoHolderReach,
}

impl Mutation {
    pub const ALL: [Mutation; 155] = {
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
            R2OneCursor,
            R3UnmapAbandonedLend,
            R3ChargeStaysWithCaller,
            AbandonNoticeMissing,
            AbandonNoticeRepeated,
            BadRecordConsumesNotice,
            EndpointDestroyNoticeKept,
            R4IgnoreMaxTransfer,
            R4OverdrawOnDelivery,
            R4aOpenCallsPerThread,
            R4aFullTakesNothing,
            R4bDeadServerFakesReply,
            R5NoMaskOnFire,
            R5NoUnmaskOnReceive,
            R5BadRecordConsumesInterrupt,
            R6ChargeAncestors,
            R6OwnPageChargedToItself,
            R6EndpointsFree,
            R6PageTablesFree,
            R6EmptyTableKept,
            R6OpenCallsFree,
            R6ProcessObjectFree,
            R6ProcessObjectChargedToBudget,
            R6LendChargedOnce,
            R6RootPageUncounted,
            R6PidUncountedAtEnd,
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
            R10HeldPidsDropped,
            R10ReapDestroysParent,
            R10ReapKeepsCarve,
            R10ReapSkipsGrandchildren,
            R10DeliveredMidDestruction,
            R11NoZeroing,
            R11SetFlagsAllowsWx,
            R11SetFlagsAllowsWriteOnly,
            R11LendStaysMapped,
            R11MapFixedSkipsOverlap,
            R11ExecOnDeviceMemory,
            R11ProcessMapSkipsFlags,
            R18DeviceByNumber,
            DeviceInfoWrongKind,
            R20NoticePidReused,
            R22MapFixedWalksFirst,
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
            R12RescaleOnlyOnReturn,
            R12DeadlineWorkUnbilled,
            R12SliceCountsExitWork,
            R12TimerWorkUnbilled,
            R12SwitchBilledToPrevious,
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
            PolicyNoFairShare,
            PolicyEndLeaseAdmitted,
            PolicyDeclassifyLive,
            PolicyDeclassifyWithoutReader,
            PolicyBlameNoWindow,
            PolicyNoLockout,
            PolicySequentialIds,
            PolicyLoginWithKeydKey,
            PolicyApproveWithLoginKey,
            PolicySubAgentOutlivesAgent,
            PolicyUnboundedLease,
            PolicyDeadSessionRequestsKept,
            PolicyRenderNotWhitelisted,
            PolicyLabelledFreeTextShown,
            PolicyNotifyLabelledToAll,
            PolicyApproveOtherChannel,
            PolicyLabelledStartsAgent,
            PolicyAgentOtherSet,
            PolicyDeclassifyFromUnlabelled,
            PolicyDeclassifyUnfit,
            PolicyEndLeaseFromVault,
            PolicyWriteUp,
            PolicyServerHoldsSystemBudget,
            PolicyAuditUnfiltered,
            PolicyContextTwice,
            DmaFreeBeforeReset,
            DmaQuarantinedSlotCountsAsReset,
            DmaUnmapFrees,
            DmaQuarantineChargeDropped,
            DmaQuarantinedDeviceUsable,
            DmaResetClearsCoHolderReach,
        ]
    };

    /// A break of the steward's policy or its embedder rather than of the kernel.
    pub fn is_policy(self) -> bool { alloc::format!("{self:?}").starts_with("Policy") }

    /// The rule or invariant it breaks, by the ID the book defines it under (kernel/model.md,
    /// "Mutations"). Every variant names one: the match has no catch-all.
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
            | R2SystemCallersShareGroup
            | R2OneCursor => "R2",
            R3UnmapAbandonedLend
            | R3ChargeStaysWithCaller
            | AbandonNoticeMissing
            | AbandonNoticeRepeated
            | BadRecordConsumesNotice
            | EndpointDestroyNoticeKept => "R3",
            R4IgnoreMaxTransfer | R4OverdrawOnDelivery => "R4",
            R4aOpenCallsPerThread | R4aFullTakesNothing | OpenCallsUnlimited | ReceiveDropsOpenCalls => "R4a",
            R4bDeadServerFakesReply => "R4b",
            R5NoMaskOnFire | R5NoUnmaskOnReceive | R5BadRecordConsumesInterrupt => "R5",
            R6ChargeAncestors
            | R6OwnPageChargedToItself
            | R6EndpointsFree
            | R6PageTablesFree
            | R6EmptyTableKept
            | R6OpenCallsFree
            | R6ProcessObjectFree
            | R6ProcessObjectChargedToBudget
            | R6LendChargedOnce
            | R6RootPageUncounted
            | R6PidUncountedAtEnd => "R6",
            R7NoCarveCheck | R7CarveToZeroFree | ProcessInWeightlessBudget => "R7",
            R8AccountFromArgument => "R8",
            R9ReceivedHandleRestamped | R9MintStampsCaller | R9MsgStampIsSenderBudget => "R9",
            R10KeepForeignHandles
            | R10KeepCarvedLimits
            | R10SpareDescendantProcesses
            | R10ExitNoticesOutlivePayer
            | R10RevokedMessageDelivered
            | R10RevokedCallAnswered
            | R10SweptHandlesDropped
            | R10CreatorDeathSparesProcess
            | R10HeldPidsDropped
            | R10ReapDestroysParent
            | R10ReapKeepsCarve
            | R10ReapSkipsGrandchildren
            | R10DeliveredMidDestruction
            | BudgetDeadlineIgnored => "R10",
            R11NoZeroing
            | R11SetFlagsAllowsWx
            | R11SetFlagsAllowsWriteOnly
            | R11LendStaysMapped
            | R11MapFixedSkipsOverlap
            | R11ExecOnDeviceMemory
            | R11ProcessMapSkipsFlags => "R11",
            R18DeviceByNumber | DeviceInfoWrongKind => "R18",
            R20NoticePidReused => "R20",
            R22MapFixedWalksFirst => "R22",
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
            | R12NoMinimumCharge
            | R12RescaleOnlyOnReturn
            | R12DeadlineWorkUnbilled
            | R12SliceCountsExitWork
            | R12TimerWorkUnbilled
            | R12SwitchBilledToPrevious => "R12",
            IpcWrongLend | IpcDropPartial | IpcFalseDelivery | IpcSkipOutputCheck | IpcLeakRollback => "R13",
            MsgNoLabels | MsgBadgeZero | MsgAccountZero | MsgIdsGlobal => "R14",
            BlameNobody
            | BlameNewestCall
            | ExitWithOpenCallsNotFaulted
            | CurrentNeverSet
            | ReceiveKeepsCurrent
            | ServeIgnored
            | ExitEndpointBadged
            | ExitNoticeDroppedIfNoReceiver => "R21",
            MintFromUnservedMessage => "I3",
            ReceiveWithBadgedHandle => "I4",
            LabelsAddedByParentClass => "I6",
            ClassNotInherited => "I8",
            TimeoutIgnoredWhileOthersRun | ExpireBudgetsFirst => "I13",
            DmaFreeBeforeReset
            | DmaQuarantinedSlotCountsAsReset
            | DmaUnmapFrees
            | DmaQuarantineChargeDropped
            | DmaQuarantinedDeviceUsable
            | DmaResetClearsCoHolderReach => "I16",
            PolicyServerHoldsSystemBudget => "R33",
            PolicyLoginWithKeydKey | PolicyApproveWithLoginKey => "R35",
            PolicySequentialIds => "R36",
            PolicyVaultWithoutOwnership
            | PolicyWriteUp
            | PolicyLabelledStartsAgent
            | PolicyAgentOtherSet
            | PolicyAuditUnfiltered => "R37",
            PolicyApproveIgnoresHash
            | PolicyShowLabelledToAll
            | PolicyRenderNotWhitelisted
            | PolicyLabelledFreeTextShown
            | PolicyNotifyLabelledToAll
            | PolicyApproveOtherChannel
            | PolicyNoPendingCap
            | PolicyDeadSessionRequestsKept => "R38",
            PolicyUnboundedLease
            | PolicySubAgentOutlivesAgent
            | PolicyEndLeaseAdmitted
            | PolicyEndLeaseFromVault
            | PolicyNoFairShare => "R39",
            PolicyBlameNoWindow | PolicyNoLockout => "R40",
            PolicyContextTwice => "R79",
            PolicyDeclassifyLive
            | PolicyDeclassifyWithoutReader
            | PolicyDeclassifyFromUnlabelled
            | PolicyDeclassifyUnfit => "R42",
        }
    }
}

/// The policy the core decides by: `Policy::SHIPPED`, or with one entry broken.
pub fn policy(m: Option<Mutation>) -> Policy {
    use broken::*;
    let mut p = Policy::SHIPPED;
    match m {
        Some(Mutation::PolicyLoginWithKeydKey) => p.login_key = login_key,
        Some(Mutation::PolicyApproveWithLoginKey) => p.approval_key = approval_key,
        Some(Mutation::PolicyVaultWithoutOwnership) => p.owns_labels = pass,
        Some(Mutation::PolicyLabelledStartsAgent) => p.caller_unlabelled = pass,
        Some(Mutation::PolicyAgentOtherSet) => p.agent_own_set = pass,
        Some(Mutation::PolicyNoLockout) => p.not_locked = pass,
        Some(Mutation::PolicyBlameNoWindow) => p.blame_window = blame_window,
        Some(Mutation::PolicyNoPendingCap) => p.pending_cap = pass,
        Some(Mutation::PolicyNoFairShare) => p.fair_share = pass,
        Some(Mutation::PolicyDeadSessionRequestsKept) => p.drop_requests = nothing,
        Some(Mutation::PolicyUnboundedLease) => p.lease_bounded = lease_bounded,
        Some(Mutation::PolicySubAgentOutlivesAgent) => p.carve_lease = carve_lease,
        Some(Mutation::PolicyShowLabelledToAll) => p.reaches = |_, _| true,
        Some(Mutation::PolicyRenderNotWhitelisted) => p.render = render_unwhitelisted,
        Some(Mutation::PolicyLabelledFreeTextShown) => p.render = render_labelled_text,
        Some(Mutation::PolicyNotifyLabelledToAll) => p.notify = notify,
        Some(Mutation::PolicyApproveOtherChannel) => p.rendered_here = pass,
        Some(Mutation::PolicyApproveIgnoresHash) => p.hash_matches = pass,
        Some(Mutation::PolicyDeclassifyFromUnlabelled) => p.exact_labels = pass,
        Some(Mutation::PolicyDeclassifyUnfit) => p.item_fits = pass,
        Some(Mutation::PolicyDeclassifyWithoutReader) => p.carve_crossing = nothing,
        Some(Mutation::PolicyDeclassifyLive) => p.copy_out = copy_out,
        Some(Mutation::PolicyEndLeaseFromVault) => p.sponsor_session = sponsor_session,
        Some(Mutation::PolicyAuditUnfiltered) => p.audit_visible = |_, _| true,
        Some(Mutation::PolicyContextTwice) => p.context_free = pass,
        _ => {}
    }
    p
}

/// The broken entries, written from the core's public context and helpers only.
mod broken {
    use alloc::string::String;
    use alloc::vec::Vec;

    use redoubt_steward::consts::BLAME_COUNT;
    use redoubt_steward::cx::Cx;
    use redoubt_steward::domain::Labels;
    use redoubt_steward::effect::{Bytes, Kind, Notice, Notified, Output, Parent, Refusal, Step};
    use redoubt_steward::effects::{carve_lease_in, show};
    use redoubt_steward::guards::{lease, request, session};
    use redoubt_steward::render::{Rules, screen};

    type Verdict = Result<(), Refusal>;

    /// A guard that always holds.
    pub fn pass(_: &Cx<'_>) -> Verdict { Ok(()) }

    /// An effect that does nothing.
    pub fn nothing(_: &mut Cx<'_>) {}

    /// A login key, or any key `keyd` holds.
    pub fn login_key(cx: &Cx<'_>) -> Verdict {
        let s = session(cx).ok_or(Refusal::Unknown)?;
        let p = &cx.fixed().principals[s.principal];
        if p.login_keys.contains(&s.key) || cx.fixed().keyd.contains(&s.key) {
            Ok(())
        } else {
            Err(Refusal::BadKey)
        }
    }

    /// An approval key, or one of the principal's login keys.
    pub fn approval_key(cx: &Cx<'_>) -> Verdict {
        let c = cx.index().channels.get(&cx.id()).ok_or(Refusal::Unknown)?;
        let p = &cx.fixed().principals[c.principal];
        let ok = p.approval_keys.contains(&c.key) || p.login_keys.contains(&c.key);
        if ok { Ok(()) } else { Err(Refusal::BadKey) }
    }

    /// Every blame kept counts, however old.
    pub fn blame_window(cx: &Cx<'_>) -> Verdict {
        if cx.state().blame.times.len() + 1 >= BLAME_COUNT { Ok(()) } else { Err(Refusal::Unknown) }
    }

    /// Any lease but 0.
    pub fn lease_bounded(cx: &Cx<'_>) -> Verdict {
        let asked = match cx.kind() {
            Kind::Lease => lease(cx).ok_or(Refusal::Unknown)?.lease,
            _ => return Ok(()),
        };
        if asked > 0 { Ok(()) } else { Err(Refusal::BadLease) }
    }

    /// Every lease from its domain's sub-budget, for as long as it asks: a sub-agent outlives its
    /// agent.
    pub fn carve_lease(cx: &mut Cx<'_>) {
        let Some(asked) = lease(cx).map(|l| l.lease) else { return };
        let (parent, limits) = (Parent::Sub(cx.domain().clone()), cx.fixed().sizes.agent);
        let deadline = cx.now().saturating_add(asked);
        carve_lease_in(cx, parent, limits, deadline);
    }

    /// Printable means not a control character: bidi and format characters pass.
    fn unwhitelisted(s: &str, cap: usize) -> String {
        let mut out = String::new();
        for c in s.chars().filter(|c| !c.is_control()).take(cap) {
            if c == '"' || c == '\\' {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }

    fn render_by(cx: &mut Cx<'_>, rules: &Rules) {
        let Some(r) = request(cx) else { return };
        let rendered = screen(cx.fixed(), cx.domain(), cx.state(), r, rules);
        show(cx, rendered);
    }

    pub fn render_unwhitelisted(cx: &mut Cx<'_>) {
        render_by(cx, &Rules { field: unwhitelisted, ..Rules::SHIPPED });
    }

    pub fn render_labelled_text(cx: &mut Cx<'_>) {
        render_by(cx, &Rules { withhold_labelled: false, ..Rules::SHIPPED });
    }

    /// Every session of the account, whatever its labels, and the principal's channels.
    pub fn notify(cx: &mut Cx<'_>) {
        let Some(principal) = request(cx).map(|r| r.principal) else { return };
        let account = cx.domain().account();
        let index = cx.index();
        let mut to: Vec<Notified> = index
            .routes
            .iter()
            .filter(|(_, x)| x.domain.account() == account)
            .map(|(b, _)| Notified::Session(*b))
            .collect();
        to.extend(
            index.channels.values().filter(|c| c.principal == principal).map(|c| Notified::Channel(c.id)),
        );
        for n in to {
            cx.output(Output::Notice { to: n, notice: Notice::ApprovalWaiting });
        }
    }

    /// The copy out reads the item again, as it is now, and writes that.
    pub fn copy_out(cx: &mut Cx<'_>) {
        let Some(item) = cx.state().crossings.get(&cx.id()).map(|c| c.item) else { return };
        let (token, labels) = (cx.token(1), cx.domain().labels().clone());
        cx.step(Step::Read { token: token.clone(), through: None, labels, item });
        cx.step(Step::Write { through: None, labels: Labels::empty(), item, bytes: Bytes::Read(token) });
    }

    /// Any session of the sponsor's account, labelled or not.
    pub fn sponsor_session(cx: &Cx<'_>) -> Verdict {
        let by = cx.caller().ok_or(Refusal::NotSponsor)?;
        let ok = by.kind == Kind::Session && by.domain.account() == cx.domain().account();
        if ok { Ok(()) } else { Err(Refusal::NotSponsor) }
    }
}
