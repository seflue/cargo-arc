//! The HTTP transport for the jump service, and the only module in the crate
//! that imports `tiny_http` or starts a thread. Requests are answered one
//! after the other on the calling thread; the editor's stdin lines and each
//! page's event stream get a thread of their own.

use std::io::{BufRead, Write};
use std::sync::{Mutex, OnceLock, PoisonError, mpsc};
use std::thread;

use anyhow::{Context, Result};
use tiny_http::{Method, Request, Response};

use super::service::{JumpService, StdinCommand, parse_stdin};

/// Binds `service` to `port`, or to an OS-assigned one without it, announces
/// the port on `out`, then serves requests until the process ends. Lines on
/// `stdin` become events for every page that holds `/events` open.
pub(crate) fn serve(
    service: &JumpService,
    port: Option<u16>,
    stdin: impl BufRead + Send,
    out: &mut impl Write,
) -> Result<()> {
    let (server, port) = Server::bind(service, port)?;
    write!(out, "{}", JumpService::ready_line(port)).context("failed to write the ready line")?;
    out.flush().context("failed to flush stdout")?;
    server.run(stdin, out)
}

/// A [`tiny_http::Server`] bound to a port, paired with the service that
/// answers its requests. Split from [`serve`] so a test can hold it on a
/// worker thread and end the request loop with [`Server::unblock`].
struct Server<'a> {
    inner: tiny_http::Server,
    service: &'a JumpService,
    /// One sender per open `/events` connection. A sender whose page has
    /// gone is dropped at the next event.
    subscribers: Mutex<Vec<mpsc::Sender<String>>>,
}

impl<'a> Server<'a> {
    fn bind(service: &'a JumpService, port: Option<u16>) -> Result<(Self, u16)> {
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
            },
            port,
        ))
    }

    /// Serves requests until [`Server::unblock`] ends `incoming_requests`,
    /// then ends the event streams; the stdin reader ends with its input.
    fn run(&self, stdin: impl BufRead + Send, out: &mut impl Write) -> Result<()> {
        thread::scope(|scope| {
            scope.spawn(|| self.forward_stdin(stdin));
            let result = self
                .inner
                .incoming_requests()
                .try_for_each(|request| self.handle(request, out, scope));
            // Without a sender left, each stream's receiver ends its loop.
            self.subscribers().clear();
            result
        })
    }

    #[cfg(test)]
    fn unblock(&self) {
        self.inner.unblock();
    }

    fn subscribers(&self) -> std::sync::MutexGuard<'_, Vec<mpsc::Sender<String>>> {
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Turns each stdin line into an event for every open stream. A line
    /// that parses to nothing, or names a file without a node, is dropped.
    fn forward_stdin(&self, stdin: impl BufRead) {
        for line in stdin.lines() {
            let Ok(line) = line else {
                return;
            };
            let event = match parse_stdin(&line) {
                Some(StdinCommand::Focus { line, file }) => {
                    let Some(focus) = self.service.focus(line, &file) else {
                        continue;
                    };
                    JumpService::focus_event(&focus)
                }
                Some(StdinCommand::Follow(state)) => JumpService::follow_event(state),
                None => continue,
            };
            self.subscribers()
                .retain(|subscriber| subscriber.send(event.clone()).is_ok());
        }
    }

    fn handle<'scope>(
        &'scope self,
        request: Request,
        out: &mut impl Write,
        scope: &'scope thread::Scope<'scope, '_>,
    ) -> Result<()> {
        if *request.method() != Method::Get {
            respond_not_found(request);
            return Ok(());
        }
        let url = request.url().to_string();
        let (path, query) = split_url(&url);
        match path {
            "/" => {
                respond_page(request, self.service.page());
                Ok(())
            }
            "/jump" => self.handle_jump(request, query, out),
            "/events" => {
                self.handle_events(request, scope);
                Ok(())
            }
            _ => {
                respond_not_found(request);
                Ok(())
            }
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
        out: &mut impl Write,
    ) -> Result<()> {
        let Some(id) = query.and_then(parse_id) else {
            respond_not_found(request);
            return Ok(());
        };
        let Some(location) = self.service.jump(id) else {
            respond_not_found(request);
            return Ok(());
        };
        write!(out, "{}", JumpService::jump_line(&location)).context("failed to write jump")?;
        out.flush().context("failed to flush stdout")?;
        respond_empty(request, 200);
        Ok(())
    }
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
    use crate::render::{RenderConfig, render};
    use std::io::{BufReader, Read as _};
    use std::net::TcpStream;
    use std::path::PathBuf;
    use std::thread;
    use std::time::Duration;

    /// Two crates, each contributing only its manifest target: `a` at id 0,
    /// `b` at id 1.
    fn two_entry_service() -> JumpService {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "a".into());
        ir.add_item(ItemKind::Crate, "b".into());
        let svg = render(&ir, &RenderConfig::default());

        let mut table = JumpTable::new();
        let a = table.insert(PathBuf::from("/ws/a/Cargo.toml"), 1);
        table.insert_node_files("0", [a]);
        let b = table.insert(PathBuf::from("/ws/b/Cargo.toml"), 1);
        table.insert_node_files("1", [b]);

        JumpService::new(&svg, table, PathBuf::from("/ws"))
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
                "event: focus\ndata: {\"node\":\"1\",\"jumps\":[1]}\n\n"
            );
            assert_eq!(second, "event: follow\ndata: off\n\n");

            drop(stdin);
            server.unblock();
            handle.join().unwrap().unwrap();
        });

        assert_eq!(out, b"");
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

            // Sends a raw HTTP/1.1 request and returns the status code, the
            // header block and the body.
            let send = |method: &str, path: &str| -> (u16, String, String) {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(
                    stream,
                    "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
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
            };

            let (status, head, body) = send("GET", "/");
            assert_eq!(status, 200);
            assert!(
                head.contains("Content-Type: application/xhtml+xml"),
                "{head}"
            );
            assert!(body.contains("STATIC_DATA"));
            // Sent with Content-Length, never chunked: the page must arrive
            // byte-for-byte for a raw reader like this one.
            assert_eq!(body, service.page());

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
