use std::{collections::BTreeMap, sync::{Arc, Mutex}};

use lsp_textdocument::FullTextDocument;
use tower_lsp::lsp_types::{TextDocumentContentChangeEvent, TextDocumentItem, Url, VersionedTextDocumentIdentifier};
use tree_sitter::{Parser, Tree};

pub struct Index {
    parser: Arc<Mutex<Parser>>,
    notes: BTreeMap<Url, Note>,
}

pub struct Note {
    pub document: FullTextDocument,
    tree: Option<Tree>,
}

impl Note {
    pub fn get_tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }
}

impl Index {
    pub fn new(parser: Parser) -> Self {
        Self{
            parser: Arc::new(Mutex::new(parser)),
            notes: BTreeMap::new(),
        }
    }

    pub fn get_note(&self, uri: &Url) -> Option<&Note> {
        self.notes.get(uri)
    }

    pub fn get_note_mut(&mut self, uri: &Url) -> Option<&mut Note> {
        self.notes.get_mut(uri)
    }

    pub fn handle_open(
        &mut self, document: TextDocumentItem
    ) -> bool {
        let doc = FullTextDocument::new(
            document.language_id,
            document.version,
            document.text,
        );

        self.notes.insert(document.uri.clone(), Note{
            tree: None,
            document: doc,
        });

        self.update_tree(&document.uri)
    }

    pub fn handle_edit(
        &mut self,
        document: VersionedTextDocumentIdentifier,
        changes: Vec<TextDocumentContentChangeEvent>
    ) -> bool {
        let changes_: Vec<lsp_types::TextDocumentContentChangeEvent> =
            serde_json::from_value(serde_json::to_value(changes).unwrap())
                .unwrap();

        if let Some(doc) = self.notes.get_mut(&document.uri) {
            doc.document.update(
                &changes_,
                document.version
            );

            self.update_tree(&document.uri)
        } else {
            false
        }
    }

    fn update_tree(
        &mut self,
        uri: &Url,
    ) -> bool {
        let new_tree = self.get_note(uri).and_then(|note| {
            let mut parser = self.parser.lock().unwrap();
            let content = note.document.get_content(None);
            parser.parse(content, None)
        });

        if let Some(note) = self.get_note_mut(uri) {
            note.tree = new_tree;
            true
        } else {
            false
        }
    }
}
