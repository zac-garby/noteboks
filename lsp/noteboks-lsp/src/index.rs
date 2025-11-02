use std::{collections::{BTreeMap, HashMap}, sync::{Arc, Mutex}};

use lsp_textdocument::FullTextDocument;
use tower_lsp::lsp_types::{TextDocumentContentChangeEvent, TextDocumentItem, Url, VersionedTextDocumentIdentifier};
use tree_sitter::{Parser, Tree};

type NoteName = String;

pub struct Index {
    parser: Arc<Mutex<Parser>>,
    notes: BTreeMap<NoteName, Note>
}

pub struct Note {
    pub document_uri: Url,
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
            notes: BTreeMap::new()
        }
    }

    pub fn note_at_uri(&self, uri: &Url) -> Option<&Note> {
        if let Some(note_name) = self.note_name_for_uri(uri) {
            self.notes.get(&note_name)
        } else {
            None
        }
    }

    pub fn note_at_uri_mut(&mut self, uri: &Url) -> Option<&mut Note> {
        if let Some(note_name) = self.note_name_for_uri(uri) {
            self.notes.get_mut(&note_name)
        } else {
            None
        }
    }

    pub fn note_name_for_uri(&self, uri: &Url) -> Option<NoteName> {
        uri.to_file_path().ok()?.file_name()?.to_str().map(|filename| {
            String::from(filename)
        })
    }

    pub fn handle_open(
        &mut self, document: TextDocumentItem
    ) -> bool {
        let doc = FullTextDocument::new(
            document.language_id,
            document.version,
            document.text,
        );

        let uri = document.uri;

        if let Some(note_name) = self.note_name_for_uri(&uri) {
            self.notes.insert(note_name, Note{
                document_uri: uri.clone(),
                tree: None,
                document: doc,
            });

            self.update_tree(&uri)
        } else {
            false
        }
    }

    pub fn handle_edit(
        &mut self,
        document: VersionedTextDocumentIdentifier,
        changes: Vec<TextDocumentContentChangeEvent>
    ) -> bool {
        let changes_: Vec<lsp_types::TextDocumentContentChangeEvent> =
            serde_json::from_value(serde_json::to_value(changes).unwrap())
                .unwrap();

        if let Some(doc) = self.note_at_uri_mut(&document.uri) {
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
        let new_tree = self.note_at_uri(uri).and_then(|note| {
            let mut parser = self.parser.lock().unwrap();
            let content = note.document.get_content(None);
            parser.parse(content, None)
        });

        if let Some(note) = self.note_at_uri_mut(uri) {
            note.tree = new_tree;
            true
        } else {
            false
        }
    }
}
