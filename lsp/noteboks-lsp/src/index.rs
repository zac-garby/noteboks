use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{Arc, Mutex},
};

use lsp_textdocument::FullTextDocument;
use tower_lsp::lsp_types::{
    TextDocumentContentChangeEvent, TextDocumentItem, Url, VersionedTextDocumentIdentifier,
};
use tree_sitter::{Parser, Tree};
use walkdir::WalkDir;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NoteKind {
    Note,
    Article,
    List,
    Index,
    Dump,
}

impl NoteKind {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("note") => Some(NoteKind::Note),
            Some("art") => Some(NoteKind::Article),
            Some("list") => Some(NoteKind::List),
            Some("index") => Some(NoteKind::Index),
            Some("dump") => Some(NoteKind::Dump),

            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NoteID {
    pub name: String,
    pub kind: NoteKind,
}

impl NoteID {
    pub fn from_uri(uri: &Url) -> Option<Self> {
        let path = uri.to_file_path().ok()?;
        NoteID::from_path(path.as_path())
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let file_stem = path.file_stem()?;
        let kind = NoteKind::from_path(path)?;

        Some(NoteID {
            name: String::from(file_stem.to_str()?),
            kind,
        })
    }
}

pub struct Index {
    root: Box<Path>,
    parser: Arc<Mutex<Parser>>,
    notes: BTreeMap<NoteID, Note>,
}

pub struct Note {
    pub document: Option<FullTextDocument>,
    tree: Option<Tree>,
}

impl Note {
    pub fn get_tree_and_doc(&self) -> Option<(&Tree, &FullTextDocument)> {
        self.tree
            .as_ref()
            .and_then(|tree| self.document.as_ref().map(|doc| (tree, doc)))
    }
}

impl Index {
    pub fn new(parser: Parser) -> Self {
        let root_path = Path::new("/Users/zacgarby/Documents/Vault");

        Self {
            root: Box::from(root_path),
            parser: Arc::new(Mutex::new(parser)),
            notes: BTreeMap::new(),
        }
    }

    pub fn scan(&mut self) {
        for entry in WalkDir::new(self.root.clone())
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            if let Some(note_name) = NoteID::from_path(entry.path()) {
                if let Some(_) = self.notes.get(&note_name) {
                } else {
                    let note = Note {
                        document: None,
                        tree: None,
                    };

                    println!("scanned note: {note_name:?}");

                    self.notes.insert(note_name, note);
                }
            }
        }
    }

    pub fn note_at_uri(&self, uri: &Url) -> Option<&Note> {
        let note_name = NoteID::from_uri(uri)?;
        self.notes.get(&note_name)
    }

    pub fn note_at_uri_mut(&mut self, uri: &Url) -> Option<&mut Note> {
        let note_name = NoteID::from_uri(uri)?;
        self.notes.get_mut(&note_name)
    }

    pub fn handle_open(&mut self, document: TextDocumentItem) -> bool {
        let doc = FullTextDocument::new(document.language_id, document.version, document.text);

        let uri = document.uri;

        if let Some(note_name) = NoteID::from_uri(&uri) {
            let note = Note {
                tree: None,
                document: Some(doc),
            };

            self.notes.insert(note_name, note);
            self.update_tree(&uri)
        } else {
            false
        }
    }

    pub fn handle_edit(
        &mut self,
        document: VersionedTextDocumentIdentifier,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> bool {
        let changes_: Vec<lsp_types::TextDocumentContentChangeEvent> =
            serde_json::from_value(serde_json::to_value(changes).unwrap()).unwrap();

        if let Some(note) = self.note_at_uri_mut(&document.uri) {
            if let Some(doc) = note.document.as_mut() {
                doc.update(&changes_, document.version);

                self.update_tree(&document.uri)
            } else {
                false
            }
        } else {
            false
        }
    }

    fn update_tree(&mut self, uri: &Url) -> bool {
        let new_tree = self.note_at_uri(uri).and_then(|note| {
            let mut parser = self.parser.lock().unwrap();
            note.document.as_ref().and_then(|doc| {
                let content = doc.get_content(None);
                parser.parse(content, None)
            })
        });

        if let Some(note) = self.note_at_uri_mut(uri) {
            note.tree = new_tree;
            true
        } else {
            false
        }
    }
}
