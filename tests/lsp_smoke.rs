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
        "<?php\nclass CustomerRecord {}\nfunction customer_report($shop) {}\ncustomer_report($shop);\n",
    );
    let function_items = server.completion(&uri, 3, 9);

    assert!(
        function_items
            .iter()
            .any(|item| item["label"] == "customer_report")
    );
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
    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["location"]["uri"] == file_uri(&service_path))
    );
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

    assert_eq!(actions.len(), 1);
    assert_eq!(
        insert_texts(&actions[0], &uri),
        vec!["invoice: ", "notify: "]
    );
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
