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
#[expect(
    clippy::too_many_lines,
    reason = "one protocol session is clearest as one sequential integration test"
)]
fn stdio_server_should_handle_documents_and_shutdown() {
    let mut server = TestServer::start();

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "capabilities": {
                "textDocument": {
                    "documentSymbol": {
                        "hierarchicalDocumentSymbolSupport": true
                    }
                }
            }
        }
    }));
    let initialized = server.receive();
    assert_eq!(
        initialized["result"]["capabilities"]["positionEncoding"],
        "utf-16"
    );
    assert_eq!(
        initialized["result"]["capabilities"]["documentSymbolProvider"],
        true
    );
    assert_eq!(initialized["result"]["capabilities"]["hoverProvider"], true);
    assert_eq!(
        initialized["result"]["capabilities"]["definitionProvider"],
        true
    );
    assert_eq!(
        initialized["result"]["capabilities"]["referencesProvider"],
        true
    );
    assert_eq!(
        initialized["result"]["capabilities"]["documentHighlightProvider"],
        true
    );
    assert_eq!(
        initialized["result"]["capabilities"]["codeLensProvider"],
        json!({ "resolveProvider": false })
    );
    assert_eq!(
        initialized["result"]["capabilities"]["renameProvider"],
        json!({ "prepareProvider": true })
    );
    assert_eq!(
        initialized["result"]["capabilities"]["semanticTokensProvider"]["full"],
        true
    );
    assert_eq!(
        initialized["result"]["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"],
        json!([
            "keyword",
            "function",
            "variable",
            "namespace",
            "string",
            "number",
            "comment",
            "type"
        ])
    );
    assert_eq!(
        initialized["result"]["capabilities"]["inlayHintProvider"],
        json!({ "resolveProvider": false })
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
                    "project { seed 1 sample_rate 48khz output stereo }\n",
                    "song \"Test\" { tempo 120bpm meter 4/4 key C major\n",
                    "  pattern melody = sequence {}\n",
                    "  arrangement { melody }\n",
                    "}\n"
                )
            }]
        }
    }));
    let cleared = server.receive();
    assert_eq!(cleared["params"]["version"], 2);
    assert_eq!(cleared["params"]["diagnostics"], json!([]));
    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/documentSymbol",
        "params": {
            "textDocument": { "uri": "file:///test.sym" }
        }
    }));
    let symbols = server.receive();
    assert_eq!(symbols["result"][1]["name"], "Test");
    assert_eq!(symbols["result"][1]["children"][0]["name"], "melody");

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "textDocument/completion",
        "params": {
            "textDocument": { "uri": "file:///test.sym" },
            "position": { "line": 5, "character": 0 }
        }
    }));
    let completion = server.receive();
    assert_eq!(completion["result"][0]["label"], "project");
    assert_eq!(completion["result"][1]["label"], "song");

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "textDocument/hover",
        "params": {
            "textDocument": { "uri": "file:///test.sym" },
            "position": { "line": 0, "character": 0 }
        }
    }));
    let hover = server.receive();
    assert_eq!(
        hover["result"]["contents"]["value"],
        "`project` — starts the project-wide settings block."
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "textDocument/definition",
        "params": {
            "textDocument": { "uri": "file:///test.sym" },
            "position": { "line": 3, "character": 17 }
        }
    }));
    let definition = server.receive();
    assert_eq!(
        definition["result"]["range"],
        json!({
            "start": { "line": 2, "character": 10 },
            "end": { "line": 2, "character": 16 }
        })
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "textDocument/references",
        "params": {
            "textDocument": { "uri": "file:///test.sym" },
            "position": { "line": 2, "character": 10 },
            "context": { "includeDeclaration": true }
        }
    }));
    let references = server.receive();
    assert_eq!(references["result"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        references["result"][0]["range"],
        json!({
            "start": { "line": 2, "character": 10 },
            "end": { "line": 2, "character": 16 }
        })
    );
    assert_eq!(
        references["result"][1]["range"],
        json!({
            "start": { "line": 3, "character": 16 },
            "end": { "line": 3, "character": 22 }
        })
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "textDocument/codeLens",
        "params": {
            "textDocument": { "uri": "file:///test.sym" }
        }
    }));
    let code_lens = server.receive();
    let melody_lens = code_lens["result"]
        .as_array()
        .expect("code lenses")
        .iter()
        .find(|lens| lens["command"]["title"] == "1 reference")
        .expect("melody should report one reference");
    assert_eq!(
        melody_lens["range"],
        json!({
            "start": { "line": 2, "character": 10 },
            "end": { "line": 2, "character": 16 }
        })
    );
    assert_eq!(melody_lens["command"]["command"], "symphra.showReferences");

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "textDocument/documentHighlight",
        "params": {
            "textDocument": { "uri": "file:///test.sym" },
            "position": { "line": 2, "character": 10 }
        }
    }));
    let highlights = server.receive();
    assert_eq!(
        highlights["result"],
        json!([
            {
                "range": {
                    "start": { "line": 2, "character": 10 },
                    "end": { "line": 2, "character": 16 }
                },
                "kind": 3
            },
            {
                "range": {
                    "start": { "line": 3, "character": 16 },
                    "end": { "line": 3, "character": 22 }
                },
                "kind": 2
            }
        ])
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "textDocument/semanticTokens/full",
        "params": {
            "textDocument": { "uri": "file:///test.sym" }
        }
    }));
    let semantic = server.receive();
    let data = semantic["result"]["data"]
        .as_array()
        .expect("semantic token data");
    assert!(
        data.len() >= 5 && data.len().is_multiple_of(5),
        "semantic tokens are groups of 5 integers, got {data:?}"
    );
    // First token should be the `project` keyword at 0:0 length 7 type keyword(0).
    assert_eq!(
        &data[..5],
        &[json!(0), json!(0), json!(7), json!(0), json!(0)]
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "textDocument/inlayHint",
        "params": {
            "textDocument": { "uri": "file:///test.sym" },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 20, "character": 0 }
            }
        }
    }));
    let inlays = server.receive();
    let inlay_labels: Vec<&str> = inlays["result"]
        .as_array()
        .expect("inlay hints")
        .iter()
        .filter_map(|hint| hint["label"].as_str())
        .collect();
    assert!(
        inlay_labels.contains(&"pattern"),
        "arrangement reference should get a pattern inlay: {inlay_labels:?}"
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "textDocument/prepareRename",
        "params": {
            "textDocument": { "uri": "file:///test.sym" },
            "position": { "line": 2, "character": 10 }
        }
    }));
    let prepared = server.receive();
    assert_eq!(
        prepared["result"],
        json!({
            "range": {
                "start": { "line": 2, "character": 10 },
                "end": { "line": 2, "character": 16 }
            },
            "placeholder": "melody"
        })
    );

    server.send(&json!({
        "jsonrpc": "2.0",
        "id": 12,
        "method": "textDocument/rename",
        "params": {
            "textDocument": { "uri": "file:///test.sym" },
            "position": { "line": 2, "character": 10 },
            "newName": "theme"
        }
    }));
    let renamed = server.receive();
    let edits = &renamed["result"]["changes"]["file:///test.sym"];
    assert_eq!(edits.as_array().map(Vec::len), Some(2));
    assert_eq!(edits[0]["newText"], "theme");
    assert_eq!(
        edits[0]["range"],
        json!({
            "start": { "line": 3, "character": 16 },
            "end": { "line": 3, "character": 22 }
        })
    );
    assert_eq!(
        edits[1]["range"],
        json!({
            "start": { "line": 2, "character": 10 },
            "end": { "line": 2, "character": 16 }
        })
    );

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
        "id": 13,
        "method": "shutdown",
        "params": null
    }));
    assert_eq!(server.receive()["id"], 13);
    server.send(&json!({
        "jsonrpc": "2.0",
        "method": "exit"
    }));
    server.wait_for_exit();
}
