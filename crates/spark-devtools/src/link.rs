//! Transport: newline-delimited JSON over a TCP stream, with I/O on background threads.

use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::marker::PhantomData;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::{EditorCommand, GameMessage};
use serde::de::DeserializeOwned;

/// Messages waiting to be written. When the peer stops reading, newer messages are dropped instead
/// of growing memory (stats / scenes are snapshots: losing one is harmless).
const OUTBOX: usize = 256;

/// Received messages waiting to be handled. When the owner stops polling, the reader thread blocks (and the TCP
/// window fills, so the peer slows down) instead of memory growing.
const INBOX: usize = 1024;

/// Longest accepted line. A scene with thousands of objects is a few hundred KB; a peer that never sends '\n'
/// is cut off here instead of growing a buffer without limit.
pub const MAX_LINE: usize = 8 << 20;

/// One end of a devtools connection: sends `Out`, receives `In`. Never blocks the caller.
pub struct Link<Out, In> {
    outbox: SyncSender<Out>,
    inbox: Receiver<In>,
    alive: Arc<AtomicBool>,
    peer: SocketAddr,
    _types: PhantomData<fn(Out) -> In>,
}

impl<Out, In> Link<Out, In>
where
    Out: Serialize + Send + 'static,
    In: DeserializeOwned + Send + 'static,
{
    /// Connects to a listening [`Server`] (the game side).
    pub fn connect(addr: &str, timeout: Duration) -> io::Result<Self> {
        // Only the local machine: a non-loopback address (or a name that resolves to one) is refused.
        let target = addr
            .to_socket_addrs()?
            .find(|a| a.ip().is_loopback())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("devtools address '{addr}' is not a loopback address")))?;
        Self::from_stream(TcpStream::connect_timeout(&target, timeout)?)
    }

    /// Wraps an accepted / connected stream and starts the reader and writer threads.
    pub fn from_stream(stream: TcpStream) -> io::Result<Self> {
        stream.set_nonblocking(false)?;
        let _ = stream.set_nodelay(true);
        let peer = stream.peer_addr()?;
        let read_half = stream.try_clone()?;
        let alive = Arc::new(AtomicBool::new(true));
        let (outbox, out_rx) = mpsc::sync_channel::<Out>(OUTBOX);
        let (in_tx, inbox) = mpsc::sync_channel::<In>(INBOX);

        let writer_alive = alive.clone();
        std::thread::Builder::new().name("spark-devtools-write".into()).spawn(move || {
            let mut w = BufWriter::new(stream);
            while let Ok(first) = out_rx.recv() {
                // Write everything queued, then flush once.
                let mut next = Some(first);
                while let Some(msg) = next.take() {
                    if write_line(&mut w, &msg).is_err() {
                        writer_alive.store(false, Ordering::SeqCst);
                        return;
                    }
                    next = out_rx.try_recv().ok();
                }
                if w.flush().is_err() {
                    writer_alive.store(false, Ordering::SeqCst);
                    return;
                }
            }
            // Our side was dropped: tell the peer we are done.
            let _ = w.flush();
            let _ = w.get_ref().shutdown(std::net::Shutdown::Write);
        })?;

        let reader_alive = alive.clone();
        std::thread::Builder::new().name("spark-devtools-read".into()).spawn(move || {
            let mut reader = BufReader::new(read_half);
            while let Some(line) = read_line_limited(&mut reader, MAX_LINE) {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<In>(&line) {
                    Ok(msg) => {
                        if in_tx.send(msg).is_err() {
                            break;
                        }
                    }
                    // A newer peer may send messages this version does not know.
                    Err(e) => log::debug!("devtools: skipped a message ({e})"),
                }
            }
            reader_alive.store(false, Ordering::SeqCst);
        })?;

        Ok(Self { outbox, inbox, alive, peer, _types: PhantomData })
    }

    /// Queues a message. `false` when the link is closed or the outbox is full (message dropped).
    pub fn send(&self, msg: Out) -> bool {
        if !self.is_alive() {
            return false;
        }
        match self.outbox.try_send(msg) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => false,
            Err(TrySendError::Disconnected(_)) => {
                self.alive.store(false, Ordering::SeqCst);
                false
            }
        }
    }

    /// Next received message, if any.
    pub fn try_recv(&self) -> Option<In> {
        self.inbox.try_recv().ok()
    }

    /// All messages received so far.
    pub fn drain(&self) -> impl Iterator<Item = In> + '_ {
        self.inbox.try_iter()
    }

    /// False once the peer closed the connection or an I/O error happened.
    /// Messages received before that can still be drained.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn peer(&self) -> SocketAddr {
        self.peer
    }
}

/// One line (without the newline), at most `max` bytes. `None` at end of stream; an `Err` for I/O errors,
/// invalid UTF-8 and lines that are too long (the connection is dropped then).
fn read_line_limited(r: &mut impl BufRead, max: usize) -> Option<io::Result<String>> {
    let mut buf = Vec::new();
    match r.take(max as u64 + 1).read_until(b'\n', &mut buf) {
        Ok(0) => None,
        Ok(_) => {
            if buf.last() == Some(&b'\n') {
                buf.pop();
            } else if buf.len() > max {
                return Some(Err(io::Error::new(io::ErrorKind::InvalidData, format!("devtools line longer than {max} bytes"))));
            }
            Some(String::from_utf8(buf).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "devtools line is not UTF-8")))
        }
        Err(e) => Some(Err(e)),
    }
}

fn write_line<T: Serialize>(w: &mut impl Write, msg: &T) -> io::Result<()> {
    serde_json::to_writer(&mut *w, msg).map_err(io::Error::other)?;
    w.write_all(b"\n")
}

/// How many connections with a wrong / missing token or a bad peer the server tolerates before it gives up.
pub const MAX_BAD_ATTEMPTS: u32 = 8;

/// A new connection must send `Hello` within this long.
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);

/// The game link as the editor sees it.
pub type GameLink = Link<EditorCommand, GameMessage>;

/// What the game said about itself in its (authenticated) `Hello`.
#[derive(Clone, Debug, PartialEq)]
pub struct GameHello {
    pub protocol: u32,
    pub engine: String,
    pub pid: u32,
    pub gpu: String,
}

/// Result of [`Server::poll_game`].
pub enum ServerEvent {
    /// A game proved it knows the token. The server accepts nothing more after this.
    Connected(GameLink, GameHello),
    /// A connection was dropped (wrong / missing token, not loopback, silent, ...). The text is for the console.
    Rejected(String),
    /// Too many bad attempts: the server stopped listening.
    GaveUp(String),
}

struct Pending {
    link: GameLink,
    since: Instant,
}

/// A random 128-bit token as 32 hex digits. Built from the OS-seeded `RandomState` plus time, pid and a counter
/// (no extra dependency); it only has to be unguessable by other local processes for the lifetime of one run.
pub fn generate_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let local = 0u8;
    let mut out = String::with_capacity(32);
    for lane in 0..2u64 {
        // A fresh RandomState has fresh random keys.
        let mut h = RandomState::new().build_hasher();
        h.write_u128(now);
        h.write_u32(std::process::id());
        h.write_u64(n);
        h.write_u64(lane);
        h.write_usize(&local as *const u8 as usize);
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// Constant-time equality (no early exit on the first differing byte).
pub fn tokens_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= (*a.get(i).unwrap_or(&0) ^ *b.get(i).unwrap_or(&0)) as usize;
    }
    diff == 0
}

/// The editor side: listens on a free localhost port, games connect to it.
///
/// Authentication (protocol 3): the editor generates a token per run and hands it to the game through the
/// `SPARK_DEVTOOLS_TOKEN` environment variable; the game repeats it in `Hello`. [`Server::poll_game`] only
/// returns a link after a loopback peer sent a `Hello` with the right token.
pub struct Server {
    listener: TcpListener,
    addr: SocketAddr,
    token: Option<String>,
    pending: Vec<Pending>,
    bad: u32,
    done: bool,
}

impl Server {
    /// Binds `127.0.0.1` on a free port (never reachable from other machines), without authentication
    /// ([`Server::accept`] only; used by tests and tools).
    pub fn bind_local() -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        Ok(Self { listener, addr, token: None, pending: Vec::new(), bad: 0, done: false })
    }

    /// Like [`Server::bind_local`], but [`Server::poll_game`] requires `token` in the game's `Hello`.
    pub fn bind_local_with_token(token: impl Into<String>) -> io::Result<Self> {
        let mut s = Self::bind_local()?;
        s.token = Some(token.into());
        Ok(s)
    }

    /// Binds with a freshly generated token (see [`Server::token`]).
    pub fn bind_local_authenticated() -> io::Result<Self> {
        Self::bind_local_with_token(generate_token())
    }

    /// The token the game must present (`None` for [`Server::bind_local`]). Pass it via [`crate::TOKEN_ENV`].
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// Non-blocking, authenticated accept: call every frame. Connections that fail the checks are dropped and
    /// reported; after [`MAX_BAD_ATTEMPTS`] of them the server gives up. A server without a token never
    /// authenticates (everything is rejected).
    pub fn poll_game(&mut self) -> Vec<ServerEvent> {
        let mut events = Vec::new();
        if self.done {
            return events;
        }
        while !self.done {
            match self.listener.accept() {
                Ok((stream, peer)) => {
                    if !peer.ip().is_loopback() {
                        events.push(self.reject(format!("connection from non-loopback address {peer}")));
                    } else {
                        match GameLink::from_stream(stream) {
                            Ok(link) => self.pending.push(Pending { link, since: Instant::now() }),
                            Err(e) => events.push(self.reject(format!("cannot use connection from {peer}: {e}"))),
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    events.push(self.reject(format!("accept failed: {e}")));
                    break;
                }
            }
        }
        let token = self.token.clone();
        let mut still = Vec::new();
        let mut verdicts = Vec::new();
        for p in std::mem::take(&mut self.pending) {
            match p.link.try_recv() {
                Some(GameMessage::Hello { protocol, engine, pid, gpu, token: sent }) => match token.as_deref() {
                    Some(expected) if !sent.is_empty() && tokens_match(expected, &sent) => {
                        verdicts.push(Ok((p.link, GameHello { protocol, engine, pid, gpu })));
                    }
                    _ => verdicts.push(Err(format!(
                        "dropped a connection from {} ({})",
                        p.link.peer(),
                        if sent.is_empty() { "no token" } else { "wrong token" }
                    ))),
                },
                Some(_) => verdicts.push(Err(format!("dropped a connection from {} (first message was not Hello)", p.link.peer()))),
                None if !p.link.is_alive() => verdicts.push(Err(format!("connection from {} closed before Hello", p.link.peer()))),
                None if p.since.elapsed() > HELLO_TIMEOUT => {
                    verdicts.push(Err(format!("connection from {} sent no Hello in time", p.link.peer())))
                }
                None => still.push(p),
            }
        }
        self.pending = still;
        for v in verdicts {
            match v {
                Ok((link, hello)) => {
                    // First valid game wins; the rest (pending or later) are dropped.
                    self.done = true;
                    self.pending.clear();
                    events.push(ServerEvent::Connected(link, hello));
                    return events;
                }
                Err(msg) => events.push(self.reject(msg)),
            }
        }
        // `reject` marks the server finished when the limit is hit; report that once, last.
        if self.done {
            events.push(ServerEvent::GaveUp(format!("stopped listening after {MAX_BAD_ATTEMPTS} rejected connections")));
        }
        events
    }

    fn reject(&mut self, msg: String) -> ServerEvent {
        self.bad += 1;
        if self.bad >= MAX_BAD_ATTEMPTS {
            self.done = true;
            self.pending.clear();
        }
        ServerEvent::Rejected(msg)
    }

    /// The address to pass to the game: `--devtools <addr>`.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Non-blocking: a game that connected since the last call.
    pub fn accept<Out, In>(&self) -> Option<io::Result<Link<Out, In>>>
    where
        Out: Serialize + Send + 'static,
        In: DeserializeOwned + Send + 'static,
    {
        match self.listener.accept() {
            Ok((stream, _)) => Some(Link::from_stream(stream)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(e) => Some(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditorCommand, GameMessage};
    use std::time::Instant;

    fn wait<T>(mut f: impl FnMut() -> Option<T>) -> T {
        let start = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn long_and_binary_lines_are_cut_off() {
        let mut ok = io::Cursor::new(b"{\"a\":1}\nnext\n".to_vec());
        assert_eq!(read_line_limited(&mut ok, 64).unwrap().unwrap(), "{\"a\":1}");
        assert_eq!(read_line_limited(&mut ok, 64).unwrap().unwrap(), "next");
        assert!(read_line_limited(&mut ok, 64).is_none());
        // no newline within the limit
        let mut long = io::Cursor::new(vec![b'x'; 1000]);
        assert!(read_line_limited(&mut long, 100).unwrap().is_err());
        // exactly at the limit with a newline is fine
        let mut edge = io::Cursor::new([vec![b'y'; 100], vec![b'\n']].concat());
        assert_eq!(read_line_limited(&mut edge, 100).unwrap().unwrap().len(), 100);
        let mut bad = io::Cursor::new(vec![0xff, 0xfe, b'\n']);
        assert!(read_line_limited(&mut bad, 100).unwrap().is_err());
    }

    fn hello(token: &str) -> GameMessage {
        GameMessage::Hello { protocol: crate::PROTOCOL_VERSION, engine: "t".into(), pid: 1, gpu: "g".into(), token: token.into() }
    }

    /// Connects a game that sends `first` (if any) and polls the server until `done` says so.
    fn poll_until(server: &mut Server, mut done: impl FnMut(&[ServerEvent]) -> bool) -> Vec<ServerEvent> {
        let start = Instant::now();
        let mut all = Vec::new();
        loop {
            all.extend(server.poll_game());
            if done(&all) {
                return all;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn connect(server: &Server) -> GameLink2 {
        Link::connect(&server.addr().to_string(), Duration::from_secs(2)).unwrap()
    }
    type GameLink2 = Link<GameMessage, EditorCommand>;

    #[test]
    fn correct_token_is_accepted() {
        let mut server = Server::bind_local_with_token("s3cret").unwrap();
        let game = connect(&server);
        assert!(game.send(hello("s3cret")));
        let events = poll_until(&mut server, |e| e.iter().any(|x| matches!(x, ServerEvent::Connected(..))));
        let Some(ServerEvent::Connected(editor, h)) = events.into_iter().find(|e| matches!(e, ServerEvent::Connected(..))) else {
            unreachable!()
        };
        assert_eq!(h.pid, 1);
        assert!(editor.send(EditorCommand::Reload));
        assert_eq!(wait(|| game.try_recv()), EditorCommand::Reload);
    }

    #[test]
    fn wrong_and_missing_token_are_rejected_and_the_right_one_still_gets_in() {
        let mut server = Server::bind_local_with_token("s3cret").unwrap();
        let wrong = connect(&server);
        wrong.send(hello("nope"));
        let missing = connect(&server);
        missing.send(hello(""));
        let not_hello = connect(&server);
        not_hello.send(GameMessage::Bye);
        let events = poll_until(&mut server, |e| e.iter().filter(|x| matches!(x, ServerEvent::Rejected(_))).count() >= 3);
        assert!(!events.iter().any(|e| matches!(e, ServerEvent::Connected(..))));
        let good = connect(&server);
        good.send(hello("s3cret"));
        let events = poll_until(&mut server, |e| e.iter().any(|x| matches!(x, ServerEvent::Connected(..))));
        assert!(events.iter().any(|e| matches!(e, ServerEvent::Connected(..))));
    }

    #[test]
    fn server_gives_up_after_too_many_bad_attempts() {
        let mut server = Server::bind_local_with_token("s3cret").unwrap();
        let mut keep = Vec::new();
        for _ in 0..MAX_BAD_ATTEMPTS {
            let g = connect(&server);
            g.send(hello("bad"));
            keep.push(g);
        }
        let events = poll_until(&mut server, |e| e.iter().any(|x| matches!(x, ServerEvent::GaveUp(_))));
        assert!(events.iter().any(|e| matches!(e, ServerEvent::GaveUp(_))));
        // the right token is too late now
        let good = connect(&server);
        good.send(hello("s3cret"));
        std::thread::sleep(Duration::from_millis(100));
        assert!(server.poll_game().is_empty());
    }

    #[test]
    fn server_without_token_never_authenticates() {
        let mut server = Server::bind_local().unwrap();
        let game = connect(&server);
        game.send(hello(""));
        let events = poll_until(&mut server, |e| !e.is_empty());
        assert!(matches!(events[0], ServerEvent::Rejected(_)));
    }

    #[test]
    fn token_helpers() {
        let (a, b) = (generate_token(), generate_token());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert!(tokens_match(&a, &a));
        assert!(!tokens_match(&a, &b));
        assert!(!tokens_match("abc", "abcd"));
        assert!(!tokens_match("", "x"));
    }

    #[test]
    fn editor_and_game_talk() {
        let server = Server::bind_local().unwrap();
        let game: Link<GameMessage, EditorCommand> = Link::connect(&server.addr().to_string(), Duration::from_secs(2)).unwrap();
        let editor: Link<EditorCommand, GameMessage> = wait(|| server.accept()).unwrap();

        assert!(game.send(GameMessage::Paused { paused: true }));
        assert_eq!(wait(|| editor.try_recv()), GameMessage::Paused { paused: true });
        assert!(editor.send(EditorCommand::Reload));
        assert_eq!(wait(|| game.try_recv()), EditorCommand::Reload);

        drop(game);
        wait(|| (!editor.is_alive()).then_some(()));
        assert!(!editor.send(EditorCommand::Quit));
    }
}
