//! The system's natives, module `redoubt` (docs/userland/beamlet.md, "Natives"), against a test
//! platform's `System`: each native's round trip, and the attacks the boundary itself refuses (a
//! term of the wrong type, a list past its cap, a handle written out and read back, a handle of the
//! wrong kind), with no panic. That a dropped handle is closed is the platform's drop, run when the
//! last process holding it is collected. The fixture's source is `src/natives.erl`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use beamlet_vm::Vm;
use beamlet_vm::platform::{
    BudgetSpec, Entry, Event, Identity, Launch, Lookup, Message, Object, Platform, PlatformError, Refused,
    System, Usage,
};

/// A handle of the test platform: its kind and name, counted when it is dropped.
struct Mock {
    kind: &'static str,
    name: &'static str,
    drops: Arc<AtomicUsize>,
}

impl Drop for Mock {
    fn drop(&mut self) {
        if self.name == "keyd" {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// What the platform was asked, for the test to read back.
#[derive(Default)]
struct Seen {
    calls: Vec<String>,
}

struct Test {
    now: u64,
    events: VecDeque<(u64, Event)>,
    drops: Arc<AtomicUsize>,
    seen: Arc<Mutex<Seen>>,
    system: bool,
}

impl Test {
    fn make(&self, kind: &'static str, name: &'static str) -> Object {
        Arc::new(Mock { kind, name, drops: Arc::clone(&self.drops) })
    }

    fn saw(&self, call: String) { self.seen.lock().unwrap().calls.push(call); }
}

fn mock(o: &Object) -> &Mock { o.downcast_ref::<Mock>().expect("only the test's handles reach it") }

fn kind(o: &Object) -> &'static str { mock(o).kind }

impl Platform for Test {
    fn monotonic_us(&mut self) -> u64 {
        self.now += 1;
        self.now
    }

    fn system_time_us(&mut self) -> Option<u64> { None }

    fn idle(&mut self, deadline: Option<u64>) {
        if let Some(d) = deadline {
            self.now = self.now.max(d);
        }
    }

    fn console_write(&mut self, _bytes: &[u8]) {}

    fn random(&mut self, buf: &mut [u8]) -> Result<(), PlatformError> {
        buf.fill(4);
        Ok(())
    }

    fn load_module(&mut self, module: &str) -> Lookup {
        match module {
            "natives" => Lookup::Found(include_bytes!("fixtures/natives.beam").to_vec()),
            _ => Lookup::Absent,
        }
    }

    fn system(&mut self) -> Option<&mut dyn System> { if self.system { Some(self) } else { None } }
}

impl System for Test {
    fn lookup(&mut self, path: &str) -> Result<(Object, String), Refused> {
        self.saw(format!("lookup {path}"));
        match path {
            "budget" => Ok((self.make("budget", "budget"), String::new())),
            "keyd" => Ok((self.make("endpoint", "keyd"), String::new())),
            "service" => Ok((self.make("endpoint", "service"), String::new())),
            // How many `keyd` handles have been dropped so far, as the rest.
            "drops" => Ok((self.make("endpoint", "counter"), self.drops.load(Ordering::SeqCst).to_string())),
            _ if path.contains("..") => Err(Refused("bad_name")),
            "/home/alice" => Ok((self.make("connection", "home"), String::new())),
            _ => match path.strip_prefix("/home/alice/") {
                Some(rest) => Ok((self.make("connection", "home"), rest.to_string())),
                None => Err(Refused("not_found")),
            },
        }
    }

    fn bind(&mut self, prefix: &str, handle: &Object) -> Result<(), Refused> {
        self.saw(format!("bind {prefix} {}", mock(handle).name));
        if kind(handle) == "budget" { Err(Refused("not_a_connection")) } else { Ok(()) }
    }

    fn table(&mut self) -> Vec<Entry> {
        vec![
            Entry {
                path: "/home/alice".into(),
                name: Some("littlefsd:home".into()),
                handle: self.make("connection", "home"),
            },
            Entry { path: "/dev/cons".into(), name: None, handle: self.make("connection", "cons") },
            Entry {
                path: "budget".into(),
                name: Some("budget".into()),
                handle: self.make("budget", "budget"),
            },
        ]
    }

    fn call(
        &mut self,
        asker: u64,
        call: u64,
        to: &Object,
        message: Message,
        timeout_us: u64,
    ) -> Result<(), Refused> {
        if kind(to) == "budget" {
            return Err(Refused("wrong_object"));
        }
        let handles: Vec<&str> = message.handles.iter().map(|h| mock(h).name).collect();
        self.saw(format!(
            "call {} {:?} {:?} {handles:?} {timeout_us}",
            mock(to).name,
            message.words,
            message.buffer
        ));
        let reply = Message {
            words: [0, 3, 0, 0],
            buffer: message.buffer.map(|_| b"key".to_vec()),
            handles: if message.words[0] == 3 { vec![self.make("endpoint", "granted")] } else { Vec::new() },
        };
        self.events.push_back((asker, Event::Reply { call, result: Ok(reply) }));
        Ok(())
    }

    fn send(&mut self, to: &Object, message: Message) -> Result<(), Refused> {
        self.saw(format!("send {} {:?}", mock(to).name, message.words));
        Ok(())
    }

    fn serve(&mut self, asker: u64, endpoint: &Object) -> Result<(), Refused> {
        self.saw(format!("serve {}", mock(endpoint).name));
        let request = Event::Request {
            request: Some(self.make("request", "request")),
            badge: 5,
            account: 1001,
            labels: vec![7],
            message: Message {
                words: [2, 0, 0, 0],
                buffer: Some(b"hi".to_vec()),
                handles: vec![self.make("endpoint", "theirs")],
            },
        };
        self.events.push_back((asker, request));
        Ok(())
    }

    fn reply(&mut self, request: &Object, reply: Message) -> Result<(), Refused> {
        self.saw(format!("reply {} {:?}", mock(request).name, reply.words));
        Ok(())
    }

    fn budget_create(&mut self, spec: &BudgetSpec) -> Result<Object, Refused> {
        self.saw(format!("budget_create {spec:?}"));
        // Left out, the VM's own; given, the kernel's verdict on a set that is not exactly the VM's.
        if spec.labels.as_ref().is_none_or(|l| *l == self.labels()) {
            Ok(self.make("budget", "child"))
        } else {
            Err(Refused("label_denied"))
        }
    }

    fn budget_destroy(&mut self, budget: &Object) -> Result<(), Refused> {
        if kind(budget) != "budget" {
            return Err(Refused("wrong_object"));
        }
        self.saw(format!("budget_destroy {}", mock(budget).name));
        Ok(())
    }

    fn budget_usage(&mut self, budget: &Object) -> Result<Usage, Refused> {
        if kind(budget) != "budget" {
            return Err(Refused("wrong_object"));
        }
        Ok(Usage { pages: (64, 3), processes: (1, 0), weight: (10, 0) })
    }

    fn labels(&mut self) -> Vec<u64> { vec![7, 9] }

    fn identity(&mut self) -> Option<Identity> {
        let labels = vec![("alice-secrets".into(), 7)];
        Some(Identity { principal: "alice".into(), labels, context: Some("work".into()) })
    }

    fn launch(&mut self, asker: u64, job: u64, launch: Launch) -> Result<(), Refused> {
        let ns: Vec<(&str, &str)> =
            launch.namespace.iter().map(|(p, h)| (p.as_str(), mock(h).name)).collect();
        let named: Vec<(&str, &str)> =
            launch.handles.iter().map(|(n, h)| (n.as_str(), mock(h).name)).collect();
        self.saw(format!(
            "launch {:?} {} {ns:?} {named:?} {:?}",
            launch.image,
            mock(&launch.budget).name,
            launch.args
        ));
        self.events.push_back((asker, Event::Exit { job, cause: "exited", code: 0 }));
        Ok(())
    }

    fn poll(&mut self) -> Option<(u64, Event)> { self.events.pop_front() }
}

/// Runs `natives:f()` on a platform with system calls (or without), and returns its result as
/// text, what the platform saw, and how many `keyd` handles were dropped by then.
fn run_on(f: &str, system: bool) -> (String, Vec<String>, Arc<AtomicUsize>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let drops = Arc::new(AtomicUsize::new(0));
    let platform =
        Test { now: 0, events: VecDeque::new(), drops: Arc::clone(&drops), seen: Arc::clone(&seen), system };
    let mut vm = Vm::new(Box::new(platform));
    let pid = vm.spawn("natives", f, |_| Vec::new()).unwrap();
    let result = vm.run_bounded(pid, 10_000_000).expect("finished").unwrap().unwrap().to_string();
    drop(vm);
    let calls = std::mem::take(&mut seen.lock().unwrap().calls);
    (result, calls, drops)
}

fn run(f: &str) -> (String, Vec<String>) {
    let (result, calls, _) = run_on(f, true);
    (result, calls)
}

/// A binary as the VM prints it.
fn text(s: &str) -> String {
    let bytes: Vec<String> = s.bytes().map(|b| b.to_string()).collect();
    format!("<<{}>>", bytes.join(","))
}

#[test]
fn a_lookup_gives_the_prefixs_connection_and_the_rest_and_refuses_by_name() {
    let (result, calls) = run("lookup");
    assert_eq!(result, format!("{{true,{},true,{{error,not_found}},{{error,bad_name}}}}", text("notes.txt")));
    assert_eq!(calls[0], "lookup /home/alice/notes.txt");
}

#[test]
fn a_bind_names_a_connection_and_refuses_anything_else() {
    let (result, calls) = run("bind");
    assert_eq!(result, "{ok,{error,not_a_connection}}");
    assert!(calls.contains(&"bind /h home".to_string()), "{calls:?}");
}

#[test]
fn the_table_lists_path_name_and_handle() {
    let (result, _) = run("table");
    assert_eq!(
        result,
        format!(
            "[{{{},{},true}},{{{},nil,true}},{{{},{},true}}]",
            text("/home/alice"),
            text("littlefsd:home"),
            text("/dev/cons"),
            text("budget"),
            text("budget")
        )
    );
}

#[test]
fn a_calls_reply_arrives_as_a_message_with_its_handles_as_resources() {
    let (result, calls) = run("call");
    assert_eq!(result, format!("{{[0,3,0,0],{},true,{{ok,{{[0,3,0,0],nil,[]}}}}}}", text("key")));
    // Words, buffer and handles reach the platform as given; the timeout in microseconds.
    assert_eq!(calls[1], "call keyd [3, 0, 0, 0] Some([97, 115, 107]) [\"keyd\"] 1000000");
    assert_eq!(calls[2], "call keyd [4, 0, 0, 0] None [] 0");
}

#[test]
fn a_send_is_one_way() {
    let (result, calls) = run("send");
    assert_eq!(result, "ok");
    assert_eq!(calls[1], "send keyd [9, 1, 2, 3]");
}

#[test]
fn budgets_are_carved_read_and_destroyed() {
    let (result, calls) = run("budgets");
    assert_eq!(result, "{#{pages => {64,3},processes => {1,0},weight => {10,0}},ok,{error,label_denied}}");
    assert!(
        calls[0]
            .contains("pages: 64, processes: 1, weight: 10, labels: None, account: 0, deadline: Some(5000)")
    );
    assert_eq!(calls[1], "budget_destroy child");
}

#[test]
fn labels_are_fixed() {
    assert_eq!(run("labels").0, "{[7,9],[7,9]}");
}

#[test]
fn the_identity_is_the_platforms_and_without_a_system_is_not_supported() {
    let (result, _) = run("identity");
    assert_eq!(
        result,
        format!(
            "{{ok,#{{context => {},labels => [{{{},7}}],principal => {}}}}}",
            text("work"),
            text("alice-secrets"),
            text("alice")
        )
    );
    assert_eq!(run_on("identity", false).0, "{error,not_supported}");
}

#[test]
fn a_launch_takes_everything_from_its_caller_and_its_end_arrives_as_a_message() {
    let (result, calls) = run("launch");
    assert_eq!(result, "{true,exited,0}");
    let launch = calls.iter().find(|c| c.starts_with("launch")).expect("launched");
    assert_eq!(
        launch,
        "launch [69, 76, 70] child [(\"/data\", \"home\")] [(\"budget\", \"child\")] [\"-v\"]"
    );
}

#[test]
fn requests_arrive_with_badge_account_and_labels_and_are_answered() {
    let (result, calls) = run("serve");
    assert_eq!(result, format!("{{5,1001,[7],[2,0,0,0],{},1,ok}}", text("hi")));
    assert!(calls.contains(&"reply request [0, 0, 0, 0]".to_string()), "{calls:?}");
}

#[test]
fn a_decoded_handle_grants_nothing() {
    let (result, calls) = run("decoded");
    // The same reference to Erlang code, and no native takes it.
    assert_eq!(result, "{true,true,{raised,badarg},{raised,badarg},{raised,badarg},{raised,badarg}}");
    assert!(!calls.iter().any(|c| c.starts_with("bind") || c.starts_with("call") || c.starts_with("budget")));
}

#[test]
fn arguments_of_the_wrong_type_are_badarg_and_reach_no_platform_call() {
    let (result, calls) = run("wrong_types");
    assert_eq!(result, format!("[{}]", ["{raised,badarg}"; 16].join(",")));
    // Only the fixture's own lookup reached the platform.
    assert_eq!(calls, ["lookup /home/alice"]);
}

#[test]
fn lists_past_their_caps_are_refused() {
    let (result, calls) = run("oversize");
    assert_eq!(result, format!("[{},{{error,too_many}}]", ["{raised,badarg}"; 7].join(",")));
    assert_eq!(calls, ["lookup /home/alice"]);
}

#[test]
fn a_handle_of_the_wrong_kind_is_refused_by_name() {
    let (result, _) = run("wrong_kind");
    assert_eq!(
        result,
        "{{error,wrong_object},{error,wrong_object},{error,wrong_object},{error,not_a_connection}}"
    );
}

#[test]
fn a_dropped_handle_is_closed_when_its_holder_is_collected() {
    // Counted while the VM runs: the process that looked it up has exited, and nothing else held it.
    assert_eq!(run("dropped").0, format!("{{{},{}}}", text("0"), text("1")));
}

#[test]
fn without_system_calls_every_native_is_not_supported() {
    let (result, _, _) = run_on("unsupported", false);
    assert_eq!(result, "{{error,not_supported},[],[]}");
}
