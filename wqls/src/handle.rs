use std::collections::HashMap;
use std::sync::Mutex;

use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};
use wqpl::builtins::Builtins;
use wqpl::completion as wq_completion;
use wqpl::cst::GreenNode;
use wqpl::doc::{self, DocRenderTarget, DocTopic};
use wqpl::frontend::Frontend;
// use wqpl::format::{FormatConfig, Formatter};
use wqpl::highlight::Highlighter;
use wqpl::symbol::{DefKind, SymbolDef, SymbolIndex, SymbolProvenance, SymbolProvenanceKind};
use wqpl::wqerror::WqError;

// const PARSER_INTERNAL_PREFIX: &str = "--";

#[derive(Clone)]
struct DocumentSnapshot {
    text: String,
    green: Option<GreenNode>,
}

pub struct Backend {
    client: Client,
    frontend: Frontend,
    documents: Mutex<HashMap<Url, DocumentSnapshot>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            frontend: Frontend::default(),
            documents: Mutex::new(HashMap::new()),
        }
    }

    fn get_document(&self, uri: &Url) -> Result<String> {
        let docs = self.documents.lock().expect("documents mutex poisoned");
        docs.get(uri)
            .map(|doc| doc.text.clone())
            .ok_or_else(|| Error::invalid_params("document not found"))
    }

    fn update_document(&self, uri: &Url, content: String, green: Option<GreenNode>) {
        self.documents
            .lock()
            .expect("documents mutex poisoned")
            .insert(
                uri.clone(),
                DocumentSnapshot {
                    text: content,
                    green,
                },
            );
    }

    fn get_document_snapshot(&self, uri: &Url) -> Result<DocumentSnapshot> {
        let docs = self.documents.lock().expect("documents mutex poisoned");
        docs.get(uri)
            .cloned()
            .ok_or_else(|| Error::invalid_params("document not found"))
    }

    fn parse_green(
        &self,
        content: &str,
        previous: Option<&GreenNode>,
        use_cache: bool,
    ) -> Option<GreenNode> {
        let parsed = if use_cache {
            previous.and_then(|green| {
                self.frontend
                    .parse_with_cst_using_cache(content, green)
                    .ok()
            })
        } else {
            None
        }
        .or_else(|| self.frontend.parse_with_cst(content).ok());
        parsed.map(|(_, green)| green)
    }

    fn compute_diagnostics(&self, content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Lexical diagnostics from recovery tokenization
        let tokens = self.frontend.tokenize_recovery(content);
        for tok in &tokens {
            if let wqpl::token::TokenType::Error = tok.token_type {
                let start = byte_offset_to_position(content, tok.byte_start);
                let end = byte_offset_to_position(content, tok.byte_end);
                diagnostics.push(Diagnostic {
                    range: Range { start, end },
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(NumberOrString::String("syntax".to_string())),
                    code_description: None,
                    source: Some("wq".to_string()),
                    message: "syntax error".to_string(),
                    related_information: None,
                    tags: None,
                    data: None,
                });
            }
        }

        // Parse errors from partial AST
        match self.frontend.analyze_symbols(content) {
            Ok(index) => {
                for ((start_byte, end_byte), err) in &index.errors {
                    let start = byte_offset_to_position(content, *start_byte);
                    let end = byte_offset_to_position(content, *end_byte);
                    diagnostics.push(Diagnostic {
                        range: Range { start, end },
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: Some(NumberOrString::String(err.err_type.name().to_string())),
                        code_description: None,
                        source: Some("wq".to_string()),
                        message: err
                            .msg
                            .clone()
                            .unwrap_or_else(|| "syntax error".to_string()),
                        related_information: None,
                        tags: None,
                        data: None,
                    });
                }
            }
            Err(err) => {
                let (line, col) = extract_error_location(&err).unwrap_or((0, 0));
                let pos = Position {
                    line,
                    character: col,
                };
                diagnostics.push(Diagnostic {
                    range: Range {
                        start: pos,
                        end: pos,
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(NumberOrString::String(err.err_type.name().to_string())),
                    code_description: None,
                    source: Some("wq".to_string()),
                    message: err
                        .msg
                        .clone()
                        .unwrap_or_else(|| "syntax error".to_string()),
                    related_information: None,
                    tags: None,
                    data: None,
                });
            }
        }

        diagnostics
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        tracing::info!("initialize request");
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                            legend: crate::token::legend(),
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                document_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                // document_formatting_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                rename_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        "[".to_string(),
                        ";".to_string(),
                        "`".to_string(),
                    ]),
                    ..CompletionOptions::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["[".to_string(), ";".to_string()]),
                    ..SignatureHelpOptions::default()
                }),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        tracing::info!("initialized notification");
        self.client
            .log_message(MessageType::INFO, "wq LSP server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = &params.text_document.uri;
        tracing::debug!(%uri, "did_open");
        let content = params.text_document.text;
        let green = self.parse_green(&content, None, false);
        self.update_document(uri, content.clone(), green);
        let diagnostics = self.compute_diagnostics(&content);
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = &params.text_document.uri;
        tracing::debug!(%uri, "did_change");
        let Ok(snapshot) = self.get_document_snapshot(uri) else {
            return;
        };
        let (content, used_ranges) = apply_content_changes(&snapshot.text, &params.content_changes);
        let green = self.parse_green(&content, snapshot.green.as_ref(), used_ranges);
        self.update_document(uri, content.clone(), green);
        let diagnostics = self.compute_diagnostics(&content);
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = &params.text_document.uri;
        tracing::debug!(%uri, "did_close");
        self.documents
            .lock()
            .expect("documents mutex poisoned")
            .remove(uri);
        self.client
            .publish_diagnostics(uri.clone(), vec![], None)
            .await;
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;
        tracing::debug!(%uri, "semantic_tokens_full");
        let content = self.get_document(uri)?;
        let highlighter = Highlighter::new();
        let symbol_index = self.frontend.analyze_symbols(&content).ok();
        let semantic_spans = symbol_index
            .as_ref()
            .map(|index| index.semantic_highlight_spans())
            .unwrap_or_default();
        let variable_infos = symbol_index
            .as_ref()
            .map(variable_token_infos)
            .unwrap_or_default();
        let events = highlighter.highlight_with_semantic_spans(&content, &semantic_spans);
        let tokens = crate::token::semantic_tokens_from_events_with_variable_info(
            &content,
            &events,
            &variable_infos,
        );
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }

    #[expect(deprecated)]
    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;
        tracing::debug!(%uri, "document_symbol");
        let content = self.get_document(uri)?;

        let symbols = match self.frontend.analyze_symbols(&content) {
            Ok(index) => {
                let valid_defs: Vec<_> = index
                    .defs
                    .iter()
                    .enumerate()
                    .filter(|(_, def)| {
                        def.kind != DefKind::Builtin && def.span.is_some()
                        // && !def.name.starts_with(PARSER_INTERNAL_PREFIX)
                    })
                    .collect();

                let mut original_to_valid: HashMap<usize, usize> = HashMap::new();
                for (valid_idx, (original_idx, _)) in valid_defs.iter().enumerate() {
                    original_to_valid.insert(*original_idx, valid_idx);
                }

                let mut children_map: HashMap<usize, Vec<usize>> = HashMap::new();
                let mut top_level = Vec::new();
                for (valid_idx, (_, def)) in valid_defs.iter().enumerate() {
                    if let Some(parent) = def.parent {
                        if let Some(&parent_valid) = original_to_valid.get(&parent) {
                            children_map
                                .entry(parent_valid)
                                .or_default()
                                .push(valid_idx);
                        }
                    } else {
                        top_level.push(valid_idx);
                    }
                }

                fn build_symbol(
                    index: &SymbolIndex,
                    defs: &[(usize, &wqpl::symbol::SymbolDef)],
                    idx: usize,
                    children_map: &HashMap<usize, Vec<usize>>,
                    content: &str,
                ) -> DocumentSymbol {
                    let (def_idx, def) = defs[idx];
                    let span = def.span.unwrap();
                    let start = byte_offset_to_position(content, span.0);
                    let end = byte_offset_to_position(content, span.1);
                    let (range, selection_range) = match def.kind {
                        DefKind::Function if def.name_span.is_some() => {
                            let name_span = def.name_span.unwrap();
                            let sel_start = byte_offset_to_position(content, name_span.0);
                            let sel_end = byte_offset_to_position(content, name_span.1);
                            (
                                Range { start, end },
                                Range {
                                    start: sel_start,
                                    end: sel_end,
                                },
                            )
                        }
                        _ => (Range { start, end }, Range { start, end }),
                    };
                    let kind = symbol_kind(def.kind);
                    let detail = symbol_detail(index, def_idx, def);
                    let children = children_map.get(&idx).map(|child_idxs| {
                        child_idxs
                            .iter()
                            .map(|&c| build_symbol(index, defs, c, children_map, content))
                            .collect()
                    });
                    DocumentSymbol {
                        name: symbol_display_name(index, def_idx, def),
                        detail,
                        kind,
                        tags: None,
                        deprecated: None,
                        range,
                        selection_range,
                        children,
                    }
                }

                top_level
                    .into_iter()
                    .map(|idx| build_symbol(&index, &valid_defs, idx, &children_map, &content))
                    .collect()
            }
            Err(_) => Vec::new(),
        };

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        tracing::debug!(%uri, "goto_definition");
        let content = self.get_document(uri)?;
        let pos = params.text_document_position_params.position;
        let byte_offset = position_to_byte_offset(&content, pos);

        if let Ok(index) = self.frontend.analyze_symbols(&content)
            && let Some(result) = index.query_at(byte_offset)
            && let Some(span) = result.def_span
        {
            let start = byte_offset_to_position(&content, span.0);
            let end = byte_offset_to_position(&content, span.1);
            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri: uri.clone(),
                range: Range { start, end },
            })));
        }

        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        tracing::debug!(%uri, "references");
        let content = self.get_document(uri)?;
        let pos = params.text_document_position.position;
        let byte_offset = position_to_byte_offset(&content, pos);

        if let Ok(index) = self.frontend.analyze_symbols(&content)
            && let Some(result) = index.query_at(byte_offset)
        {
            let mut locations = Vec::new();

            if let Some(span) = result.def_span {
                locations.push(Location {
                    uri: uri.clone(),
                    range: Range {
                        start: byte_offset_to_position(&content, span.0),
                        end: byte_offset_to_position(&content, span.1),
                    },
                });
            }

            for u in &result.uses {
                locations.push(Location {
                    uri: uri.clone(),
                    range: Range {
                        start: byte_offset_to_position(&content, u.span.0),
                        end: byte_offset_to_position(&content, u.span.1),
                    },
                });
            }

            if !params.context.include_declaration
                && let Some(span) = result.def_span
            {
                let def_range = Range {
                    start: byte_offset_to_position(&content, span.0),
                    end: byte_offset_to_position(&content, span.1),
                };
                locations.retain(|loc| loc.range != def_range);
            }

            return Ok(Some(locations));
        }

        Ok(None)
    }

    // async fn formatting(&self, params: DocumentFormattingParams) ->
    // Result<Option<Vec<TextEdit>>> {     let uri = &params.text_document.uri;
    //     tracing::debug!(%uri, "formatting");
    //     let content = self.get_document(uri)?;

    //     // Block formatting when there are lexical or parse errors in the
    // document.     let tokens = self.frontend.tokenize_recovery(&content);
    //     if tokens
    //         .iter()
    //         .any(|t| matches!(t.token_type, wqpl::token::TokenType::Error))
    //     {
    //         return Ok(None);
    //     }
    //     if let Ok(index) = self.frontend.analyze_symbols(&content) {
    //         if !index.errors.is_empty() {
    //             return Ok(None);
    //         }
    //     } else {
    //         return Ok(None);
    //     }

    //     let formatter = Formatter::new(FormatConfig::default());

    //     match formatter.format_script(&content) {
    //         Ok(formatted) => {
    //             let full_range = Range {
    //                 start: Position {
    //                     line: 0,
    //                     character: 0,
    //                 },
    //                 end: byte_offset_to_position(&content, content.len()),
    //             };
    //             Ok(Some(vec![TextEdit {
    //                 range: full_range,
    //                 new_text: formatted,
    //             }]))
    //         }
    //         Err(_) => Ok(None),
    //     }
    // }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        tracing::debug!(%uri, "hover");
        let content = self.get_document(uri)?;
        let pos = params.text_document_position_params.position;
        let byte_offset = position_to_byte_offset(&content, pos);

        let mut name = None;
        let mut user_params = None;
        let mut ref_capture_at_cursor = false;
        let mut ref_capture_count = 0usize;
        let mut provenance = None;
        let mut user_symbol_at_cursor = false;

        // Try symbol index first
        if let Ok(index) = self.frontend.analyze_symbols(&content) {
            if let Some(result) = index.query_at(byte_offset) {
                if index
                    .defs
                    .get(result.def_idx)
                    .is_some_and(|def| def.kind == DefKind::Builtin)
                    && let Some(topic) = doc::resolve(&result.name)
                {
                    return Ok(Some(hover_from_doc(&topic)));
                }
                user_symbol_at_cursor = true;
                ref_capture_at_cursor = result.uses.iter().any(|loc| {
                    loc.kind.is_ref_capture()
                        && loc.span.0 <= byte_offset
                        && byte_offset < loc.span.1
                });
                ref_capture_count = result
                    .uses
                    .iter()
                    .filter(|loc| loc.kind.is_ref_capture())
                    .count();
                name = Some(result.name.clone());
                user_params = result.params;
                provenance = index
                    .def_provenance(result.def_idx)
                    .map(|provenance| provenance_label(&provenance));
            } else if let Some(val) = index.query_literal_at(byte_offset) {
                let mut text = format!("**{}**", val.category());
                match &val {
                    wqpl::value::Value::Int(n) => {
                        let abs = if *n < 0 {
                            (-(*n as i128)) as u64
                        } else {
                            *n as u64
                        };
                        text.push_str(&format!("\n\n`{}`", n));
                        text.push_str(&format!("\n\nbin: `0b{:b}`", abs));
                        text.push_str(&format!("\n\noct: `0o{:o}`", abs));
                        text.push_str(&format!("\n\nhex: `0x{:x}`", abs));
                    }
                    wqpl::value::Value::BigInt(b) => {
                        let v = &**b;
                        text.push_str(&format!("\n\n`{}`", v));
                        text.push_str(&format!("\n\nbin: `0b{:b}`", v));
                        text.push_str(&format!("\n\noct: `0o{:o}`", v));
                        text.push_str(&format!("\n\nhex: `0x{:x}`", v));
                    }
                    _ => {
                        text.push_str(&format!("\n\n`{}`", val));
                    }
                }
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: text,
                    }),
                    range: None,
                }));
            }
        }

        // Fallback: extract identifier at cursor
        if name.is_none() {
            name = extract_hover_name_at(&content, byte_offset);
        }

        if let Some(name) = name {
            if !user_symbol_at_cursor && let Some(topic) = doc::resolve(&name) {
                return Ok(Some(hover_from_doc(&topic)));
            }

            let mut text = format!("**{}**", name);
            if ref_capture_at_cursor {
                text.push_str("\n\n`ref capture`");
            } else if ref_capture_count > 0 {
                text.push_str(&format!("\n\nref captures: `{}`", ref_capture_count));
            }
            if let Some(provenance) = provenance {
                text.push_str(&format!("\n\nprovenance: `{}`", provenance));
            }

            let builtins = self.frontend.builtins();
            if !user_symbol_at_cursor
                && builtins.is_known_name(&name)
                && let Some(id) = builtins.get_id(&name)
            {
                if let Some(usage) = Builtins::usage_from_id(id as u16) {
                    text.push_str(&format!("\n\n```wq\n{}\n```", usage));
                }
                if let Some(arity) = Builtins::arity_from_id(id as u16) {
                    text.push_str(&format!("\n\narity: `{}`", arity));
                }
            } else if let Some(params) = user_params {
                let arity = params.len();
                if !params.is_empty() {
                    text.push_str(&format!("\n\n```wq\n{}[{}]\n```", name, params.join(";")));
                }
                text.push_str(&format!("\n\narity: `{}`", arity));
            }

            return Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: text,
                }),
                range: None,
            }));
        }

        Ok(None)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        tracing::debug!(%uri, "rename");
        let content = self.get_document(uri)?;
        let pos = params.text_document_position.position;
        let byte_offset = position_to_byte_offset(&content, pos);
        let new_name = params.new_name;

        if let Ok(index) = self.frontend.analyze_symbols(&content)
            && let Some(result) = index.query_at(byte_offset)
        {
            let mut edits = Vec::new();

            if let Some(span) = result.def_span {
                edits.push(TextEdit {
                    range: Range {
                        start: byte_offset_to_position(&content, span.0),
                        end: byte_offset_to_position(&content, span.1),
                    },
                    new_text: new_name.clone(),
                });
            }

            for u in &result.uses {
                edits.push(TextEdit {
                    range: Range {
                        start: byte_offset_to_position(&content, u.span.0),
                        end: byte_offset_to_position(&content, u.span.1),
                    },
                    new_text: new_name.clone(),
                });
            }

            let mut changes = HashMap::new();
            changes.insert(uri.clone(), edits);

            return Ok(Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }));
        }

        Ok(None)
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        tracing::debug!(%uri, "document_highlight");
        let content = self.get_document(uri)?;
        let pos = params.text_document_position_params.position;
        let byte_offset = position_to_byte_offset(&content, pos);

        if let Ok(index) = self.frontend.analyze_symbols(&content)
            && let Some(result) = index.query_at(byte_offset)
        {
            let mut highlights = Vec::new();

            if let Some(span) = result.def_span {
                highlights.push(DocumentHighlight {
                    range: Range {
                        start: byte_offset_to_position(&content, span.0),
                        end: byte_offset_to_position(&content, span.1),
                    },
                    kind: Some(DocumentHighlightKind::WRITE),
                });
            }

            for u in &result.uses {
                let kind = if u.kind.is_write() {
                    DocumentHighlightKind::WRITE
                } else {
                    DocumentHighlightKind::READ
                };
                highlights.push(DocumentHighlight {
                    range: Range {
                        start: byte_offset_to_position(&content, u.span.0),
                        end: byte_offset_to_position(&content, u.span.1),
                    },
                    kind: Some(kind),
                });
            }

            return Ok(Some(highlights));
        }

        Ok(None)
    }

    #[expect(deprecated)]
    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        tracing::debug!("workspace_symbol query={}", params.query);
        let query = params.query.to_lowercase();
        let mut symbols = Vec::new();

        let docs = self.documents.lock().expect("documents mutex poisoned");
        for (uri, doc) in docs.iter() {
            if let Ok(index) = self.frontend.analyze_symbols(&doc.text) {
                for (def_idx, def) in index.defs.iter().enumerate() {
                    if def.kind != DefKind::Builtin
                        // && !def.name.starts_with(PARSER_INTERNAL_PREFIX)
                        && def.name.to_lowercase().contains(&query)
                        && let Some(span) = def.span
                    {
                        let start = byte_offset_to_position(&doc.text, span.0);
                        let end = byte_offset_to_position(&doc.text, span.1);
                        let kind = symbol_kind(def.kind);
                        let container_name = def
                            .parent
                            .and_then(|p| index.defs.get(p).map(|d| d.name.clone()));
                        symbols.push(SymbolInformation {
                            name: symbol_display_name(&index, def_idx, def),
                            kind,
                            tags: None,
                            deprecated: None,
                            location: Location {
                                uri: uri.clone(),
                                range: Range { start, end },
                            },
                            container_name,
                        });
                    }
                }
            }
        }

        Ok(Some(symbols))
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = &params.text_document.uri;
        tracing::debug!(%uri, "code_lens");

        let mut lenses = Vec::new();
        lenses.push(CodeLens {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            command: Some(Command {
                title: "Run".to_string(),
                command: "wq.run".to_string(),
                arguments: Some(vec![serde_json::json!({
                    "uri": uri.to_string(),
                    "mode": "debug"
                })]),
            }),
            data: None,
        });
        Ok(Some(lenses))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let uri = &params.text_document.uri;
        tracing::debug!(%uri, "folding_range");
        let content = self.get_document(uri)?;

        let mut ranges = Vec::new();
        let mut stack = Vec::new();

        for (i, c) in content.char_indices() {
            match c {
                '{' => {
                    let line = byte_offset_to_position(&content, i).line;
                    stack.push(line);
                }
                '}' => {
                    if let Some(start_line) = stack.pop() {
                        let end_line = byte_offset_to_position(&content, i).line;
                        if end_line > start_line {
                            ranges.push(FoldingRange {
                                start_line,
                                start_character: None,
                                end_line,
                                end_character: None,
                                kind: Some(FoldingRangeKind::Region),
                                collapsed_text: None,
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(Some(ranges))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        tracing::debug!(%uri, "completion");
        let content = self.get_document(uri)?;
        let pos = params.text_document_position.position;
        let byte_offset = position_to_byte_offset(&content, pos);

        if let Some(items) =
            builtin_named_arg_completion_items(&self.frontend, &content, byte_offset)
        {
            return Ok(Some(CompletionResponse::List(CompletionList {
                is_incomplete: false,
                items,
            })));
        }

        if wq_completion::should_suppress_expression_completion(
            &self.frontend,
            &content,
            byte_offset,
        ) {
            return Ok(None);
        }

        let items = wq_completion::expression_completion_candidates(&self.frontend, &content)
            .into_iter()
            .map(completion_item_from_wq)
            .collect();

        Ok(Some(CompletionResponse::List(CompletionList {
            is_incomplete: false,
            items,
        })))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        tracing::debug!(%uri, "signature_help");
        let content = self.get_document(uri)?;
        let pos = params.text_document_position_params.position;
        let byte_offset = position_to_byte_offset(&content, pos);

        if let Some((name, active_param)) = find_call_context(&content, byte_offset) {
            let builtins = self.frontend.builtins();
            if builtins.is_known_name(name)
                && let Some(id) = builtins.get_id(name)
            {
                let usage = Builtins::usage_from_id(id as u16).unwrap_or("");
                let arity = Builtins::arity_from_id(id as u16)
                    .map(|arity| arity.to_string())
                    .unwrap_or_default();

                let named_args = Builtins::named_args_from_id(id as u16).unwrap_or_default();
                let parameters = parse_params_from_usage(usage, named_args);
                let documentation = Builtins::doc_for_id(id as u16)
                    .map(|topic| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: doc::render_markdown(&topic, DocRenderTarget::Lsp),
                        })
                    })
                    .unwrap_or_else(|| Documentation::String(format!("arity: `{}`", arity)));

                let sig = SignatureInformation {
                    label: usage.to_string(),
                    documentation: Some(documentation),
                    parameters: Some(parameters),
                    active_parameter: None,
                };

                return Ok(Some(SignatureHelp {
                    signatures: vec![sig],
                    active_signature: Some(0),
                    active_parameter: Some(active_param as u32),
                }));
            }
        }

        Ok(None)
    }
}

fn builtin_named_arg_completion_items(
    frontend: &Frontend,
    content: &str,
    byte_offset: usize,
) -> Option<Vec<CompletionItem>> {
    let context =
        wq_completion::builtin_named_arg_completion_context(frontend, content, byte_offset)?;
    let builtin_id = frontend.builtins().get_id(&context.builtin_name)?;
    let builtin_id = u16::try_from(builtin_id).ok()?;
    let named_args = Builtins::named_args_from_id(builtin_id)?;
    let range = Range {
        start: byte_offset_to_position(content, context.replace_start),
        end: byte_offset_to_position(content, byte_offset),
    };
    Some(
        named_args
            .iter()
            .filter(|arg| arg.name.starts_with(&context.prefix))
            .filter(|arg| !context.used_names.iter().any(|name| name == arg.name))
            .map(|arg| {
                let replacement = format!("`{}:", arg.name);
                CompletionItem {
                    label: replacement.clone(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail: Some(format!("{} · {}", arg.value_label, arg.summary)),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range,
                        new_text: replacement,
                    })),
                    ..CompletionItem::default()
                }
            })
            .collect(),
    )
}

fn completion_item_from_wq(candidate: wq_completion::CompletionCandidate) -> CompletionItem {
    let documentation = candidate.documentation.map(|topic| {
        Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: doc::render_markdown(&topic, DocRenderTarget::Lsp),
        })
    });
    CompletionItem {
        label: candidate.label,
        kind: Some(completion_kind_from_wq(candidate.kind)),
        detail: candidate.detail,
        documentation,
        ..CompletionItem::default()
    }
}

fn completion_kind_from_wq(kind: wq_completion::CompletionKind) -> CompletionItemKind {
    match kind {
        wq_completion::CompletionKind::Assignment => CompletionItemKind::VARIABLE,
        wq_completion::CompletionKind::Function => CompletionItemKind::FUNCTION,
        wq_completion::CompletionKind::Parameter => CompletionItemKind::CONSTANT,
        wq_completion::CompletionKind::ImplicitParam => CompletionItemKind::VARIABLE,
        wq_completion::CompletionKind::LoopCounter => CompletionItemKind::VARIABLE,
        wq_completion::CompletionKind::Builtin => CompletionItemKind::FUNCTION,
    }
}

fn hover_from_doc(topic: &DocTopic) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: doc::render_markdown(topic, DocRenderTarget::Lsp),
        }),
        range: None,
    }
}

fn symbol_kind(kind: DefKind) -> SymbolKind {
    match kind {
        DefKind::Assignment => SymbolKind::VARIABLE,
        DefKind::Function => SymbolKind::FUNCTION,
        DefKind::Parameter => SymbolKind::CONSTANT,
        DefKind::ImplicitParam => SymbolKind::VARIABLE,
        DefKind::LoopCounter => SymbolKind::VARIABLE,
        DefKind::Builtin => unreachable!(),
    }
}

fn symbol_display_name(index: &SymbolIndex, def_idx: usize, def: &SymbolDef) -> String {
    if index.def_has_ref_capture(def_idx) {
        format!("{} [ref]", def.name)
    } else {
        def.name.clone()
    }
}

fn symbol_detail(index: &SymbolIndex, def_idx: usize, def: &SymbolDef) -> Option<String> {
    let mut parts = Vec::new();
    if def.kind == DefKind::Function
        && let Some(params) = &def.params
    {
        if params.is_empty() {
            parts.push("[]".to_string());
        } else {
            parts.push(format!("[{}]", params.join(";")));
        }
    }
    if def.kind != DefKind::Function
        && let Some(provenance) = index.def_provenance(def_idx)
    {
        parts.push(provenance_label(&provenance));
    }

    let ref_capture_count = index.ref_capture_count(def_idx);
    if ref_capture_count > 0 {
        parts.push(format!("ref captures: {ref_capture_count}"));
    }

    (!parts.is_empty()).then(|| parts.join(" | "))
}

fn variable_token_infos(index: &SymbolIndex) -> Vec<crate::token::VariableTokenInfo> {
    index
        .occurrences()
        .into_iter()
        .filter_map(|occurrence| {
            let provenance = match index.def_provenance(occurrence.def_idx)?.kind {
                SymbolProvenanceKind::Global => crate::token::VariableProvenance::Global,
                SymbolProvenanceKind::Local => crate::token::VariableProvenance::Local,
                SymbolProvenanceKind::Parameter => crate::token::VariableProvenance::Parameter,
                SymbolProvenanceKind::ImplicitParameter => {
                    crate::token::VariableProvenance::ImplicitParameter
                }
                SymbolProvenanceKind::LoopCounter => crate::token::VariableProvenance::LoopCounter,
                SymbolProvenanceKind::Builtin => return None,
            };
            let def = index.defs.get(occurrence.def_idx)?;
            if def.kind == DefKind::Function {
                return None;
            }
            Some(crate::token::VariableTokenInfo {
                span: occurrence.span,
                provenance,
                ref_capture: occurrence.kind.is_ref_capture(),
            })
        })
        .collect()
}

fn provenance_label(provenance: &SymbolProvenance) -> String {
    match provenance.kind {
        SymbolProvenanceKind::Builtin => "builtin".to_string(),
        SymbolProvenanceKind::Global => "global".to_string(),
        SymbolProvenanceKind::Local => provenance
            .origin
            .as_ref()
            .map(|origin| format!("local in {origin}"))
            .unwrap_or_else(|| "local".to_string()),
        SymbolProvenanceKind::Parameter => provenance
            .origin
            .as_ref()
            .map(|origin| format!("parameter of {origin}"))
            .unwrap_or_else(|| "parameter".to_string()),
        SymbolProvenanceKind::ImplicitParameter => provenance
            .origin
            .as_ref()
            .map(|origin| format!("implicit parameter of {origin}"))
            .unwrap_or_else(|| "implicit parameter".to_string()),
        SymbolProvenanceKind::LoopCounter => provenance
            .origin
            .as_ref()
            .map(|origin| format!("loop counter in {origin}"))
            .unwrap_or_else(|| "loop counter".to_string()),
    }
}

fn byte_offset_to_position(src: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (i, c) in src.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            line_start = i + c.len_utf8();
        }
    }
    let line_text = &src[line_start..offset.min(src.len())];
    let character = line_text.encode_utf16().count() as u32;
    Position { line, character }
}

fn apply_content_changes(src: &str, changes: &[TextDocumentContentChangeEvent]) -> (String, bool) {
    let mut content = src.to_string();
    let mut used_ranges = false;

    for change in changes {
        match change.range {
            Some(range) => {
                used_ranges = true;
                let start = position_to_byte_offset(&content, range.start);
                let end = position_to_byte_offset(&content, range.end);
                if start <= end && end <= content.len() {
                    content.replace_range(start..end, &change.text);
                }
            }
            None => {
                content = change.text.clone();
                used_ranges = false;
            }
        }
    }

    (content, used_ranges)
}

fn position_to_byte_offset(src: &str, pos: Position) -> usize {
    let mut line = 0u32;
    let mut byte_offset = 0usize;
    for (i, c) in src.char_indices() {
        if line == pos.line {
            break;
        }
        if c == '\n' {
            line += 1;
            byte_offset = i + c.len_utf8();
        }
    }
    let rest = &src[byte_offset..];
    let mut char_count = 0u32;
    for (i, c) in rest.char_indices() {
        if char_count >= pos.character {
            return byte_offset + i;
        }
        char_count += c.len_utf16() as u32;
    }
    src.len()
}

fn extract_hover_name_at(src: &str, offset: usize) -> Option<String> {
    extract_at_construct_at(src, offset).or_else(|| extract_word_at(src, offset))
}

fn extract_at_construct_at(src: &str, offset: usize) -> Option<String> {
    let offset = offset.min(src.len());
    if src[offset..].starts_with('@') {
        let start = offset;
        let suffix_start = start + 1;
        let end = src[suffix_start..]
            .char_indices()
            .take_while(|(_, c)| is_word_char(*c))
            .last()
            .map(|(i, c)| suffix_start + i + c.len_utf8())
            .unwrap_or(suffix_start);
        if end == suffix_start {
            return None;
        }
        return Some(src[start..end].to_string());
    }

    let (start, end) = word_range_at(src, offset)?;
    if start > 0 && src[..start].ends_with('@') {
        Some(src[start - 1..end].to_string())
    } else {
        None
    }
}

fn extract_word_at(src: &str, offset: usize) -> Option<String> {
    let (start, end) = word_range_at(src, offset)?;
    Some(src[start..end].to_string())
}

fn word_range_at(src: &str, offset: usize) -> Option<(usize, usize)> {
    let offset = offset.min(src.len());
    let at_ident = src[offset..].chars().next().is_some_and(is_word_char);
    let prev_ident = offset > 0 && src[..offset].chars().last().is_some_and(is_word_char);
    if !(at_ident || (offset == src.len() && prev_ident)) {
        return None;
    }

    let start = src[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map(|(i, _)| i)
        .unwrap_or(offset);
    let end = src[offset..]
        .char_indices()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map(|(i, c)| offset + i + c.len_utf8())
        .unwrap_or(offset);
    let word = &src[start..end];
    if word.is_empty() {
        None
    } else {
        Some((start, end))
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '?'
}

fn extract_error_location(err: &WqError) -> Option<(u32, u32)> {
    if let Some(note) = err.notes.first() {
        let first_line = note.lines().next()?;
        let rest = first_line.strip_prefix("at ")?;
        let (line_str, col_str) = rest.split_once(':')?;
        let line = line_str.trim().parse::<u32>().ok()?.saturating_sub(1);
        let col = col_str.trim().parse::<u32>().ok()?.saturating_sub(1);
        return Some((line, col));
    }
    None
}

#[cfg(test)]
mod change_tests {
    use super::*;

    #[test]
    fn apply_content_changes_handles_incremental_ranges() {
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 2,
                },
                end: Position {
                    line: 0,
                    character: 3,
                },
            }),
            range_length: None,
            text: "20".to_string(),
        };
        let (updated, used_ranges) = apply_content_changes("b:2\n", &[change]);
        assert!(used_ranges);
        assert_eq!(updated, "b:20\n");
    }

    #[test]
    fn apply_content_changes_handles_full_replacements() {
        let change = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "x:1\n".to_string(),
        };
        let (updated, used_ranges) = apply_content_changes("b:2\n", &[change]);
        assert!(!used_ranges);
        assert_eq!(updated, "x:1\n");
    }
}

/// Look backwards from `byte_offset` to find a function call context like
/// `name[...` and return the function name + active parameter index.
fn find_call_context(content: &str, byte_offset: usize) -> Option<(&str, usize)> {
    let before = &content[..byte_offset];
    let mut bracket_depth = 0i32;
    let mut arg_idx = 0usize;
    let mut found_bracket = None;

    for (i, c) in before.char_indices().rev() {
        match c {
            ']' | ')' | '}' => bracket_depth += 1,
            '[' | '(' | '{' if bracket_depth > 0 => bracket_depth -= 1,
            '[' if bracket_depth == 0 => {
                found_bracket = Some(i);
                break;
            }
            ';' if bracket_depth == 0 => arg_idx += 1,
            _ => {}
        }
    }

    let bracket_pos = found_bracket?;
    let prefix = &content[..bracket_pos];

    // Skip trailing whitespace
    let mut end = prefix.len();
    while end > 0 {
        let ch = prefix[end - 1..].chars().next()?;
        if ch.is_whitespace() {
            end -= ch.len_utf8();
        } else {
            break;
        }
    }

    // Extract identifier backwards
    let mut start = end;
    while start > 0 {
        let ch = prefix[start - 1..].chars().next()?;
        if ch.is_alphanumeric() || ch == '_' || ch == '?' {
            start -= ch.len_utf8();
        } else {
            break;
        }
    }

    let name = &prefix[start..end];
    if name.is_empty() {
        return None;
    }

    Some((name, arg_idx))
}

/// Parse parameters from a usage string like `map[xs;f;d?]`.
fn parse_params_from_usage(
    usage: &str,
    named_args: &[wqpl::builtins::BuiltinNamedArg],
) -> Vec<ParameterInformation> {
    let positional = usage
        .split(", ")
        .filter_map(|pattern| pattern.split_once('['))
        .filter_map(|(_, rest)| rest.split_once(']'))
        .map(|(inner, _)| {
            inner
                .split(';')
                .map(str::trim)
                .filter(|param| !param.is_empty() && !param.starts_with('`'))
                .collect::<Vec<_>>()
        })
        .max_by_key(Vec::len)
        .unwrap_or_default();
    positional
        .into_iter()
        .map(|param| ParameterInformation {
            label: ParameterLabel::Simple(param.to_string()),
            documentation: None,
        })
        .chain(named_args.iter().map(|arg| ParameterInformation {
            label: ParameterLabel::Simple(format!("`{}:{}", arg.name, arg.value_label)),
            documentation: Some(Documentation::String(arg.summary.to_string())),
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_word_at() {
        assert_eq!(extract_word_at("sum[1;2;3]", 0), Some("sum".to_string()));
        assert_eq!(extract_word_at("sum[1;2;3]", 2), Some("sum".to_string()));
        assert_eq!(extract_word_at("sum[1;2;3]", 3), None);
        assert_eq!(extract_word_at("abc def", 4), Some("def".to_string()));
        assert_eq!(extract_word_at("abc def", 3), None);
    }

    #[test]
    fn test_extract_hover_name_at_keeps_at_construct_marker() {
        assert_eq!(extract_hover_name_at("@r 1", 1), Some("@r".to_string()));
        assert_eq!(
            extract_hover_name_at("@12 has?[x; y]", 2),
            Some("@12".to_string())
        );
        assert_eq!(
            extract_hover_name_at("sum[1;2;3]", 0),
            Some("sum".to_string())
        );
    }

    #[test]
    fn doc_hover_renders_builtin_and_keyword_markdown() {
        let map = doc::resolve("map").expect("map doc");
        let hover = hover_from_doc(&map);
        match hover.contents {
            HoverContents::Markup(content) => {
                assert_eq!(content.kind, MarkupKind::Markdown);
                assert!(content.value.contains("map builtin"));
                assert!(content.value.contains("arity: `2 3`"));
            }
            other => panic!("expected markup hover, got {other:?}"),
        }

        let ret = doc::resolve("@r").expect("@r doc");
        let hover = hover_from_doc(&ret);
        match hover.contents {
            HoverContents::Markup(content) => {
                assert!(content.value.contains("@r Return"));
                assert!(content.value.contains("Return early"));
            }
            other => panic!("expected markup hover, got {other:?}"),
        }
    }

    #[test]
    fn builtin_completion_docs_use_catalog() {
        let frontend = Frontend::default();
        let topic = frontend
            .builtins()
            .doc_for_name("words")
            .expect("words doc");
        let rendered = doc::render_markdown(&topic, DocRenderTarget::Lsp);
        assert!(rendered.contains("words builtin"));
        assert!(rendered.contains("words[s]"));
    }

    #[test]
    fn builtin_named_argument_completion_uses_registry_metadata() {
        let frontend = Frontend::default();
        let src = "split[\"a,b\";`ma";

        let items =
            builtin_named_arg_completion_items(&frontend, src, src.len()).expect("named args");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "`max:");
        assert_eq!(
            items[0].detail.as_deref(),
            Some("n · maximum number of splits")
        );
        let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
            panic!("expected text edit");
        };
        assert_eq!(edit.new_text, "`max:");
        assert_eq!(edit.range.start.character, 12);
        assert_eq!(edit.range.end.character, 15);
    }

    #[test]
    fn signature_docs_use_builtin_doc_metadata() {
        let topic = Builtins::doc_for_id(Builtins::MAP).expect("map doc");
        let rendered = doc::render_markdown(&topic, DocRenderTarget::Lsp);
        assert!(rendered.contains("map[xs;f;d?]"));
        assert!(rendered.contains("arity: `2 3`"));
    }

    #[test]
    fn signature_parameters_use_longest_pattern_and_named_arg_metadata() {
        let params = parse_params_from_usage(
            Builtins::usage_from_id(Builtins::SPLIT).expect("split usage"),
            Builtins::named_args_from_id(Builtins::SPLIT).expect("split named args"),
        );
        let labels = params
            .iter()
            .map(|param| match &param.label {
                ParameterLabel::Simple(label) => label.as_str(),
                ParameterLabel::LabelOffsets(_) => panic!("expected simple parameter label"),
            })
            .collect::<Vec<_>>();

        assert_eq!(labels, vec!["xs", "delim", "`max:n"]);
        assert_eq!(
            params[2].documentation,
            Some(Documentation::String(
                "maximum number of splits".to_string()
            ))
        );
    }

    #[test]
    fn test_find_call_context() {
        assert_eq!(find_call_context("sum[1;2;3]", 5), Some(("sum", 0)));
        assert_eq!(find_call_context("fib[n-1]", 6), Some(("fib", 0)));
        assert_eq!(find_call_context("map[xs;f]", 7), Some(("map", 1)));
    }

    #[test]
    fn variable_token_infos_include_provenance() {
        let frontend = Frontend::default();
        let index = frontend
            .analyze_symbols("g:1; f:{[x] y:2; x+y+g}")
            .expect("symbol analysis");
        let infos = variable_token_infos(&index);

        assert!(infos.iter().any(|info| {
            info.span == (17, 18) && info.provenance == crate::token::VariableProvenance::Parameter
        }));
        assert!(infos.iter().any(|info| {
            info.span == (19, 20) && info.provenance == crate::token::VariableProvenance::Local
        }));
        assert!(infos.iter().any(|info| {
            info.span == (21, 22) && info.provenance == crate::token::VariableProvenance::Global
        }));
    }
}
