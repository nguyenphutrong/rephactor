use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

use serde_json::{Value, json};

struct LspProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl LspProcess {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rephactor"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn rephactor");

        let stdin = child.stdin.take().expect("child stdin");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));

        Self {
            child,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).expect("serialize json-rpc message");
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write header");
        self.stdin.write_all(&body).expect("write body");
        self.stdin.flush().expect("flush message");
    }

    fn read_response(&mut self, id: i64) -> Value {
        loop {
            let message = self.read_message();
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                return message;
            }
        }
    }

    fn read_message(&mut self) -> Value {
        let mut content_length = None;

        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read lsp header");
            assert_ne!(read, 0, "language server exited before response");

            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break;
            }

            if let Some(value) = line.strip_prefix("Content-Length: ") {
                content_length = Some(value.parse::<usize>().expect("content length"));
            }
        }

        let length = content_length.expect("content-length header");
        let mut body = vec![0; length];
        self.stdout.read_exact(&mut body).expect("read lsp body");
        serde_json::from_slice(&body).expect("parse lsp body")
    }
}

impl Drop for LspProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn lsp_returns_named_argument_code_action_for_open_document() {
    let mut server = LspProcess::start();
    let root = std::env::temp_dir().join(format!("rephactor-lsp-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("create temp root");

    let file = root.join("example.php");
    let uri = format!("file://{}", file.display());
    let text =
        "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";

    server.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": format!("file://{}", root.display()),
            "capabilities": {}
        }
    }));
    let initialize = server.read_response(1);
    assert_eq!(
        initialize["result"]["capabilities"]["codeActionProvider"]["codeActionKinds"],
        json!(["refactor.rewrite"])
    );

    server.send(json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    }));
    server.send(json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": "php",
                "version": 1,
                "text": text
            }
        }
    }));
    server.send(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 2, "character": 5 },
                "end": { "line": 2, "character": 5 }
            },
            "context": { "diagnostics": [] }
        }
    }));

    let code_action = server.read_response(2);
    let actions = code_action["result"].as_array().expect("code action array");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["title"], "Add names to arguments");
    assert_eq!(actions[0]["kind"], "refactor.rewrite");

    let changes = actions[0]["edit"]["changes"][&uri]
        .as_array()
        .expect("workspace edits for open document");
    let inserted: Vec<_> = changes
        .iter()
        .map(|edit| edit["newText"].as_str().expect("insert text"))
        .collect();
    assert_eq!(inserted, vec!["invoice: ", "notify: "]);

    server.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "shutdown"
    }));
    let shutdown = server.read_response(3);
    assert!(shutdown.get("error").is_none());
    server.send(json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    }));
}
