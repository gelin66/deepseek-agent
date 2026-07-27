//! Controlled DeepSeek loopback failures shared by real CLI and PTY tests.
//!
//! This is deliberately a wire-level fault source only. It does not classify
//! failures, decide retries, maintain a second request ledger, or project UI
//! state; those remain production responsibilities.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultScript {
    HeaderTimeoutThenSuccess,
    RateLimitThenSuccess,
    ResetResetThenSuccess,
    ServiceUnavailableExhausted,
    Unauthorized,
    PartialContentClose,
}

pub struct ModelFaultProxy {
    address: SocketAddr,
    attempts: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl ModelFaultProxy {
    pub fn start(script: FaultScript) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind model fault proxy");
        let address = listener.local_addr().expect("model fault proxy address");
        let attempts = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_attempts = Arc::clone(&attempts);
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => serve_connection(stream, script, &worker_attempts),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) if worker_stop.load(Ordering::Acquire) => break,
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            attempts,
            stop,
            worker: Some(worker),
        }
    }

    pub fn uri(&self) -> String {
        format!("http://{}", self.address)
    }

    pub fn attempts(&self) -> usize {
        self.attempts.load(Ordering::Acquire)
    }
}

impl Drop for ModelFaultProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(100));
        if let Some(worker) = self.worker.take() {
            worker.join().expect("join model fault proxy");
        }
    }
}

fn serve_connection(mut stream: TcpStream, script: FaultScript, attempts: &AtomicUsize) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let reader_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return,
    };
    let mut reader = BufReader::new(reader_stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }
    let mut content_length = 0_usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || matches!(header.as_str(), "\r\n" | "\n" | "") {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            let Ok(parsed) = value.trim().parse::<usize>() else {
                return;
            };
            content_length = parsed;
        }
    }
    if content_length > 0 {
        let mut body = vec![0_u8; content_length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
    }
    let path = request_line
        .split_ascii_whitespace()
        .nth(1)
        .unwrap_or_default();
    if !path.ends_with("/chat/completions") {
        write_response(&mut stream, "404 Not Found", &[], b"not found");
        return;
    }

    let attempt = attempts.fetch_add(1, Ordering::AcqRel) + 1;
    match script {
        FaultScript::HeaderTimeoutThenSuccess if attempt == 1 => {
            // The production exec response-header deadline is 45 seconds.
            // Hold the accepted request beyond it, then accept the retry. The
            // extra margin keeps the fixture deterministic under scheduler
            // load while leaving the second request well inside its deadline.
            std::thread::sleep(Duration::from_secs(48));
        }
        FaultScript::RateLimitThenSuccess if attempt == 1 => write_response(
            &mut stream,
            "429 Too Many Requests",
            &[("Retry-After", "2")],
            b"rate limited by fixture",
        ),
        FaultScript::ResetResetThenSuccess if attempt <= 2 => {
            // Closing after consuming the complete request produces a
            // response-before-headers transport failure without corrupting the
            // request-body write on macOS.
        }
        FaultScript::ServiceUnavailableExhausted => write_response(
            &mut stream,
            "503 Service Unavailable",
            &[],
            b"temporarily unavailable",
        ),
        FaultScript::Unauthorized => write_response(
            &mut stream,
            "401 Unauthorized",
            &[],
            b"invalid fixture credential",
        ),
        FaultScript::PartialContentClose => {
            write_sse(&mut stream, &partial_content_sse());
        }
        FaultScript::HeaderTimeoutThenSuccess
        | FaultScript::RateLimitThenSuccess
        | FaultScript::ResetResetThenSuccess => {
            write_sse(&mut stream, &complete_sse("M34_RECOVERED"));
        }
    }
}

fn write_response(stream: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        let _ = write!(stream, "{name}: {value}\r\n");
    }
    let _ = stream
        .write_all(b"\r\n")
        .and_then(|()| stream.write_all(body))
        .and_then(|()| stream.flush());
}

fn write_sse(stream: &mut TcpStream, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .and_then(|()| stream.write_all(body.as_bytes()))
    .and_then(|()| stream.flush());
}

fn complete_sse(content: &str) -> String {
    [
        format!(
            "data: {}\n\n",
            serde_json::json!({
                "id": "chatcmpl-m34-recovered",
                "object": "chat.completion.chunk",
                "model": "deepseek-v4-flash",
                "choices": [{
                    "index": 0,
                    "delta": { "content": content },
                    "finish_reason": null
                }]
            })
        ),
        format!(
            "data: {}\n\n",
            serde_json::json!({
                "id": "chatcmpl-m34-recovered",
                "object": "chat.completion.chunk",
                "model": "deepseek-v4-flash",
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 2,
                    "total_tokens": 12,
                    "prompt_cache_hit_tokens": 0,
                    "prompt_cache_miss_tokens": 10
                }
            })
        ),
        "data: [DONE]\n\n".to_owned(),
    ]
    .join("")
}

fn partial_content_sse() -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({
            "id": "chatcmpl-m34-partial",
            "object": "chat.completion.chunk",
            "model": "deepseek-v4-flash",
            "choices": [{
                "index": 0,
                "delta": { "content": "M34_PARTIAL" },
                "finish_reason": null
            }]
        })
    )
}
