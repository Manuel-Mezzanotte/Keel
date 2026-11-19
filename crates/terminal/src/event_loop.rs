//! The thread that reads the program's output into the screen and writes
//! keys to it. It is Alacritty's event loop (alacritty_terminal 0.26,
//! Apache-2.0) with one change: the output is cut right after each mark the
//! shell prints, so the mark gets the line the cursor is on then.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read as _, Write as _};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, OnResize as _};
use alacritty_terminal::event_loop::Msg;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::tty::{self, EventedPty as _, EventedReadWrite as _};
use alacritty_terminal::vte::ansi;
use polling::{Event as PollingEvent, Events, PollMode, Poller};

use crate::marks::{Mark, Scanner};
use crate::{Listener, Shared};

/// Bytes read before the screen is brought up to date whatever happens.
const READ_BUFFER_SIZE: usize = 0x10_0000;
/// Bytes read while the screen is locked, so drawing still gets a turn.
const MAX_LOCKED_READ: usize = u16::MAX as usize;
/// The keys Alacritty's pseudo-terminal registers itself with; private
/// there, and the same on every Unix.
const PTY_READ_WRITE_TOKEN: usize = 0;
const PTY_CHILD_EVENT_TOKEN: usize = 1;

/// Sends keys, sizes and the end to the loop, and wakes it.
#[derive(Clone)]
pub(crate) struct LoopSender {
    sender: Sender<Msg>,
    poller: Arc<Poller>,
}

impl LoopSender {
    pub(crate) fn send(&self, message: Msg) -> io::Result<()> {
        self.sender
            .send(message)
            .map_err(|_| io::Error::from(ErrorKind::BrokenPipe))?;
        self.poller.notify()
    }
}

pub(crate) struct EventLoop {
    poll: Arc<Poller>,
    pty: tty::Pty,
    rx: PeekableReceiver<Msg>,
    tx: Sender<Msg>,
    terminal: Arc<FairMutex<Term<Listener>>>,
    event_proxy: Listener,
    shared: Arc<Shared>,
}

#[derive(Default)]
struct State {
    write_list: VecDeque<Cow<'static, [u8]>>,
    writing: Option<Writing>,
    parser: ansi::Processor,
    scanner: Scanner,
}

impl EventLoop {
    pub(crate) fn new(
        terminal: Arc<FairMutex<Term<Listener>>>,
        shared: Arc<Shared>,
        pty: tty::Pty,
    ) -> io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        Ok(Self {
            poll: Poller::new()?.into(),
            pty,
            tx,
            rx: PeekableReceiver::new(rx),
            terminal,
            event_proxy: Listener(shared.clone()),
            shared,
        })
    }

    pub(crate) fn channel(&self) -> LoopSender {
        LoopSender {
            sender: self.tx.clone(),
            poller: self.poll.clone(),
        }
    }

    /// Takes the messages waiting; `false` once told to end.
    fn drain_recv_channel(&mut self, state: &mut State) -> bool {
        while let Some(message) = self.rx.recv() {
            match message {
                Msg::Input(input) => state.write_list.push_back(input),
                Msg::Resize(window_size) => self.pty.on_resize(window_size),
                Msg::Shutdown => return false,
            }
        }
        true
    }

    /// Hands output to the screen, cut after each mark so the mark is taken
    /// with the cursor where that output left it.
    fn parse(&self, state: &mut State, terminal: &mut Term<Listener>, bytes: &[u8]) {
        let mut found: Vec<(usize, Mark)> = Vec::new();
        state.scanner.scan(bytes, &mut found);
        let mut start = 0;
        for (end, mark) in found {
            state.parser.advance(terminal, &bytes[start..end]);
            start = end;
            // Output held back for a synchronized update would leave the
            // cursor behind: let it through first.
            if state.parser.sync_bytes_count() > 0 {
                state.parser.stop_sync(terminal);
            }
            let events = self.shared.tracker().mark(terminal, mark);
            for event in events {
                (self.shared.on_event)(event);
            }
        }
        state.parser.advance(terminal, &bytes[start..]);
    }

    fn pty_read(&mut self, state: &mut State, buf: &mut [u8]) -> io::Result<()> {
        let mut unprocessed = 0;
        let mut processed = 0;

        // The next lock is the reader's.
        let _terminal_lease = Some(self.terminal.lease());
        let mut terminal = None;

        loop {
            match self.pty.reader().read(&mut buf[unprocessed..]) {
                // What the Mac says when there is nothing more to read.
                Ok(0) if unprocessed == 0 => break,
                Ok(got) => unprocessed += got,
                Err(error) => match error.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                        if unprocessed == 0 {
                            break;
                        }
                    }
                    _ => return Err(error),
                },
            }

            let terminal = match &mut terminal {
                Some(terminal) => terminal,
                None => terminal.insert(match self.terminal.try_lock_unfair() {
                    // Wait for the lock once the buffer is full.
                    None if unprocessed >= READ_BUFFER_SIZE => self.terminal.lock_unfair(),
                    None => continue,
                    Some(terminal) => terminal,
                }),
            };

            self.parse(state, terminal, &buf[..unprocessed]);

            processed += unprocessed;
            unprocessed = 0;

            if processed >= MAX_LOCKED_READ {
                break;
            }
        }

        // Keep the count of lines that went up while the lock is held, so a
        // long output never leaves the places behind.
        if let Some(terminal) = &mut terminal {
            self.shared.tracker().catch_up(terminal);
        }
        drop(terminal);

        // Draw again unless all of it waits for a synchronized update.
        if state.parser.sync_bytes_count() < processed && processed > 0 {
            self.event_proxy.send_event(Event::Wakeup);
        }

        Ok(())
    }

    fn pty_write(&mut self, state: &mut State) -> io::Result<()> {
        state.ensure_next();

        'write_many: while let Some(mut current) = state.take_current() {
            'write_one: loop {
                match self.pty.writer().write(current.remaining_bytes()) {
                    Ok(0) => {
                        state.set_current(Some(current));
                        break 'write_many;
                    }
                    Ok(written) => {
                        current.advance(written);
                        if current.finished() {
                            state.goto_next();
                            break 'write_one;
                        }
                    }
                    Err(error) => {
                        state.set_current(Some(current));
                        match error.kind() {
                            ErrorKind::Interrupted | ErrorKind::WouldBlock => break 'write_many,
                            _ => return Err(error),
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) fn spawn(mut self) -> io::Result<JoinHandle<()>> {
        std::thread::Builder::new()
            .name("PTY reader".to_owned())
            .spawn(move || {
                let mut state = State::default();
                let mut buf = vec![0u8; READ_BUFFER_SIZE];

                let poll_opts = PollMode::Level;
                let mut interest = PollingEvent::readable(0);

                // SAFETY: the pty is deregistered before the loop ends, as
                // Alacritty does.
                if let Err(error) = unsafe { self.pty.register(&self.poll, interest, poll_opts) } {
                    eprintln!("keel: the terminal could not watch its program: {error}");
                    return;
                }

                let mut events = Events::with_capacity(NonZeroUsize::new(1024).expect("not zero"));

                'event_loop: loop {
                    // Wake up when a synchronized update has waited too long.
                    let handler = state.parser.sync_timeout();
                    let timeout = handler
                        .sync_timeout()
                        .map(|timeout| timeout.saturating_duration_since(Instant::now()));

                    events.clear();
                    if let Err(error) = self.poll.wait(&mut events, timeout) {
                        match error.kind() {
                            ErrorKind::Interrupted => continue,
                            _ => {
                                eprintln!("keel: the terminal stopped watching: {error}");
                                break 'event_loop;
                            }
                        }
                    }

                    if events.is_empty() && self.rx.peek().is_none() {
                        state.parser.stop_sync(&mut *self.terminal.lock());
                        self.event_proxy.send_event(Event::Wakeup);
                        continue;
                    }

                    if !self.drain_recv_channel(&mut state) {
                        break;
                    }

                    for event in events.iter() {
                        match event.key {
                            PTY_CHILD_EVENT_TOKEN => {
                                if let Some(tty::ChildEvent::Exited(status)) =
                                    self.pty.next_child_event()
                                {
                                    if let Some(status) = status {
                                        self.event_proxy.send_event(Event::ChildExit(status));
                                    }
                                    // What the program printed last.
                                    let _ = self.pty_read(&mut state, &mut buf);
                                    self.terminal.lock().exit();
                                    self.event_proxy.send_event(Event::Wakeup);
                                    break 'event_loop;
                                }
                            }
                            PTY_READ_WRITE_TOKEN => {
                                if event.is_interrupt() {
                                    continue;
                                }
                                if event.readable
                                    && let Err(error) = self.pty_read(&mut state, &mut buf)
                                {
                                    eprintln!("keel: could not read the terminal: {error}");
                                    break 'event_loop;
                                }
                                if event.writable
                                    && let Err(error) = self.pty_write(&mut state)
                                {
                                    eprintln!("keel: could not write to the terminal: {error}");
                                    break 'event_loop;
                                }
                            }
                            _ => {}
                        }
                    }

                    let needs_write = state.needs_write();
                    if needs_write != interest.writable {
                        interest.writable = needs_write;
                        if self
                            .pty
                            .reregister(&self.poll, interest, poll_opts)
                            .is_err()
                        {
                            break 'event_loop;
                        }
                    }
                }

                let _ = self.pty.deregister(&self.poll);
            })
    }
}

/// Bytes being written, and how many are.
struct Writing {
    source: Cow<'static, [u8]>,
    written: usize,
}

impl Writing {
    fn new(source: Cow<'static, [u8]>) -> Self {
        Self { source, written: 0 }
    }

    fn advance(&mut self, written: usize) {
        self.written += written;
    }

    fn remaining_bytes(&self) -> &[u8] {
        &self.source[self.written..]
    }

    fn finished(&self) -> bool {
        self.written >= self.source.len()
    }
}

impl State {
    fn ensure_next(&mut self) {
        if self.writing.is_none() {
            self.goto_next();
        }
    }

    fn goto_next(&mut self) {
        self.writing = self.write_list.pop_front().map(Writing::new);
    }

    fn take_current(&mut self) -> Option<Writing> {
        self.writing.take()
    }

    fn needs_write(&self) -> bool {
        self.writing.is_some() || !self.write_list.is_empty()
    }

    fn set_current(&mut self, new: Option<Writing>) {
        self.writing = new;
    }
}

struct PeekableReceiver<T> {
    rx: Receiver<T>,
    peeked: Option<T>,
}

impl PeekableReceiver<Msg> {
    fn new(rx: Receiver<Msg>) -> Self {
        Self { rx, peeked: None }
    }

    fn peek(&mut self) -> Option<&Msg> {
        if self.peeked.is_none() {
            self.peeked = self.rx.try_recv().ok();
        }
        self.peeked.as_ref()
    }

    fn recv(&mut self) -> Option<Msg> {
        if self.peeked.is_some() {
            self.peeked.take()
        } else {
            match self.rx.try_recv() {
                // Every sender is gone: the terminal was dropped.
                Err(TryRecvError::Disconnected) => Some(Msg::Shutdown),
                result => result.ok(),
            }
        }
    }
}
