use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

struct TestServer {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<String>,
}

impl TestServer {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_symphra-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("language server should start");
        let stdin = child.stdin.take().expect("stdin should be piped");
        let stdout = child.stdout.take().expect("stdout should be piped");
        let (sender, messages) = mpsc::channel();

        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut content_length = None;
                loop {
                    let mut header = String::new();
                    if reader
                        .read_line(&mut header)
                        .expect("header should be readable")
                        == 0
                    {
                        return;
                    }
                    if header == "\r\n" {
                        break;
                    }
                    if let Some(value) = header.strip_prefix("Content-Length:") {
                        content_length = Some(
                            value
                                .trim()
                                .parse::<usize>()
                                .expect("content length should be numeric"),
                        );
                    }
                }

                let mut body = vec![0; content_length.expect("content length should be present")];
                reader
                    .read_exact(&mut body)
                    .expect("message body should be readable");
                if sender
                    .send(String::from_utf8(body).expect("message body should be UTF-8"))
                    .is_err()
                {
                    return;
                }
            }
        });

        Self {
            child,
            stdin: Some(stdin),
            messages,
        }
    }

    fn send(&mut self, message: &Value) {
        let body = serde_json::to_string(message).expect("request should serialize");
        let stdin = self.stdin.as_mut().expect("server stdin should be open");
        write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len())
            .expect("request should be writable");
        stdin.flush().expect("request should flush");
    }

    fn receive(&self) -> Value {
        let body = self
            .messages
            .recv_timeout(RESPONSE_TIMEOUT)
            .expect("language server should respond before timeout");
        serde_json::from_str(&body).expect("response should be JSON")
    }

    fn wait_for_exit(&mut self) {
        self.stdin.take();
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            if self
                .child
                .try_wait()
                .expect("server status should be readable")
                .is_some()
            {
                return;
            }
            assert!(Instant::now() < deadline, "language server should exit");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn stdio_server_should_publish_diagnostics_and_shutdown() {
    let mut server = TestServer::start();

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "capabilities": {} }
    }));
    let initialized = server.receive();
    assert_eq!(
        initialized["result"]["capabilities"]["positionEncoding"],
        "utf-16"
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///test.sym",
                "languageId": "symphra",
                "version": 1,
                "text": "project { seed nope }"
            }
        }
    }));
    let published = server.receive();
    assert_eq!(published["method"], "textDocument/publishDiagnostics");
    assert_eq!(published["params"]["version"], 1);
    assert_eq!(
        published["params"]["diagnostics"][0]["message"],
        "expected an integer seed"
    );
    assert_eq!(
        published["params"]["diagnostics"][0]["range"],
        json!({
            "start": { "line": 0, "character": 15 },
            "end": { "line": 0, "character": 19 }
        })
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": "file:///test.sym",
                "version": 2
            },
            "contentChanges": [{
                "text": concat!(
                    "project { seed 1 sample_rate 48khz output stereo } ",
                    "song \"Test\" { tempo 120bpm meter 4/4 key C major ",
                    "pattern melody = sequence {} }"
                )
            }]
        }
    }));
    let cleared = server.receive();
    assert_eq!(cleared["params"]["version"], 2);
    assert_eq!(cleared["params"]["diagnostics"], json!([]));

    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": "file:///test.sym",
                "version": 3
            },
            "contentChanges": [{ "text": "project { seed nope }" }]
        }
    }));
    assert_eq!(server.receive()["params"]["version"], 3);
    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": {
            "textDocument": { "uri": "file:///test.sym" }
        }
    }));
    let closed = server.receive();
    assert_eq!(closed["params"]["diagnostics"], json!([]));
    assert!(closed["params"].get("version").is_none());

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": null
    }));
    assert_eq!(server.receive()["id"], 2);
    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "exit"
    }));
    server.wait_for_exit();
}
