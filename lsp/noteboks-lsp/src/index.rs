use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use lsp_textdocument::FullTextDocument;
use regex::Regex;
use tower_lsp::lsp_types::{
    Position, Range, TextDocumentContentChangeEvent, TextDocumentItem, Url,
    VersionedTextDocumentIdentifier,
};

/// Normalise a note name for consistent lookup:
/// lowercase, collapse spaces/underscores/hyphens into a single hyphen.
pub fn normalize_name(s: &str) -> String {
    let lower = s.to_lowercase();
    // Replace runs of whitespace, hyphens, or underscores with a single '-'
    let re = Regex::new(r"[\s\-_]+").unwrap();
    re.replace_all(&lower, "-").trim_matches('-').to_string()
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NoteKind {
    /// A small self-contained note, Zettelkasten style, for one concept.
    Note,

    /// A reference to an external article, with some notes and maybe some
    /// internal links for context.
    Article,

    /// A list of things, perhaps a reading list or an ideas dump.
    List,

    /// An index page for a concept or topic.
    Index,

    /// Information about a person.
    Person,

    /// Unspecified kind — defaults to Note for resolution.
    Any,
}

impl NoteKind {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "note" => Some(NoteKind::Note),
            "article" => Some(NoteKind::Article),
            "list" => Some(NoteKind::List),
            "index" => Some(NoteKind::Index),
            "person" => Some(NoteKind::Person),
            _ => None,
        }
    }

    pub fn to_str(&self) -> &str {
        match self {
            NoteKind::Note => "note",
            NoteKind::Article => "article",
            NoteKind::List => "list",
            NoteKind::Index => "index",
            NoteKind::Person => "person",
            NoteKind::Any => "note",
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
            name: normalize_name(file_stem.to_str()?),
            kind,
        })
    }

    pub fn to_filename(&self) -> PathBuf {
        PathBuf::from(format!("{}.{}", self.name, self.kind.to_str()))
    }

    /// Parse a link target like "lambda calculus" or "modal logic (index)".
    pub fn from_link(link: &str) -> Option<Self> {
        let re = Regex::new(
            r"^(?<name>[_\-\?\:\/\\\w\d ]+)\s*(?<kind>(?:\((?:note|article|list|index|person)\))?)$",
        )
        .unwrap();

        re.captures(link).map(|c| {
            let (_, [name, kind_str]) = c.extract();
            // If no kind annotation is present, use Any so resolve_link can
            // search all extensions rather than assuming .note
            let kind = NoteKind::from_str(kind_str.trim_matches(&['(', ')']))
                .unwrap_or(NoteKind::Any);

            NoteID {
                name: normalize_name(name.trim()),
                kind,
            }
        })
    }
}

pub struct Note {
    pub id: NoteID,
    pub path: Option<PathBuf>,
    pub document: Option<FullTextDocument>,
    pub title: Option<String>,
    pub aliases: Vec<String>,
    pub outlinks: HashSet<NoteID>,
}

impl Note {
    pub fn new(id: NoteID) -> Self {
        Note {
            id,
            path: None,
            document: None,
            title: None,
            aliases: Vec::new(),
            outlinks: HashSet::new(),
        }
    }

    pub fn of_file(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        let document = FullTextDocument::new(String::from("noteboks"), 0, content);
        let mut note = Note {
            path: Some(path.to_path_buf()),
            document: Some(document),
            ..Note::new(NoteID::from_path(path)?)
        };
        note.update_links();
        Some(note)
    }

    /// Parse title and aliases out of a YAML front matter block.
    fn parse_front_matter(content: &str) -> (Option<String>, Vec<String>) {
        let content = content.trim_start();
        if !content.starts_with("---") {
            return (None, vec![]);
        }
        let after = match content.strip_prefix("---") {
            Some(s) => s.trim_start_matches('\n').trim_start_matches('\r'),
            None => return (None, vec![]),
        };
        let end = match after.find("\n---") {
            Some(i) => i,
            None => return (None, vec![]),
        };
        let fm = &after[..end];

        let mut title = None;
        let mut aliases = Vec::new();
        let mut in_aliases = false;

        for line in fm.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("title:") {
                title = Some(rest.trim().to_string());
                in_aliases = false;
            } else if let Some(rest) = trimmed.strip_prefix("aliases:") {
                in_aliases = false;
                let rest = rest.trim();
                if rest.starts_with('[') && rest.ends_with(']') {
                    for a in rest[1..rest.len() - 1].split(',') {
                        let a = a.trim().trim_matches('"').trim_matches('\'');
                        if !a.is_empty() {
                            aliases.push(a.to_string());
                        }
                    }
                } else if rest.is_empty() {
                    in_aliases = true;
                }
            } else if in_aliases {
                if let Some(rest) = trimmed.strip_prefix("- ") {
                    aliases.push(rest.trim().trim_matches('"').trim_matches('\'').to_string());
                } else if !trimmed.is_empty() {
                    in_aliases = false;
                }
            }
        }

        (title, aliases)
    }

    pub fn update_links(&mut self) {
        let content = match self.document.as_ref() {
            Some(doc) => doc.get_content(None).to_string(),
            None => return,
        };

        let (title, aliases) = Self::parse_front_matter(&content);
        self.title = title;
        self.aliases = aliases;

        let re = Regex::new(r"\[\[([^\]]+)\]\]").unwrap();
        let mut new_links = Vec::new();

        let mut in_code_block = false;
        let mut in_front_matter = false;
        let mut fm_started = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // Track front matter (skip links inside it)
            if !fm_started && trimmed == "---" {
                in_front_matter = true;
                fm_started = true;
                continue;
            }
            if in_front_matter {
                if trimmed == "---" {
                    in_front_matter = false;
                }
                continue;
            }

            // Track fenced code blocks
            if trimmed.starts_with("```") {
                in_code_block = !in_code_block;
                continue;
            }
            if in_code_block {
                continue;
            }

            for cap in re.captures_iter(line) {
                let inner = cap.get(1).unwrap().as_str();
                // Skip external URLs
                if inner.starts_with("http://") || inner.starts_with("https://") {
                    continue;
                }
                if let Some(id) = NoteID::from_link(inner) {
                    new_links.push(id);
                }
            }
        }

        // Also collect #tag outlinks
        let tag_re = Regex::new(r"#([a-zA-Z][a-zA-Z0-9_\-]*)").unwrap();
        for line in content.lines() {
            for cap in tag_re.captures_iter(line) {
                let tag = cap.get(1).unwrap().as_str();
                new_links.push(NoteID { name: normalize_name(tag), kind: NoteKind::Any });
            }
        }

        self.outlinks.clear();
        for id in new_links {
            self.outlinks.insert(id);
        }
    }
}

/// Scan `content` for all `#tag` references (outside front matter and code blocks).
/// Returns `(tag_name_without_hash, lsp_range)` for each match.
pub fn scan_tags(content: &str) -> Vec<(String, Range)> {
    let re = Regex::new(r"#([a-zA-Z][a-zA-Z0-9_\-]*)").unwrap();
    let mut results = Vec::new();

    let mut in_code_block = false;
    let mut in_front_matter = false;
    let mut fm_started = false;

    for (row, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if !fm_started && trimmed == "---" { in_front_matter = true; fm_started = true; continue; }
        if in_front_matter { if trimmed == "---" { in_front_matter = false; } continue; }
        if trimmed.starts_with("```") { in_code_block = !in_code_block; continue; }
        if in_code_block { continue; }

        for cap in re.captures_iter(line) {
            let full = cap.get(0).unwrap();
            let name = cap.get(1).unwrap().as_str();
            results.push((name.to_string(), Range::new(
                Position::new(row as u32, full.start() as u32),
                Position::new(row as u32, full.end() as u32),
            )));
        }
    }

    results
}

/// Scan `content` for all `[[...]]` links (outside front matter and code blocks).
/// Returns `(raw_link_text, lsp_range)` for each match.
pub fn scan_links(content: &str) -> Vec<(String, Range)> {
    let re = Regex::new(r"\[\[([^\]]+)\]\]").unwrap();
    let mut results = Vec::new();

    let mut in_code_block = false;
    let mut in_front_matter = false;
    let mut fm_started = false;

    for (row, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if !fm_started && trimmed == "---" {
            in_front_matter = true;
            fm_started = true;
            continue;
        }
        if in_front_matter {
            if trimmed == "---" {
                in_front_matter = false;
            }
            continue;
        }
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        for cap in re.captures_iter(line) {
            let full = cap.get(0).unwrap();
            let inner = cap.get(1).unwrap().as_str();
            if inner.starts_with("http://") || inner.starts_with("https://") {
                continue;
            }
            let range = Range::new(
                Position::new(row as u32, full.start() as u32),
                Position::new(row as u32, full.end() as u32),
            );
            results.push((inner.to_string(), range));
        }
    }

    results
}

pub struct Index {
    pub root: Box<Path>,
    pub notes: BTreeMap<NoteID, Note>,
}

impl Index {
    pub fn new(root: &Path) -> Self {
        Self {
            root: Box::from(root),
            notes: BTreeMap::new(),
        }
    }

    pub fn note_at_uri(&self, uri: &Url) -> Option<&Note> {
        let id = NoteID::from_uri(uri)?;
        self.notes.get(&id)
    }

    pub fn note_at_uri_mut(&mut self, uri: &Url) -> Option<&mut Note> {
        let id = NoteID::from_uri(uri)?;
        self.notes.get_mut(&id)
    }

    /// Resolve a link ID to a note, handling Any-kind fallback and aliases.
    ///
    /// Resolution order:
    ///   1. Exact name + kind match (skipped when kind is Any)
    ///   2. For Any: try Index, Note, Article, List, Person in that order
    ///   3. Alias search (normalised), respecting kind constraint if present
    pub fn resolve_link(&self, id: &NoteID) -> Option<&Note> {
        if id.kind != NoteKind::Any {
            if let Some(note) = self.notes.get(id) {
                return Some(note);
            }
        } else {
            for kind in &[
                NoteKind::Index,
                NoteKind::Note,
                NoteKind::Article,
                NoteKind::List,
                NoteKind::Person,
            ] {
                let candidate = NoteID { name: id.name.clone(), kind: kind.clone() };
                if let Some(note) = self.notes.get(&candidate) {
                    return Some(note);
                }
            }
        }

        // Alias fallback
        let want_kind_any = id.kind == NoteKind::Any;
        for note in self.notes.values() {
            if !want_kind_any && note.id.kind != id.kind {
                continue;
            }
            if note.aliases.iter().any(|a| normalize_name(a) == id.name) {
                return Some(note);
            }
        }

        None
    }

    #[allow(dead_code)]
    pub fn handle_open(&mut self, document: TextDocumentItem) {
        let uri = document.uri.clone();
        let doc = FullTextDocument::new(document.language_id, document.version, document.text);
        if let Some(id) = NoteID::from_uri(&uri) {
            let note = self.notes.entry(id.clone()).or_insert_with(|| Note::new(id));
            note.document = Some(doc);
            note.update_links();
        }
    }

    pub fn handle_edit(
        &mut self,
        document: VersionedTextDocumentIdentifier,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) {
        let changes_: Vec<lsp_types::TextDocumentContentChangeEvent> =
            serde_json::from_value(serde_json::to_value(changes).unwrap()).unwrap();

        if let Some(note) = self.note_at_uri_mut(&document.uri) {
            if let Some(doc) = note.document.as_mut() {
                doc.update(&changes_, document.version);
            }
            note.update_links();
        }
    }
}
