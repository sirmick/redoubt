//! The kernel's IPC lists: who waits where, kept on the objects waited on (kernel/ipc.md R2, R3,
//! R4a; kernel/budgets.md R10 step 4; kernel/timer.md, "Expiry").
//!
//! Every list is intrusive: its head lives in the frame of the object it belongs to, and its links
//! in its members' own pages, a thread's IPC page or an open call's page. So a list costs no
//! allocation and no table, and nothing a budget is not already paying for. The kernel keeps each
//! word where [`Words`] puts it; this crate holds only the links, R2's order and the audits, so they
//! can be host-tested (`src/tests.rs`). Every rule decision (who may take what, a doomed receiver,
//! R4a's limit, R4's costs) stays in the kernel's `message.rs`.
//!
//! - **Members.** A thread is named by `pid << 8 | tid` (TIDs to 255), an open call or a process object by
//!   its frame plus 1. 0 names nothing, so a zeroed frame is an empty list and an unlinked member.
//! - **One wait, one list.** A thread waits on one thing, so one pair of links ([`T_PREV`], [`T_NEXT`])
//!   serves every list a wait puts it on: an endpoint's receivers, a device's interrupt waiters, or its R2
//!   group's sends or calls. A queued message is also on its stamp budget's chain ([`T_SPREV`]); a wait with
//!   a deadline, from when it starts until it ends, on its process's timed waits ([`T_DPREV`]); and a due
//!   wait, inside an expiry only, on the due list ([`T_XPREV`]). An open call is on its endpoint's open list
//!   while its caller waits and on its notice list while its notice is owed (never both: [`C_EPREV`]), and on
//!   its stamp budget's chain while its caller waits ([`C_SPREV`]). A process object is on its exit
//!   endpoint's reporters while its process runs and on its exits while its notice is owed (never both:
//!   [`P_PREV`]).
//! - **R2's groups** ([`enqueue`], [`pick`], [`served`], [`dequeue`]). An endpoint keeps the groups with a
//!   message queued in the order their turns fall due, and apart, the groups with a send queued in the order
//!   their oldest sends fall due, for a receiver at `MAX_OPEN_CALLS` (R4a). A group's node lives in its
//!   oldest message's thread.
//! - **The audits** ([`List::audit`], [`audit_groups`]) check a list against its members' own words, for the
//!   checked build.

#![no_std]
#![forbid(unsafe_code)]

/// A page the lists keep words in.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Page {
    /// A thread's IPC page: `pid << 8 | tid`.
    Thread(u64),
    /// An open call's page: its frame + 1.
    Call(u64),
    /// A process object's frame + 1.
    Process(u64),
    /// An endpoint's frame.
    Endpoint(u32),
    /// A device object's frame.
    Device(u32),
    /// A budget's frame: the chains of what was sent under its stamp.
    Budget(u32),
    /// The kernel's own words: the expiry's due list, and each process's timed waits.
    Kernel,
}

/// Where the lists' words are: word `word` of the lists' words in `page`. The kernel places them
/// above each kind of page's own words; a page that does not exist reads as 0.
pub trait Words {
    fn read(&self, page: Page, word: usize) -> u64;
    fn write(&mut self, page: Page, word: usize, value: u64);
}

// --- The words -----------------------------------------------------------------------------------

/// A thread's links on the list its wait puts it on.
pub const T_PREV: usize = 0;
pub const T_NEXT: usize = 1;
/// Its place in the one order of arrivals and takes: its message's arrival, or when it began to
/// receive.
pub const T_SEQ: usize = 2;
/// While its message is queued: the thread holding its group's node.
pub const T_GROUP: usize = 3;
/// While its message is queued: its links on its stamp budget's chain.
pub const T_SPREV: usize = 4;
pub const T_SNEXT: usize = 5;
/// Inside an expiry: its links on the due list.
pub const T_XPREV: usize = 6;
pub const T_XNEXT: usize = 7;
/// While it waits with a deadline: its links on its process's timed waits.
pub const T_DPREV: usize = 8;
pub const T_DNEXT: usize = 9;
// A group's node, on the thread that holds it: its links on the endpoint's two group lists, its
// two chains' ends, its count and its last take.
const G_PREV: usize = 10;
const G_NEXT: usize = 11;
const G_SPREV: usize = 12;
const G_SNEXT: usize = 13;
const G_SENDS: usize = 14;
const G_CALLS: usize = 16;
const G_COUNT: usize = 18;
/// The group's last take while it had anything queued (R2); 0 for none.
const G_TAKEN: usize = 19;
/// Words the lists take in a thread's IPC page.
pub const THREAD_WORDS: usize = 20;

/// An open call's links on its endpoint's open or notice list.
pub const C_EPREV: usize = 0;
pub const C_ENEXT: usize = 1;
/// An open call's links on its stamp budget's chain.
pub const C_SPREV: usize = 2;
pub const C_SNEXT: usize = 3;
/// Words the lists take in an open call's page.
pub const CALL_WORDS: usize = 4;

/// A process object's links on its exit endpoint's reporters or exits.
pub const P_PREV: usize = 0;
pub const P_NEXT: usize = 1;
/// Words the lists take in a process object's frame.
pub const PROCESS_WORDS: usize = 2;

/// An endpoint's lists: receivers, groups and send groups (head, tail), notices and open calls
/// (head), exit notices owed (head, tail) and the processes that report here (head).
const E_RECEIVERS: usize = 0;
const E_GROUPS: usize = 2;
const E_SENDS: usize = 4;
const E_NOTICES: usize = 6;
const E_OPEN: usize = 7;
/// How many members its lists hold: receivers, queued messages, owed notices, open calls, owed
/// exit notices and reporters. One read says whether anything waits there or names it (a
/// destruction skips an endpoint where nothing does).
const E_WAITING: usize = 8;
const E_EXITS: usize = 9;
const E_REPORTERS: usize = 11;
/// Words the lists take in an endpoint's frame.
pub const ENDPOINT_WORDS: usize = 12;

/// A device's interrupt waiters (head, tail).
const D_IRQ: usize = 0;
/// Words the lists take in a device's frame.
pub const DEVICE_WORDS: usize = 2;

/// A budget's chains: the queued messages and the waiting taken calls stamped with it.
pub const B_QUEUED: usize = 0;
pub const B_OPEN: usize = 1;

/// The due list (head, tail), in the kernel's own words; then each process slot's timed waits
/// (head).
const K_DUE: usize = 0;
const K_TIMED: usize = 2;

/// Words the lists take in the kernel's own, for `slots` process slots.
pub const fn kernel_words(slots: usize) -> usize { K_TIMED + slots }

// --- A list ----------------------------------------------------------------------------------------

/// What a list's members are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Member {
    Thread,
    Call,
    Process,
}

/// One doubly linked list: where its head (and tail, if it keeps one) is, and which words of its
/// members link them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct List {
    owner: Page,
    head: usize,
    tail: Option<usize>,
    member: Member,
    /// Its members count in its endpoint's [`waiting`].
    counted: bool,
    prev: usize,
    next: usize,
}

impl List {
    const fn threads(owner: Page, head: usize, tail: bool, (prev, next): (usize, usize)) -> List {
        List {
            owner,
            head,
            tail: if tail { Some(head + 1) } else { None },
            member: Member::Thread,
            counted: false,
            prev,
            next,
        }
    }

    const fn calls(owner: Page, head: usize, (prev, next): (usize, usize)) -> List {
        List { owner, head, tail: None, member: Member::Call, counted: false, prev, next }
    }

    const fn processes(e: u32, head: usize, tail: bool) -> List {
        List {
            owner: Page::Endpoint(e),
            head,
            tail: if tail { Some(head + 1) } else { None },
            member: Member::Process,
            counted: true,
            prev: P_PREV,
            next: P_NEXT,
        }
    }

    const fn counted(self) -> List { List { counted: true, ..self } }

    /// The threads in `receive` on endpoint `e`, in the order they began to wait.
    pub const fn receivers(e: u32) -> List {
        List::threads(Page::Endpoint(e), E_RECEIVERS, true, (T_PREV, T_NEXT)).counted()
    }

    /// The threads in `receive` on device `d`'s interrupt, in the order they began to wait.
    pub const fn irq_waiters(d: u32) -> List { List::threads(Page::Device(d), D_IRQ, true, (T_PREV, T_NEXT)) }

    /// The open calls owing an abandoned-call notice on endpoint `e` (R3).
    pub const fn notices(e: u32) -> List {
        List::calls(Page::Endpoint(e), E_NOTICES, (C_EPREV, C_ENEXT)).counted()
    }

    /// The calls taken on endpoint `e` whose callers still wait.
    pub const fn open(e: u32) -> List { List::calls(Page::Endpoint(e), E_OPEN, (C_EPREV, C_ENEXT)).counted() }

    /// The process objects owing an exit notice on endpoint `e`, in the order their notices came.
    pub const fn exits(e: u32) -> List { List::processes(e, E_EXITS, true) }

    /// The process objects naming endpoint `e` as their exit endpoint whose processes still run.
    pub const fn reporters(e: u32) -> List { List::processes(e, E_REPORTERS, false) }

    /// The queued messages sent through a handle stamped with budget `b`.
    pub const fn queued(b: u32) -> List {
        List::threads(Page::Budget(b), B_QUEUED, false, (T_SPREV, T_SNEXT))
    }

    /// The taken calls, their callers still waiting, sent through a handle stamped with budget `b`.
    pub const fn taken(b: u32) -> List { List::calls(Page::Budget(b), B_OPEN, (C_SPREV, C_SNEXT)) }

    /// An expiry's due waits.
    pub const fn due() -> List { List::threads(Page::Kernel, K_DUE, true, (T_XPREV, T_XNEXT)) }

    /// The threads of the process in slot `slot` in a wait with a deadline.
    pub const fn timed(slot: usize) -> List {
        List::threads(Page::Kernel, K_TIMED + slot, false, (T_DPREV, T_DNEXT))
    }

    /// The groups with a message queued on endpoint `e`, by when their turns fall due (R2).
    const fn groups(e: u32) -> List { List::threads(Page::Endpoint(e), E_GROUPS, true, (G_PREV, G_NEXT)) }

    /// The groups with a send queued on endpoint `e`, by when their oldest sends fall due.
    const fn send_groups(e: u32) -> List {
        List::threads(Page::Endpoint(e), E_SENDS, true, (G_SPREV, G_SNEXT))
    }

    /// The sends, or the calls, of the group whose node `node` holds, in arrival order.
    const fn chain(node: u64, send: bool) -> List {
        List::threads(Page::Thread(node), if send { G_SENDS } else { G_CALLS }, true, (T_PREV, T_NEXT))
    }

    fn page(&self, r: u64) -> Page {
        match self.member {
            Member::Thread => Page::Thread(r),
            Member::Call => Page::Call(r),
            Member::Process => Page::Process(r),
        }
    }

    /// Its first member, 0 for none.
    pub fn first(&self, w: &impl Words) -> u64 { w.read(self.owner, self.head) }

    pub fn is_empty(&self, w: &impl Words) -> bool { self.first(w) == 0 }

    /// The member after `r`, 0 for none.
    pub fn next(&self, w: &impl Words, r: u64) -> u64 { w.read(self.page(r), self.next) }

    fn prev(&self, w: &impl Words, r: u64) -> u64 { w.read(self.page(r), self.prev) }

    fn last(&self, w: &impl Words) -> u64 { w.read(self.owner, self.tail.expect("a list with a tail")) }

    /// Whether `r` is on it, read from `r`'s own links: one read, or two for its first member.
    pub fn contains(&self, w: &impl Words, r: u64) -> bool {
        r != 0 && (self.prev(w, r) != 0 || self.first(w) == r)
    }

    /// Link `r` after `after`, or first for 0.
    fn insert_after(&self, w: &mut impl Words, after: u64, r: u64) {
        let next = if after == 0 { self.first(w) } else { self.next(w, after) };
        w.write(self.page(r), self.prev, after);
        w.write(self.page(r), self.next, next);
        if after == 0 {
            w.write(self.owner, self.head, r);
        } else {
            w.write(self.page(after), self.next, r);
        }
        if next != 0 {
            w.write(self.page(next), self.prev, r);
        } else if let Some(tail) = self.tail {
            w.write(self.owner, tail, r);
        }
        if self.counted {
            counts(w, self.owner, 1);
        }
    }

    pub fn push_front(&self, w: &mut impl Words, r: u64) { self.insert_after(w, 0, r) }

    /// Only for a list that keeps a tail.
    pub fn push_back(&self, w: &mut impl Words, r: u64) {
        let last = self.last(w);
        self.insert_after(w, last, r)
    }

    /// Unlink `r`, which is on the list.
    pub fn remove(&self, w: &mut impl Words, r: u64) {
        let (prev, next) = (self.prev(w, r), self.next(w, r));
        if prev == 0 {
            w.write(self.owner, self.head, next);
        } else {
            w.write(self.page(prev), self.next, next);
        }
        if next != 0 {
            w.write(self.page(next), self.prev, prev);
        } else if let Some(tail) = self.tail {
            w.write(self.owner, tail, prev);
        }
        w.write(self.page(r), self.prev, 0);
        w.write(self.page(r), self.next, 0);
        if self.counted {
            counts(w, self.owner, -1);
        }
    }

    /// Unlink the first member and return it, 0 for none.
    pub fn pop_front(&self, w: &mut impl Words) -> u64 {
        let r = self.first(w);
        if r != 0 {
            self.remove(w, r);
        }
        r
    }

    /// `new` takes `old`'s place.
    fn replace(&self, w: &mut impl Words, old: u64, new: u64) {
        let (prev, next) = (self.prev(w, old), self.next(w, old));
        w.write(self.page(new), self.prev, prev);
        w.write(self.page(new), self.next, next);
        if prev == 0 {
            w.write(self.owner, self.head, new);
        } else {
            w.write(self.page(prev), self.next, new);
        }
        if next != 0 {
            w.write(self.page(next), self.prev, new);
        } else if let Some(tail) = self.tail {
            w.write(self.owner, tail, new);
        }
        w.write(self.page(old), self.prev, 0);
        w.write(self.page(old), self.next, 0);
    }

    /// `r`'s key rose: move it toward the tail, past every member with a lower key. Keys are values
    /// of one counter, so no two are equal.
    fn moved_back<W: Words>(&self, w: &mut W, r: u64, key: impl Fn(&W, u64) -> u64) {
        let k = key(w, r);
        let mut after = self.next(w, r);
        if after == 0 || key(w, after) > k {
            return;
        }
        self.remove(w, r);
        loop {
            let next = self.next(w, after);
            if next == 0 || key(w, next) > k {
                break;
            }
            after = next;
        }
        self.insert_after(w, after, r);
    }

    /// Order the list by `key`, stably: a bottom-up merge sort on the links, n log n steps for n
    /// members, with no memory but a few words.
    pub fn sort<W: Words, K: Ord>(&self, w: &mut W, key: impl Fn(&W, u64) -> K) {
        let mut n = 0usize;
        let mut r = self.first(w);
        while r != 0 {
            n += 1;
            r = self.next(w, r);
        }
        let mut head = self.first(w);
        let mut width = 1;
        while width < n {
            let (mut out, mut tail) = (0, 0);
            let mut p = head;
            while p != 0 {
                // Two runs of up to `width`, from `p`; `q` is what follows them.
                let mut l = p;
                let mut lsize = 0;
                let mut q = p;
                while q != 0 && lsize < width {
                    q = self.next(w, q);
                    lsize += 1;
                }
                let mut r = q;
                let mut rsize = 0;
                while q != 0 && rsize < width {
                    q = self.next(w, q);
                    rsize += 1;
                }
                while lsize > 0 || rsize > 0 {
                    // The left run's on an equal key: stable.
                    let left = rsize == 0 || (lsize > 0 && key(w, l) <= key(w, r));
                    let e = if left { l } else { r };
                    if left {
                        l = self.next(w, l);
                        lsize -= 1;
                    } else {
                        r = self.next(w, r);
                        rsize -= 1;
                    }
                    if tail == 0 {
                        out = e;
                    } else {
                        w.write(self.page(tail), self.next, e);
                    }
                    tail = e;
                }
                p = q;
            }
            w.write(self.page(tail), self.next, 0);
            head = out;
            width *= 2;
        }
        // The back links, the head and the tail, from the sorted forward links.
        w.write(self.owner, self.head, head);
        let (mut prev, mut r) = (0, head);
        while r != 0 {
            w.write(self.page(r), self.prev, prev);
            prev = r;
            r = self.next(w, r);
        }
        if let Some(tail) = self.tail {
            w.write(self.owner, tail, prev);
        }
    }

    /// The checked build's audit: every member's back link names the member before it, the head
    /// and the tail are the ends, and `each` passes every member, in order. A cycle breaks a back
    /// link, so the walk ends. Returns the members.
    pub fn audit<W: Words>(
        &self,
        w: &W,
        mut each: impl FnMut(&W, u64) -> Result<(), Fault>,
    ) -> Result<usize, Fault> {
        let (mut prev, mut r, mut n) = (0, self.first(w), 0);
        while r != 0 {
            if self.prev(w, r) != prev {
                return Err(Fault::Links(self.owner, self.head));
            }
            each(w, r)?;
            n += 1;
            prev = r;
            r = self.next(w, r);
        }
        if self.tail.is_some() && self.last(w) != prev {
            return Err(Fault::Links(self.owner, self.head));
        }
        Ok(n)
    }
}

/// What an audit found wrong.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// The list headed at this word of this page has a broken link, head or tail.
    Links(Page, usize),
    /// This member does not belong where it is.
    Member(Page),
    /// This member is out of its list's order.
    Order(Page),
    /// The group whose node this thread holds is not as its members say.
    Group(u64),
}

// --- R2's groups -----------------------------------------------------------------------------------

fn seq(w: &impl Words, t: u64) -> u64 { w.read(Page::Thread(t), T_SEQ) }

fn first_of(w: &impl Words, node: u64, send: bool) -> u64 {
    w.read(Page::Thread(node), if send { G_SENDS } else { G_CALLS })
}

/// The group's oldest message: the older of its two chains' heads.
fn oldest(w: &impl Words, node: u64) -> u64 {
    match (first_of(w, node, true), first_of(w, node, false)) {
        (0, c) => c,
        (s, 0) => s,
        (s, c) if seq(w, s) < seq(w, c) => s,
        (_, c) => c,
    }
}

/// When the group's turn falls due: its oldest message's arrival, or its last take if that came
/// later (R2; the model's `due`, which a take sets on every message then queued).
fn key<W: Words>(w: &W, node: u64) -> u64 { seq(w, oldest(w, node)).max(w.read(Page::Thread(node), G_TAKEN)) }

/// When the group's oldest send falls due, for a receiver that takes no calls.
fn send_key<W: Words>(w: &W, node: u64) -> u64 {
    seq(w, first_of(w, node, true)).max(w.read(Page::Thread(node), G_TAKEN))
}

/// The thread holding queued thread `t`'s group node.
pub fn group_of(w: &impl Words, t: u64) -> u64 { w.read(Page::Thread(t), T_GROUP) }

/// The messages the group whose node `node` holds has queued (R2's `WAIT_CAP` counts them).
pub fn count(w: &impl Words, node: u64) -> u64 { w.read(Page::Thread(node), G_COUNT) }

/// The node of the group on endpoint `e` that `same` picks, if it has a message queued there: a
/// walk of `e`'s groups only.
pub fn find<W: Words>(w: &W, e: u32, mut same: impl FnMut(&W, u64) -> bool) -> Option<u64> {
    let groups = List::groups(e);
    let mut node = groups.first(w);
    while node != 0 {
        if same(w, node) {
            return Some(node);
        }
        node = groups.next(w, node);
    }
    None
}

/// Queue thread `t`'s message (a send if `send`) on endpoint `e` at `seq`, the newest value of
/// the one counter, in the group whose node `node` holds, or in a new group for `None`. A new
/// group's turn, and a group's first send, fall due after every other's, so each goes last.
pub fn enqueue(w: &mut impl Words, e: u32, node: Option<u64>, t: u64, send: bool, seq: u64) {
    w.write(Page::Thread(t), T_SEQ, seq);
    let node = match node {
        Some(node) => node,
        None => {
            clear_node(w, t);
            List::groups(e).push_back(w, t);
            t
        }
    };
    w.write(Page::Thread(t), T_GROUP, node);
    let chain = List::chain(node, send);
    if send && chain.is_empty(w) {
        List::send_groups(e).push_back(w, node);
    }
    chain.push_back(w, t);
    w.write(Page::Thread(node), G_COUNT, count(w, node) + 1);
    counts(w, Page::Endpoint(e), 1);
}

/// R2: the message the next receiver on `e` takes: the oldest of the group whose turn has been due
/// longest; for a receiver that takes no calls (R4a), the oldest send of the group whose oldest
/// send has been due longest. 0 for none.
pub fn pick(w: &impl Words, e: u32, calls: bool) -> u64 {
    let node = if calls { List::groups(e).first(w) } else { List::send_groups(e).first(w) };
    match node {
        0 => 0,
        node if calls => oldest(w, node),
        node => first_of(w, node, true),
    }
}

/// R2: queued thread `t`'s group on `e` has its turn, delivered or refused, at `now`, the newest
/// value of the counter. Its turn is due again from now, behind every group already waiting, on
/// both lists it is on: one write for the take and two moves, whatever the group holds.
pub fn served(w: &mut impl Words, e: u32, t: u64, now: u64) {
    let node = group_of(w, t);
    w.write(Page::Thread(node), G_TAKEN, now);
    for list in [List::groups(e), List::send_groups(e)] {
        if list.contains(w, node) {
            list.remove(w, node);
            list.push_back(w, node);
        }
    }
}

/// Queued thread `t`'s message (a send if `send`) leaves endpoint `e`, taken or not. A group
/// left with nothing leaves both lists; otherwise, if `t` held the node, the node moves to the
/// group's oldest message left, and a group whose oldest message, or oldest send, left without a
/// take falls due later, so it moves back past the groups now due before it: a walk of `e`'s
/// groups at most.
pub fn dequeue(w: &mut impl Words, e: u32, t: u64, send: bool) {
    let mut node = group_of(w, t);
    let chain = List::chain(node, send);
    let first_send = send && chain.first(w) == t;
    chain.remove(w, t);
    w.write(Page::Thread(t), T_GROUP, 0);
    counts(w, Page::Endpoint(e), -1);
    let left = count(w, node) - 1;
    let (groups, sends) = (List::groups(e), List::send_groups(e));
    if left == 0 {
        groups.remove(w, node);
        if sends.contains(w, node) {
            sends.remove(w, node);
        }
        clear_node(w, node);
        return;
    }
    w.write(Page::Thread(node), G_COUNT, left);
    if t == node {
        node = oldest(w, t);
        move_node(w, e, t, node);
        groups.moved_back(w, node, key::<_>);
    }
    if first_send {
        if List::chain(node, true).is_empty(w) {
            sends.remove(w, node);
        } else {
            sends.moved_back(w, node, send_key::<_>);
        }
    }
}

/// The node moves from `old` to `new`, a member of its group: its place on both lists, its words,
/// and every member's [`T_GROUP`], at most `WAIT_CAP - 1` writes.
fn move_node(w: &mut impl Words, e: u32, old: u64, new: u64) {
    let (groups, sends) = (List::groups(e), List::send_groups(e));
    groups.replace(w, old, new);
    if sends.contains(w, old) {
        sends.replace(w, old, new);
    }
    for i in G_SENDS..=G_TAKEN {
        let v = w.read(Page::Thread(old), i);
        w.write(Page::Thread(new), i, v);
    }
    clear_node(w, old);
    for send in [true, false] {
        let chain = List::chain(new, send);
        let mut m = chain.first(w);
        while m != 0 {
            w.write(Page::Thread(m), T_GROUP, new);
            m = chain.next(w, m);
        }
    }
}

fn clear_node(w: &mut impl Words, node: u64) {
    for i in G_PREV..=G_TAKEN {
        w.write(Page::Thread(node), i, 0);
    }
}

/// The checked build's audit of endpoint `e`'s groups: both lists' links and order, each group's
/// node on its oldest message, its chains' links and arrival order, its count, every member's
/// [`T_GROUP`], a group on the send list exactly when it has a send, and `member` for every
/// member, with whether it is a send. Returns the members.
pub fn audit_groups<W: Words>(
    w: &W,
    e: u32,
    mut member: impl FnMut(&W, u64, bool) -> Result<(), Fault>,
) -> Result<usize, Fault> {
    let (groups, sends) = (List::groups(e), List::send_groups(e));
    let (mut last, mut members, mut with_sends) = (0, 0, 0);
    groups.audit(w, |w, node| {
        let k = key(w, node);
        if k <= last {
            return Err(Fault::Order(Page::Thread(node)));
        }
        last = k;
        if oldest(w, node) != node {
            return Err(Fault::Group(node));
        }
        let mut n = 0;
        for send in [true, false] {
            let mut prev_seq = 0;
            n += List::chain(node, send).audit(w, |w, m| {
                if group_of(w, m) != node {
                    return Err(Fault::Group(node));
                }
                let s = seq(w, m);
                if s <= prev_seq {
                    return Err(Fault::Order(Page::Thread(m)));
                }
                prev_seq = s;
                member(w, m, send)
            })?;
        }
        if n == 0 || n as u64 != count(w, node) {
            return Err(Fault::Group(node));
        }
        let has_sends = !List::chain(node, true).is_empty(w);
        if has_sends != sends.contains(w, node) {
            return Err(Fault::Group(node));
        }
        with_sends += usize::from(has_sends);
        members += n;
        Ok(())
    })?;
    let mut last = 0;
    let listed = sends.audit(w, |w, node| {
        let k = send_key(w, node);
        if k <= last {
            return Err(Fault::Order(Page::Thread(node)));
        }
        last = k;
        if !groups.contains(w, node) || List::chain(node, true).is_empty(w) {
            return Err(Fault::Group(node));
        }
        Ok(())
    })?;
    if listed != with_sends {
        return Err(Fault::Links(Page::Endpoint(e), E_SENDS));
    }
    Ok(members)
}

/// How many members endpoint `e`'s lists hold: its receivers, queued messages, owed notices, open
/// calls, owed exit notices and reporters. 0 exactly when nothing waits there or names it.
pub fn waiting(w: &impl Words, e: u32) -> u64 { w.read(Page::Endpoint(e), E_WAITING) }

/// Add `by` to endpoint `page`'s count of members.
fn counts(w: &mut impl Words, page: Page, by: i64) {
    let n = w.read(page, E_WAITING).checked_add_signed(by).expect("an endpoint's count of members");
    w.write(page, E_WAITING, n);
}

/// Whether endpoint `e` has nothing queued.
pub fn no_groups(w: &impl Words, e: u32) -> bool { List::groups(e).is_empty(w) }

/// The node of the group on endpoint `e` whose turn is due first, 0 for none.
pub fn first_group(w: &impl Words, e: u32) -> u64 { List::groups(e).first(w) }

#[cfg(test)]
mod tests;
