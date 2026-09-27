//! `on-air-record health`: asks the running service whether it is answering, for a container health check.
//!
//! The image has no `curl` and should not grow one for this, so the program probes itself. It speaks just
//! enough HTTP/1.1 over a std `TcpStream` to send one request and read one response: no runtime, no
//! database, nothing that could make the check heavier than the thing it checks. It reads the same
//! `OAR_HOST` and `OAR_PORT` as the service, so a port changed inside the container is followed.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};

/// Per step, so connect, write and read each get this long. A healthy service answers in milliseconds;
/// Docker's own timeout on the check is longer, so a hang here fails as a message rather than a kill.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// The health body is a few dozen bytes. Reading more than this means something else holds the port.
const MAX_RESPONSE_BYTES: u64 = 16 * 1024;

pub fn run(config: &AppConfig) -> AppResult<()> {
    let address = probe_address(config);
    let response = fetch_health(address, PROBE_TIMEOUT).map_err(|error| {
        // A read timeout surfaces as WouldBlock on Linux, which prints as "Resource temporarily
        // unavailable" in `docker inspect` and reads like a different fault entirely.
        let reason = match error.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => format!(
                "timed out after {} seconds, the service is up but not responding",
                PROBE_TIMEOUT.as_secs()
            ),
            _ => error.to_string(),
        };
        AppError::Internal(format!("no answer from {address}: {reason}"))
    })?;
    if !is_healthy(&response) {
        let status_line = response.lines().next().unwrap_or("an empty response");
        return Err(AppError::Internal(format!(
            "{address} answered, but not healthy: {status_line}"
        )));
    }
    println!("healthy on {address}");
    Ok(())
}

/// Where to knock. A service bound to every interface is reached over loopback, which always exists and
/// never leaves the machine; one bound to a single address has to be asked on that address.
pub fn probe_address(config: &AppConfig) -> SocketAddr {
    let bound = config.socket_addr();
    let ip = match bound.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    SocketAddr::new(ip, bound.port())
}

/// One `GET /api/health`, returning the raw response. `Connection: close` makes the server end the
/// response by closing, so reading to the end needs no parsing of lengths or chunks.
pub fn fetch_health(address: SocketAddr, timeout: Duration) -> std::io::Result<String> {
    let mut stream = TcpStream::connect_timeout(&address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    // No Origin header: the guard only compares Origin with Host on state changing requests, and
    // /api/health is public in every access mode, so this works with logins switched on. Formatted first
    // and sent in one write, because `write!` straight onto a socket sends each piece as its own packet.
    let request = format!(
        "GET /api/health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nUser-Agent: on-air-record-health\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;
    let mut response = Vec::new();
    stream.take(MAX_RESPONSE_BYTES).read_to_end(&mut response)?;
    Ok(String::from_utf8_lossy(&response).into_owned())
}

/// Healthy means a 200 whose body says so. The body check is what tells this service apart from anything
/// else that might have taken the port and happens to answer 200.
pub fn is_healthy(response: &str) -> bool {
    let status_ok = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        == Some("200");
    status_ok && response.contains(r#""status":"ok""#)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn config(host: &str, port: u16) -> AppConfig {
        AppConfig {
            host: host.to_string(),
            port,
            ..AppConfig::default()
        }
    }

    /// A one shot server on a free loopback port that reads the request and answers with `reply`,
    /// handing back what it was sent so the request itself can be checked.
    fn serve_once(reply: &'static str) -> (SocketAddr, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            // Up to the blank line that ends the headers, as a real server would, since a request is
            // not bound to arrive in one read.
            let mut request = Vec::new();
            let mut chunk = [0u8; 256];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
            }
            stream.write_all(reply.as_bytes()).expect("reply");
            String::from_utf8_lossy(&request).into_owned()
        });
        (address, handle)
    }

    #[test]
    fn every_interface_is_probed_over_loopback() {
        assert_eq!(
            probe_address(&config("0.0.0.0", 8080)),
            "127.0.0.1:8080".parse().expect("address")
        );
        assert_eq!(
            probe_address(&config("::", 9000)),
            "[::1]:9000".parse().expect("address")
        );
    }

    #[test]
    fn a_single_address_is_probed_where_it_is_bound() {
        assert_eq!(
            probe_address(&config("192.168.1.20", 8080)),
            "192.168.1.20:8080".parse().expect("address")
        );
        assert_eq!(
            probe_address(&config("127.0.0.1", 8099)),
            "127.0.0.1:8099".parse().expect("address")
        );
    }

    #[test]
    fn healthy_needs_both_a_200_and_the_body() {
        let ok = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"status\":\"ok\",\"version\":\"0.7.0\"}";
        assert!(is_healthy(ok));
        // Something else on the port answering 200.
        assert!(!is_healthy("HTTP/1.1 200 OK\r\n\r\n<html>hello</html>"));
        // The right body behind the wrong status.
        assert!(!is_healthy(
            "HTTP/1.1 503 Service Unavailable\r\n\r\n{\"status\":\"ok\"}"
        ));
        assert!(!is_healthy(""));
        assert!(!is_healthy("garbage"));
    }

    #[test]
    fn it_asks_for_health_and_reads_the_answer() {
        let (address, server) = serve_once(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"status\":\"ok\"}",
        );
        let response = fetch_health(address, PROBE_TIMEOUT).expect("a response");
        assert!(is_healthy(&response));

        let request = server.join().expect("server thread");
        assert!(request.starts_with("GET /api/health HTTP/1.1\r\n"));
        assert!(request.contains(&format!("Host: {address}\r\n")));
        assert!(request.contains("Connection: close\r\n"));
    }

    #[test]
    fn an_unhealthy_answer_is_an_error_naming_the_status() {
        let (address, server) = serve_once("HTTP/1.1 500 Internal Server Error\r\n\r\n");
        let error = run(&config("127.0.0.1", address.port())).expect_err("unhealthy");
        assert!(
            error.to_string().contains("500 Internal Server Error"),
            "{error}"
        );
        let _ = server.join();
    }

    #[test]
    fn a_service_that_accepts_but_never_answers_times_out() {
        // Accepted by the kernel's backlog and then left alone, like a frozen process.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let started = std::time::Instant::now();
        let error = fetch_health(address, Duration::from_millis(200)).expect_err("no answer");
        assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
            "{error:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(listener);
    }

    #[test]
    fn nobody_listening_is_an_error_not_a_hang() {
        // Bind then drop, so the port is known to be free.
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .expect("free port")
            .port();
        let error = run(&config("127.0.0.1", port)).expect_err("nothing there");
        assert!(error.to_string().starts_with("no answer from"), "{error}");
    }
}
