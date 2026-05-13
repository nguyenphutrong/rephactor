use std::collections::HashMap;

use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, Position,
    Range, TextDocumentContentChangeEvent, Url,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenDocument {
    pub text: String,
    pub version: i32,
    pub language_id: String,
}

#[derive(Debug, Default)]
pub struct DocumentStore {
    documents: HashMap<Url, OpenDocument>,
}

impl DocumentStore {
    pub fn open(&mut self, params: DidOpenTextDocumentParams) {
        let text_document = params.text_document;
        self.documents.insert(
            text_document.uri,
            OpenDocument {
                text: text_document.text,
                version: text_document.version,
                language_id: text_document.language_id,
            },
        );
    }

    pub fn change(&mut self, params: DidChangeTextDocumentParams) {
        let Some(document) = self.documents.get_mut(&params.text_document.uri) else {
            return;
        };

        for change in params.content_changes {
            document.apply_change(change);
        }

        document.version = params.text_document.version;
    }

    pub fn close(&mut self, params: DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri);
    }

    pub fn get(&self, uri: &Url) -> Option<&OpenDocument> {
        self.documents.get(uri)
    }

    pub fn texts(&self) -> HashMap<Url, String> {
        self.documents
            .iter()
            .map(|(uri, document)| (uri.clone(), document.text.clone()))
            .collect()
    }
}

impl OpenDocument {
    fn apply_change(&mut self, change: TextDocumentContentChangeEvent) {
        let Some(range) = change.range else {
            self.text = change.text;
            return;
        };

        let Some((start, end)) = byte_range_for_lsp_range(&self.text, range) else {
            return;
        };

        self.text.replace_range(start..end, &change.text);
    }
}

pub fn byte_range_for_lsp_range(text: &str, range: Range) -> Option<(usize, usize)> {
    let start = byte_offset_for_lsp_position(text, range.start)?;
    let end = byte_offset_for_lsp_position(text, range.end)?;

    if start <= end {
        Some((start, end))
    } else {
        None
    }
}

pub fn byte_offset_for_lsp_position(text: &str, position: Position) -> Option<usize> {
    let mut line = 0;
    let mut line_start = 0;

    while line < position.line {
        let newline_offset = text[line_start..].find('\n')?;
        line_start += newline_offset + 1;
        line += 1;
    }

    let mut utf16_units = 0;

    for (relative_byte, character) in text[line_start..].char_indices() {
        if character == '\n' {
            return (utf16_units == position.character).then_some(line_start + relative_byte);
        }

        if utf16_units == position.character {
            return Some(line_start + relative_byte);
        }

        utf16_units += character.len_utf16() as u32;

        if utf16_units > position.character {
            return None;
        }
    }

    (utf16_units == position.character).then_some(text.len())
}

pub fn lsp_position_for_byte_offset(text: &str, byte_offset: usize) -> Option<Position> {
    if byte_offset > text.len() || !text.is_char_boundary(byte_offset) {
        return None;
    }

    let mut line = 0;
    let mut character = 0;

    for (current_offset, ch) in text.char_indices() {
        if current_offset == byte_offset {
            return Some(Position { line, character });
        }

        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    (byte_offset == text.len()).then_some(Position { line, character })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{
        DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
        TextDocumentIdentifier, TextDocumentItem, VersionedTextDocumentIdentifier,
    };

    fn uri() -> Url {
        Url::parse("file:///tmp/example.php").expect("valid uri")
    }

    fn open_params(text: &str, version: i32) -> DidOpenTextDocumentParams {
        DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri(),
                language_id: "php".to_string(),
                version,
                text: text.to_string(),
            },
        }
    }

    fn change_params(
        version: i32,
        content_changes: Vec<TextDocumentContentChangeEvent>,
    ) -> DidChangeTextDocumentParams {
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri(),
                version,
            },
            content_changes,
        }
    }

    fn close_params() -> DidCloseTextDocumentParams {
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri() },
        }
    }

    fn full_change(text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: text.to_string(),
        }
    }

    fn ranged_change(
        start_line: u32,
        start_character: u32,
        end_line: u32,
        end_character: u32,
        text: &str,
    ) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: start_line,
                    character: start_character,
                },
                end: Position {
                    line: end_line,
                    character: end_character,
                },
            }),
            range_length: None,
            text: text.to_string(),
        }
    }

    #[test]
    fn open_stores_initial_document_state() {
        let mut store = DocumentStore::default();

        store.open(open_params("<?php echo $name;", 7));

        let document = store.get(&uri()).expect("document stored");
        assert_eq!(document.text, "<?php echo $name;");
        assert_eq!(document.version, 7);
        assert_eq!(document.language_id, "php");
    }

    #[test]
    fn full_change_replaces_document_text_and_version() {
        let mut store = DocumentStore::default();
        store.open(open_params("<?php echo $name;", 1));

        store.change(change_params(2, vec![full_change("<?php echo $other;")]));

        let document = store.get(&uri()).expect("document stored");
        assert_eq!(document.text, "<?php echo $other;");
        assert_eq!(document.version, 2);
    }

    #[test]
    fn ranged_change_replaces_only_requested_range() {
        let mut store = DocumentStore::default();
        store.open(open_params("<?php\nfoo($bar);\n", 1));

        store.change(change_params(2, vec![ranged_change(1, 4, 1, 8, "$baz")]));

        let document = store.get(&uri()).expect("document stored");
        assert_eq!(document.text, "<?php\nfoo($baz);\n");
        assert_eq!(document.version, 2);
    }

    #[test]
    fn multiple_changes_apply_in_order() {
        let mut store = DocumentStore::default();
        store.open(open_params("alpha beta", 1));

        store.change(change_params(
            2,
            vec![
                ranged_change(0, 0, 0, 5, "one"),
                ranged_change(0, 4, 0, 8, "two"),
            ],
        ));

        let document = store.get(&uri()).expect("document stored");
        assert_eq!(document.text, "one two");
        assert_eq!(document.version, 2);
    }

    #[test]
    fn ranged_change_uses_utf16_character_offsets() {
        let mut store = DocumentStore::default();
        store.open(open_params("<?php\n// 😀\nfoo($bar);\n", 1));

        store.change(change_params(2, vec![ranged_change(1, 3, 1, 5, "ok")]));

        let document = store.get(&uri()).expect("document stored");
        assert_eq!(document.text, "<?php\n// ok\nfoo($bar);\n");
        assert_eq!(document.version, 2);
    }

    #[test]
    fn close_removes_document_state() {
        let mut store = DocumentStore::default();
        store.open(open_params("<?php echo $name;", 1));

        store.close(close_params());

        assert!(store.get(&uri()).is_none());
    }

    #[test]
    fn byte_offset_round_trips_utf16_positions() {
        let text = "<?php\n// 😀\nfoo($bar);\n";
        let byte_offset = byte_offset_for_lsp_position(
            text,
            Position {
                line: 1,
                character: 5,
            },
        )
        .expect("offset");

        assert_eq!(&text[byte_offset..byte_offset + 1], "\n");
        assert_eq!(
            lsp_position_for_byte_offset(text, byte_offset),
            Some(Position {
                line: 1,
                character: 5
            })
        );
    }
}
