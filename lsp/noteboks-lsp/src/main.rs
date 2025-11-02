mod index;

use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tree_sitter::{Parser, Point};

use crate::index::Index;

struct Backend {
    client: Client,
    index: Arc<Mutex<Index>>,
}

impl Backend {
    async fn log<M>(&self, message: M)
    where
        M: std::fmt::Display,
    {
        self.client
            .log_message(MessageType::INFO, message.to_string())
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.log("server initialized!").await;
    }

    async fn hover(&self, p: HoverParams) -> Result<Option<Hover>> {
        let pos = p.text_document_position_params;

        let pt = Point::new(
            pos.position.line.try_into().unwrap(),
            pos.position.character.try_into().unwrap(),
        );

        let uri = pos.text_document.uri;

        self.log(format!("\n\nhovered at {}:{}", pt.row, pt.column))
            .await;

        if let Some(note) = self.index.lock().await.get_note(&uri) {
            self.log(format!("note found for {}", uri)).await;

            self.log(format!("\n```\n{}\n```\n", note.document.get_content(None)))
                .await;

            if let Some(tree) = note.get_tree() {
                self.log(format!("tree found for {}", uri)).await;

                let mut cur = tree.walk();

                while cur.goto_first_child_for_point(pt).is_some() {
                    if cur.node().grammar_name() == "link" {
                        break;
                    }
                }

                let node = cur.node();

                if node.grammar_name() == "link" {
                    self.log(format!("link found: {:?}", node)).await;

                    if let Some(url) = node.child_by_field_name("uri") {
                        let text = note.document.get_content(None).as_bytes()
                            [url.start_byte()..url.end_byte()]
                            .as_ref();

                        self.log(format!("url found: {}", String::from_utf8_lossy(text)))
                            .await;

                        let str = String::from_utf8_lossy(text);

                        return Ok(Some(Hover {
                            contents: HoverContents::Scalar(MarkedString::String(String::from(
                                str,
                            ))),
                            range: None,
                        }));
                    }
                }
            }
        }

        Ok(None)
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.log(format!("opened doc: {}", params.text_document.uri))
            .await;

        let r = self.index.lock().await.handle_open(params.text_document);
        self.log(format!("  -> r: {}", r)).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.log(format!("changed doc: {}", params.text_document.uri))
            .await;

        let r = self
            .index
            .lock()
            .await
            .handle_edit(params.text_document, params.content_changes);
        self.log(format!("  -> r: {}", r)).await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_org::language())
        .expect("could not load parser");

    let (service, socket) = LspService::new(|client| Backend {
        client,
        index: Arc::new(Mutex::new(Index::new(parser))),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
