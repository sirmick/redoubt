//! Scripted ABI seam, not a kernel model or evidence of kernel isolation. One test per binary.
#![allow(dead_code)]

use std::sync::Mutex;

use redoubt_rt::abi::*;
use redoubt_rt::{HostKernel, install_host_kernel};

#[repr(align(4096))]
struct Page([u8; PAGE_SIZE]);

pub struct Kernel(pub Mutex<State>);
pub struct State {
    page: Box<Page>,
    pub mapped: bool,
    pub unmapped: usize,
    pub closed: Vec<Handle>,
    pub call: CallOutcome,
    pub reply: Result<ReplyOutcome, Error>,
    /// The fallback reply's outcome after one injected validation failure.
    pub fallback: Result<ReplyOutcome, Error>,
    pub open_calls: usize,
    pub lent_pages: usize,
    pub exited: bool,
    pub body: ReceivedBody,
    pub request: Option<Received>,
    pub replies: Vec<Body>,
}

impl Kernel {
    pub fn install() -> &'static Self {
        let kernel = Box::leak(Box::new(Self(Mutex::new(State {
            page: Box::new(Page([0; PAGE_SIZE])),
            mapped: false,
            unmapped: 0,
            closed: vec![],
            call: CallOutcome { status: Ok(()), lend: LendDisposition::Returned, reply_present: true },
            reply: Ok(ReplyOutcome { delivered: true, installed: 0 }),
            fallback: Ok(ReplyOutcome { delivered: true, installed: 0 }),
            open_calls: 0,
            lent_pages: 0,
            exited: false,
            body: ReceivedBody { words: [42, 0, 0, 0], handles: ReceivedHandles::new() },
            request: None,
            replies: vec![],
        }))));
        install_host_kernel(kernel);
        kernel
    }

    pub fn request(&self, words: [u64; WORDS], lend: Option<Pages>) {
        let mut state = self.0.lock().unwrap();
        state.request = Some(Received::Message(Message {
            kind: MessageKind::Call { lend },
            msg_id: std::num::NonZeroU64::new(1).unwrap(),
            badge: 1,
            account: 1001,
            labels: Labels::new(),
            body: ReceivedBody { words: words.map(|x| x as usize), handles: ReceivedHandles::new() },
        }));
    }
}

impl HostKernel for Kernel {
    fn syscall(&self, call: &Call) -> Result<Return, Error> {
        let mut s = self.0.lock().unwrap();
        match *call {
            Call::MapAnon { len, .. } => {
                assert_eq!(len, PAGE_SIZE);
                assert!(!s.mapped);
                s.mapped = true;
                s.page.0.fill(0);
                Ok(Return::Addr(s.page.0.as_mut_ptr() as usize))
            }
            Call::Unmap { addr, .. } => {
                assert_eq!(addr, s.page.0.as_mut_ptr() as usize);
                assert!(s.mapped, "stale or duplicate unmap");
                s.mapped = false;
                s.unmapped += 1;
                Ok(Return::Nothing)
            }
            Call::Call { body_rec, .. } => {
                if s.call.lend == LendDisposition::Consumed {
                    s.mapped = false;
                }
                if s.call.reply_present {
                    // SAFETY: the runtime owns this aligned, exclusively borrowed live record.
                    unsafe {
                        (body_rec as *mut [u64; BODY_SLOTS]).write(s.body.encode());
                    }
                }
                decode_result(Number::Call, &encode_result(&Ok(Return::Call(s.call))))
            }
            Call::Receive { received_rec, .. } => {
                let request = s.request.take().unwrap();
                assert_eq!(s.open_calls, 0, "previous reply obligation was lost");
                s.open_calls = 1;
                if let Received::Message(Message { kind: MessageKind::Call { lend }, .. }) = request {
                    s.lent_pages = lend.map_or(0, |pages| pages.npages.get());
                }
                // SAFETY: the runtime supplies a live, aligned, exclusively borrowed record.
                unsafe {
                    (received_rec as *mut [u64; RECEIVED_SLOTS]).write(request.encode());
                }
                Ok(Return::Nothing)
            }
            Call::Reply { body_rec, .. } => {
                assert_eq!(s.open_calls, 1, "no open call to reply to");
                // SAFETY: Request::reply keeps this aligned input record live across the seam.
                let body = unsafe { (body_rec as *const [u64; BODY_SLOTS]).read() };
                s.replies.push(Body::decode(&body).unwrap());
                let reply = s.reply;
                if reply.is_ok() {
                    s.open_calls = 0;
                    s.lent_pages = 0;
                } else {
                    s.reply = s.fallback;
                }
                reply.map(Return::Reply)
            }
            Call::ProcessExit { code } => {
                s.open_calls = 0;
                s.lent_pages = 0;
                s.exited = true;
                drop(s);
                std::panic::resume_unwind(Box::new(code))
            }
            Call::HandleClose { handle } => {
                s.closed.push(handle);
                Ok(Return::Nothing)
            }
            Call::Mint { .. } => Ok(Return::Handle(Handle::new(77).unwrap())),
            Call::Random => Ok(Return::Random(0x1234_5678_9012)),
            other => panic!("unexpected call {other:?}"),
        }
    }
}
