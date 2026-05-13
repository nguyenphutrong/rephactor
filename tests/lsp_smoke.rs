use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tower_lsp::lsp_types::Url;

struct LspProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    next_id: i64,
}

impl LspProcess {
    fn start(root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rephactor"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn rephactor");

        let stdin = child.stdin.take().expect("child stdin");
        let stdout = BufReader::new(child.stdout.take().expect("child stdout"));
        let mut server = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        let initialize = server.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": file_uri(root),
                "capabilities": {}
            }),
        );
        assert_eq!(
            initialize["result"]["capabilities"]["codeActionProvider"]["codeActionKinds"],
            json!(["refactor.rewrite"])
        );
        assert_eq!(
            initialize["result"]["capabilities"]["signatureHelpProvider"]["triggerCharacters"],
            json!(["(", ",", ":"])
        );
        assert_eq!(
            initialize["result"]["capabilities"]["definitionProvider"],
            json!(true)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["declarationProvider"],
            json!(true)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["typeDefinitionProvider"],
            json!(true)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["implementationProvider"],
            json!(true)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["renameProvider"],
            json!(true)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["codeLensProvider"]["resolveProvider"],
            json!(false)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["hoverProvider"],
            json!(true)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["completionProvider"]["triggerCharacters"],
            json!(["\\", ":", ">", "$"])
        );
        assert_eq!(
            initialize["result"]["capabilities"]["documentSymbolProvider"],
            json!(true)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["workspaceSymbolProvider"],
            json!(true)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["referencesProvider"],
            json!(true)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["documentHighlightProvider"],
            json!(true)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["foldingRangeProvider"],
            json!(true)
        );
        assert!(initialize["result"]["capabilities"]["inlayHintProvider"].is_object());
        assert!(initialize["result"]["capabilities"]["documentLinkProvider"].is_object());
        assert_eq!(
            initialize["result"]["capabilities"]["documentFormattingProvider"],
            json!(true)
        );
        assert_eq!(
            initialize["result"]["capabilities"]["documentRangeFormattingProvider"],
            json!(true)
        );
        assert!(initialize["result"]["capabilities"]["inlineValueProvider"].is_object());
        assert_eq!(
            initialize["result"]["capabilities"]["selectionRangeProvider"],
            json!(true)
        );
        server.notify("initialized", json!({}));
        server
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }));
        self.read_response(id)
    }

    fn shutdown(&mut self) {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "shutdown"
        }));
        let _ = self.read_response(id);
        self.notify("exit", Value::Null);
    }

    fn open_php(&mut self, path: &Path, text: &str) -> String {
        let uri = file_uri(path);
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "php",
                    "version": 1,
                    "text": text
                }
            }),
        );
        uri
    }

    fn code_actions(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let response = self.request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": line, "character": character },
                    "end": { "line": line, "character": character }
                },
                "context": { "diagnostics": [] }
            }),
        );
        response["result"]
            .as_array()
            .expect("code action array")
            .clone()
    }

    fn code_lens(&mut self, uri: &str) -> Vec<Value> {
        let response = self.request(
            "textDocument/codeLens",
            json!({
                "textDocument": { "uri": uri }
            }),
        );
        response["result"]
            .as_array()
            .expect("code lens array")
            .clone()
    }

    fn signature_help(&mut self, uri: &str, line: u32, character: u32) -> Option<Value> {
        let response = self.request(
            "textDocument/signatureHelp",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        );
        response
            .get("result")
            .filter(|result| !result.is_null())
            .cloned()
    }

    fn definition(&mut self, uri: &str, line: u32, character: u32) -> Option<Value> {
        let response = self.request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        );
        response
            .get("result")
            .filter(|result| !result.is_null())
            .cloned()
    }

    fn declaration(&mut self, uri: &str, line: u32, character: u32) -> Option<Value> {
        let response = self.request(
            "textDocument/declaration",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        );
        response
            .get("result")
            .filter(|result| !result.is_null())
            .cloned()
    }

    fn type_definition(&mut self, uri: &str, line: u32, character: u32) -> Option<Value> {
        let response = self.request(
            "textDocument/typeDefinition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        );
        response
            .get("result")
            .filter(|result| !result.is_null())
            .cloned()
    }

    fn implementation(&mut self, uri: &str, line: u32, character: u32) -> Option<Value> {
        let response = self.request(
            "textDocument/implementation",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        );
        response
            .get("result")
            .filter(|result| !result.is_null())
            .cloned()
    }

    fn selection_range(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let response = self.request(
            "textDocument/selectionRange",
            json!({
                "textDocument": { "uri": uri },
                "positions": [{ "line": line, "character": character }]
            }),
        );
        response["result"]
            .as_array()
            .expect("selection range array")
            .clone()
    }

    fn rename(&mut self, uri: &str, line: u32, character: u32, new_name: &str) -> Value {
        let response = self.request(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "newName": new_name
            }),
        );
        response["result"].clone()
    }

    fn hover(&mut self, uri: &str, line: u32, character: u32) -> Option<Value> {
        let response = self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        );
        response
            .get("result")
            .filter(|result| !result.is_null())
            .cloned()
    }

    fn completion(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let response = self.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        );
        response["result"]
            .as_array()
            .expect("completion array")
            .clone()
    }

    fn document_symbols(&mut self, uri: &str) -> Vec<Value> {
        let response = self.request(
            "textDocument/documentSymbol",
            json!({
                "textDocument": { "uri": uri }
            }),
        );
        response["result"]
            .as_array()
            .expect("document symbol array")
            .clone()
    }

    fn workspace_symbols(&mut self, query: &str) -> Vec<Value> {
        let response = self.request(
            "workspace/symbol",
            json!({
                "query": query
            }),
        );
        response["result"]
            .as_array()
            .expect("workspace symbol array")
            .clone()
    }

    fn references(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Vec<Value> {
        let response = self.request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": { "includeDeclaration": include_declaration }
            }),
        );
        response["result"]
            .as_array()
            .expect("references array")
            .clone()
    }

    fn document_highlights(&mut self, uri: &str, line: u32, character: u32) -> Vec<Value> {
        let response = self.request(
            "textDocument/documentHighlight",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        );
        response["result"]
            .as_array()
            .expect("document highlights array")
            .clone()
    }

    fn folding_ranges(&mut self, uri: &str) -> Vec<Value> {
        let response = self.request(
            "textDocument/foldingRange",
            json!({
                "textDocument": { "uri": uri }
            }),
        );
        response["result"]
            .as_array()
            .expect("folding range array")
            .clone()
    }

    fn inlay_hints(
        &mut self,
        uri: &str,
        start_line: u32,
        start_character: u32,
        end_line: u32,
        end_character: u32,
    ) -> Vec<Value> {
        let response = self.request(
            "textDocument/inlayHint",
            json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": start_character },
                    "end": { "line": end_line, "character": end_character }
                }
            }),
        );
        response["result"]
            .as_array()
            .expect("inlay hint array")
            .clone()
    }

    fn document_links(&mut self, uri: &str) -> Vec<Value> {
        let response = self.request(
            "textDocument/documentLink",
            json!({
                "textDocument": { "uri": uri }
            }),
        );
        response["result"]
            .as_array()
            .expect("document link array")
            .clone()
    }

    fn formatting(&mut self, uri: &str) -> Vec<Value> {
        let response = self.request(
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": uri },
                "options": {
                    "tabSize": 4,
                    "insertSpaces": true
                }
            }),
        );
        response["result"]
            .as_array()
            .expect("formatting edits array")
            .clone()
    }

    fn range_formatting(&mut self, uri: &str, start_line: u32, end_line: u32) -> Vec<Value> {
        let response = self.request(
            "textDocument/rangeFormatting",
            json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": 0 },
                    "end": { "line": end_line, "character": 0 }
                },
                "options": {
                    "tabSize": 4,
                    "insertSpaces": true
                }
            }),
        );
        response["result"]
            .as_array()
            .expect("range formatting edits array")
            .clone()
    }

    fn inline_values(&mut self, uri: &str, start_line: u32, end_line: u32) -> Vec<Value> {
        let response = self.request(
            "textDocument/inlineValue",
            json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": start_line, "character": 0 },
                    "end": { "line": end_line, "character": 0 }
                },
                "context": {
                    "frameId": 1,
                    "stoppedLocation": {
                        "start": { "line": start_line, "character": 0 },
                        "end": { "line": end_line, "character": 0 }
                    }
                }
            }),
        );
        response["result"]
            .as_array()
            .expect("inline values array")
            .clone()
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

    fn read_notification(&mut self, method: &str) -> Value {
        loop {
            let message = self.read_message();
            if message.get("method").and_then(Value::as_str) == Some(method) {
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
        self.shutdown();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn file_uri(path: &Path) -> String {
    Url::from_file_path(path).expect("file uri").to_string()
}

fn temp_project(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("rephactor-lsp-{name}-{nanos}"));
    std::fs::create_dir_all(&root).expect("create temp root");
    root
}

fn insert_texts(action: &Value, uri: &str) -> Vec<String> {
    action["edit"]["changes"][uri]
        .as_array()
        .expect("workspace edits for open document")
        .iter()
        .map(|edit| edit["newText"].as_str().expect("insert text").to_string())
        .collect()
}

#[test]
fn lsp_returns_signature_help_for_open_document() {
    let root = temp_project("signature-same-file");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text =
        "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";
    let uri = server.open_php(&file, text);

    let help = server
        .signature_help(&uri, 2, 22)
        .expect("signature help result");

    assert_eq!(
        help["signatures"][0]["label"],
        "send_invoice($invoice, $notify)"
    );
    assert_eq!(help["activeSignature"], 0);
    assert_eq!(help["activeParameter"], 1);
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_signature_help_for_grouped_import_static_method() {
    let root = temp_project("signature-grouped-import");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnamespace App\\Http;\nuse App\\Models\\{customer_supplier};\nnamespace App\\Models;\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id) {} }\nnamespace App\\Http;\ncustomer_supplier::accumulatePoints($shop_id, $promotion_id);\n";
    let uri = server.open_php(&file, text);

    let help = server
        .signature_help(&uri, 6, 45)
        .expect("signature help result");

    assert_eq!(
        help["signatures"][0]["label"],
        "customer_supplier::accumulatePoints($shop_id, $promotion_id)"
    );
    assert_eq!(help["activeParameter"], 1);
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_null_signature_help_for_dynamic_call() {
    let root = temp_project("signature-unsupported");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nfunction send_invoice($invoice, $notify) {}\n$fn($invoice, true);\n";
    let uri = server.open_php(&file, text);

    assert!(server.signature_help(&uri, 2, 5).is_none());
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_definition_for_same_file_function_call() {
    let root = temp_project("definition-same-file");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text =
        "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";
    let uri = server.open_php(&file, text);

    let definition = server.definition(&uri, 2, 5).expect("definition result");

    assert_eq!(definition["uri"], uri);
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 1, "character": 9 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_definition_for_classmap_static_method() {
    let root = temp_project("definition-classmap");
    let legacy_dir = root.join("legacy");
    let app_dir = root.join("app");
    std::fs::create_dir_all(&legacy_dir).expect("create legacy dir");
    std::fs::create_dir_all(&app_dir).expect("create app dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"autoload":{"classmap":["legacy/CustomerSupplier.php"]}}"#,
    )
    .expect("write composer");
    let service_path = legacy_dir.join("CustomerSupplier.php");
    std::fs::write(
        &service_path,
        "<?php\nnamespace Legacy;\nclass CustomerSupplier { public static function sync($shop_id, $customer_id) {} }\n",
    )
    .expect("write classmap class");
    let mut server = LspProcess::start(&root);
    let uri = server.open_php(
        &app_dir.join("Caller.php"),
        "<?php\nnamespace App;\nuse Legacy\\CustomerSupplier;\nCustomerSupplier::sync($shop_id, $customer_id);\n",
    );

    let definition = server.definition(&uri, 3, 25).expect("definition result");

    assert_eq!(definition["uri"], file_uri(&service_path));
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 2, "character": 48 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_null_definition_for_dynamic_call() {
    let root = temp_project("definition-unsupported");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nfunction send_invoice($invoice, $notify) {}\n$fn($invoice, true);\n";
    let uri = server.open_php(&file, text);

    assert!(server.definition(&uri, 2, 2).is_none());
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_hover_for_resolved_function_call() {
    let root = temp_project("hover-function");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\n/** Send an invoice. */\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";
    let uri = server.open_php(&file, text);

    let hover = server.hover(&uri, 3, 5).expect("hover result");

    assert_eq!(hover["contents"]["kind"], "markdown");
    assert!(
        hover["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("send_invoice($invoice, $notify)")
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("Send an invoice.")
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_type_definition_for_typed_variable() {
    let root = temp_project("type-definition");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass CustomerRecord {}\nfunction handle(CustomerRecord $customer) { return $customer; }\n";
    let uri = server.open_php(&file, text);

    let definition = server
        .type_definition(&uri, 2, 52)
        .expect("type definition result");

    assert_eq!(definition["uri"], uri);
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 1, "character": 6 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_php_manual_link_for_internal_function_hover() {
    let root = temp_project("hover-internal-function");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nstr_replace($search, $replace, $subject);\n";
    let uri = server.open_php(&file, text);

    let hover = server.hover(&uri, 1, 5).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("str_replace($search, $replace, $subject, $count)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/str_replace)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_declaration_for_implemented_method() {
    let root = temp_project("method-declaration");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\ninterface Sender { public function dispatch($invoice); }\nclass EmailSender implements Sender { public function dispatch($invoice) {} }\n";
    let uri = server.open_php(&file, text);

    let declaration = server.declaration(&uri, 2, 54).expect("declaration result");

    assert_eq!(declaration["uri"], uri);
    assert_eq!(
        declaration["range"]["start"],
        json!({ "line": 1, "character": 35 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_implementations_for_interface() {
    let root = temp_project("implementation-interface");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\ninterface Sender {}\nclass EmailSender implements Sender {}\nclass OtherSender {}\n";
    let uri = server.open_php(&file, text);

    let implementations = server
        .implementation(&uri, 1, 12)
        .expect("implementation result")
        .as_array()
        .expect("implementation array")
        .clone();

    assert_eq!(implementations.len(), 1);
    assert_eq!(implementations[0]["uri"], uri);
    assert_eq!(
        implementations[0]["range"]["start"],
        json!({ "line": 2, "character": 6 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_implementations_for_interface_method() {
    let root = temp_project("implementation-method");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\ninterface Sender { public function dispatch($invoice); }\nclass EmailSender implements Sender { public function dispatch($invoice) {} }\nclass OtherSender { public function dispatch($invoice) {} }\n";
    let uri = server.open_php(&file, text);

    let implementations = server
        .implementation(&uri, 1, 35)
        .expect("implementation result")
        .as_array()
        .expect("implementation array")
        .clone();

    assert_eq!(implementations.len(), 1);
    assert_eq!(implementations[0]["uri"], uri);
    assert_eq!(
        implementations[0]["range"]["start"],
        json!({ "line": 2, "character": 54 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_selection_ranges_from_syntax_tree() {
    let root = temp_project("selection-range");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nfunction handle($customer) { return $customer->name(); }\n";
    let uri = server.open_php(&file, text);

    let ranges = server.selection_range(&uri, 1, 38);

    assert_eq!(
        ranges[0]["range"]["start"],
        json!({ "line": 1, "character": 37 })
    );
    assert!(ranges[0]["parent"].is_object());
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_workspace_edit_for_rename() {
    let root = temp_project("rename");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nfunction send_invoice($invoice) {}\nsend_invoice($invoice);\n";
    let uri = server.open_php(&file, text);

    let edit = server.rename(&uri, 1, 12, "dispatch_invoice");
    let edits = edit["changes"]
        .get(&uri)
        .expect("rename edits for uri")
        .as_array()
        .expect("rename edits for uri");

    assert_eq!(edits.len(), 2);
    assert!(
        edits
            .iter()
            .all(|edit| edit["newText"] == "dispatch_invoice")
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_null_hover_for_dynamic_call() {
    let root = temp_project("hover-unsupported");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nfunction send_invoice($invoice, $notify) {}\n$fn($invoice, true);\n";
    let uri = server.open_php(&file, text);

    assert!(server.hover(&uri, 2, 2).is_none());
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_class_and_function_completions() {
    let root = temp_project("completion-basic");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass CustomerRecord {}\nfunction customer_report($shop) {}\nCustomerRecord::sync();\n";
    let uri = server.open_php(&file, text);

    let class_items = server.completion(&uri, 3, 4);

    assert!(
        class_items
            .iter()
            .any(|item| item["label"] == "CustomerRecord")
    );
    let uri = server.open_php(
        &file,
        "<?php\nclass CustomerRecord {}\nfunction customer_report($shop) {}\nCR::sync();\n",
    );
    let camel_items = server.completion(&uri, 3, 2);

    assert!(
        camel_items
            .iter()
            .any(|item| item["label"] == "CustomerRecord")
    );

    let uri = server.open_php(
        &file,
        "<?php\nclass CustomerRecord {}\nfunction customer_report($shop) {}\ncustomer_report($shop);\n",
    );
    let function_items = server.completion(&uri, 3, 9);

    assert!(
        function_items
            .iter()
            .any(|item| item["label"] == "customer_report")
    );
    let uri = server.open_php(
        &file,
        "<?php\nclass CustomerRecord {}\nfunction customer_report($shop) {}\nc_r($shop);\n",
    );
    let underscore_items = server.completion(&uri, 3, 3);

    assert!(
        underscore_items
            .iter()
            .any(|item| item["label"] == "customer_report")
    );

    let uri = server.open_php(
        &file,
        "<?php\nclass CustomerRecord {}\nfunction customer_report($shop) {}\nforeach ($items as $item) {}\n",
    );
    let keyword_items = server.completion(&uri, 3, 4);

    assert!(keyword_items.iter().any(|item| item["label"] == "foreach"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_adds_use_declaration_for_unambiguous_class_completion() {
    let root = temp_project("completion-auto-import");
    let model_dir = root.join("src/Models");
    let controller_dir = root.join("src/Http");
    std::fs::create_dir_all(&model_dir).expect("create model dir");
    std::fs::create_dir_all(&controller_dir).expect("create controller dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("write composer");
    std::fs::write(
        model_dir.join("CustomerRecord.php"),
        "<?php\nnamespace App\\Models;\nclass CustomerRecord {}\n",
    )
    .expect("write model");
    let mut server = LspProcess::start(&root);
    let file = controller_dir.join("Controller.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App\\Http;\nCustomerRecord::sync();\n",
    );

    let items = server.completion(&uri, 2, 4);
    let item = items
        .iter()
        .find(|item| item["label"] == "CustomerRecord")
        .expect("CustomerRecord completion");

    assert_eq!(item["detail"], "App\\Models\\CustomerRecord");
    assert_eq!(
        item["additionalTextEdits"][0]["newText"],
        "use App\\Models\\CustomerRecord;\n"
    );
    assert_eq!(
        item["additionalTextEdits"][0]["range"]["start"],
        json!({ "line": 2, "character": 0 })
    );

    let uri = server.open_php(
        &file,
        "<?php\nnamespace App\\Http;\nuse Vendor\\CustomerRecord;\nCustomerRecord::sync();\n",
    );
    let items = server.completion(&uri, 3, 4);
    let item = items
        .iter()
        .find(|item| item["label"] == "CustomerRecord")
        .expect("CustomerRecord completion");

    assert!(item.get("additionalTextEdits").is_none());
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_method_completion_after_static_scope() {
    let root = temp_project("completion-static-method");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass CustomerRecord { public static function syncOrder($order) {} }\nCustomerRecord::syncOrder();\n";
    let uri = server.open_php(&file, text);

    let items = server.completion(&uri, 2, 20);

    assert!(items.iter().any(|item| item["label"] == "syncOrder"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_related_instance_method_completions() {
    let root = temp_project("completion-related-methods");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseSender { public function baseDispatch() {} }\ninterface SenderContract { public function contractDispatch(); }\ntrait SenderTrait { public function traitDispatch() {} }\nclass SenderMixin { public function mixinDispatch() {} }\n/** @mixin SenderMixin */\nclass Sender extends BaseSender implements SenderContract { use SenderTrait; }\nfunction run(Sender $sender) {\n    $sender->baseDispatch();\n    $sender->contractDispatch();\n    $sender->traitDispatch();\n    $sender->mixinDispatch();\n}\n";
    let uri = server.open_php(&file, text);

    let base_items = server.completion(&uri, 8, 17);
    let contract_items = server.completion(&uri, 9, 21);
    let trait_items = server.completion(&uri, 10, 18);
    let mixin_items = server.completion(&uri, 11, 18);

    assert!(
        base_items
            .iter()
            .any(|item| item["label"] == "baseDispatch")
    );
    assert!(
        contract_items
            .iter()
            .any(|item| item["label"] == "contractDispatch")
    );
    assert!(
        trait_items
            .iter()
            .any(|item| item["label"] == "traitDispatch")
    );
    assert!(
        mixin_items
            .iter()
            .any(|item| item["label"] == "mixinDispatch")
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_instance_method_completion_for_variable_alias() {
    let root = temp_project("completion-variable-alias-methods");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass Sender { public function dispatch() {} }\nfunction run() {\n    $sender = new Sender();\n    $alias = $sender;\n    $alias->dispatch();\n}\n";
    let uri = server.open_php(&file, text);

    let items = server.completion(&uri, 5, 12);

    assert!(items.iter().any(|item| item["label"] == "dispatch"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_instance_method_completion_for_call_return_receiver() {
    let root = temp_project("completion-call-return-methods");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass Sender { public function dispatch() {} }\nfunction make_sender(): Sender { return new Sender(); }\nfunction run() {\n    $sender = make_sender();\n    $sender->dispatch();\n}\n";
    let uri = server.open_php(&file, text);

    let items = server.completion(&uri, 5, 13);

    assert!(items.iter().any(|item| item["label"] == "dispatch"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_instance_method_completion_for_self_typed_parameter() {
    let root = temp_project("completion-self-typed-parameter-methods");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnamespace App;\nclass Sender { public function dispatch() {} public function run(self $sender) {\n    $sender->dispatch();\n} }\n";
    let uri = server.open_php(&file, text);

    let items = server.completion(&uri, 3, 13);

    assert!(items.iter().any(|item| item["label"] == "dispatch"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_instance_method_completion_for_parent_typed_parameter() {
    let root = temp_project("completion-parent-typed-parameter-methods");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnamespace App;\nclass BaseSender { public function baseDispatch() {} }\nclass Sender extends BaseSender { public function run(parent $sender) {\n    $sender->baseDispatch();\n} }\n";
    let uri = server.open_php(&file, text);

    let items = server.completion(&uri, 4, 13);

    assert!(items.iter().any(|item| item["label"] == "baseDispatch"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_nested_document_symbols() {
    let root = temp_project("document-symbol");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnamespace App;\nfunction send_invoice($invoice) {}\nclass InvoiceSender { public function dispatch($invoice) {} }\n";
    let uri = server.open_php(&file, text);

    let symbols = server.document_symbols(&uri);

    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["name"] == "send_invoice")
    );
    let class_symbol = symbols
        .iter()
        .find(|symbol| symbol["name"] == "InvoiceSender")
        .expect("class symbol");
    assert!(
        class_symbol["children"]
            .as_array()
            .expect("class children")
            .iter()
            .any(|symbol| symbol["name"] == "dispatch")
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_workspace_symbols_from_composer_project() {
    let root = temp_project("workspace-symbol");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).expect("create source dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("write composer");
    let service_path = src_dir.join("InvoiceSender.php");
    std::fs::write(
        &service_path,
        "<?php\nnamespace App;\nfunction send_invoice($invoice) {}\nclass InvoiceSender { public function dispatch($invoice) {} }\n",
    )
    .expect("write service");
    let mut server = LspProcess::start(&root);

    let symbols = server.workspace_symbols("Invoice");

    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["name"] == "App\\InvoiceSender")
    );
    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["name"] == "App\\InvoiceSender::dispatch")
    );
    let symbols = server.workspace_symbols("IS");

    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["name"] == "App\\InvoiceSender")
    );
    let symbols = server.workspace_symbols("s_i");

    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["name"] == "App\\send_invoice")
    );
    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["location"]["uri"] == file_uri(&service_path))
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_workspace_references_for_function_name() {
    let root = temp_project("references");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).expect("create source dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("write composer");
    let functions_path = src_dir.join("Functions.php");
    std::fs::write(
        &functions_path,
        "<?php\nnamespace App;\nfunction send_invoice($invoice) {}\n",
    )
    .expect("write functions");
    let caller_path = src_dir.join("Caller.php");
    let mut server = LspProcess::start(&root);
    let caller_uri = server.open_php(
        &caller_path,
        "<?php\nnamespace App;\nsend_invoice($first);\nsend_invoice($second);\n",
    );

    let references = server.references(&caller_uri, 2, 5, true);

    assert_eq!(references.len(), 3);
    assert!(
        references
            .iter()
            .any(|reference| reference["uri"] == file_uri(&functions_path))
    );
    assert_eq!(
        references
            .iter()
            .filter(|reference| reference["uri"] == caller_uri)
            .count(),
        2
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_reference_code_lenses_for_declarations() {
    let root = temp_project("code-lens");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nfunction send_invoice($invoice) {}\nsend_invoice($invoice);\n",
    );

    let lenses = server.code_lens(&uri);

    assert!(lenses.iter().any(|lens| {
        lens["command"]["title"] == "1 reference"
            && lens["command"]["command"] == "editor.action.showReferences"
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_parse_diagnostics_for_open_document() {
    let root = temp_project("diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(&file, "<?php\nfunction broken( {\n");

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    assert!(
        !notification["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics array")
            .is_empty()
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_unresolved_call() {
    let root = temp_project("semantic-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(&file, "<?php\nmissing_invoice($invoice);\n");

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unresolved callable missing_invoice")
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_unresolved_type() {
    let root = temp_project("type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass ExistingCustomer {}\nfunction handle(MissingCustomer $customer): ExistingCustomer { return $customer; }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unresolved type MissingCustomer")
    }));
    assert!(!diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("ExistingCustomer")
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_duplicate_declarations() {
    let root = temp_project("duplicate-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nfunction send_invoice($invoice) {}\nfunction send_invoice($invoice) {}\nclass CustomerRecord {}\ninterface CustomerRecord {}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("duplicate function declaration App\\send_invoice")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("duplicate type declaration App\\CustomerRecord")
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_duplicate_parameters() {
    let root = temp_project("duplicate-parameter-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nfunction send_invoice($invoice, $invoice) {}\nclass Sender { public function dispatch($order, $order) {} }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic["message"] == "duplicate parameter $invoice")
            .count(),
        1
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic["message"] == "duplicate parameter $order")
            .count(),
        1
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_unknown_named_arguments() {
    let root = temp_project("unknown-named-argument-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(invoice: $invoice, notifiy: true);\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "unknown named argument notifiy" && diagnostic["severity"] == 1
    }));
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| { diagnostic["message"] == "unknown named argument invoice" })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_duplicate_named_arguments() {
    let root = temp_project("duplicate-named-argument-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(invoice: $first, invoice: $second);\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "duplicate named argument invoice" && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_too_many_arguments() {
    let root = temp_project("too-many-argument-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nfunction send_invoice($invoice) {}\nfunction collect_all(...$items) {}\nsend_invoice($invoice, true);\ncollect_all($one, $two);\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "too many arguments for send_invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| { diagnostic["message"] == "too many arguments for collect_all" })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_unused_imports() {
    let root = temp_project("unused-import-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nuse Domain\\CustomerRecord;\nuse Domain\\InvoiceRecord;\nfunction handle(CustomerRecord $customer) {}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unused import InvoiceRecord")
            && diagnostic["severity"] == 2
    }));
    assert!(!diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unused import CustomerRecord")
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_return_type_mismatch() {
    let root = temp_project("return-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction count_items(): int { return \"many\"; }\nfunction customer(): Customer { return new Invoice(); }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "return type mismatch: declared int, returned string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "return type mismatch: declared App\\Customer, returned App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_phpdoc_return_type_mismatch() {
    let root = temp_project("phpdoc-return-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\n/** @return Customer */\nfunction customer() { return new Invoice(); }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "return type mismatch: declared App\\Customer, returned App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_allows_null_for_nullable_return_type_diagnostics() {
    let root = temp_project("nullable-return-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nfunction customer(): ?Customer { return null; }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic["message"] != "return type mismatch: declared App\\Customer, returned null"
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_allows_null_for_null_union_return_type_diagnostics() {
    let root = temp_project("null-union-return-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nfunction customer(): Customer|null { return null; }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic["message"] != "return type mismatch: declared App\\Customer, returned null"
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_local_variable_return_type_mismatch() {
    let root = temp_project("local-return-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction count_items(): int { $value = \"many\"; return $value; }\nfunction customer(): Customer { $value = new Invoice(); return $value; }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "return type mismatch: declared int, returned string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "return type mismatch: declared App\\Customer, returned App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_call_return_type_mismatch() {
    let root = temp_project("call-return-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction make_invoice(): Invoice { return new Invoice(); }\nfunction customer(): Customer { return make_invoice(); }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "return type mismatch: declared App\\Customer, returned App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_assigned_call_return_type_mismatch() {
    let root = temp_project("assigned-call-return-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction make_invoice(): Invoice { return new Invoice(); }\nfunction customer(): Customer { $value = make_invoice(); return $value; }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "return type mismatch: declared App\\Customer, returned App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_argument_type_mismatch() {
    let root = temp_project("argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction send(Customer $customer, int $count) {}\nsend(new Invoice(), \"many\");\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for count: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_internal_function_argument_type_mismatch() {
    let root = temp_project("internal-function-argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Invoice {}\nstrlen(new Invoice());\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for string: expected string, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_phpdoc_param_argument_type_mismatch() {
    let root = temp_project("phpdoc-param-argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\n/** @param Customer $customer */\nfunction send($customer) {}\nsend(new Invoice());\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_allows_null_for_nullable_parameter_argument_diagnostics() {
    let root = temp_project("nullable-parameter-argument-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nfunction send(?Customer $customer) {}\nsend(null);\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic["message"]
            != "argument type mismatch for customer: expected App\\Customer, got null"
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_allows_null_for_phpdoc_null_union_parameter_argument_diagnostics() {
    let root = temp_project("phpdoc-null-union-parameter-argument-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\n/** @param Customer|null $customer */\nfunction send($customer) {}\nsend(null);\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic["message"]
            != "argument type mismatch for customer: expected App\\Customer, got null"
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_top_level_variable_argument_type_mismatch() {
    let root = temp_project("top-level-argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction send(Customer $customer) {}\n$value = new Invoice();\nsend($value);\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_call_argument_type_mismatch() {
    let root = temp_project("call-argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction make_invoice(): Invoice { return new Invoice(); }\nfunction send(Customer $customer) {}\nsend(make_invoice());\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_imported_function_call_argument_type_mismatch() {
    let root = temp_project("imported-function-call-argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace Lib;\nclass Invoice {}\nfunction make_invoice(): Invoice { return new Invoice(); }\nnamespace App;\nuse function Lib\\make_invoice;\nclass Customer {}\nfunction send(Customer $customer) {}\nsend(make_invoice());\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for customer: expected App\\Customer, got Lib\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_phpdoc_return_call_argument_type_mismatch() {
    let root = temp_project("phpdoc-return-call-argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\n/** @return Invoice */\nfunction make_invoice() { return new Invoice(); }\nfunction send(Customer $customer) {}\nsend(make_invoice());\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_assigned_call_argument_type_mismatch() {
    let root = temp_project("assigned-call-argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction make_invoice(): Invoice { return new Invoice(); }\nfunction send(Customer $customer) {}\n$value = make_invoice();\nsend($value);\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_variable_alias_argument_type_mismatch() {
    let root = temp_project("variable-alias-argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction make_invoice(): Invoice { return new Invoice(); }\nfunction send(Customer $customer) {}\n$source = make_invoice();\n$value = $source;\nsend($value);\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_assignment_type_mismatch() {
    let root = temp_project("assignment-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction handle(Customer $customer, int $count): void { $customer = new Invoice(); $count = \"many\"; }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "assignment type mismatch for $count: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_phpdoc_param_assignment_type_mismatch() {
    let root = temp_project("phpdoc-param-assignment-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\n/** @param Customer $customer */\nfunction handle($customer): void { $customer = new Invoice(); }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_variable_alias_assignment_type_mismatch() {
    let root = temp_project("variable-alias-assignment-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction handle(Customer $customer): void { $invoice = new Invoice(); $customer = $invoice; }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_call_assignment_type_mismatch() {
    let root = temp_project("call-assignment-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction make_invoice(): Invoice { return new Invoice(); }\nfunction handle(Customer $customer): void { $customer = make_invoice(); }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_phpdoc_var_assignment_type_mismatch() {
    let root = temp_project("phpdoc-var-assignment-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\n/** @var Customer $customer */\n$customer = new Invoice();\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_inline_phpdoc_var_assignment_type_mismatch() {
    let root = temp_project("inline-phpdoc-var-assignment-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\n/** @var Customer */\n$customer = new Invoice();\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_phpdoc_var_call_assignment_type_mismatch() {
    let root = temp_project("phpdoc-var-call-assignment-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nfunction make_invoice(): Invoice { return new Invoice(); }\n/** @var Customer $customer */\n$customer = make_invoice();\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_document_highlights_for_symbol_name() {
    let root = temp_project("document-highlight");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nfunction send_invoice($invoice) {}\nsend_invoice($first);\nsend_invoice($second);\n",
    );

    let highlights = server.document_highlights(&uri, 2, 5);

    assert_eq!(highlights.len(), 3);
    assert!(highlights.iter().all(|highlight| highlight["kind"] == 1));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_folding_ranges_for_blocks_and_comments() {
    let root = temp_project("folding-range");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\n/**\n * Summary.\n */\nclass InvoiceSender {\n    public function dispatch($invoice) {\n        send_invoice($invoice);\n    }\n}\n",
    );

    let ranges = server.folding_ranges(&uri);

    assert!(ranges.iter().any(|range| range["kind"] == "comment"));
    assert!(
        ranges
            .iter()
            .any(|range| range["startLine"] == 4 && range["endLine"] == 8)
    );
    assert!(
        ranges
            .iter()
            .any(|range| range["startLine"] == 5 && range["endLine"] == 7)
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_parameter_name_inlay_hints() {
    let root = temp_project("inlay-hints");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n",
    );

    let hints = server.inlay_hints(&uri, 0, 0, 3, 0);

    assert_eq!(
        hints
            .iter()
            .map(|hint| hint["label"].as_str().expect("hint label"))
            .collect::<Vec<_>>(),
        vec!["invoice:", "notify:"]
    );
    assert!(hints.iter().all(|hint| hint["kind"] == 2));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_inferred_return_type_inlay_hints() {
    let root = temp_project("return-type-inlay-hints");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nclass InvoiceSender {}\nfunction make_sender() { return new InvoiceSender(); }\nfunction declared_sender(): InvoiceSender { return new InvoiceSender(); }\n",
    );

    let hints = server.inlay_hints(&uri, 0, 0, 4, 0);

    assert!(hints.iter().any(|hint| {
        hint["label"] == ": InvoiceSender"
            && hint["kind"] == 1
            && hint["position"] == json!({ "line": 2, "character": 22 })
    }));
    assert_eq!(
        hints
            .iter()
            .filter(|hint| hint["label"] == ": InvoiceSender")
            .count(),
        1
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_document_links_for_require_paths() {
    let root = temp_project("document-links");
    let mut server = LspProcess::start(&root);
    let lib_dir = root.join("lib");
    std::fs::create_dir_all(&lib_dir).expect("create lib dir");
    let target = lib_dir.join("helpers.php");
    std::fs::write(&target, "<?php\nfunction helper() {}\n").expect("write helper");
    let file = root.join("example.php");
    let uri = server.open_php(&file, "<?php\nrequire 'lib/helpers.php';\n");

    let links = server.document_links(&uri);

    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["target"], file_uri(&target));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_formats_trailing_whitespace_and_final_newline() {
    let root = temp_project("formatting-whitespace");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php   \nfunction run() {\t\n    return true;   \n}",
    );

    let edits = server.formatting(&uri);

    assert_eq!(edits.len(), 1);
    assert_eq!(
        edits[0]["newText"],
        "<?php\nfunction run() {\n    return true;\n}\n"
    );
    assert_eq!(
        edits[0]["range"]["start"],
        json!({ "line": 0, "character": 0 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_range_formats_trailing_whitespace() {
    let root = temp_project("range-formatting-whitespace");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php   \nfunction run() {\t\n    return true;   \n}\n",
    );

    let edits = server.range_formatting(&uri, 1, 3);

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0]["newText"], "function run() {\n    return true;\n");
    assert_eq!(
        edits[0]["range"]["start"],
        json!({ "line": 1, "character": 0 })
    );
    assert_eq!(
        edits[0]["range"]["end"],
        json!({ "line": 3, "character": 0 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_inline_value_variable_lookups() {
    let root = temp_project("inline-values");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nfunction run($invoice) {\n    $total = $invoice->total();\n    return $total;\n}\n",
    );

    let values = server.inline_values(&uri, 2, 4);

    assert!(values.iter().any(|value| {
        value["variableName"] == "$total"
            && value["caseSensitiveLookup"] == true
            && value["range"]["start"] == json!({ "line": 2, "character": 4 })
    }));
    assert!(values.iter().any(|value| {
        value["variableName"] == "$invoice"
            && value["caseSensitiveLookup"] == true
            && value["range"]["start"] == json!({ "line": 2, "character": 13 })
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_named_argument_code_action_for_open_document() {
    let root = temp_project("same-file");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text =
        "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 2, 5);

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["title"], "[Rephactor] Add names to arguments");
    assert_eq!(actions[0]["kind"], "refactor.rewrite");
    assert_eq!(
        insert_texts(&actions[0], &uri),
        vec!["invoice: ", "notify: "]
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_phpdoc_code_action_for_function_declaration() {
    let root = temp_project("phpdoc-action");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nfunction send_invoice(int $invoice, $notify): string { throw new RuntimeException(); }\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 1, 12);
    let action = actions
        .iter()
        .find(|action| action["title"] == "[Rephactor] Add PHPDoc")
        .expect("PHPDoc action");

    assert_eq!(
        insert_texts(action, &uri),
        vec![
            "/**\n * @param int $invoice\n * @param mixed $notify\n * @return string\n * @throws RuntimeException\n */\n"
        ]
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_implement_interface_methods_code_action() {
    let root = temp_project("implement-interface-action");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\ninterface Sender { public function dispatch($invoice, $notify); }\nclass EmailSender implements Sender {}\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 2, 6);
    let action = actions
        .iter()
        .find(|action| action["title"] == "[Rephactor] Implement interface methods")
        .expect("implement interface action");

    assert_eq!(
        insert_texts(action, &uri),
        vec![
            "\n    public function dispatch($invoice, $notify) {\n        throw new \\BadMethodCallException('Not implemented');\n    }\n"
        ]
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_implement_abstract_methods_code_action() {
    let root = temp_project("implement-abstract-action");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nabstract class BaseSender { abstract public function dispatch($invoice, $notify); }\nclass EmailSender extends BaseSender {}\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 2, 6);
    let action = actions
        .iter()
        .find(|action| action["title"] == "[Rephactor] Implement abstract methods")
        .expect("implement abstract action");

    assert_eq!(
        insert_texts(action, &uri),
        vec![
            "\n    public function dispatch($invoice, $notify) {\n        throw new \\BadMethodCallException('Not implemented');\n    }\n"
        ]
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_import_refactor_for_fully_qualified_class_name() {
    let root = temp_project("import-refactor");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnamespace App\\Http;\nclass Controller { public function run() { \\App\\Models\\Customer::sync(); } }\nnamespace App\\Models;\nclass Customer { public static function sync() {} }\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 2, 60);

    assert!(
        actions.iter().any(|action| {
            action["title"] == "[Rephactor] Add import for 'App\\Models\\Customer'"
        })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_handles_grouped_import_static_method() {
    let root = temp_project("grouped-import");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnamespace App\\Http;\nuse App\\Models\\{customer_supplier};\nnamespace App\\Models;\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id) {} }\nnamespace App\\Http;\ncustomer_supplier::accumulatePoints($shop_id, $promotion_id);\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 6, 35);

    assert_eq!(actions.len(), 1);
    assert_eq!(
        insert_texts(&actions[0], &uri),
        vec!["shop_id: ", "promotion_id: "]
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_handles_partial_named_argument() {
    let root = temp_project("partial-named");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass customer_supplier { public static function accumulatePoints($shop_id, $grand_total, $exchange_gift = null) {} }\ncustomer_supplier::accumulatePoints(\n    shop_id: $shop_id,\n    grand_total: $request->grand_total,\n    $request->exchange_gift,\n);\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 5, 5);

    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0]["title"],
        "[Rephactor] Add name identifier 'exchange_gift'"
    );
    assert_eq!(insert_texts(&actions[0], &uri), vec!["exchange_gift: "]);
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_resolves_classmap_symbol() {
    let root = temp_project("classmap");
    let legacy_dir = root.join("legacy");
    let app_dir = root.join("app");
    std::fs::create_dir_all(&legacy_dir).expect("create legacy dir");
    std::fs::create_dir_all(&app_dir).expect("create app dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"autoload":{"classmap":["legacy/CustomerSupplier.php"]}}"#,
    )
    .expect("write composer");
    std::fs::write(
        legacy_dir.join("CustomerSupplier.php"),
        "<?php\nnamespace Legacy;\nclass CustomerSupplier { public static function sync($shop_id, $customer_id) {} }\n",
    )
    .expect("write classmap class");
    let mut server = LspProcess::start(&root);
    let uri = server.open_php(
        &app_dir.join("Caller.php"),
        "<?php\nnamespace App;\nuse Legacy\\CustomerSupplier;\nCustomerSupplier::sync($shop_id, $customer_id);\n",
    );

    let actions = server.code_actions(&uri, 3, 25);

    assert_eq!(actions.len(), 1);
    assert_eq!(
        insert_texts(&actions[0], &uri),
        vec!["shop_id: ", "customer_id: "]
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_resolves_inherited_method() {
    let root = temp_project("inherited");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\ninterface Sender { public function dispatch($invoice, $notify); }\nclass InvoiceSender implements Sender {}\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch($invoice, true);\n}\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 4, 15);
    let action = actions
        .iter()
        .find(|action| action["title"] == "[Rephactor] Add names to arguments")
        .expect("named argument action");

    assert_eq!(insert_texts(action, &uri), vec!["invoice: ", "notify: "]);
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_resolves_instance_method_from_phpdoc_var() {
    let root = temp_project("phpdoc-var");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\nfunction run($sender, $invoice) {\n    /** @var InvoiceSender $sender */\n    $sender->dispatch($invoice, true);\n}\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 4, 15);
    let action = actions
        .iter()
        .find(|action| action["title"] == "[Rephactor] Add names to arguments")
        .expect("named argument action");

    assert_eq!(insert_texts(action, &uri), vec!["invoice: ", "notify: "]);
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_resolves_instance_method_from_phpdoc_param() {
    let root = temp_project("phpdoc-param");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\n/** @param InvoiceSender $sender */\nfunction run($sender, $invoice) {\n    $sender->dispatch($invoice, true);\n}\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 4, 15);
    let action = actions
        .iter()
        .find(|action| action["title"] == "[Rephactor] Add names to arguments")
        .expect("named argument action");

    assert_eq!(insert_texts(action, &uri), vec!["invoice: ", "notify: "]);
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_resolves_instance_method_from_phpdoc_mixin() {
    let root = temp_project("phpdoc-mixin");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass SenderMixin { public function dispatch($invoice, $notify) {} }\n/** @mixin SenderMixin */\nclass Sender {}\nfunction run(Sender $sender, $invoice) {\n    $sender->dispatch($invoice, true);\n}\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 5, 15);
    let action = actions
        .iter()
        .find(|action| action["title"] == "[Rephactor] Add names to arguments")
        .expect("named argument action");

    assert_eq!(insert_texts(action, &uri), vec!["invoice: ", "notify: "]);
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_resolves_instance_method_from_phpdoc_method() {
    let root = temp_project("phpdoc-method");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\n/** @method void dispatch($invoice, $notify) */\nclass InvoiceSender {}\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch($invoice, true);\n}\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 4, 15);
    let action = actions
        .iter()
        .find(|action| action["title"] == "[Rephactor] Add names to arguments")
        .expect("named argument action");

    assert_eq!(insert_texts(action, &uri), vec!["invoice: ", "notify: "]);
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_empty_for_unsupported_calls() {
    let root = temp_project("unsupported");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, ...$flags);\n$fn($invoice, true);\n";
    let uri = server.open_php(&file, text);

    assert!(server.code_actions(&uri, 2, 5).is_empty());
    assert!(server.code_actions(&uri, 3, 2).is_empty());
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_empty_when_project_allows_php_7() {
    let root = temp_project("php7");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).expect("create src dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"require":{"php":"^7.4"},"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("write composer");
    let mut server = LspProcess::start(&root);
    let uri = server.open_php(
        &src_dir.join("Caller.php"),
        "<?php\nnamespace App;\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n",
    );

    assert!(server.code_actions(&uri, 3, 5).is_empty());
    std::fs::remove_dir_all(root).expect("remove temp root");
}
