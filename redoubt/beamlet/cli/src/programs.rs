//! Programs the VM starts behind ports (`--exec`): host processes, with their standard input
//! and output connected to the VM through threads, so the VM never blocks on them.
//!
//! A program is not sandboxed: it sees the host's file system and runs with the user's rights,
//! whatever `--root` gives the VM. That is why starting programs needs `--exec`. What the VM
//! does decide is which executable (a path in its own name space, mapped to the host's), the
//! program's environment (the VM's own, not the host's) and its working directory.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use beamlet_vm::platform::{ConsoleInput, FileError, Program, ProgramEvent, Spawn, Spawned};

/// Something from outside the VM: console input, or what a program did.
pub enum Event {
    Console(ConsoleInput),
    Program(u64, ProgramEvent),
}

/// A program the VM is talking to.
struct Running {
    /// Input for the writer thread; dropping it closes the program's input.
    input: Option<Sender<Vec<u8>>>,
    /// Set when the VM stops listening: the reader thread then stops reading.
    closed: Arc<AtomicBool>,
}

/// How many started programs have not yet exited, for waiting until they have.
#[derive(Clone, Default)]
pub struct Alive(Arc<(std::sync::Mutex<usize>, std::sync::Condvar)>);

impl Alive {
    fn add(&self, n: isize) {
        let (count, changed) = &*self.0;
        let mut c = count.lock().unwrap_or_else(|e| e.into_inner());
        *c = c.saturating_add_signed(n);
        changed.notify_all();
    }

    /// Wait until every program has exited, or `limit` has passed.
    pub fn wait(&self, limit: std::time::Duration) {
        let (count, changed) = &*self.0;
        let c = count.lock().unwrap_or_else(|e| e.into_inner());
        let _ = changed.wait_timeout_while(c, limit, |c| *c > 0);
    }
}

pub struct Programs {
    events: Sender<Event>,
    alive: Alive,
    running: BTreeMap<u64, Running>,
    next: u64,
}

impl Programs {
    pub fn new(events: Sender<Event>, alive: Alive) -> Programs {
        Programs {
            events,
            alive,
            running: BTreeMap::new(),
            next: 1,
        }
    }

    /// Whether any program may still send events.
    pub fn any(&self) -> bool {
        !self.running.is_empty()
    }

    /// Start `spawn`, with `host` mapping VM paths to host paths.
    pub fn spawn(
        &mut self,
        spawn: &Spawn,
        host: impl Fn(&str) -> Result<PathBuf, FileError>,
    ) -> Result<Spawned, FileError> {
        let mut cmd = match &spawn.program {
            // As BEAM runs `{spawn, Command}`.
            Program::Shell(line) => {
                let mut cmd = Command::new("/bin/sh");
                cmd.arg("-c").arg(format!("exec {line}"));
                cmd
            }
            Program::Executable { path, arg0, args } => {
                let mut cmd = Command::new(host(path)?);
                if let Some(arg0) = arg0 {
                    std::os::unix::process::CommandExt::arg0(&mut cmd, arg0);
                }
                cmd.args(args);
                cmd
            }
        };
        cmd.env_clear()
            .envs(spawn.env.iter().map(|(k, v)| (k, v)))
            .current_dir(host(&spawn.cwd)?);
        cmd.stdin(if spawn.input {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let merged = if spawn.output && spawn.stderr_to_stdout {
            let (reader, writer) = std::io::pipe().map_err(crate::files::error)?;
            cmd.stdout(writer.try_clone().map_err(crate::files::error)?);
            cmd.stderr(writer);
            Some(reader)
        } else {
            cmd.stdout(if spawn.output {
                Stdio::piped()
            } else {
                Stdio::null()
            });
            None
        };
        let mut child = cmd.spawn().map_err(crate::files::error)?;
        // The command holds the write end of a merged pipe: drop it, or the output never ends.
        drop(cmd);
        let handle = self.next;
        self.next += 1;
        let os_pid = Some(child.id() as u64);

        let input = child.stdin.take().map(|mut stdin| {
            let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
            std::thread::spawn(move || {
                for data in rx {
                    if stdin.write_all(&data).and_then(|()| stdin.flush()).is_err() {
                        break;
                    }
                }
            });
            tx
        });
        let output: Option<Box<dyn Read + Send>> = match merged {
            Some(reader) => Some(Box::new(reader)),
            None => child
                .stdout
                .take()
                .map(|o| Box::new(o) as Box<dyn Read + Send>),
        };
        let closed = Arc::new(AtomicBool::new(false));
        let (events, stop) = (self.events.clone(), closed.clone());
        let alive = self.alive.clone();
        alive.add(1);
        std::thread::spawn(move || {
            let send = |e: ProgramEvent| events.send(Event::Program(handle, e)).is_ok();
            if let Some(mut output) = output {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match output.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) if !stop.load(Ordering::Relaxed) => {
                            if !send(ProgramEvent::Output(buf[..n].to_vec())) {
                                break;
                            }
                        }
                        // Closed by the VM: stop reading, as BEAM closes its end.
                        Ok(_) => break,
                    }
                }
                send(ProgramEvent::Eof);
            }
            let status = match child.wait() {
                Ok(s) => s.code().unwrap_or_else(|| {
                    128 + std::os::unix::process::ExitStatusExt::signal(&s).unwrap_or(0)
                }),
                Err(_) => 128,
            };
            alive.add(-1);
            send(ProgramEvent::Exit(status));
        });
        self.running.insert(handle, Running { input, closed });
        Ok(Spawned { handle, os_pid })
    }

    pub fn write(&mut self, handle: u64, data: &[u8]) -> Result<(), FileError> {
        let running = self.running.get(&handle).ok_or(FileError::Ebadf)?;
        let input = running.input.as_ref().ok_or(FileError::Ebadf)?;
        input.send(data.to_vec()).map_err(|_| FileError::Eio)
    }

    pub fn close(&mut self, handle: u64) {
        if let Some(running) = self.running.remove(&handle) {
            running.closed.store(true, Ordering::Relaxed);
        }
    }

    /// A program has exited: it sends nothing more.
    pub fn exited(&mut self, handle: u64) {
        self.running.remove(&handle);
    }
}
