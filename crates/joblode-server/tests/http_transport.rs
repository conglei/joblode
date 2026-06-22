//! End-to-end coverage of the shipping binary's transports. Spawns the real
//! `joblode-server` process and drives it over the wire, so `main` and the
//! `serve_http` bootstrap are exercised exactly as a deployment would.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Path to the fixture parquet shared with the in-process tests in `mcp.rs`.
fn fixture_path() -> String {
    format!(
        "{}/../../testdata/fixture.parquet",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Reserves a free loopback port by binding and immediately releasing it.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

/// Stops the child server on drop so a failed assertion never leaks a process.
/// Prefers a graceful SIGINT (the server's shutdown signal) so it exits cleanly
/// and flushes its coverage profile; falls back to SIGKILL if it lingers.
struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: kill(2) with a valid pid and signal; ignoring the result is fine.
        unsafe {
            libc::kill(self.0.id() as libc::pid_t, libc::SIGINT);
        }
        for _ in 0..30 {
            match self.0.try_wait() {
                Ok(Some(_)) => return,
                _ => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn http_transport_serves_the_mcp_handshake_and_search() {
    let addr = format!("127.0.0.1:{}", free_port());
    let url = format!("http://{addr}/mcp");

    let child = Command::new(env!("CARGO_BIN_EXE_joblode-server"))
        .arg("http")
        .env("JOBLODE_PARQUET", fixture_path())
        .env("JOBLODE_HTTP_ADDR", &addr)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn joblode-server");
    let _guard = ServerGuard(child);

    // A per-request timeout so a bound-but-stalled server can't hang CI.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build reqwest client");
    let init_body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"itest","version":"0"}}}"#;

    // Poll until the server has bound and answers `initialize` (parquet load is fast).
    let mut init = None;
    for _ in 0..100 {
        match client
            .post(&url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(init_body)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                init = Some(resp);
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    let init = init.expect("server should answer initialize within 10s");

    let session = init
        .headers()
        .get("mcp-session-id")
        .expect("initialize returns a session id")
        .to_str()
        .expect("session id is ascii")
        .to_string();

    // Complete the lifecycle handshake before issuing any tool call.
    let ack = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-session-id", &session)
        .body(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .send()
        .await
        .expect("notifications/initialized");
    assert!(ack.status().is_success());

    // Call search_jobs and confirm the fixture's known total comes back over SSE.
    let search = client
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-session-id", &session)
        .body(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search_jobs","arguments":{"cities":["san francisco"]}}}"#)
        .send()
        .await
        .expect("tools/call search_jobs");
    let body = search.text().await.expect("search response body");

    // The fixture has exactly 3 San Francisco roles (see mcp.rs in-process tests).
    assert!(
        body.contains("\"total\":3"),
        "expected total of 3 in SSE body, got: {body}"
    );
}

#[test]
fn rejects_an_unknown_transport() {
    // The transport is validated before the dataset is touched (see main.rs), so
    // no JOBLODE_PARQUET is needed; assert it fails for that specific reason.
    let output = Command::new(env!("CARGO_BIN_EXE_joblode-server"))
        .arg("carrier-pigeon")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("run joblode-server");

    assert!(
        !output.status.success(),
        "unknown transport should exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown transport"),
        "expected an unknown-transport error, got: {stderr}"
    );
}

#[test]
fn refuses_to_bind_a_non_loopback_address() {
    // The server is local-only; binding 0.0.0.0 must be refused before it listens.
    let status = Command::new(env!("CARGO_BIN_EXE_joblode-server"))
        .arg("http")
        .env("JOBLODE_PARQUET", fixture_path())
        .env("JOBLODE_HTTP_ADDR", "0.0.0.0:0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run joblode-server");

    assert!(
        !status.success(),
        "non-loopback bind address should exit non-zero"
    );
}
