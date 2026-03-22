mod index;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use regex::Regex;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::lsp_types::{notification::Progress, request::WorkDoneProgressCreate};
use tower_lsp::{Client, LanguageServer, LspService, Server};
use walkdir::WalkDir;

use crate::index::{scan_links, scan_tags, Index, Note, NoteID, NoteKind};

struct Backend {
    client: Client,
    index: Arc<Mutex<Index>>,
    /// Set to `true` the first time a scan is kicked off, so we never scan twice.
    scan_started: Arc<AtomicBool>,
}

impl Backend {
    /// Push diagnostics for every indexed note to the client.
    async fn publish_all_diagnostics(&self) {
        let uris: Vec<Url> = self
            .index
            .lock()
            .await
            .notes
            .values()
            .filter_map(|n| n.path.as_ref().and_then(|p| Url::from_file_path(p).ok()))
            .collect();
        for uri in uris {
            self.publish_diagnostics(uri).await;
        }
    }

    /// Push diagnostics for a single document to the client.
    async fn publish_diagnostics(&self, _uri: Url) {
        // let index = self.index.lock().await;
        // let diagnostics = if let Some(note) = index.note_at_uri(&uri) {
        //     if let Some(doc) = &note.document {
        //         let content = doc.get_content(None);
        //         scan_links(content)
        //             .into_iter()
        //             .filter_map(|(text, range)| {
        //                 let id = NoteID::from_link(&text)?;
        //                 if index.resolve_link(&id).is_some() {
        //                     return None; // link is fine
        //                 }
        //                 Some(Diagnostic {
        //                     range,
        //                     severity: Some(DiagnosticSeverity::HINT),
        //                     message: format!("No note found for [[{}]]", text),
        //                     source: Some("noteboks".to_string()),
        //                     ..Default::default()
        //                 })
        //             })
        //             .collect()
        //     } else {
        //         vec![]
        //     }
        // } else {
        //     vec![]
        // };

        // self.client
        //     .publish_diagnostics(uri, diagnostics, None)
        //     .await;
    }

    /// Kick off a background vault scan, but only if one hasn't already started.
    fn trigger_scan(&self) {
        // swap returns the *old* value; if it was already true, someone else started.
        if self.scan_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let client = self.client.clone();
        let index = self.index.clone();

        tokio::spawn(async move {
            let root = index.lock().await.root.to_path_buf();

            // Collect all recognisable note file paths up front.
            let paths: Vec<PathBuf> = WalkDir::new(&root)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .map(|e| e.path().to_path_buf())
                .filter(|p| NoteKind::from_path(p).is_some())
                .collect();

            let total = paths.len();
            if total == 0 {
                return;
            }

            let token = NumberOrString::String("noteboks/indexing".to_string());

            // Ask the client to create a progress indicator.
            let _ = client
                .send_request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
                    token: token.clone(),
                })
                .await;

            // Begin.
            client
                .send_notification::<Progress>(ProgressParams {
                    token: token.clone(),
                    value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                        WorkDoneProgressBegin {
                            title: "Noteboks".to_string(),
                            cancellable: Some(false),
                            message: Some(format!("Indexing {} files…", total)),
                            percentage: Some(0),
                        },
                    )),
                })
                .await;

            // Read every file (pure I/O, no lock needed).
            let mut notes = Vec::with_capacity(total);
            for (i, path) in paths.iter().enumerate() {
                if let Some(note) = Note::of_file(path) {
                    notes.push(note);
                }

                // Send a progress report every 25 files and on the last file.
                if (i + 1) % 25 == 0 || i + 1 == total {
                    let pct = ((i + 1) * 100 / total) as u32;
                    client
                        .send_notification::<Progress>(ProgressParams {
                            token: token.clone(),
                            value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                                WorkDoneProgressReport {
                                    cancellable: Some(false),
                                    message: Some(format!("{}/{}", i + 1, total)),
                                    percentage: Some(pct),
                                },
                            )),
                        })
                        .await;
                }
            }

            // Insert all notes into the index in one lock acquisition.
            let indexed = notes.len();
            {
                let mut idx = index.lock().await;
                for note in notes {
                    idx.notes.insert(note.id.clone(), note);
                }
            }

            // Done.
            client
                .send_notification::<Progress>(ProgressParams {
                    token,
                    value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(
                        WorkDoneProgressEnd {
                            message: Some(format!("{} notes indexed", indexed)),
                        },
                    )),
                })
                .await;
        });
    }
}

/// Find the `#tag` name at a given (line, col) position in content.
/// Returns the tag name without the leading `#`.
fn find_tag_at(content: &str, line: u32, col: u32) -> Option<String> {
    let line_text = content.lines().nth(line as usize)?;
    let re = Regex::new(r"#([a-zA-Z][a-zA-Z0-9_\-]*)").unwrap();
    for cap in re.captures_iter(line_text) {
        let full = cap.get(0).unwrap();
        if col as usize >= full.start() && (col as usize) < full.end() {
            return Some(cap.get(1).unwrap().as_str().to_string());
        }
    }
    None
}

/// Find the `[[link target]]` text at a given (line, col) position in content.
fn find_link_at(content: &str, line: u32, col: u32) -> Option<String> {
    let line_text = content.lines().nth(line as usize)?;
    let re = Regex::new(r"\[\[([^\]]+)\]\]").unwrap();
    for cap in re.captures_iter(line_text) {
        let full = cap.get(0).unwrap();
        if col as usize >= full.start() && (col as usize) < full.end() {
            return Some(cap.get(1).unwrap().as_str().to_string());
        }
    }
    None
}

/// Extract the partial link text being typed after `[[` on a line, up to `col`.
/// Returns `(partial_text, partial_start_col)` where `partial_start_col` is the
/// column index of the first character after `[[`.  Returns `None` if the cursor
/// is not inside an open `[[`.
fn partial_link_at(line_text: &str, col: usize) -> Option<(String, usize)> {
    let prefix = &line_text[..col.min(line_text.len())];
    // Find the last `[[` before the cursor that isn't closed by `]]`
    let open = prefix.rfind("[[")?;
    let after_open = &prefix[open + 2..];
    // If there's a `]]` after the `[[` opening, we're outside any link
    if after_open.contains("]]") {
        return None;
    }
    Some((after_open.to_string(), open + 2))
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let root = params
            .root_uri
            .as_ref()
            .and_then(|u| u.to_file_path().ok())
            .or_else(|| {
                params.workspace_folders.as_ref().and_then(|folders| {
                    folders.first().and_then(|f| f.uri.to_file_path().ok())
                })
            });

        if let Some(root_path) = root {
            let mut index = self.index.lock().await;
            *index = Index::new(&root_path);
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["[".to_string()]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.trigger_scan();
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let pos = params.text_document_position_params;
        let uri = pos.text_document.uri;
        let line = pos.position.line;
        let col = pos.position.character;

        let index = self.index.lock().await;

        let content = match index.note_at_uri(&uri).and_then(|n| n.document.as_ref()) {
            Some(doc) => doc.get_content(None).to_string(),
            None => return Ok(None),
        };

        let id = if let Some(link_target) = find_link_at(&content, line, col) {
            NoteID::from_link(&link_target)
        } else if let Some(tag) = find_tag_at(&content, line, col) {
            Some(NoteID { name: crate::index::normalize_name(&tag), kind: NoteKind::Any })
        } else {
            None
        };

        if let Some(id) = id {
            let hover_text = if let Some(linked_note) = index.resolve_link(&id) {
                let title = linked_note.title.as_deref().unwrap_or(linked_note.id.name.as_str());
                format!("→ **{}** ({})", title, linked_note.id.kind.to_str())
            } else {
                format!("→ {} (new note)", id.name)
            };
            return Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: hover_text,
                }),
                range: None,
            }));
        }

        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let pos = params.text_document_position_params;
        let uri = pos.text_document.uri;
        let line = pos.position.line;
        let col = pos.position.character;

        let index = self.index.lock().await;

        // Extract content before taking any further borrows
        let content = match index.note_at_uri(&uri).and_then(|n| n.document.as_ref()) {
            Some(doc) => doc.get_content(None).to_string(),
            None => return Ok(None),
        };

        let id = if let Some(link_target) = find_link_at(&content, line, col) {
            match NoteID::from_link(&link_target) {
                Some(id) => id,
                None => return Ok(None),
            }
        } else if let Some(tag) = find_tag_at(&content, line, col) {
            NoteID { name: crate::index::normalize_name(&tag), kind: NoteKind::Any }
        } else {
            return Ok(None);
        };

        // Resolve existing note — clone path out to drop the borrow
        if let Some(path) = index.resolve_link(&id).and_then(|n| n.path.clone()) {
            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri: Url::from_file_path(&path).unwrap(),
                range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            })));
        }

        // Note doesn't exist — point to where it would be and let the editor create it
        let kind = if id.kind == NoteKind::Any { NoteKind::Note } else { id.kind.clone() };
        let new_id = NoteID { name: id.name.clone(), kind };
        let path = index.root.join(new_id.to_filename());

        Ok(Some(GotoDefinitionResponse::Scalar(Location {
            uri: Url::from_file_path(&path).unwrap(),
            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
        })))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let index = self.index.lock().await;

        let target_id = match NoteID::from_uri(&uri) {
            Some(id) => id,
            None => return Ok(None),
        };

        // All normalised names this note can be reached by (its own name + all aliases)
        let target_note = index.notes.get(&target_id);
        let mut target_names: Vec<String> = vec![target_id.name.clone()];
        if let Some(note) = target_note {
            for alias in &note.aliases {
                target_names.push(crate::index::normalize_name(alias));
            }
        }

        let mut locations = Vec::new();

        for (_, note) in &index.notes {
            // Skip the note itself
            if note.id == target_id {
                continue;
            }
            // Quick pre-filter: does this note link to any of our target names?
            let has_link = note.outlinks.iter().any(|id| target_names.contains(&id.name));
            if !has_link {
                continue;
            }
            // Re-scan to get precise positions
            let content = match note.document.as_ref() {
                Some(doc) => doc.get_content(None).to_string(),
                None => continue,
            };
            let note_path = match &note.path {
                Some(p) => p.clone(),
                None => continue,
            };
            for (text, range) in scan_links(&content) {
                if let Some(id) = NoteID::from_link(&text) {
                    if target_names.contains(&id.name) {
                        locations.push(Location {
                            uri: Url::from_file_path(&note_path).unwrap(),
                            range,
                        });
                    }
                }
            }
            for (tag, range) in scan_tags(&content) {
                let norm = crate::index::normalize_name(&tag);
                if target_names.contains(&norm) {
                    locations.push(Location {
                        uri: Url::from_file_path(&note_path).unwrap(),
                        range,
                    });
                }
            }
        }

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let line = params.text_document_position.position.line;
        let col = params.text_document_position.position.character as usize;

        let index = self.index.lock().await;

        // Find the partial link text the user has typed after `[[`
        let line_text = if let Some(note) = index.note_at_uri(&uri) {
            if let Some(doc) = &note.document {
                doc.get_content(None)
                    .lines()
                    .nth(line as usize)
                    .unwrap_or("")
                    .to_string()
            } else {
                return Ok(None);
            }
        } else {
            return Ok(None);
        };

        let (partial, partial_start) = match partial_link_at(&line_text, col) {
            Some(p) => (p.0.to_lowercase(), p.1),
            None => return Ok(None),
        };

        // Normalise the partial so spaces and hyphens are equivalent when matching
        // against the hyphenated note name (e.g. "what is" matches "what-is-a-compiler").
        let partial_norm = partial.replace(' ', "-");

        // The range to replace when a completion is accepted: from right after `[[`
        // to the cursor.  This ensures the whole partially-typed text is replaced.
        let replace_range = Range::new(
            Position::new(line, partial_start as u32),
            Position::new(line, col as u32),
        );

        let mut items: Vec<CompletionItem> = Vec::new();

        for (id, note) in &index.notes {
            let display_name = note.title.as_deref().unwrap_or(id.name.as_str());
            // Nice label: hyphens → spaces so the popup reads naturally.
            let nice_name = id.name.replace('-', " ");
            // Append kind suffix for non-Note kinds so the link resolves unambiguously.
            let insert_text = match id.kind {
                NoteKind::Note => nice_name.clone(),
                _ => format!("{} ({})", nice_name, id.kind.to_str()),
            };

            if id.name.contains(&partial_norm) || display_name.to_lowercase().contains(&partial) {
                items.push(CompletionItem {
                    label: insert_text.clone(),
                    kind: Some(CompletionItemKind::FILE),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range: replace_range,
                        new_text: insert_text,
                    })),
                    filter_text: Some(format!("{} {}", nice_name, id.name)),
                    ..Default::default()
                });
            }

            // Alias completions
            for alias in &note.aliases {
                let alias_lower = alias.to_lowercase();
                if alias_lower.contains(&partial) || alias_lower.replace(' ', "-").contains(&partial_norm) {
                    items.push(CompletionItem {
                        label: alias.clone(),
                        kind: Some(CompletionItemKind::FILE),
                        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                            range: replace_range,
                            new_text: alias.clone(),
                        })),
                        filter_text: Some(format!("{} {}", alias, alias.replace(' ', "-"))),
                        ..Default::default()
                    });
                }
            }
        }

        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(items)))
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let id = match NoteID::from_uri(&uri) {
            Some(id) => id,
            None => return,
        };

        {
            let mut index = self.index.lock().await;
            let note = index.notes.entry(id.clone()).or_insert_with(|| Note::new(id));
            note.document = Some(lsp_textdocument::FullTextDocument::new(
                String::from("noteboks"),
                params.text_document.version,
                params.text_document.text,
            ));
            note.update_links();
        }

        self.publish_all_diagnostics().await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri.clone();

        // Re-read from disk in case this is a newly created file
        if let Some(path) = uri.to_file_path().ok() {
            if let Some(note) = crate::index::Note::of_file(&path) {
                let mut index = self.index.lock().await;
                index.notes.insert(note.id.clone(), note);
            }
        }

        self.publish_all_diagnostics().await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();

        self.index
            .lock()
            .await
            .handle_edit(params.text_document, params.content_changes);

        self.publish_diagnostics(uri).await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let default_root = std::env::var("NOTEBOKS_VAULT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/Users/zacgarby/Documents/Vault"));

    let index = Index::new(&default_root);

    let (service, socket) = LspService::new(|client| Backend {
        client,
        index: Arc::new(Mutex::new(index)),
        scan_started: Arc::new(AtomicBool::new(false))
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
