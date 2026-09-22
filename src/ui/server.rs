//! The HTTP transport for the jump service, and the only module in the crate
//! that imports `tiny_http` or starts a thread. Requests are answered one
//! after the other on the calling thread; the editor's stdin lines, the
//! recomputation and each page's event stream get a thread of their own.

use std::io::{BufRead, Write};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, PoisonError, mpsc};
use std::thread;

use anyhow::{Context, Result};
use tiny_http::{Method, Request, Response};

use super::service::{Command, JumpService, Page, parse_command};
use crate::render::AnalysisSwitches;

/// Binds `service` to `port`, or to an OS-assigned one without it, announces
/// the port and the switches on `out`, then serves requests until the
/// process ends. Command lines arrive on `stdin` from the editor and as the
/// body of `POST /command` from the page.
pub(crate) fn serve(
    service: &JumpService<'_>,
    port: Option<u16>,
    stdin: impl BufRead + Send,
    out: &mut (impl Write + Send),
) -> Result<()> {
    let (server, port) = Server::bind(service, port)?;
    write!(
        out,
        "{}{}",
        JumpService::ready_line(port),
        JumpService::analysis_line(service.switches())
    )
    .context("failed to write the ready and analysis lines")?;
    out.flush().context("failed to flush stdout")?;
    server.run(stdin, out)
}

/// A [`tiny_http::Server`] bound to a port, paired with the service that
/// answers its requests. Split from [`serve`] so a test can hold it on a
/// worker thread and end the request loop with [`Server::unblock`].
struct Server<'a> {
    inner: tiny_http::Server,
    service: &'a JumpService<'a>,
    /// One sender per open `/events` connection. A sender whose page has
    /// gone is dropped at the next event.
    subscribers: Mutex<Vec<mpsc::Sender<String>>>,
    /// The switches the last command asked for; the recomputation thread
    /// waits on `wanted_changed` for the next command.
    wanted: Mutex<Wanted>,
    wanted_changed: Condvar,
}

struct Wanted {
    switches: AnalysisSwitches,
    /// Counts the switch commands, so the recomputation thread sees a
    /// command that repeats the switches it already tried: after a failed
    /// run the page still shows the old state and sends the same command
    /// again.
    commands: u64,
    /// Set when the request loop has ended, so the recomputation thread
    /// returns instead of waiting for a next command.
    stopped: bool,
}

impl<'a> Server<'a> {
    fn bind(service: &'a JumpService<'a>, port: Option<u16>) -> Result<(Self, u16)> {
        let port = port.unwrap_or(0);
        let inner = tiny_http::Server::http(("127.0.0.1", port))
            .map_err(|err| anyhow::anyhow!("{err}"))
            .with_context(|| match port {
                0 => "failed to bind the jump service to a port".to_string(),
                _ => format!("failed to bind the jump service to port {port}"),
            })?;
        let port = inner
            .server_addr()
            .to_ip()
            .context("jump service bound to a non-IP address")?
            .port();
        Ok((
            Self {
                inner,
                service,
                subscribers: Mutex::new(Vec::new()),
                wanted: Mutex::new(Wanted {
                    switches: service.switches(),
                    commands: 0,
                    stopped: false,
                }),
                wanted_changed: Condvar::new(),
            },
            port,
        ))
    }

    /// Serves requests until [`Server::unblock`] ends `incoming_requests`,
    /// then ends the recomputation and the event streams; the stdin reader
    /// ends with its input. `out` is shared with the recomputation thread,
    /// which writes its lines between the request loop's.
    fn run(&self, stdin: impl BufRead + Send, out: &mut (impl Write + Send)) -> Result<()> {
        let out = Mutex::new(out);
        thread::scope(|scope| {
            scope.spawn(|| self.forward_stdin(stdin));
            scope.spawn(|| self.recompute_on_demand(&out));
            let result = self
                .inner
                .incoming_requests()
                .try_for_each(|request| self.handle(request, &out, scope));
            self.wanted().stopped = true;
            self.wanted_changed.notify_all();
            // Without a sender left, each stream's receiver ends its loop.
            self.subscribers().clear();
            result
        })
    }

    #[cfg(test)]
    fn unblock(&self) {
        self.inner.unblock();
    }

    fn subscribers(&self) -> MutexGuard<'_, Vec<mpsc::Sender<String>>> {
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn wanted(&self) -> MutexGuard<'_, Wanted> {
        self.wanted.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn broadcast(&self, event: &str) {
        self.subscribers()
            .retain(|subscriber| subscriber.send(event.to_string()).is_ok());
    }

    /// Runs each stdin line as a command. A line that parses to nothing is
    /// dropped.
    fn forward_stdin(&self, stdin: impl BufRead) {
        for line in stdin.lines() {
            let Ok(line) = line else {
                return;
            };
            if let Some(command) = parse_command(&line) {
                self.execute(command);
            }
        }
    }

    /// A focus, follow or theme command becomes an event for every open
    /// stream (a focus on a file without a node or a repeat of the editor's
    /// mode is dropped); a switch command changes what the recomputation
    /// thread works toward.
    fn execute(&self, command: Command) {
        match command {
            Command::Focus { line, file } => {
                if let Some(focus) = self.service.focus(line, &file) {
                    self.broadcast(&JumpService::focus_event(&focus));
                }
            }
            Command::Follow(state) => self.broadcast(&JumpService::follow_event(state)),
            Command::Theme(mode) => {
                if self.service.set_editor_mode(mode) {
                    self.broadcast(&JumpService::theme_event(mode));
                }
            }
            Command::Switch { switch, on } => {
                let mut wanted = self.wanted();
                wanted.switches = switch.set(wanted.switches, on);
                wanted.commands += 1;
                self.wanted_changed.notify_all();
            }
        }
    }

    /// Runs the analysis for the wanted switches after each command that
    /// asks for something other than the served page, so commands that
    /// arrive during a run cost one further run at most and the last one
    /// wins. After each run the page hears `analysis` and the editor
    /// `arc analysis`, or both hear the error; a failed run waits for the
    /// next command.
    fn recompute_on_demand(&self, out: &Mutex<&mut (impl Write + Send)>) {
        let mut seen = 0;
        loop {
            let switches = {
                let wanted = self
                    .wanted_changed
                    .wait_while(self.wanted(), |wanted| {
                        !wanted.stopped && wanted.commands == seen
                    })
                    .unwrap_or_else(PoisonError::into_inner);
                if wanted.stopped {
                    return;
                }
                seen = wanted.commands;
                wanted.switches
            };
            if switches == self.service.switches() {
                continue;
            }
            let (event, line) = match self.service.recompute(switches) {
                Ok(()) => (
                    JumpService::analysis_event(switches),
                    JumpService::analysis_line(switches),
                ),
                Err(err) => (
                    JumpService::analysis_error_event(&err),
                    JumpService::analysis_error_line(&err),
                ),
            };
            self.broadcast(&event);
            if let Err(err) = write_line(out, &line) {
                tracing::debug!("failed to report the analysis: {err}");
            }
        }
    }

    fn handle<'scope>(
        &'scope self,
        request: Request,
        out: &Mutex<&mut (impl Write + Send)>,
        scope: &'scope thread::Scope<'scope, '_>,
    ) -> Result<()> {
        let url = request.url().to_string();
        let (path, query) = split_url(&url);
        match (request.method(), path) {
            (Method::Get, "/") => {
                respond_page(request, &self.service.page(Page::Arc));
                Ok(())
            }
            (Method::Get, "/hotspots") => {
                respond_page(request, &self.service.page(Page::Hotspots));
                Ok(())
            }
            (Method::Get, "/jump") => self.handle_jump(request, query, out),
            (Method::Get, "/events") => {
                self.handle_events(request, scope);
                Ok(())
            }
            (Method::Post, "/command") => {
                self.handle_command(request);
                Ok(())
            }
            _ => {
                respond_not_found(request);
                Ok(())
            }
        }
    }

    /// Runs the body as one command line. The answer carries no state: that
    /// arrives for every page alike over the event stream.
    fn handle_command(&self, mut request: Request) {
        let mut body = String::new();
        if request.as_reader().read_to_string(&mut body).is_err() {
            respond_empty(request, 400);
            return;
        }
        let line = body.strip_suffix('\n').unwrap_or(&body);
        match parse_command(line) {
            Some(command) => {
                self.execute(command);
                respond_empty(request, 202);
            }
            None => respond_empty(request, 400),
        }
    }

    /// Hands the connection to a thread that writes events until the page
    /// closes it or the server ends.
    fn handle_events<'scope>(
        &'scope self,
        request: Request,
        scope: &'scope thread::Scope<'scope, '_>,
    ) {
        let (sender, receiver) = mpsc::channel();
        self.subscribers().push(sender);
        scope.spawn(move || stream_events(request, &receiver));
    }

    fn handle_jump(
        &self,
        request: Request,
        query: Option<&str>,
        out: &Mutex<&mut (impl Write + Send)>,
    ) -> Result<()> {
        let Some(id) = query.and_then(parse_id) else {
            respond_not_found(request);
            return Ok(());
        };
        let Some(location) = self.service.jump(id) else {
            respond_not_found(request);
            return Ok(());
        };
        write_line(out, &JumpService::jump_line(&location)).context("failed to write jump")?;
        respond_empty(request, 200);
        Ok(())
    }
}

/// Writes one line to the editor and flushes it, so the editor sees it at
/// once. Both threads that write share the lock for the write itself.
fn write_line(out: &Mutex<&mut (impl Write + Send)>, line: &str) -> std::io::Result<()> {
    let mut out = out.lock().unwrap_or_else(PoisonError::into_inner);
    out.write_all(line.as_bytes())?;
    out.flush()
}

/// Writes the response head by hand and then every event as it arrives: a
/// [`Response`] would end the connection and choose its own transfer
/// encoding, while an event stream stays open and is written as plain
/// blocks. A write error means the page is gone and ends the stream.
fn stream_events(request: Request, events: &mpsc::Receiver<String>) {
    let mut connection = request.into_writer();
    let head =
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n";
    let mut write = |bytes: &[u8]| -> std::io::Result<()> {
        connection.write_all(bytes)?;
        connection.flush()
    };
    if write(head.as_bytes()).is_err() {
        return;
    }
    for event in events {
        if write(event.as_bytes()).is_err() {
            return;
        }
    }
}

fn split_url(url: &str) -> (&str, Option<&str>) {
    match url.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (url, None),
    }
}

fn parse_id(query: &str) -> Option<usize> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("id="))
        .and_then(|value| value.parse().ok())
}

fn respond_page(request: Request, page: &str) {
    static CONTENT_TYPE: OnceLock<tiny_http::Header> = OnceLock::new();
    let content_type = CONTENT_TYPE.get_or_init(|| {
        "Content-Type: application/xhtml+xml; charset=utf-8"
            .parse()
            .expect("static content-type header is well-formed")
    });
    // tiny_http switches to chunked encoding above 32 KiB by default; the
    // page is larger, and a Content-Length body is what a raw reader (the
    // tests, an editor plugin) can take as-is.
    respond(
        request,
        Response::from_string(page)
            .with_header(content_type.clone())
            .with_chunked_threshold(usize::MAX),
    );
}

fn respond_empty(request: Request, status: u16) {
    respond(request, Response::empty(status));
}

fn respond_not_found(request: Request) {
    respond_empty(request, 404);
}

/// Answers `request`; a client that disconnected before the answer arrived
/// is logged and does not stop the request loop.
fn respond<R: std::io::Read>(request: Request, response: Response<R>) {
    if let Err(err) = request.respond(response) {
        tracing::debug!("failed to answer a jump service request: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{ItemKind, JumpTable, LayoutIR};
    use crate::render::{AnalysisSwitches, RenderConfig, render};
    use crate::ui::service::{Diagram, Pages};
    use std::io::{BufReader, Read as _};
    use std::net::TcpStream;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    /// Two crates, each contributing only its manifest target: `a` at id 0,
    /// `b` at id 1. The hotspot map gets its own, distinguishable SVG, as
    /// `run_ui` would render from the same analysis run; both pages share
    /// the table. The service never recomputes.
    fn two_entry_service() -> JumpService<'static> {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "a".into());
        ir.add_item(ItemKind::Crate, "b".into());
        let svg = render(&ir, &RenderConfig::default());

        let mut table = JumpTable::new();
        let a = table.insert(PathBuf::from("/ws/a/Cargo.toml"), 1);
        table.insert_node_files("0", [a]);
        let b = table.insert(PathBuf::from("/ws/b/Cargo.toml"), 1);
        table.insert_node_files("1", [b]);

        JumpService::new(
            Pages {
                arc: Diagram { svg },
                hotspots: Diagram {
                    svg: "<svg>const STATIC_DATA = {};</svg>".to_string(),
                },
                table,
            },
            AnalysisSwitches::default(),
            PathBuf::from("/ws"),
            None,
            Box::new(|_| anyhow::bail!("this service does not recompute")),
        )
    }

    /// Opens the event stream and returns the connection once the response
    /// head has arrived, so lines written to stdin afterwards reach it.
    fn subscribe(port: u16) -> TcpStream {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(stream, "GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let head = read_until(&mut stream, "\r\n\r\n");
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert!(head.contains("Content-Type: text/event-stream"), "{head}");
        stream
    }

    /// Sends a raw HTTP/1.1 request and returns the status code, the header
    /// block and the body.
    fn send(port: u16, method: &str, path: &str, body: &str) -> (u16, String, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let status = response
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status line in response: {response:?}"));
        let (head, body) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
        (status, head.to_string(), body.to_string())
    }

    /// Reads until `terminator` has arrived and returns everything up to
    /// and including it. Panics on the read timeout.
    fn read_until(stream: &mut TcpStream, terminator: &str) -> String {
        let mut received = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let n = stream.read(&mut byte).unwrap();
            assert_ne!(n, 0, "connection closed before {terminator:?} arrived");
            received.push(byte[0]);
            if received.ends_with(terminator.as_bytes()) {
                return String::from_utf8(received).unwrap();
            }
        }
    }

    #[test]
    fn pushes_the_stdin_commands_to_an_events_subscriber_in_order() {
        let service = two_entry_service();
        let (server, port) = Server::bind(&service, None).unwrap();
        let (reader, mut stdin) = std::io::pipe().unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(BufReader::new(reader), &mut out));

            let mut events = subscribe(port);
            writeln!(stdin, "arc focus 1 /ws/b/Cargo.toml").unwrap();
            writeln!(stdin, "arc follow off").unwrap();

            let first = read_until(&mut events, "\n\n");
            let second = read_until(&mut events, "\n\n");
            assert_eq!(
                first,
                "event: focus\ndata: {\"node\":\"1\",\"file\":\"b/Cargo.toml\",\"jumps\":[1]}\n\n"
            );
            assert_eq!(second, "event: follow\ndata: off\n\n");

            drop(stdin);
            server.unblock();
            handle.join().unwrap().unwrap();
        });

        assert_eq!(out, b"");
    }

    /// A theme line reaches an open page as an event and a page loaded
    /// afterwards through the root attribute.
    #[test]
    fn pushes_a_theme_line_and_serves_later_pages_in_that_mode() {
        let service = two_entry_service();
        let (server, port) = Server::bind(&service, None).unwrap();
        let (reader, mut stdin) = std::io::pipe().unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(BufReader::new(reader), &mut out));

            let mut events = subscribe(port);
            writeln!(stdin, "arc theme dark").unwrap();
            // A repeated mode is no change and sends nothing.
            writeln!(stdin, "arc theme dark").unwrap();
            writeln!(stdin, "arc follow on").unwrap();
            assert_eq!(
                read_until(&mut events, "\n\n"),
                "event: theme\ndata: dark\n\n"
            );
            assert_eq!(
                read_until(&mut events, "\n\n"),
                "event: follow\ndata: on\n\n"
            );

            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(
                stream,
                "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(
                response
                    .contains("<html xmlns=\"http://www.w3.org/1999/xhtml\" data-mode=\"dark\">")
            );

            drop(stdin);
            server.unblock();
            handle.join().unwrap().unwrap();
        });
    }

    #[test]
    fn drops_a_focus_line_without_a_node_and_serves_the_next_one() {
        let service = two_entry_service();
        let (server, port) = Server::bind(&service, None).unwrap();
        let (reader, mut stdin) = std::io::pipe().unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(BufReader::new(reader), &mut out));

            let mut events = subscribe(port);
            writeln!(stdin, "arc focus 1 /elsewhere/Cargo.toml").unwrap();
            writeln!(stdin, "arc focus 1 /ws/nope/Cargo.toml").unwrap();
            writeln!(stdin, "not a command").unwrap();
            writeln!(stdin, "arc follow on").unwrap();

            // The three lines before the follow line produced nothing, so
            // the follow event is the first block to arrive.
            assert_eq!(
                read_until(&mut events, "\n\n"),
                "event: follow\ndata: on\n\n"
            );

            drop(stdin);
            server.unblock();
            handle.join().unwrap().unwrap();
        });

        assert_eq!(out, b"");
    }

    #[test]
    fn serves_the_page_and_resolves_a_jump_id() {
        let service = two_entry_service();
        let (server, port) = Server::bind(&service, None).unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(std::io::empty(), &mut out));

            let send = |method: &str, path: &str| send(port, method, path, "");

            let (status, head, body) = send("GET", "/");
            assert_eq!(status, 200);
            assert!(
                head.contains("Content-Type: application/xhtml+xml"),
                "{head}"
            );
            assert!(body.contains("STATIC_DATA"));
            // Sent with Content-Length, never chunked: the page must arrive
            // byte-for-byte for a raw reader like this one.
            assert_eq!(body, service.page(Page::Arc));

            let (status, _, _) = send("GET", "/jump?id=1");
            assert_eq!(status, 200);

            let (status, _, _) = send("POST", "/");
            assert_eq!(status, 404);

            let (status, _, _) = send("GET", "/jump");
            assert_eq!(status, 404);

            let (status, _, _) = send("GET", "/jump?id=abc");
            assert_eq!(status, 404);

            let (status, _, _) = send("GET", "/jump?id=99");
            assert_eq!(status, 404);

            let (status, _, _) = send("GET", "/nope");
            assert_eq!(status, 404);

            let (status, _, _) = send("POST", "/events");
            assert_eq!(status, 404);

            server.unblock();
            handle.join().unwrap().unwrap();
        });

        assert_eq!(out, b"arc jump 1 /ws/b/Cargo.toml\n");
    }

    /// `GET /hotspots` serves the hotspot map's own page, with its own
    /// `STATIC_DATA`, next to `GET /` which keeps serving the arc diagram.
    #[test]
    fn serves_the_hotspot_map_next_to_the_arc_diagram() {
        let service = two_entry_service();
        let (server, port) = Server::bind(&service, None).unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(std::io::empty(), &mut out));

            let (status, head, body) = send(port, "GET", "/hotspots", "");
            assert_eq!(status, 200);
            assert!(
                head.contains("Content-Type: application/xhtml+xml"),
                "{head}"
            );
            assert!(body.contains("STATIC_DATA"), "{body}");
            assert_eq!(body, service.page(Page::Hotspots));
            assert_ne!(body, service.page(Page::Arc));

            server.unblock();
            handle.join().unwrap().unwrap();
        });
    }

    /// A service whose recomputation renders the switches into the SVG,
    /// counts its runs, and blocks each run until the test releases it:
    /// `started` reports a run has begun, `release` lets it finish.
    struct Recomputing {
        service: JumpService<'static>,
        runs: Arc<AtomicUsize>,
        started: mpsc::Receiver<AnalysisSwitches>,
        release: mpsc::Sender<()>,
    }

    fn recomputing_service() -> Recomputing {
        let runs = Arc::new(AtomicUsize::new(0));
        let (started_tx, started) = mpsc::channel();
        let (release, release_rx) = mpsc::channel::<()>();
        // A receiver is not `Sync`; the closure must be.
        let release_rx = Mutex::new(release_rx);
        let counter = runs.clone();
        let service = JumpService::new(
            Pages {
                arc: Diagram {
                    svg: "<svg>initial</svg>".to_string(),
                },
                hotspots: Diagram {
                    svg: "<svg>initial hotspots</svg>".to_string(),
                },
                table: JumpTable::new(),
            },
            AnalysisSwitches::default(),
            PathBuf::from("/ws"),
            None,
            Box::new(move |switches| {
                counter.fetch_add(1, Ordering::SeqCst);
                started_tx.send(switches).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
                Ok(Pages {
                    arc: Diagram {
                        svg: format!("<svg>{switches:?}</svg>"),
                    },
                    hotspots: Diagram {
                        svg: format!("<svg>hotspots {switches:?}</svg>"),
                    },
                    table: JumpTable::new(),
                })
            }),
        );
        Recomputing {
            service,
            runs,
            started,
            release,
        }
    }

    fn page_for(switches: AnalysisSwitches) -> String {
        crate::render::html_page(
            &format!("<svg>{switches:?}</svg>"),
            Some("ws"),
            crate::render::Appearance::default(),
        )
    }

    const EXTERNALS_ON: AnalysisSwitches = AnalysisSwitches {
        externals: true,
        tests: false,
    };

    #[test]
    fn a_posted_command_recomputes_and_pushes_analysis_when_the_page_is_ready() {
        let fake = recomputing_service();
        let (server, port) = Server::bind(&fake.service, None).unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(std::io::empty(), &mut out));
            let mut events = subscribe(port);

            let (status, _, _) = send(port, "POST", "/command", "arc externals on\n");
            assert_eq!(status, 202);
            assert_eq!(
                fake.started.recv_timeout(Duration::from_secs(5)).unwrap(),
                EXTERNALS_ON
            );
            // The old page is served until the run is through.
            assert_eq!(send(port, "GET", "/", "").2, fake.service.page(Page::Arc));
            assert_eq!(fake.service.switches(), AnalysisSwitches::default());

            fake.release.send(()).unwrap();
            assert_eq!(
                read_until(&mut events, "\n\n"),
                "event: analysis\ndata: externals=on tests=off\n\n"
            );
            assert_eq!(send(port, "GET", "/", "").2, page_for(EXTERNALS_ON));
            assert_eq!(fake.service.switches(), EXTERNALS_ON);

            server.unblock();
            handle.join().unwrap().unwrap();
        });

        assert_eq!(fake.runs.load(Ordering::SeqCst), 1);
        assert_eq!(out, b"arc analysis externals=on tests=off\n");
    }

    #[test]
    fn a_switch_line_on_stdin_recomputes_too() {
        let fake = recomputing_service();
        let (server, port) = Server::bind(&fake.service, None).unwrap();
        let (reader, mut stdin) = std::io::pipe().unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(BufReader::new(reader), &mut out));
            let mut events = subscribe(port);

            writeln!(stdin, "arc tests on").unwrap();
            let wanted = AnalysisSwitches {
                externals: false,
                tests: true,
            };
            assert_eq!(
                fake.started.recv_timeout(Duration::from_secs(5)).unwrap(),
                wanted
            );
            fake.release.send(()).unwrap();
            assert_eq!(
                read_until(&mut events, "\n\n"),
                "event: analysis\ndata: externals=off tests=on\n\n"
            );

            drop(stdin);
            server.unblock();
            handle.join().unwrap().unwrap();
        });

        assert_eq!(out, b"arc analysis externals=off tests=on\n");
    }

    #[test]
    fn rejects_a_body_that_is_no_command_without_a_run() {
        let fake = recomputing_service();
        let (server, port) = Server::bind(&fake.service, None).unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(std::io::empty(), &mut out));

            assert_eq!(send(port, "POST", "/command", "arc externals maybe").0, 400);
            assert_eq!(send(port, "POST", "/command", "hello\n").0, 400);
            assert_eq!(send(port, "POST", "/command", "").0, 400);
            assert_eq!(send(port, "GET", "/command", "").0, 404);
            assert_eq!(send(port, "POST", "/jump?id=1", "").0, 404);

            server.unblock();
            handle.join().unwrap().unwrap();
        });

        assert_eq!(fake.runs.load(Ordering::SeqCst), 0);
        assert_eq!(out, b"");
    }

    #[test]
    fn a_failed_run_keeps_the_page_and_reports_the_error_both_ways() {
        let service = JumpService::new(
            Pages {
                arc: Diagram {
                    svg: "<svg>initial</svg>".to_string(),
                },
                hotspots: Diagram {
                    svg: "<svg>initial hotspots</svg>".to_string(),
                },
                table: JumpTable::new(),
            },
            AnalysisSwitches::default(),
            PathBuf::from("/ws"),
            None,
            Box::new(|_| Err(anyhow::anyhow!("no such\nmanifest").context("analysis failed"))),
        );
        let (server, port) = Server::bind(&service, None).unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(std::io::empty(), &mut out));
            let mut events = subscribe(port);

            assert_eq!(send(port, "POST", "/command", "arc externals on").0, 202);
            assert_eq!(
                read_until(&mut events, "\n\n"),
                "event: analysis-error\ndata: analysis failed: no such manifest\n\n"
            );
            assert_eq!(
                send(port, "GET", "/", "").2,
                crate::render::html_page(
                    "<svg>initial</svg>",
                    Some("ws"),
                    crate::render::Appearance::default(),
                )
            );
            assert_eq!(service.switches(), AnalysisSwitches::default());

            server.unblock();
            handle.join().unwrap().unwrap();
        });

        assert_eq!(
            out,
            b"arc analysis-error analysis failed: no such manifest\n"
        );
    }

    /// The page still shows the old state after a failed run, so the same
    /// command comes again; it must run again.
    #[test]
    fn the_same_command_after_a_failed_run_runs_again() {
        let attempts = AtomicUsize::new(0);
        let service = JumpService::new(
            Pages {
                arc: Diagram {
                    svg: "<svg>initial</svg>".to_string(),
                },
                hotspots: Diagram {
                    svg: "<svg>initial hotspots</svg>".to_string(),
                },
                table: JumpTable::new(),
            },
            AnalysisSwitches::default(),
            PathBuf::from("/ws"),
            None,
            Box::new(|switches| {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    anyhow::bail!("the first run fails");
                }
                Ok(Pages {
                    arc: Diagram {
                        svg: format!("<svg>{switches:?}</svg>"),
                    },
                    hotspots: Diagram {
                        svg: format!("<svg>hotspots {switches:?}</svg>"),
                    },
                    table: JumpTable::new(),
                })
            }),
        );
        let (server, port) = Server::bind(&service, None).unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(std::io::empty(), &mut out));
            let mut events = subscribe(port);

            assert_eq!(send(port, "POST", "/command", "arc externals on").0, 202);
            assert_eq!(
                read_until(&mut events, "\n\n"),
                "event: analysis-error\ndata: the first run fails\n\n"
            );
            assert_eq!(send(port, "POST", "/command", "arc externals on").0, 202);
            assert_eq!(
                read_until(&mut events, "\n\n"),
                "event: analysis\ndata: externals=on tests=off\n\n"
            );
            assert_eq!(send(port, "GET", "/", "").2, page_for(EXTERNALS_ON));

            server.unblock();
            handle.join().unwrap().unwrap();
        });

        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    /// A command that names the state already served is nothing to run.
    #[test]
    fn a_command_for_the_served_state_runs_nothing() {
        let fake = recomputing_service();
        let (server, port) = Server::bind(&fake.service, None).unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(std::io::empty(), &mut out));
            let mut events = subscribe(port);

            assert_eq!(send(port, "POST", "/command", "arc externals off").0, 202);
            assert_eq!(send(port, "POST", "/command", "arc tests off").0, 202);
            // A follow event proves the two commands above were handled.
            assert_eq!(send(port, "POST", "/command", "arc follow off").0, 202);
            assert_eq!(
                read_until(&mut events, "\n\n"),
                "event: follow\ndata: off\n\n"
            );

            server.unblock();
            handle.join().unwrap().unwrap();
        });

        assert_eq!(fake.runs.load(Ordering::SeqCst), 0);
        assert_eq!(out, b"");
    }

    /// Two commands arriving during a run cost one further run, with the
    /// state both of them together asked for.
    #[test]
    fn commands_during_a_run_end_in_the_last_state_with_one_further_run() {
        let fake = recomputing_service();
        let (server, port) = Server::bind(&fake.service, None).unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(std::io::empty(), &mut out));
            let mut events = subscribe(port);

            assert_eq!(send(port, "POST", "/command", "arc externals on").0, 202);
            assert_eq!(
                fake.started.recv_timeout(Duration::from_secs(5)).unwrap(),
                EXTERNALS_ON
            );
            assert_eq!(send(port, "POST", "/command", "arc tests on").0, 202);
            assert_eq!(send(port, "POST", "/command", "arc externals off").0, 202);

            fake.release.send(()).unwrap();
            assert_eq!(
                read_until(&mut events, "\n\n"),
                "event: analysis\ndata: externals=on tests=off\n\n"
            );
            let last = AnalysisSwitches {
                externals: false,
                tests: true,
            };
            assert_eq!(
                fake.started.recv_timeout(Duration::from_secs(5)).unwrap(),
                last
            );
            fake.release.send(()).unwrap();
            assert_eq!(
                read_until(&mut events, "\n\n"),
                "event: analysis\ndata: externals=off tests=on\n\n"
            );
            assert_eq!(send(port, "GET", "/", "").2, page_for(last));

            server.unblock();
            handle.join().unwrap().unwrap();
        });

        assert_eq!(fake.runs.load(Ordering::SeqCst), 2);
        assert_eq!(
            out,
            b"arc analysis externals=on tests=off\narc analysis externals=off tests=on\n"
        );
    }

    #[test]
    fn binds_the_requested_port() {
        let service = two_entry_service();
        // A free port from the OS, released before the bind by name. A plain
        // listener closes on drop; a tiny_http server closes on its worker
        // thread, and the rebind would race it.
        let port = {
            let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            probe.local_addr().unwrap().port()
        };
        let (_server, bound) = Server::bind(&service, Some(port)).unwrap();
        assert_eq!(bound, port);
    }
}
