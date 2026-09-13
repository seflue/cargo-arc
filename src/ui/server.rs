//! The HTTP transport for the jump service. The only module in the crate
//! that imports `tiny_http`.

use std::io::Write;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use tiny_http::{Method, Request, Response};

use super::service::JumpService;

/// Binds `service` to `port`, or to an OS-assigned one without it, announces
/// the port on `out`, then serves requests until the process ends.
pub(crate) fn serve(service: &JumpService, port: Option<u16>, out: &mut impl Write) -> Result<()> {
    let (server, port) = Server::bind(service, port)?;
    write!(out, "{}", JumpService::ready_line(port)).context("failed to write the ready line")?;
    out.flush().context("failed to flush stdout")?;
    server.run(out)
}

/// A [`tiny_http::Server`] bound to a port, paired with the service that
/// answers its requests. Split from [`serve`] so a test can hold it on a
/// worker thread and end the request loop with [`Server::unblock`].
struct Server<'a> {
    inner: tiny_http::Server,
    service: &'a JumpService,
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
        Ok((Self { inner, service }, port))
    }

    /// Serves requests until [`Server::unblock`] ends `incoming_requests`.
    fn run(&self, out: &mut impl Write) -> Result<()> {
        for request in self.inner.incoming_requests() {
            self.handle(request, out)?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn unblock(&self) {
        self.inner.unblock();
    }

    fn handle(&self, request: Request, out: &mut impl Write) -> Result<()> {
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
            _ => {
                respond_not_found(request);
                Ok(())
            }
        }
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
    use std::io::Read as _;
    use std::net::TcpStream;
    use std::path::PathBuf;
    use std::thread;

    /// Two crates, each contributing only its manifest target: `a` at id 0,
    /// `b` at id 1.
    fn two_entry_service() -> JumpService {
        let mut ir = LayoutIR::new();
        ir.add_item(ItemKind::Crate, "a".into());
        ir.add_item(ItemKind::Crate, "b".into());
        let svg = render(&ir, &RenderConfig::default());

        let mut table = JumpTable::new();
        table.insert(PathBuf::from("/ws/a/Cargo.toml"), 1);
        table.insert(PathBuf::from("/ws/b/Cargo.toml"), 1);

        JumpService::new(&svg, table, PathBuf::from("/ws"))
    }

    #[test]
    fn serves_the_page_and_resolves_a_jump_id() {
        let service = two_entry_service();
        let (server, port) = Server::bind(&service, None).unwrap();
        let mut out = Vec::new();

        thread::scope(|scope| {
            let handle = scope.spawn(|| server.run(&mut out));

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
