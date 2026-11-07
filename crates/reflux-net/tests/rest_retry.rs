//! REST retry/rate-limit behaviour against a one-request-per-connection local server
//! (`RestTransport`'s retry loop and `ChunkedAttachmentUploader`'s part retries).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reflux_model::{CreateMessage, Snowflake};
use reflux_net::{Client, MessagePage};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn ok(body: &str) -> Self {
        Self { status: 200, headers: Vec::new(), body: body.into() }
    }
    fn status(status: u16) -> Self {
        Self { status, headers: Vec::new(), body: String::new() }
    }
    fn json(status: u16, body: &str) -> Self {
        Self { status, headers: vec![("Content-Type".into(), "application/json".into())], body: body.into() }
    }
    fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// Serves `replies` in order, one connection each, and records `METHOD PATH` per request.
async fn serve(replies: Vec<Reply>) -> (String, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        for reply in replies {
            let Ok((mut tcp, _)) = l.accept().await else { return };
            let Some(line) = read_request(&mut tcp).await else { continue };
            log.lock().unwrap().push(line);
            let reason = match reply.status {
                200 => "OK",
                400 => "Bad Request",
                403 => "Forbidden",
                404 => "Not Found",
                429 => "Too Many Requests",
                500 => "Internal Server Error",
                502 => "Bad Gateway",
                504 => "Gateway Timeout",
                _ => "Status",
            };
            let mut out = format!(
                "HTTP/1.1 {} {reason}\r\nConnection: close\r\nContent-Length: {}\r\n",
                reply.status,
                reply.body.len()
            );
            for (name, value) in &reply.headers {
                out.push_str(&format!("{name}: {value}\r\n"));
            }
            out.push_str("\r\n");
            out.push_str(&reply.body);
            let _ = tcp.write_all(out.as_bytes()).await;
            let _ = tcp.shutdown().await;
        }
    });
    (base, seen)
}

async fn read_request(tcp: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        let n = tcp.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let lower = head.to_ascii_lowercase();
    expect_body(tcp, &mut buf, header_end, &lower).await;
    Some(head.lines().next()?.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
}

/// Reads the whole request body so the client isn't answered mid-write (which can reset it).
async fn expect_body(tcp: &mut TcpStream, buf: &mut Vec<u8>, header_end: usize, head: &str) {
    let mut chunk = [0u8; 8192];
    if let Some(len) = head.lines().find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())) {
        while buf.len() < header_end + len {
            let Ok(n) = tcp.read(&mut chunk).await else { return };
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
    } else if head.contains("transfer-encoding: chunked") {
        while !buf[header_end..].ends_with(b"0\r\n\r\n") {
            let Ok(n) = tcp.read(&mut chunk).await else { return };
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
    }
}

fn client(base: &str) -> Client {
    Client::new(reflux_net::http_client(), base)
}

async fn send(c: &Client) -> reflux_net::Result<reflux_model::Message> {
    let msg = CreateMessage { nonce: Some("retry-nonce".into()), ..Default::default() };
    timeout(Duration::from_secs(20), c.send_message(Snowflake(42), &msg)).await.expect("send timed out")
}

#[tokio::test]
async fn sends_and_gets_retry_transient_server_errors() {
    let (base, seen) = serve(vec![
        Reply::status(502),
        Reply::ok("{}"),
        Reply::status(504),
        Reply::ok("{}"),
    ])
    .await;
    let c = client(&base);
    assert!(send(&c).await.is_ok());
    assert!(timeout(Duration::from_secs(20), c.current_user()).await.expect("GET timed out").is_ok());
    assert_eq!(*seen.lock().unwrap(), ["POST /v1/channels/42/messages", "POST /v1/channels/42/messages", "GET /v1/users/@me", "GET /v1/users/@me"]);
}

#[tokio::test]
async fn sends_without_a_nonce_are_not_retried() {
    let (base, seen) = serve(vec![Reply::status(502)]).await;
    let e = timeout(Duration::from_secs(20), client(&base).send_message(Snowflake(42), &CreateMessage::default()))
        .await
        .expect("send timed out")
        .unwrap_err();
    assert_eq!(e.code(), Some("502"));
    assert_eq!(seen.lock().unwrap().len(), 1, "a repeat could post a duplicate");
}

#[tokio::test]
async fn client_errors_are_not_retried() {
    let (base, seen) = serve(vec![Reply::json(400, r#"{"code":"BAD_REQUEST","message":"nope"}"#)]).await;
    let e = send(&client(&base)).await.unwrap_err();
    assert_eq!(e.code(), Some("BAD_REQUEST"));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn short_rate_limits_are_waited_out() {
    let (base, seen) = serve(vec![
        Reply::json(429, r#"{"code":"RATE_LIMITED","message":"slow down","retry_after":0.05}"#),
        Reply::ok("{}"),
    ])
    .await;
    let start = Instant::now();
    assert!(send(&client(&base)).await.is_ok());
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn long_rate_limits_fail_with_retry_after() {
    let (base, seen) = serve(vec![Reply::json(429, r#"{"code":"RATE_LIMITED","message":"slow down","retry_after":60}"#)]).await;
    let start = Instant::now();
    let e = send(&client(&base)).await.unwrap_err();
    assert_eq!(e.code(), Some("RATE_LIMITED"));
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(start.elapsed() < Duration::from_secs(2), "a long wait must not block the caller");
    let reflux_net::Error::Api { error, .. } = e else { panic!("not an API error") };
    assert!((error.retry_after.unwrap() - 60.0).abs() < 0.1);
}

#[tokio::test]
async fn upload_parts_retry_and_progress_is_not_double_counted() {
    let (base, seen) = serve(vec![Reply::status(500), Reply::ok("")]).await;
    let (bytes, sent) = (Arc::new(vec![7u8; 1024]), Arc::new(AtomicU64::new(0)));
    let progress = Arc::new(|_: u64| {});
    client(&base)
        .put_upload(&format!("{base}/upload/1"), bytes, 0..1024, Some("application/octet-stream"), sent.clone(), progress)
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert_eq!(sent.load(Ordering::Relaxed), 1024);
}

#[tokio::test]
async fn upload_parts_wait_out_a_rate_limit() {
    let (base, seen) = serve(vec![Reply::status(429).header("Retry-After", "0.05"), Reply::ok("")]).await;
    let (bytes, sent) = (Arc::new(vec![1u8; 256]), Arc::new(AtomicU64::new(0)));
    client(&base)
        .put_upload(&format!("{base}/upload/2"), bytes, 0..256, None, sent, Arc::new(|_: u64| {}))
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn upload_parts_do_not_retry_client_errors() {
    let (base, seen) = serve(vec![Reply::status(403)]).await;
    let e = client(&base)
        .put_upload(&format!("{base}/upload/3"), Arc::new(vec![1u8; 16]), 0..16, None, Arc::new(AtomicU64::new(0)), Arc::new(|_: u64| {}))
        .await
        .unwrap_err();
    assert!(e.to_string().contains("403"), "{e}");
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn message_pages_use_before_after_around_and_latest() {
    let (base, seen) = serve(vec![Reply::ok("[]"), Reply::ok("[]"), Reply::ok("[]"), Reply::ok("[]")]).await;
    let c = client(&base);
    c.messages(Snowflake(42), MessagePage::Before(Snowflake(99)), 50).await.unwrap();
    c.messages(Snowflake(42), MessagePage::After(Snowflake(98)), 50).await.unwrap();
    c.messages(Snowflake(42), MessagePage::Around(Snowflake(97)), 50).await.unwrap();
    c.messages(Snowflake(42), MessagePage::Latest, 50).await.unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [
            "GET /v1/channels/42/messages?limit=50&before=99",
            "GET /v1/channels/42/messages?limit=50&after=98",
            "GET /v1/channels/42/messages?limit=50&around=97",
            "GET /v1/channels/42/messages?limit=50",
        ]
    );
}

#[tokio::test]
async fn an_empty_page_is_an_empty_list() {
    let (base, _) = serve(vec![Reply::ok("[]")]).await;
    let page = client(&base).messages(Snowflake(42), MessagePage::Latest, 50).await.unwrap();
    assert!(page.is_empty());
}
