use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use lsp_textdocument::FullTextDocument;
use lsp_types::{Position, Range};
use regex::Regex;
use tower_lsp::lsp_types::{
    TextDocumentContentChangeEvent, TextDocumentItem, Url, VersionedTextDocumentIdentifier,
};
use tree_sitter::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor, Tree};
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
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "note" => Some(NoteKind::Note),
            "article" => Some(NoteKind::Article),
            "list" => Some(NoteKind::List),
            "index" => Some(NoteKind::Index),
            "dump" => Some(NoteKind::Dump),

            _ => None,
        }
    }

    pub fn to_str(&self) -> &str {
        match self {
            NoteKind::Note => "note",
            NoteKind::Article => "article",
            NoteKind::List => "list",
            NoteKind::Index => "index",
            NoteKind::Dump => "dump",
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        Self::from_str(path.extension()?.to_str()?)
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

    pub fn to_filename(&self) -> PathBuf {
        let str = format!("{}.{}", self.name, self.kind.to_str());
        return PathBuf::from(str);
    }

    pub fn from_link(link: &str) -> Option<Self> {
        let re = Regex::new(
            r"^(?<name>[_\-\?\:\/\\\w\d ]+)\s*(?<kind>(?:\((?:note|article|list|index|dump)\))?)$",
        )
        .unwrap();

        re.captures(link).map(|c| {
            let (_, [name, kind_str]) = c.extract();
            let kind =
                NoteKind::from_str(kind_str.trim_matches(&['(', ')'])).unwrap_or(NoteKind::Note);

            NoteID {
                name: String::from(name.trim()),
                kind,
            }
        })
    }
}

pub struct Index {
    pub root: Box<Path>,
    parser: Arc<Mutex<Parser>>,
    notes: BTreeMap<NoteID, Note>,
}

pub struct Note {
    pub id: NoteID,
    pub document: Option<FullTextDocument>,
    pub tree: Option<Tree>,
    pub outlinks: HashSet<NoteID>,
}

impl Note {
    pub fn new(id: NoteID) -> Self {
        Note {
            id,
            document: None,
            tree: None,
            outlinks: HashSet::new(),
        }
    }

    pub fn of_file(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;

        let document = FullTextDocument::new(
            String::from(tree_sitter_org::language().name()?),
            0,
            content,
        );

        Some(Note {
            document: Some(document),
            ..Note::new(NoteID::from_path(path)?)
        })
    }

    pub fn get_tree_and_doc(&self) -> Option<(&Tree, &FullTextDocument)> {
        self.tree
            .as_ref()
            .and_then(|tree| self.document.as_ref().map(|doc| (tree, doc)))
    }

    pub fn update_links(&mut self) {
        println!("updating links in {:?}", self.id);

        let mut new_links = Vec::new();

        if let Some((tree, doc)) = self.get_tree_and_doc() {
            let query = Query::new(
                &tree_sitter_org::language(),
                "(link uri: \"uri\" @uri) @link",
            )
            .expect("invalid query");

            let mut cur = QueryCursor::new();
            let mut matches =
                cur.matches(&query, tree.root_node(), doc.get_content(None).as_bytes());

            while let Some(m) = matches.next() {
                let uri_node = m.captures[1].node;
                let start = uri_node.start_position();
                let end = uri_node.end_position();

                let source = doc.get_content(Some(Range::new(
                    Position::new(start.row as u32, start.column as u32),
                    Position::new(end.row as u32, end.column as u32),
                )));

                if let Some(id) = NoteID::from_link(source) {
                    new_links.push(id);
                }
            }
        }

        self.outlinks.clear();
        new_links.iter().for_each(|id| {
            self.outlinks.insert(id.clone());
        });
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
            if let Some(note) = Note::of_file(entry.path()) {
                println!("scanned note: {:?}", note.id);
                let id = note.id.clone();
                self.notes.insert(id.clone(), note);
                self.update_tree(&id);
            }
        }
    }

    pub fn note_at_uri(&self, uri: &Url) -> Option<&Note> {
        let note_id = NoteID::from_uri(uri)?;
        self.notes.get(&note_id)
    }

    pub fn note_at_uri_mut(&mut self, uri: &Url) -> Option<&mut Note> {
        let note_id = NoteID::from_uri(uri)?;
        self.notes.get_mut(&note_id)
    }

    pub fn handle_open(&mut self, document: TextDocumentItem) -> bool {
        let doc = FullTextDocument::new(document.language_id, document.version, document.text);

        false
    }

    pub fn handle_edit(
        &mut self,
        document: VersionedTextDocumentIdentifier,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> bool {
        let changes_: Vec<lsp_types::TextDocumentContentChangeEvent> =
            serde_json::from_value(serde_json::to_value(changes).unwrap()).unwrap();

        if let Some(note) = self.note_at_uri_mut(&document.uri) {
            let id = note.id.clone();
            if let Some(doc) = note.document.as_mut() {
                doc.update(&changes_, document.version);
            }
            self.update_tree(&id)
        } else {
            false
        }
    }

    fn update_tree(&mut self, id: &NoteID) -> bool {
        let new_tree = self.notes.get(id).and_then(|note| {
            let mut parser = self.parser.lock().unwrap();
            note.document.as_ref().and_then(|doc| {
                let content = doc.get_content(None);
                parser.parse(content, None)
            })
        });

        if let Some(note) = self.notes.get_mut(id) {
            note.tree = new_tree;
            println!("got new tree for {id:?}");
            note.update_links();
            true
        } else {
            println!("failed to get new tree for {id:?}");
            false
        }
    }
}
