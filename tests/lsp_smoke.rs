use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tower_lsp::lsp_types::{CompletionItemKind, Url};

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
fn lsp_returns_signature_help_for_internal_constructor() {
    let root = temp_project("signature-internal-constructor");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(&file, "<?php\nnew DateTimeImmutable('now');\n");

    let help = server
        .signature_help(&uri, 1, 23)
        .expect("signature help result");

    assert_eq!(
        help["signatures"][0]["label"],
        "DateTimeImmutable::__construct($datetime, $timezone)"
    );
    assert_eq!(help["activeParameter"], 0);
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_signature_help_for_internal_instance_method() {
    let root = temp_project("signature-internal-instance-method");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\n$date = new DateTimeImmutable('now');\n$date->format('Y-m-d');\n";
    let uri = server.open_php(&file, text);

    let help = server
        .signature_help(&uri, 2, 15)
        .expect("signature help result");

    assert_eq!(help["signatures"][0]["label"], "$date->format($format)");
    assert_eq!(help["activeParameter"], 0);
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_signature_help_for_internal_static_method() {
    let root = temp_project("signature-internal-static-method");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nDateTimeImmutable::createFromFormat('Y-m-d', $value);\n";
    let uri = server.open_php(&file, text);

    let help = server
        .signature_help(&uri, 1, 39)
        .expect("signature help result");

    assert_eq!(
        help["signatures"][0]["label"],
        "DateTimeImmutable::createFromFormat($format, $datetime, $timezone)"
    );
    assert_eq!(help["activeParameter"], 0);
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
fn lsp_returns_definition_for_imported_constant() {
    let root = temp_project("definition-constant");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnamespace App\\Config;\nconst API_VERSION = '1';\nnamespace App\\Http;\nuse const App\\Config\\API_VERSION;\necho API_VERSION;\n";
    let uri = server.open_php(&file, text);

    let definition = server.definition(&uri, 5, 7).expect("definition result");

    assert_eq!(definition["uri"], uri);
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 2, "character": 6 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_definition_for_class_constant() {
    let root = temp_project("definition-class-constant");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass CustomerRecord { const STATUS_PAID = 'paid'; }\necho CustomerRecord::STATUS_PAID;\n";
    let uri = server.open_php(&file, text);

    let definition = server.definition(&uri, 2, 25).expect("definition result");

    assert_eq!(definition["uri"], uri);
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 1, "character": 29 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_definition_for_inherited_class_constant() {
    let root = temp_project("definition-inherited-class-constant");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseRecord { const STATUS_OPEN = 'open'; }\nclass CustomerRecord extends BaseRecord {}\necho CustomerRecord::STATUS_OPEN;\n";
    let uri = server.open_php(&file, text);

    let definition = server.definition(&uri, 3, 25).expect("definition result");

    assert_eq!(definition["uri"], uri);
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 1, "character": 25 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_definition_for_static_scope_class_constants() {
    let root = temp_project("definition-static-scope-class-constant");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseRecord { const STATUS_OPEN = 'open'; }\nclass CustomerRecord extends BaseRecord { const STATUS_PAID = 'paid'; public function run() {\n    echo self::STATUS_PAID;\n    echo parent::STATUS_OPEN;\n} }\n";
    let uri = server.open_php(&file, text);

    let self_definition = server.definition(&uri, 3, 18).expect("self definition");
    let parent_definition = server.definition(&uri, 4, 20).expect("parent definition");

    assert_eq!(
        self_definition["range"]["start"],
        json!({ "line": 2, "character": 48 })
    );
    assert_eq!(
        parent_definition["range"]["start"],
        json!({ "line": 1, "character": 25 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_definition_for_instance_properties() {
    let root = temp_project("definition-instance-properties");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseSender { protected $queue; }\nclass Sender extends BaseSender { private $transport; }\nfunction run(Sender $sender) {\n    echo $sender->transport;\n    echo $sender->queue;\n}\n";
    let uri = server.open_php(&file, text);

    let own_definition = server.definition(&uri, 4, 22).expect("own definition");
    let inherited_definition = server
        .definition(&uri, 5, 20)
        .expect("inherited definition");

    assert_eq!(
        own_definition["range"]["start"],
        json!({ "line": 2, "character": 42 })
    );
    assert_eq!(
        inherited_definition["range"]["start"],
        json!({ "line": 1, "character": 29 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_definition_for_static_properties() {
    let root = temp_project("definition-static-properties");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseSender { protected static $queue; protected $draft; }\nclass Sender extends BaseSender { private static $transport; private $instance; }\necho Sender::$transport;\necho Sender::$queue;\nclass Child extends BaseSender { private static $local; public function run() {\n    echo self::$local;\n    echo parent::$queue;\n} }\n";
    let uri = server.open_php(&file, text);

    let own_definition = server.definition(&uri, 3, 18).expect("own definition");
    let inherited_definition = server
        .definition(&uri, 4, 16)
        .expect("inherited definition");
    let self_definition = server.definition(&uri, 6, 16).expect("self definition");
    let parent_definition = server.definition(&uri, 7, 18).expect("parent definition");

    assert_eq!(
        own_definition["range"]["start"],
        json!({ "line": 2, "character": 49 })
    );
    assert_eq!(
        inherited_definition["range"]["start"],
        json!({ "line": 1, "character": 36 })
    );
    assert_eq!(
        self_definition["range"]["start"],
        json!({ "line": 5, "character": 48 })
    );
    assert_eq!(
        parent_definition["range"]["start"],
        json!({ "line": 1, "character": 36 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_definition_for_this_property() {
    let root = temp_project("definition-this-property");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass Sender { private $transport; public function run() {\n    echo $this->transport;\n} }\n";
    let uri = server.open_php(&file, text);

    let definition = server.definition(&uri, 2, 19).expect("definition result");

    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 1, "character": 23 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_definition_for_this_method_call() {
    let root = temp_project("definition-this-method");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass Sender { public function dispatch($invoice) {} public function run() {\n    $this->dispatch($invoice);\n} }\n";
    let uri = server.open_php(&file, text);

    let definition = server.definition(&uri, 2, 15).expect("definition result");

    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 1, "character": 31 })
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
    let text = "<?php\n/**\n * Send an invoice.\n * @param Invoice $invoice\n * @return void\n */\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";
    let uri = server.open_php(&file, text);

    let hover = server.hover(&uri, 7, 5).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert_eq!(hover["contents"]["kind"], "markdown");
    assert!(markdown.contains("send_invoice($invoice, $notify)"));
    assert!(markdown.contains("Send an invoice."));
    assert!(markdown.contains("@param Invoice $invoice"));
    assert!(markdown.contains("@return void"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_hover_for_imported_constant() {
    let root = temp_project("hover-constant");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnamespace App\\Config;\nconst API_VERSION = '1';\nnamespace App\\Http;\nuse const App\\Config\\API_VERSION;\necho API_VERSION;\n";
    let uri = server.open_php(&file, text);

    let hover = server.hover(&uri, 5, 7).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert_eq!(hover["contents"]["kind"], "markdown");
    assert!(markdown.contains("const App\\Config\\API_VERSION"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_hover_for_class_constant() {
    let root = temp_project("hover-class-constant");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass CustomerRecord { const STATUS_PAID = 'paid'; }\necho CustomerRecord::STATUS_PAID;\n";
    let uri = server.open_php(&file, text);

    let hover = server.hover(&uri, 2, 25).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert_eq!(hover["contents"]["kind"], "markdown");
    assert!(markdown.contains("const CustomerRecord::STATUS_PAID"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_hover_for_inherited_class_constant() {
    let root = temp_project("hover-inherited-class-constant");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseRecord { const STATUS_OPEN = 'open'; }\nclass CustomerRecord extends BaseRecord {}\necho CustomerRecord::STATUS_OPEN;\n";
    let uri = server.open_php(&file, text);

    let hover = server.hover(&uri, 3, 25).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert_eq!(hover["contents"]["kind"], "markdown");
    assert!(markdown.contains("const BaseRecord::STATUS_OPEN"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_hover_for_static_scope_class_constants() {
    let root = temp_project("hover-static-scope-class-constant");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseRecord { const STATUS_OPEN = 'open'; }\nclass CustomerRecord extends BaseRecord { const STATUS_PAID = 'paid'; public function run() {\n    echo self::STATUS_PAID;\n    echo parent::STATUS_OPEN;\n} }\n";
    let uri = server.open_php(&file, text);

    let self_hover = server.hover(&uri, 3, 18).expect("self hover");
    let parent_hover = server.hover(&uri, 4, 20).expect("parent hover");
    let self_markdown = self_hover["contents"]["value"]
        .as_str()
        .expect("self hover markdown");
    let parent_markdown = parent_hover["contents"]["value"]
        .as_str()
        .expect("parent hover markdown");

    assert!(self_markdown.contains("const CustomerRecord::STATUS_PAID"));
    assert!(parent_markdown.contains("const BaseRecord::STATUS_OPEN"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_hover_for_instance_properties() {
    let root = temp_project("hover-instance-properties");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseSender { protected $queue; }\nclass Sender extends BaseSender { private $transport; }\nfunction run(Sender $sender) {\n    echo $sender->transport;\n    echo $sender->queue;\n}\n";
    let uri = server.open_php(&file, text);

    let own_hover = server.hover(&uri, 4, 22).expect("own hover");
    let inherited_hover = server.hover(&uri, 5, 20).expect("inherited hover");
    let own_markdown = own_hover["contents"]["value"]
        .as_str()
        .expect("own hover markdown");
    let inherited_markdown = inherited_hover["contents"]["value"]
        .as_str()
        .expect("inherited hover markdown");

    assert!(own_markdown.contains("property Sender::$transport"));
    assert!(inherited_markdown.contains("property BaseSender::$queue"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_hover_for_static_properties() {
    let root = temp_project("hover-static-properties");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseSender { protected static $queue; protected $draft; }\nclass Sender extends BaseSender { private static $transport; private $instance; }\necho Sender::$transport;\necho Sender::$queue;\n";
    let uri = server.open_php(&file, text);

    let own_hover = server.hover(&uri, 3, 18).expect("own hover");
    let inherited_hover = server.hover(&uri, 4, 16).expect("inherited hover");
    let own_markdown = own_hover["contents"]["value"]
        .as_str()
        .expect("own hover markdown");
    let inherited_markdown = inherited_hover["contents"]["value"]
        .as_str()
        .expect("inherited hover markdown");

    assert!(own_markdown.contains("static property Sender::$transport"));
    assert!(inherited_markdown.contains("static property BaseSender::$queue"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_hover_for_this_property() {
    let root = temp_project("hover-this-property");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass Sender { private $transport; public function run() {\n    echo $this->transport;\n} }\n";
    let uri = server.open_php(&file, text);

    let hover = server.hover(&uri, 2, 19).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("property Sender::$transport"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_hover_for_this_method_call() {
    let root = temp_project("hover-this-method");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass Sender { public function dispatch($invoice) {} public function run() {\n    $this->dispatch($invoice);\n} }\n";
    let uri = server.open_php(&file, text);

    let hover = server.hover(&uri, 2, 15).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("dispatch($invoice)"));
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
fn lsp_returns_type_definition_for_this_property() {
    let root = temp_project("type-definition-this-property");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass CustomerRecord {}\nclass Sender { private CustomerRecord $customer; public function handle() { return $this->customer; } }\n";
    let uri = server.open_php(&file, text);

    let definition = server
        .type_definition(&uri, 2, 92)
        .expect("type definition result");

    assert_eq!(definition["uri"], uri);
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 1, "character": 6 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_type_definition_for_phpdoc_this_property() {
    let root = temp_project("type-definition-phpdoc-this-property");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass CustomerRecord {}\n/** @property-read CustomerRecord $customer */\nclass Sender { public function handle() { return $this->customer; } }\n";
    let uri = server.open_php(&file, text);

    let definition = server
        .type_definition(&uri, 3, 58)
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
fn lsp_returns_hover_for_internal_constant() {
    let root = temp_project("hover-internal-constant");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\necho PHP_VERSION;\n";
    let uri = server.open_php(&file, text);

    let hover = server.hover(&uri, 1, 8).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("const PHP_VERSION"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_php_manual_link_for_internal_class_hover() {
    let root = temp_project("hover-internal-class");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnew DateTimeImmutable();\nfunction handle(Throwable $error) {}\n";
    let uri = server.open_php(&file, text);

    let class_hover = server.hover(&uri, 1, 8).expect("class hover result");
    let class_markdown = class_hover["contents"]["value"]
        .as_str()
        .expect("class hover markdown");

    assert!(class_markdown.contains("class DateTimeImmutable"));
    assert!(class_markdown.contains("[PHP manual](https://www.php.net/datetimeimmutable)"));

    let interface_hover = server.hover(&uri, 2, 17).expect("interface hover result");
    let interface_markdown = interface_hover["contents"]["value"]
        .as_str()
        .expect("interface hover markdown");

    assert!(interface_markdown.contains("interface Throwable"));
    assert!(interface_markdown.contains("[PHP manual](https://www.php.net/throwable)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_php_manual_link_for_internal_constructor_hover() {
    let root = temp_project("hover-internal-constructor");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnew DateTimeImmutable('now');\n";
    let uri = server.open_php(&file, text);

    let hover = server.hover(&uri, 1, 8).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("DateTimeImmutable::__construct($datetime, $timezone)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/datetimeimmutable)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_php_manual_link_for_internal_method_hover() {
    let root = temp_project("hover-internal-method");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\n$date = new DateTimeImmutable('now');\n$date->format('Y-m-d');\n";
    let uri = server.open_php(&file, text);

    let hover = server.hover(&uri, 2, 9).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("$date->format($format)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/datetimeimmutable.format)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_expanded_internal_function_metadata() {
    let root = temp_project("expanded-internal-functions");
    let mut server = LspProcess::start(&root);
    let completion_file = root.join("completion.php");
    let completion_uri = server.open_php(&completion_file, "<?php\nstr_starts_with();\n");
    let _ = server.read_notification("textDocument/publishDiagnostics");

    let items = server.completion(&completion_uri, 1, 5);

    assert!(items.iter().any(|item| item["label"] == "str_starts_with"));

    let diagnostics_file = root.join("diagnostics.php");
    let diagnostics_uri = server.open_php(
        &diagnostics_file,
        "<?php\nstr_starts_with($haystack, $needle);\narray_values(\"not an array\");\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], diagnostics_uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for array: expected array, got string"
            && diagnostic["severity"] == 1
    }));

    let hover = server.hover(&diagnostics_uri, 1, 5).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("str_starts_with($haystack, $needle)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/str_starts_with)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_array_utility_internal_function_metadata() {
    let root = temp_project("array-utility-internal-functions");
    let mut server = LspProcess::start(&root);
    let completion_file = root.join("completion.php");
    let completion_uri = server.open_php(&completion_file, "<?php\nks;\narray_fil;\n");
    let _ = server.read_notification("textDocument/publishDiagnostics");

    let items = server.completion(&completion_uri, 1, 2);

    assert!(items.iter().any(|item| item["label"] == "ksort"));

    let diagnostics_file = root.join("diagnostics.php");
    let diagnostics_uri = server.open_php(
        &diagnostics_file,
        "<?php\narray_reverse('bad');\narray_unique([], 'bad');\narray_reduce('bad', $callback);\narray_diff('bad', []);\narray_chunk([], 'bad');\nsort('bad', 'bad');\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], diagnostics_uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for array: expected array, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for flags: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for length: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for flags: expected int, got string"
            && diagnostic["severity"] == 1
    }));

    let hover = server.hover(&diagnostics_uri, 1, 8).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("array_reverse($array, $preserve_keys)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/array_reverse)"));

    let reduce_hover = server.hover(&diagnostics_uri, 3, 8).expect("hover result");
    let reduce_markdown = reduce_hover["contents"]["value"]
        .as_str()
        .expect("hover markdown");

    assert!(reduce_markdown.contains("array_reduce($array, $callback, $initial)"));
    assert!(reduce_markdown.contains("[PHP manual](https://www.php.net/array_reduce)"));

    let diff_hover = server.hover(&diagnostics_uri, 4, 7).expect("hover result");
    let diff_markdown = diff_hover["contents"]["value"]
        .as_str()
        .expect("hover markdown");

    assert!(diff_markdown.contains("array_diff($array, $arrays)"));
    assert!(diff_markdown.contains("[PHP manual](https://www.php.net/array_diff)"));

    let chunk_hover = server.hover(&diagnostics_uri, 5, 8).expect("hover result");
    let chunk_markdown = chunk_hover["contents"]["value"]
        .as_str()
        .expect("hover markdown");

    assert!(chunk_markdown.contains("array_chunk($array, $length, $preserve_keys)"));
    assert!(chunk_markdown.contains("[PHP manual](https://www.php.net/array_chunk)"));

    let sort_hover = server.hover(&diagnostics_uri, 6, 2).expect("hover result");
    let sort_markdown = sort_hover["contents"]["value"]
        .as_str()
        .expect("hover markdown");

    assert!(sort_markdown.contains("sort($array, $flags)"));
    assert!(sort_markdown.contains("[PHP manual](https://www.php.net/sort)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_datetime_internal_function_metadata() {
    let root = temp_project("datetime-internal-functions");
    let mut server = LspProcess::start(&root);
    let completion_file = root.join("completion.php");
    let completion_uri = server.open_php(&completion_file, "<?php\nstrtot;\n");
    let _ = server.read_notification("textDocument/publishDiagnostics");

    let items = server.completion(&completion_uri, 1, 6);

    assert!(items.iter().any(|item| item["label"] == "strtotime"));

    let diagnostics_file = root.join("diagnostics.php");
    let diagnostics_uri = server.open_php(
        &diagnostics_file,
        "<?php\ndate([], 'bad');\nfunction takes_string(string $value) {}\ntakes_string(time());\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], diagnostics_uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for format: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for timestamp: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for value: expected string, got int"
            && diagnostic["severity"] == 1
    }));

    let hover = server.hover(&diagnostics_uri, 1, 2).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("date($format, $timestamp)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/date)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_math_internal_function_metadata() {
    let root = temp_project("math-internal-functions");
    let mut server = LspProcess::start(&root);
    let completion_file = root.join("completion.php");
    let completion_uri = server.open_php(&completion_file, "<?php\nsq;\nrou;\n");
    let _ = server.read_notification("textDocument/publishDiagnostics");

    let items = server.completion(&completion_uri, 1, 2);

    assert!(items.iter().any(|item| item["label"] == "sqrt"));

    let diagnostics_file = root.join("diagnostics.php");
    let diagnostics_uri = server.open_php(
        &diagnostics_file,
        "<?php\nround('bad', 'precision');\npow('bad', 'exp');\nfunction takes_string(string $value) {}\ntakes_string(abs(1));\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], diagnostics_uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for num: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for precision: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for exponent: expected float, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for value: expected string, got int"
            && diagnostic["severity"] == 1
    }));

    let hover = server.hover(&diagnostics_uri, 1, 2).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("round($num, $precision, $mode)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/round)"));

    let pow_hover = server.hover(&diagnostics_uri, 2, 2).expect("hover result");
    let pow_markdown = pow_hover["contents"]["value"]
        .as_str()
        .expect("hover markdown");

    assert!(pow_markdown.contains("pow($num, $exponent)"));
    assert!(pow_markdown.contains("[PHP manual](https://www.php.net/pow)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_filesystem_internal_function_metadata() {
    let root = temp_project("filesystem-internal-functions");
    let mut server = LspProcess::start(&root);
    let completion_file = root.join("completion.php");
    let completion_uri = server.open_php(&completion_file, "<?php\nfile_ex;\n");
    let _ = server.read_notification("textDocument/publishDiagnostics");

    let items = server.completion(&completion_uri, 1, 7);

    assert!(items.iter().any(|item| item["label"] == "file_exists"));

    let diagnostics_file = root.join("diagnostics.php");
    let diagnostics_uri = server.open_php(
        &diagnostics_file,
        "<?php\nfile_exists([]);\ndirname('/tmp', 'bad');\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], diagnostics_uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for filename: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for levels: expected int, got string"
            && diagnostic["severity"] == 1
    }));

    let hover = server.hover(&diagnostics_uri, 2, 2).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("dirname($path, $levels)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/dirname)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_filesystem_io_internal_function_metadata() {
    let root = temp_project("filesystem-io-internal-functions");
    let mut server = LspProcess::start(&root);
    let completion_file = root.join("completion.php");
    let completion_uri = server.open_php(&completion_file, "<?php\nfile_get;\n");
    let _ = server.read_notification("textDocument/publishDiagnostics");

    let items = server.completion(&completion_uri, 1, 8);

    assert!(
        items
            .iter()
            .any(|item| item["label"] == "file_get_contents")
    );

    let diagnostics_file = root.join("diagnostics.php");
    let diagnostics_uri = server.open_php(
        &diagnostics_file,
        "<?php\nfile_get_contents([], 'bad', null, 'bad');\nfile_put_contents([], 'data', 'bad');\nfunction takes_string(string $value) {}\ntakes_string(filesize('/tmp/file'));\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], diagnostics_uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for filename: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for use_include_path: expected bool, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for offset: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for flags: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for value: expected string, got int"
            && diagnostic["severity"] == 1
    }));

    let hover = server.hover(&diagnostics_uri, 1, 5).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(
        markdown.contains(
            "file_get_contents($filename, $use_include_path, $context, $offset, $length)"
        )
    );
    assert!(markdown.contains("[PHP manual](https://www.php.net/file_get_contents)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_string_internal_function_metadata() {
    let root = temp_project("string-internal-functions");
    let mut server = LspProcess::start(&root);
    let completion_file = root.join("completion.php");
    let completion_uri = server.open_php(&completion_file, "<?php\nucw;\nstr_pa;\n");
    let _ = server.read_notification("textDocument/publishDiagnostics");

    let items = server.completion(&completion_uri, 1, 3);

    assert!(items.iter().any(|item| item["label"] == "ucwords"));

    let diagnostics_file = root.join("diagnostics.php");
    let diagnostics_uri = server.open_php(
        &diagnostics_file,
        "<?php\nstr_repeat('x', 'bad');\nstrpos([], 'x');\nstr_pad([], 'bad');\nucfirst([]);\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], diagnostics_uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for times: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for haystack: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for length: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for string: expected string, got array"
            && diagnostic["severity"] == 1
    }));

    let hover = server.hover(&diagnostics_uri, 1, 5).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("str_repeat($string, $times)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/str_repeat)"));

    let pad_hover = server.hover(&diagnostics_uri, 3, 5).expect("hover result");
    let pad_markdown = pad_hover["contents"]["value"]
        .as_str()
        .expect("hover markdown");

    assert!(pad_markdown.contains("str_pad($string, $length, $pad_string, $pad_type)"));
    assert!(pad_markdown.contains("[PHP manual](https://www.php.net/str_pad)"));

    let ucfirst_hover = server.hover(&diagnostics_uri, 4, 3).expect("hover result");
    let ucfirst_markdown = ucfirst_hover["contents"]["value"]
        .as_str()
        .expect("hover markdown");

    assert!(ucfirst_markdown.contains("ucfirst($string)"));
    assert!(ucfirst_markdown.contains("[PHP manual](https://www.php.net/ucfirst)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_mbstring_internal_function_metadata() {
    let root = temp_project("mbstring-internal-functions");
    let mut server = LspProcess::start(&root);
    let completion_file = root.join("completion.php");
    let completion_uri = server.open_php(&completion_file, "<?php\nmb_sub;\n");
    let _ = server.read_notification("textDocument/publishDiagnostics");

    let items = server.completion(&completion_uri, 1, 6);

    assert!(items.iter().any(|item| item["label"] == "mb_substr"));

    let diagnostics_file = root.join("diagnostics.php");
    let diagnostics_uri = server.open_php(
        &diagnostics_file,
        "<?php\nmb_substr([], 'bad', 'bad', []);\nmb_strpos([], [], 'bad');\nfunction takes_string(string $value) {}\ntakes_string(mb_strlen('tieng viet'));\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], diagnostics_uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for string: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for start: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for length: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for encoding: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for haystack: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for needle: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for offset: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for value: expected string, got int"
            && diagnostic["severity"] == 1
    }));

    let hover = server.hover(&diagnostics_uri, 1, 5).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("mb_substr($string, $start, $length, $encoding)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/mb_substr)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_type_check_internal_function_metadata() {
    let root = temp_project("type-check-internal-functions");
    let mut server = LspProcess::start(&root);
    let completion_file = root.join("completion.php");
    let completion_uri = server.open_php(&completion_file, "<?php\nis_str;\n");
    let _ = server.read_notification("textDocument/publishDiagnostics");

    let items = server.completion(&completion_uri, 1, 6);

    assert!(items.iter().any(|item| item["label"] == "is_string"));

    let diagnostics_file = root.join("diagnostics.php");
    let diagnostics_uri = server.open_php(
        &diagnostics_file,
        "<?php\nfunction takes_string(string $value) {}\ntakes_string(is_string('x'));\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], diagnostics_uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for value: expected string, got bool"
            && diagnostic["severity"] == 1
    }));

    let hover = server.hover(&diagnostics_uri, 2, 16).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("is_string($value)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/is_string)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_regex_escaping_internal_function_metadata() {
    let root = temp_project("regex-escaping-internal-functions");
    let mut server = LspProcess::start(&root);
    let completion_file = root.join("completion.php");
    let completion_uri = server.open_php(&completion_file, "<?php\nhtmlsp;\n");
    let _ = server.read_notification("textDocument/publishDiagnostics");

    let items = server.completion(&completion_uri, 1, 6);

    assert!(items.iter().any(|item| item["label"] == "htmlspecialchars"));

    let diagnostics_file = root.join("diagnostics.php");
    let diagnostics_uri = server.open_php(
        &diagnostics_file,
        "<?php\nhtmlspecialchars([], 'bad', []);\npreg_split([], [], 'bad');\nfunction takes_string(string $value) {}\ntakes_string(preg_match_all('/x/', 'x'));\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], diagnostics_uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for string: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for flags: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for encoding: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for pattern: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for subject: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for limit: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for value: expected string, got int"
            && diagnostic["severity"] == 1
    }));

    let hover = server.hover(&diagnostics_uri, 1, 5).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("htmlspecialchars($string, $flags, $encoding, $double_encode)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/htmlspecialchars)"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_runtime_internal_function_metadata() {
    let root = temp_project("runtime-internal-functions");
    let mut server = LspProcess::start(&root);
    let completion_file = root.join("completion.php");
    let completion_uri = server.open_php(&completion_file, "<?php\nrandom_i;\nser;\n");
    let _ = server.read_notification("textDocument/publishDiagnostics");

    let items = server.completion(&completion_uri, 1, 8);

    assert!(items.iter().any(|item| item["label"] == "random_int"));

    let diagnostics_file = root.join("diagnostics.php");
    let diagnostics_uri = server.open_php(
        &diagnostics_file,
        "<?php\nhash([], [], false, 'bad');\nmd5([], 'bad');\nrandom_int('min', 'max');\nfunction takes_int(int $value) {}\ntakes_int(strval(10));\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], diagnostics_uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for algo: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for data: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for options: expected array, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for string: expected string, got array"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for binary: expected bool, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for min: expected int, got string"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for value: expected int, got string"
            && diagnostic["severity"] == 1
    }));

    let hover = server.hover(&diagnostics_uri, 1, 2).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");

    assert!(markdown.contains("hash($algo, $data, $binary, $options)"));
    assert!(markdown.contains("[PHP manual](https://www.php.net/hash)"));

    let random_hover = server.hover(&diagnostics_uri, 3, 8).expect("hover result");
    let random_markdown = random_hover["contents"]["value"]
        .as_str()
        .expect("hover markdown");

    assert!(random_markdown.contains("random_int($min, $max)"));
    assert!(random_markdown.contains("[PHP manual](https://www.php.net/random_int)"));
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
fn lsp_returns_workspace_edit_for_constant_rename() {
    let root = temp_project("rename-constant");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnamespace App;\nconst API_VERSION = '1';\necho API_VERSION;\n";
    let uri = server.open_php(&file, text);

    let edit = server.rename(&uri, 2, 8, "APP_VERSION");
    let edits = edit["changes"]
        .get(&uri)
        .expect("rename edits for uri")
        .as_array()
        .expect("rename edits for uri");

    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|edit| edit["newText"] == "APP_VERSION"));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_renames_matching_class_file_for_class_rename() {
    let root = temp_project("rename-class-file");
    let mut server = LspProcess::start(&root);
    let file = root.join("CustomerRecord.php");
    let uri = server.open_php(
        &file,
        "<?php\nclass CustomerRecord {}\nnew CustomerRecord();\n",
    );

    let edit = server.rename(&uri, 1, 8, "ClientRecord");
    let edits = edit["changes"]
        .get(&uri)
        .expect("rename edits for uri")
        .as_array()
        .expect("rename edits for uri");
    let operations = edit["documentChanges"]
        .as_array()
        .expect("document change operations");

    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|edit| edit["newText"] == "ClientRecord"));
    assert!(operations.iter().any(|operation| {
        operation["kind"] == "rename"
            && operation["oldUri"] == uri
            && operation["newUri"]
                .as_str()
                .expect("new uri")
                .ends_with("/ClientRecord.php")
    }));
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
fn lsp_adds_use_const_declaration_for_unambiguous_constant_completion() {
    let root = temp_project("completion-auto-import-const");
    let config_dir = root.join("src/Config");
    let controller_dir = root.join("src/Http");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    std::fs::create_dir_all(&controller_dir).expect("create controller dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("write composer");
    std::fs::write(
        config_dir.join("constants.php"),
        "<?php\nnamespace App\\Config;\nconst API_VERSION = '1';\n",
    )
    .expect("write constants");
    let mut server = LspProcess::start(&root);
    let file = controller_dir.join("Controller.php");
    let uri = server.open_php(&file, "<?php\nnamespace App\\Http;\nAPI_VERSION;\n");

    let items = server.completion(&uri, 2, 4);
    let item = items
        .iter()
        .find(|item| item["label"] == "API_VERSION")
        .expect("API_VERSION completion");

    assert_eq!(item["detail"], "App\\Config\\API_VERSION");
    assert_eq!(
        item["additionalTextEdits"][0]["newText"],
        "use const App\\Config\\API_VERSION;\n"
    );
    assert_eq!(
        item["additionalTextEdits"][0]["range"]["start"],
        json!({ "line": 2, "character": 0 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_internal_constant_completions() {
    let root = temp_project("completion-internal-constant");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(&file, "<?php\nPHP_VER;\n");

    let items = server.completion(&uri, 1, 7);
    let constant_kind =
        serde_json::to_value(CompletionItemKind::CONSTANT).expect("constant kind json");
    let item = items
        .iter()
        .find(|item| item["label"] == "PHP_VERSION")
        .expect("PHP_VERSION completion");

    assert_eq!(item["kind"], constant_kind);
    assert_eq!(item["detail"], "PHP internal constant");
    assert!(item.get("additionalTextEdits").is_none());
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_superglobal_completions() {
    let root = temp_project("completion-superglobals");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(&file, "<?php\n$_SER;\n");

    let items = server.completion(&uri, 1, 6);
    let variable_kind =
        serde_json::to_value(CompletionItemKind::VARIABLE).expect("variable kind json");
    let item = items
        .iter()
        .find(|item| item["label"] == "$_SERVER")
        .expect("$_SERVER completion");

    assert_eq!(item["kind"], variable_kind);
    assert!(item.get("additionalTextEdits").is_none());
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_internal_class_completions() {
    let root = temp_project("completion-internal-class");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(&file, "<?php\nnew DateTimeImm;\nThr;\n");

    let class_items = server.completion(&uri, 1, 15);
    let class_kind = serde_json::to_value(CompletionItemKind::CLASS).expect("class kind json");
    let class_item = class_items
        .iter()
        .find(|item| item["label"] == "DateTimeImmutable")
        .expect("DateTimeImmutable completion");

    assert_eq!(class_item["kind"], class_kind);
    assert_eq!(class_item["detail"], "PHP internal symbol");
    assert!(class_item.get("additionalTextEdits").is_none());

    let interface_items = server.completion(&uri, 2, 3);
    let interface_kind =
        serde_json::to_value(CompletionItemKind::INTERFACE).expect("interface kind json");
    let interface_item = interface_items
        .iter()
        .find(|item| item["label"] == "Throwable")
        .expect("Throwable completion");

    assert_eq!(interface_item["kind"], interface_kind);
    assert!(interface_item.get("additionalTextEdits").is_none());
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_method_completion_after_static_scope() {
    let root = temp_project("completion-static-method");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseRecord { const STATUS_OPEN = 'open'; }\nclass CustomerRecord extends BaseRecord { const STATUS_PAID = 'paid'; public static function syncOrder($order) {} }\nCustomerRecord::syncOrder();\n";
    let uri = server.open_php(&file, text);

    let items = server.completion(&uri, 3, 17);
    let constant_kind =
        serde_json::to_value(CompletionItemKind::CONSTANT).expect("constant kind json");

    assert!(items.iter().any(|item| item["label"] == "syncOrder"));
    assert!(
        items
            .iter()
            .any(|item| { item["label"] == "STATUS_PAID" && item["kind"] == constant_kind })
    );
    assert!(
        items
            .iter()
            .any(|item| { item["label"] == "STATUS_OPEN" && item["kind"] == constant_kind })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_static_scope_completion_for_self_and_parent() {
    let root = temp_project("completion-static-scope-keywords");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseRecord { const STATUS_OPEN = 'open'; public static function baseSync() {} }\nclass CustomerRecord extends BaseRecord { const STATUS_PAID = 'paid'; public static function syncOrder() {} public function run() {\n    self::syncOrder();\n    parent::baseSync();\n} }\n";
    let uri = server.open_php(&file, text);

    let self_items = server.completion(&uri, 3, 10);
    let parent_items = server.completion(&uri, 4, 12);
    let constant_kind =
        serde_json::to_value(CompletionItemKind::CONSTANT).expect("constant kind json");

    assert!(self_items.iter().any(|item| item["label"] == "syncOrder"));
    assert!(
        self_items
            .iter()
            .any(|item| { item["label"] == "STATUS_PAID" && item["kind"] == constant_kind })
    );
    assert!(parent_items.iter().any(|item| item["label"] == "baseSync"));
    assert!(
        parent_items
            .iter()
            .any(|item| { item["label"] == "STATUS_OPEN" && item["kind"] == constant_kind })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_adds_use_function_declaration_for_unambiguous_function_completion() {
    let root = temp_project("completion-auto-import-function");
    let support_dir = root.join("src/Support");
    let controller_dir = root.join("src/Http");
    std::fs::create_dir_all(&support_dir).expect("create support dir");
    std::fs::create_dir_all(&controller_dir).expect("create controller dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("write composer");
    std::fs::write(
        support_dir.join("functions.php"),
        "<?php\nnamespace App\\Support;\nfunction send_invoice($invoice) {}\n",
    )
    .expect("write functions");
    let mut server = LspProcess::start(&root);
    let file = controller_dir.join("Controller.php");
    let uri = server.open_php(&file, "<?php\nnamespace App\\Http;\nsend_invoice();\n");

    let items = server.completion(&uri, 2, 4);
    let item = items
        .iter()
        .find(|item| item["label"] == "send_invoice")
        .expect("send_invoice completion");

    assert_eq!(item["detail"], "App\\Support\\send_invoice");
    assert_eq!(
        item["additionalTextEdits"][0]["newText"],
        "use function App\\Support\\send_invoice;\n"
    );
    assert_eq!(
        item["additionalTextEdits"][0]["range"]["start"],
        json!({ "line": 2, "character": 0 })
    );

    let uri = server.open_php(
        &file,
        "<?php\nnamespace App\\Http;\nuse function Vendor\\send_invoice;\nsend_invoice();\n",
    );
    let items = server.completion(&uri, 3, 4);
    let item = items
        .iter()
        .find(|item| item["label"] == "send_invoice")
        .expect("send_invoice completion");

    assert!(item.get("additionalTextEdits").is_none());
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_static_property_completions() {
    let root = temp_project("completion-static-properties");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseRecord { protected static $shared; protected $queue; }\nclass CustomerRecord extends BaseRecord { private static $transport; private $instance; }\nCustomerRecord::$transport;\n";
    let uri = server.open_php(&file, text);

    let items = server.completion(&uri, 3, 17);
    let property_kind =
        serde_json::to_value(CompletionItemKind::PROPERTY).expect("property kind json");

    assert!(
        items
            .iter()
            .any(|item| { item["label"] == "transport" && item["kind"] == property_kind })
    );
    assert!(
        items
            .iter()
            .any(|item| { item["label"] == "shared" && item["kind"] == property_kind })
    );
    assert!(!items.iter().any(|item| item["label"] == "instance"));
    assert!(!items.iter().any(|item| item["label"] == "queue"));
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
fn lsp_returns_internal_instance_method_completions() {
    let root = temp_project("completion-internal-instance-methods");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\n$date = new DateTimeImmutable('now');\n$date->for;\n";
    let uri = server.open_php(&file, text);

    let items = server.completion(&uri, 2, 10);
    let method_kind = serde_json::to_value(CompletionItemKind::METHOD).expect("method kind json");
    let item = items
        .iter()
        .find(|item| item["label"] == "format")
        .expect("format completion");

    assert_eq!(item["kind"], method_kind);
    assert_eq!(item["detail"], "PHP internal method");
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_internal_static_method_completions() {
    let root = temp_project("completion-internal-static-methods");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(&file, "<?php\nDateTimeImmutable::create;\n");

    let items = server.completion(&uri, 1, 25);
    let method_kind = serde_json::to_value(CompletionItemKind::METHOD).expect("method kind json");
    let item = items
        .iter()
        .find(|item| item["label"] == "createFromFormat")
        .expect("createFromFormat completion");

    assert_eq!(item["kind"], method_kind);
    assert_eq!(item["detail"], "PHP internal method");
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_instance_property_completions() {
    let root = temp_project("completion-instance-properties");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass BaseSender { protected $queue; }\ntrait SenderTrait { protected $channel; }\nclass Sender extends BaseSender { use SenderTrait; private $transport; public function dispatch() {} }\nfunction run(Sender $sender) {\n    $sender->transport;\n}\n";
    let uri = server.open_php(&file, text);

    let items = server.completion(&uri, 5, 13);
    let property_kind =
        serde_json::to_value(CompletionItemKind::PROPERTY).expect("property kind json");

    assert!(items.iter().any(|item| item["label"] == "dispatch"));
    assert!(
        items
            .iter()
            .any(|item| { item["label"] == "transport" && item["kind"] == property_kind })
    );
    assert!(
        items
            .iter()
            .any(|item| { item["label"] == "queue" && item["kind"] == property_kind })
    );
    assert!(
        items
            .iter()
            .any(|item| { item["label"] == "channel" && item["kind"] == property_kind })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_phpdoc_property_completions_and_hover() {
    let root = temp_project("completion-phpdoc-properties");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\n/**\n * @property string $status\n */\nclass InvoiceSender {}\nfunction run(InvoiceSender $sender) {\n    $sender->sta;\n    $sender->status;\n}\n";
    let uri = server.open_php(&file, text);

    let items = server.completion(&uri, 6, 16);
    let property_kind =
        serde_json::to_value(CompletionItemKind::PROPERTY).expect("property kind json");

    assert!(
        items
            .iter()
            .any(|item| { item["label"] == "status" && item["kind"] == property_kind })
    );

    let hover = server.hover(&uri, 7, 16).expect("hover result");
    let markdown = hover["contents"]["value"].as_str().expect("hover markdown");
    let definition = server.definition(&uri, 7, 16).expect("definition result");

    assert!(markdown.contains("property InvoiceSender::$status"));
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 2, "character": 20 })
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_this_property_completions() {
    let root = temp_project("completion-this-properties");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nclass Sender { private $transport; public function dispatch() {} public function run() {\n    $this->transport;\n} }\n";
    let uri = server.open_php(&file, text);

    let items = server.completion(&uri, 2, 11);
    let property_kind =
        serde_json::to_value(CompletionItemKind::PROPERTY).expect("property kind json");

    assert!(items.iter().any(|item| item["label"] == "dispatch"));
    assert!(
        items
            .iter()
            .any(|item| { item["label"] == "transport" && item["kind"] == property_kind })
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
    let text = "<?php\nnamespace App;\nconst API_VERSION = '1';\nfunction send_invoice($invoice) {}\nclass InvoiceSender { const DEFAULT_CHANNEL = 'mail'; private string $channel; public function dispatch($invoice) {} }\n";
    let uri = server.open_php(&file, text);

    let symbols = server.document_symbols(&uri);

    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["name"] == "send_invoice")
    );
    assert!(symbols.iter().any(|symbol| symbol["name"] == "API_VERSION"));
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
    assert!(
        class_symbol["children"]
            .as_array()
            .expect("class children")
            .iter()
            .any(|symbol| symbol["name"] == "DEFAULT_CHANNEL")
    );
    assert!(
        class_symbol["children"]
            .as_array()
            .expect("class children")
            .iter()
            .any(|symbol| symbol["name"] == "$channel")
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
        "<?php\nnamespace App;\nconst API_VERSION = '1';\nfunction send_invoice($invoice) {}\nclass InvoiceSender { public function dispatch($invoice) {} }\n",
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
    let symbols = server.workspace_symbols("API");

    assert!(
        symbols
            .iter()
            .any(|symbol| symbol["name"] == "App\\API_VERSION")
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
fn lsp_returns_workspace_references_for_constant_name() {
    let root = temp_project("constant-references");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).expect("create source dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("write composer");
    let constants_path = src_dir.join("constants.php");
    std::fs::write(
        &constants_path,
        "<?php\nnamespace App;\nconst API_VERSION = '1';\n",
    )
    .expect("write constants");
    let caller_path = src_dir.join("Caller.php");
    let mut server = LspProcess::start(&root);
    let caller_uri = server.open_php(
        &caller_path,
        "<?php\nnamespace App;\necho API_VERSION;\necho API_VERSION;\n",
    );

    let references = server.references(&caller_uri, 2, 7, true);

    assert_eq!(references.len(), 3);
    assert!(
        references
            .iter()
            .any(|reference| reference["uri"] == file_uri(&constants_path))
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
        "<?php\nconst API_VERSION = '1';\nfunction send_invoice($invoice) {}\necho API_VERSION;\nsend_invoice($invoice);\n",
    );

    let lenses = server.code_lens(&uri);

    assert!(
        lenses
            .iter()
            .filter(|lens| {
                lens["command"]["title"] == "1 reference"
                    && lens["command"]["command"] == "editor.action.showReferences"
            })
            .count()
            >= 2
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_implementation_code_lenses_for_class_like_declarations() {
    let root = temp_project("implementation-code-lens");
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).expect("create source dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("write composer");
    std::fs::write(
        src_dir.join("EmailSender.php"),
        "<?php\nnamespace App;\nclass EmailSender implements Sender { public function dispatch($invoice) {} }\n",
    )
    .expect("write implementation");
    let mut server = LspProcess::start(&root);
    let contract_path = src_dir.join("Sender.php");
    let contract_uri = server.open_php(
        &contract_path,
        "<?php\nnamespace App;\ninterface Sender { public function dispatch($invoice); }\n",
    );

    let lenses = server.code_lens(&contract_uri);

    assert_eq!(
        lenses
            .iter()
            .filter(|lens| {
                lens["command"]["title"] == "1 implementation"
                    && lens["command"]["command"] == "editor.action.showReferences"
            })
            .count(),
        2
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_usage_code_lenses_for_trait_declarations() {
    let root = temp_project("trait-usage-code-lens");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\ntrait Dispatchable {}\nclass EmailSender { use Dispatchable; }\n",
    );

    let lenses = server.code_lens(&uri);

    assert!(lenses.iter().any(|lens| {
        lens["command"]["title"] == "1 usage"
            && lens["command"]["command"] == "editor.action.showReferences"
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_parent_code_lenses_for_method_implementations() {
    let root = temp_project("method-parent-code-lens");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\ninterface Sender { public function dispatch($invoice); }\nclass EmailSender implements Sender { public function dispatch($invoice) {} }\n",
    );

    let lenses = server.code_lens(&uri);

    assert!(lenses.iter().any(|lens| {
        lens["command"]["title"] == "1 parent"
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
fn lsp_publishes_semantic_diagnostics_for_unresolved_phpdoc_types() {
    let root = temp_project("phpdoc-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass ExistingCustomer {}\n/**\n * @param MissingCustomer $customer\n * @return ExistingCustomer\n */\nfunction handle($customer) { return $customer; }\n",
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
            .contains("unresolved PHPDoc type MissingCustomer")
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
fn lsp_publishes_semantic_diagnostics_for_unresolved_phpdoc_throws_types() {
    let root = temp_project("phpdoc-throws-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass ExistingException extends \\Exception {}\n/**\n * @throws MissingException\n * @throws ExistingException\n */\nfunction handle() {}\n",
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
            .contains("unresolved PHPDoc type MissingException")
    }));
    assert!(!diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("ExistingException")
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_unresolved_class_phpdoc_types() {
    let root = temp_project("class-phpdoc-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass ExistingCustomer {}\n/**\n * @property MissingCustomer $customer\n * @mixin MissingMixin\n * @property-read ExistingCustomer $existing\n */\nclass Sender {}\n",
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
            .contains("unresolved PHPDoc type MissingCustomer")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unresolved PHPDoc type MissingMixin")
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
fn lsp_publishes_semantic_diagnostics_for_unresolved_phpdoc_method_types() {
    let root = temp_project("phpdoc-method-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass ExistingCustomer {}\n/** @method MissingReturn make(MissingParameter $customer, ExistingCustomer $existing) */\nclass Sender {}\n",
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
            .contains("unresolved PHPDoc type MissingReturn")
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unresolved PHPDoc type MissingParameter")
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
fn lsp_publishes_semantic_diagnostics_for_unresolved_phpdoc_var_types() {
    let root = temp_project("phpdoc-var-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass ExistingCustomer {}\nfunction handle() {\n    /** @var MissingCustomer $customer */\n    $customer = null;\n    /** @var ExistingCustomer */\n    $existing = new ExistingCustomer();\n}\n",
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
            .contains("unresolved PHPDoc type MissingCustomer")
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
fn lsp_publishes_semantic_diagnostics_for_duplicate_methods() {
    let root = temp_project("duplicate-method-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Sender { public function dispatch() {} public function dispatch() {} }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "duplicate method declaration App\\Sender::dispatch"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_duplicate_properties() {
    let root = temp_project("duplicate-property-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Sender { private $customer; protected $customer; }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "duplicate property declaration App\\Sender::$customer"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_duplicate_class_constants() {
    let root = temp_project("duplicate-class-constant-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Sender { const CHANNEL = 'mail'; const CHANNEL = 'sms'; }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "duplicate class constant declaration App\\Sender::CHANNEL"
            && diagnostic["severity"] == 1
    }));
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
fn lsp_publishes_semantic_diagnostics_for_unused_function_imports() {
    let root = temp_project("unused-function-import-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nuse function Domain\\send_invoice;\nuse function Domain\\unused_helper;\nsend_invoice();\n",
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
            .contains("unused import unused_helper")
            && diagnostic["severity"] == 2
    }));
    assert!(!diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unused import send_invoice")
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
fn lsp_publishes_semantic_diagnostics_for_phpdoc_relative_return_type_mismatch() {
    let root = temp_project("phpdoc-relative-return-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass BaseSender {}\nclass Invoice {}\nclass Sender extends BaseSender {\n    /** @return self */\n    public function make() { return new Invoice(); }\n    /** @return parent */\n    public function base() { return new Invoice(); }\n}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "return type mismatch: declared App\\Sender, returned App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "return type mismatch: declared App\\BaseSender, returned App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_uses_nullable_relative_phpdoc_types_for_diagnostics() {
    let root = temp_project("nullable-relative-phpdoc-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass BaseSender {}\nclass Invoice {}\n/** @property self|null $child */\nclass Sender extends BaseSender {\n    /** @param self|null $sender\n     * @return parent|null\n     */\n    public function pick($sender) { return new Invoice(); }\n    public function run() {\n        $this->child = null;\n        $this->child = new Invoice();\n        $this->pick(null);\n        $this->pick(new Invoice());\n        /** @var parent|null $base */\n        $base = new Invoice();\n    }\n}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "return type mismatch: declared App\\BaseSender, returned App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $this->child: expected App\\Sender, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for sender: expected App\\Sender, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $base: expected App\\BaseSender, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(!diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $this->child: expected App\\Sender, got null"
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_allows_phpdoc_generic_array_return_type_diagnostics() {
    let root = temp_project("phpdoc-generic-array-return-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\n/** @return array<int,Customer> */\nfunction customers() { return []; }\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic["message"]
            != "return type mismatch: declared App\\array<int,Customer>, returned array"
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
fn lsp_publishes_semantic_diagnostics_for_internal_method_argument_type_mismatch() {
    let root = temp_project("internal-method-argument-type-mismatch");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\n$date = new DateTimeImmutable('now');\n$date->format(123);\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for format: expected string, got int"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_internal_constructor_argument_type_mismatch() {
    let root = temp_project("internal-constructor-argument-type-mismatch");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(&file, "<?php\nnew DateTimeImmutable([]);\n");

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "argument type mismatch for datetime: expected string, got array"
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
fn lsp_publishes_semantic_diagnostics_for_constructor_argument_type_mismatch() {
    let root = temp_project("constructor-argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nclass Sender { public function __construct(Customer $customer) {} }\nnew Sender(new Invoice());\n",
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
fn lsp_publishes_semantic_diagnostics_for_self_parameter_argument_type_mismatch() {
    let root = temp_project("self-parameter-argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Invoice {}\nclass Sender { public function accept(self $sender) {} }\n$sender = new Sender();\n$sender->accept(new Invoice());\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for sender: expected App\\Sender, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_phpdoc_self_parameter_argument_type_mismatch() {
    let root = temp_project("phpdoc-self-parameter-argument-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Invoice {}\nclass Sender { /** @param self $sender */ public function accept($sender) {} }\n$sender = new Sender();\n$sender->accept(new Invoice());\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for sender: expected App\\Sender, got App\\Invoice"
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
fn lsp_allows_phpdoc_list_parameter_argument_diagnostics() {
    let root = temp_project("phpdoc-list-parameter-argument-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\n/** @param list<Customer> $customers */\nfunction send($customers) {}\nsend([]);\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic["message"]
            != "argument type mismatch for customers: expected App\\list<Customer>, got array"
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
fn lsp_uses_phpdoc_var_self_static_parent_types_for_diagnostics() {
    let root = temp_project("phpdoc-var-relative-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass BaseSender {}\nclass Invoice {}\nclass Sender extends BaseSender {\n    public function set() {\n        /** @var self $sender */\n        $sender = new Invoice();\n        /** @var parent */\n        $base = new Invoice();\n    }\n}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $sender: expected App\\Sender, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $base: expected App\\BaseSender, got App\\Invoice"
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
fn lsp_publishes_semantic_diagnostics_for_property_assignment_type_mismatch() {
    let root = temp_project("property-assignment-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\nclass Sender {\n    private Customer $customer;\n    public function set() { $this->customer = new Invoice(); }\n}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $this->customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_phpdoc_property_assignment_type_mismatch() {
    let root = temp_project("phpdoc-property-assignment-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\n/** @property Customer $customer */\nclass Sender {\n    public function set() { $this->customer = new Invoice(); }\n}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $this->customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_phpdoc_readonly_property_assignment() {
    let root = temp_project("phpdoc-readonly-property-assignment-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\n/** @property-read Customer $customer */\nclass Sender {\n    public function set(Customer $customer) { $this->customer = $customer; }\n}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"] == "assignment to read-only PHPDoc property $this->customer"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_publishes_semantic_diagnostics_for_phpdoc_write_property_assignment_type_mismatch() {
    let root = temp_project("phpdoc-write-property-assignment-type-mismatch-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\n/** @property-write Customer $customer */\nclass Sender {\n    public function set() { $this->customer = new Invoice(); }\n}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $this->customer: expected App\\Customer, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_uses_phpdoc_property_self_static_parent_types_for_diagnostics() {
    let root = temp_project("phpdoc-property-relative-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass BaseSender {}\nclass Invoice {}\n/**\n * @property self $child\n * @property-write parent $base\n */\nclass Sender extends BaseSender {\n    public function set() {\n        $this->child = new Invoice();\n        $this->base = new Invoice();\n    }\n}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $this->child: expected App\\Sender, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "assignment type mismatch for $this->base: expected App\\BaseSender, got App\\Invoice"
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
fn lsp_returns_document_highlights_for_php_keyword() {
    let root = temp_project("document-highlight-keyword");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nfunction pick($first) {\n    if ($first) { return 1; }\n    return 2;\n}\n",
    );

    let highlights = server.document_highlights(&uri, 2, 20);

    assert_eq!(highlights.len(), 2);
    assert!(highlights.iter().all(|highlight| highlight["kind"] == 1));
    assert!(
        highlights.iter().any(|highlight| {
            highlight["range"]["start"] == json!({ "line": 2, "character": 18 })
        })
    );
    assert!(
        highlights.iter().any(|highlight| {
            highlight["range"]["start"] == json!({ "line": 3, "character": 4 })
        })
    );
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
fn lsp_returns_folding_ranges_for_heredoc_and_custom_regions() {
    let root = temp_project("folding-range-heredoc-region");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\n$html = <<<HTML\n<div>\n</div>\nHTML;\n// #region helpers\nfunction send_invoice($invoice) {\n    return $invoice;\n}\n// #endregion\n",
    );

    let ranges = server.folding_ranges(&uri);

    assert!(
        ranges
            .iter()
            .any(|range| range["startLine"] == 1 && range["endLine"] == 4)
    );
    assert!(
        ranges
            .iter()
            .any(|range| range["startLine"] == 5 && range["endLine"] == 9)
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_folding_ranges_for_array_literals() {
    let root = temp_project("folding-range-array-literal");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\n$config = [\n    'invoice' => [\n        'enabled' => true,\n    ],\n];\n",
    );

    let ranges = server.folding_ranges(&uri);

    assert!(ranges.iter().any(|range| range["kind"] == "region"
        && range["startLine"] == 1
        && range["endLine"] == 5));
    assert!(ranges.iter().any(|range| range["kind"] == "region"
        && range["startLine"] == 2
        && range["endLine"] == 4));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_folding_ranges_for_match_expressions() {
    let root = temp_project("folding-range-match-expression");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\n$status = match ($code) {\n    200 => 'ok',\n    default => 'error',\n};\n",
    );

    let ranges = server.folding_ranges(&uri);

    assert!(ranges.iter().any(|range| range["kind"] == "region"
        && range["startLine"] == 1
        && range["endLine"] == 4));
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
        "<?php\nclass InvoiceSender {}\nfunction object_sender() { return new InvoiceSender(); }\nfunction make_sender(): InvoiceSender { return new InvoiceSender(); }\nfunction alias_sender() { return make_sender(); }\nfunction declared_sender(): InvoiceSender { return new InvoiceSender(); }\n",
    );

    let hints = server.inlay_hints(&uri, 0, 0, 6, 0);

    assert!(hints.iter().any(|hint| {
        hint["label"] == ": InvoiceSender"
            && hint["kind"] == 1
            && hint["position"] == json!({ "line": 2, "character": 24 })
    }));
    assert!(hints.iter().any(|hint| {
        hint["label"] == ": InvoiceSender"
            && hint["kind"] == 1
            && hint["position"] == json!({ "line": 4, "character": 23 })
    }));
    assert_eq!(
        hints
            .iter()
            .filter(|hint| hint["label"] == ": InvoiceSender")
            .count(),
        2
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_inferred_return_type_inlay_hints_for_internal_methods() {
    let root = temp_project("internal-method-return-type-inlay-hints");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nfunction formatted_date() {\n    $date = new DateTimeImmutable('now');\n    return $date->format('Y-m-d');\n}\n",
    );

    let hints = server.inlay_hints(&uri, 0, 0, 5, 0);

    assert!(
        hints
            .iter()
            .any(|hint| hint["label"] == ": string" && hint["kind"] == 1)
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_inferred_return_type_inlay_hints_for_anonymous_functions() {
    let root = temp_project("anonymous-return-type-inlay-hints");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nclass InvoiceSender {}\n$factory = function() { return new InvoiceSender(); };\n",
    );

    let hints = server.inlay_hints(&uri, 0, 0, 3, 0);

    assert!(
        hints.iter().any(|hint| {
            hint["label"] == ": InvoiceSender"
                && hint["kind"] == 1
                && hint["position"] == json!({ "line": 2, "character": 21 })
        }),
        "hints: {hints:#?}"
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_inferred_return_type_inlay_hints_for_arrow_functions() {
    let root = temp_project("arrow-return-type-inlay-hints");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nclass InvoiceSender {}\n$factory = fn() => new InvoiceSender();\n",
    );

    let hints = server.inlay_hints(&uri, 0, 0, 3, 0);

    assert!(hints.iter().any(|hint| {
        hint["label"] == ": InvoiceSender"
            && hint["kind"] == 1
            && hint["position"] == json!({ "line": 2, "character": 15 })
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_phpdoc_parameter_type_inlay_hints() {
    let root = temp_project("phpdoc-parameter-type-inlay-hints");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\n/** @param Customer $customer */\nfunction send($customer) { return $customer; }\n",
    );

    let hints = server.inlay_hints(&uri, 0, 0, 5, 0);

    assert!(hints.iter().any(|hint| {
        hint["label"] == ": App\\Customer"
            && hint["kind"] == 1
            && hint["position"] == json!({ "line": 4, "character": 23 })
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_phpdoc_parameter_type_inlay_hints_for_anonymous_functions() {
    let root = temp_project("phpdoc-anonymous-parameter-type-inlay-hints");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\n/** @param Customer $customer */\n$handler = function($customer) { return $customer; };\n",
    );

    let hints = server.inlay_hints(&uri, 0, 0, 5, 0);

    assert!(hints.iter().any(|hint| {
        hint["label"] == ": App\\Customer"
            && hint["kind"] == 1
            && hint["position"] == json!({ "line": 4, "character": 29 })
    }));
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
fn lsp_returns_document_links_for_dir_concatenated_require_paths() {
    let root = temp_project("document-links-dir");
    let mut server = LspProcess::start(&root);
    let lib_dir = root.join("lib");
    std::fs::create_dir_all(&lib_dir).expect("create lib dir");
    let target = lib_dir.join("helpers.php");
    std::fs::write(&target, "<?php\nfunction helper() {}\n").expect("write helper");
    let file = root.join("example.php");
    let uri = server.open_php(&file, "<?php\nrequire_once __DIR__ . '/lib/helpers.php';\n");

    let links = server.document_links(&uri);

    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["target"], file_uri(&target));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_document_links_for_dirname_file_require_paths() {
    let root = temp_project("document-links-dirname-file");
    let mut server = LspProcess::start(&root);
    let lib_dir = root.join("lib");
    std::fs::create_dir_all(&lib_dir).expect("create lib dir");
    let target = lib_dir.join("helpers.php");
    std::fs::write(&target, "<?php\nfunction helper() {}\n").expect("write helper");
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nrequire_once dirname(__FILE__) . '/lib/helpers.php';\n",
    );

    let links = server.document_links(&uri);

    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["target"], file_uri(&target));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_document_links_for_concatenated_literal_require_paths() {
    let root = temp_project("document-links-concatenated-literals");
    let mut server = LspProcess::start(&root);
    let lib_dir = root.join("lib");
    std::fs::create_dir_all(&lib_dir).expect("create lib dir");
    let target = lib_dir.join("helpers.php");
    std::fs::write(&target, "<?php\nfunction helper() {}\n").expect("write helper");
    let file = root.join("example.php");
    let uri = server.open_php(&file, "<?php\nrequire 'lib/' . 'helpers.php';\n");

    let links = server.document_links(&uri);

    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["target"], file_uri(&target));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_document_links_for_directory_separator_require_paths() {
    let root = temp_project("document-links-directory-separator");
    let mut server = LspProcess::start(&root);
    let lib_dir = root.join("lib");
    std::fs::create_dir_all(&lib_dir).expect("create lib dir");
    let target = lib_dir.join("helpers.php");
    std::fs::write(&target, "<?php\nfunction helper() {}\n").expect("write helper");
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nrequire __DIR__ . '/lib' . DIRECTORY_SEPARATOR . 'helpers.php';\n",
    );

    let links = server.document_links(&uri);

    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["target"], file_uri(&target));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_document_links_for_parent_dir_require_paths() {
    let root = temp_project("document-links-parent-dir");
    let mut server = LspProcess::start(&root);
    let app_dir = root.join("app");
    std::fs::create_dir_all(&app_dir).expect("create app dir");
    let shared_dir = root.join("shared");
    std::fs::create_dir_all(&shared_dir).expect("create shared dir");
    let target = shared_dir.join("helpers.php");
    std::fs::write(&target, "<?php\nfunction helper() {}\n").expect("write helper");
    let file = app_dir.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nrequire_once dirname(__DIR__) . '/shared/helpers.php';\n",
    );

    let links = server.document_links(&uri);

    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["target"], file_uri(&target));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_document_links_for_dirname_levels_require_paths() {
    let root = temp_project("document-links-dirname-levels");
    let mut server = LspProcess::start(&root);
    let app_dir = root.join("app").join("Http");
    std::fs::create_dir_all(&app_dir).expect("create app dir");
    let shared_dir = root.join("shared");
    std::fs::create_dir_all(&shared_dir).expect("create shared dir");
    let target = shared_dir.join("helpers.php");
    std::fs::write(&target, "<?php\nfunction helper() {}\n").expect("write helper");
    let file = app_dir.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nrequire_once dirname(__FILE__, 3) . '/shared/helpers.php';\n",
    );

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
fn lsp_returns_import_refactor_for_fully_qualified_constant_name() {
    let root = temp_project("const-import-refactor");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnamespace App\\Http;\necho \\App\\Config\\API_VERSION;\nnamespace App\\Config;\nconst API_VERSION = '1';\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 2, 20);

    assert!(actions.iter().any(|action| {
        action["title"] == "[Rephactor] Add import for 'App\\Config\\API_VERSION'"
    }));
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_returns_import_refactor_for_fully_qualified_function_name() {
    let root = temp_project("function-import-refactor");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let text = "<?php\nnamespace App\\Http;\n\\App\\Support\\send_invoice($invoice);\nnamespace App\\Support;\nfunction send_invoice($invoice) {}\n";
    let uri = server.open_php(&file, text);

    let actions = server.code_actions(&uri, 2, 20);

    assert!(actions.iter().any(|action| {
        action["title"] == "[Rephactor] Add import for 'App\\Support\\send_invoice'"
    }));
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
fn lsp_resolves_composer_autoload_files_functions() {
    let root = temp_project("autoload-files");
    let app_dir = root.join("app");
    std::fs::create_dir_all(&app_dir).expect("create app dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"autoload":{"files":["helpers.php"]}}"#,
    )
    .expect("write composer");
    std::fs::write(
        root.join("helpers.php"),
        "<?php\nfunction send_invoice($invoice, $notify) {}\n",
    )
    .expect("write helper file");
    let mut server = LspProcess::start(&root);
    let uri = server.open_php(
        &app_dir.join("Caller.php"),
        "<?php\nsend_invoice($invoice, true);\n",
    );

    let actions = server.code_actions(&uri, 1, 5);

    assert_eq!(actions.len(), 1);
    assert_eq!(
        insert_texts(&actions[0], &uri),
        vec!["invoice: ", "notify: "]
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_resolves_composer_autoload_dev_files_functions() {
    let root = temp_project("autoload-dev-files");
    let app_dir = root.join("app");
    let tests_dir = root.join("tests");
    std::fs::create_dir_all(&app_dir).expect("create app dir");
    std::fs::create_dir_all(&tests_dir).expect("create tests dir");
    std::fs::write(
        root.join("composer.json"),
        r#"{"autoload-dev":{"files":["tests/helpers.php"]}}"#,
    )
    .expect("write composer");
    std::fs::write(
        tests_dir.join("helpers.php"),
        "<?php\nfunction fake_invoice($invoice, $notify) {}\n",
    )
    .expect("write helper file");
    let mut server = LspProcess::start(&root);
    let uri = server.open_php(
        &app_dir.join("Caller.php"),
        "<?php\nfake_invoice($invoice, true);\n",
    );

    let actions = server.code_actions(&uri, 1, 5);

    assert_eq!(actions.len(), 1);
    assert_eq!(
        insert_texts(&actions[0], &uri),
        vec!["invoice: ", "notify: "]
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
fn lsp_uses_phpdoc_method_types_for_diagnostics() {
    let root = temp_project("phpdoc-method-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass Customer {}\nclass Invoice {}\n/** @method Customer make(Invoice $invoice) */\nclass Sender {}\nfunction takes_invoice(Invoice $invoice) {}\nfunction run(Sender $sender) {\n    $sender->make(new Customer());\n    takes_invoice($sender->make(new Invoice()));\n}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic["message"]
                    == "argument type mismatch for invoice: expected App\\Invoice, got App\\Customer"
                    && diagnostic["severity"] == 1
            })
            .count()
            >= 2
    );
    std::fs::remove_dir_all(root).expect("remove temp root");
}

#[test]
fn lsp_uses_phpdoc_method_self_static_parent_types_for_diagnostics() {
    let root = temp_project("phpdoc-method-relative-type-diagnostics");
    let mut server = LspProcess::start(&root);
    let file = root.join("example.php");
    let uri = server.open_php(
        &file,
        "<?php\nnamespace App;\nclass BaseSender {}\nclass Invoice {}\n/** @method self make(static $sender, parent $base) */\nclass Sender extends BaseSender {}\nfunction takes_invoice(Invoice $invoice) {}\nfunction run(Sender $sender, BaseSender $base) {\n    $sender->make(new Invoice(), new Invoice());\n    takes_invoice($sender->make($sender, $base));\n}\n",
    );

    let notification = server.read_notification("textDocument/publishDiagnostics");

    assert_eq!(notification["params"]["uri"], uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for sender: expected App\\Sender, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for base: expected App\\BaseSender, got App\\Invoice"
            && diagnostic["severity"] == 1
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic["message"]
            == "argument type mismatch for invoice: expected App\\Invoice, got App\\Sender"
            && diagnostic["severity"] == 1
    }));
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
