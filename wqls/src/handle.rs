use std::collections::HashMap;
use std::sync::Mutex;

use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};
use wqpl::builtins::Builtins;
use wqpl::cst::GreenNode;
// use wqpl::format::{FormatConfig, Formatter};
use wqpl::highlight::Highlighter;
use wqpl::session::Session;
use wqpl::symbol::DefKind;
use wqpl::wqerror::WqError;

// const PARSER_INTERNAL_PREFIX: &str = "--";

#[derive(Clone)]
struct DocumentSnapshot {
    text: String,
    green: Option<GreenNode>,
}

pub struct Backend {
    client: Client,
    documents: Mutex<HashMap<Url, DocumentSnapshot>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
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
        let session = Session::new();
        let parsed = if use_cache {
            previous.and_then(|green| session.parse_with_cst_using_cache(content, green).ok())
        } else {
            None
        }
        .or_else(|| session.parse_with_cst(content).ok());
        parsed.map(|(_, green)| green)
    }

    fn compute_diagnostics(&self, content: &str) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let session = Session::new();

        // Lexical diagnostics from recovery tokenization
        let tokens = session.tokenize_recovery(content);
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
        match session.analyze_symbols(content) {
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
                    trigger_characters: Some(vec!["[".to_string(), ";".to_string()]),
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
        let events = highlighter.highlight(&content);
        let tokens = crate::token::semantic_tokens_from_events(&content, &events);
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
        let session = Session::new();

        let symbols = match session.analyze_symbols(&content) {
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
                    defs: &[(usize, &wqpl::symbol::SymbolDef)],
                    idx: usize,
                    children_map: &HashMap<usize, Vec<usize>>,
                    content: &str,
                ) -> DocumentSymbol {
                    let def = defs[idx].1;
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
                    let kind = match def.kind {
                        DefKind::Assignment => SymbolKind::VARIABLE,
                        DefKind::Function => SymbolKind::FUNCTION,
                        DefKind::Parameter => SymbolKind::CONSTANT,
                        DefKind::ImplicitParam => SymbolKind::VARIABLE,
                        DefKind::LoopCounter => SymbolKind::VARIABLE,
                        DefKind::Builtin => unreachable!(),
                    };
                    let detail = if def.kind == DefKind::Function {
                        def.params.as_ref().map(|ps| {
                            if ps.is_empty() {
                                "[]".to_string()
                            } else {
                                format!("[{}]", ps.join(";"))
                            }
                        })
                    } else {
                        None
                    };
                    let children = children_map.get(&idx).map(|child_idxs| {
                        child_idxs
                            .iter()
                            .map(|&c| build_symbol(defs, c, children_map, content))
                            .collect()
                    });
                    DocumentSymbol {
                        name: def.name.clone(),
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
                    .map(|idx| build_symbol(&valid_defs, idx, &children_map, &content))
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

        let session = Session::new();
        if let Ok(index) = session.analyze_symbols(&content)
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

        let session = Session::new();
        if let Ok(index) = session.analyze_symbols(&content)
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
    // document.     let session = Session::new();
    //     let tokens = session.tokenize_recovery(&content);
    //     if tokens
    //         .iter()
    //         .any(|t| matches!(t.token_type, wqpl::token::TokenType::Error))
    //     {
    //         return Ok(None);
    //     }
    //     if let Ok(index) = session.analyze_symbols(&content) {
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

        let session = Session::new();
        let mut name = None;
        let mut user_params = None;

        // Try symbol index first
        if let Ok(index) = session.analyze_symbols(&content) {
            if let Some(result) = index.query_at(byte_offset) {
                name = Some(result.name.clone());
                user_params = result.params;
            } else if let Some(val) = index.query_literal_at(byte_offset) {
                let mut text = format!("**{}**", val.type_name());
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
            name = extract_word_at(&content, byte_offset);
        }

        if let Some(name) = name {
            let mut text = format!("**{}**", name);

            let builtins = session.builtins();
            if builtins.is_known_name(&name)
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

        let session = Session::new();
        if let Ok(index) = session.analyze_symbols(&content)
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

        let session = Session::new();
        if let Ok(index) = session.analyze_symbols(&content)
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
                let kind = match u.kind {
                    wqpl::symbol::UseKind::Read | wqpl::symbol::UseKind::OuterRead => {
                        DocumentHighlightKind::READ
                    }
                    wqpl::symbol::UseKind::Write | wqpl::symbol::UseKind::OuterWrite => {
                        DocumentHighlightKind::WRITE
                    }
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
            let session = Session::new();
            if let Ok(index) = session.analyze_symbols(&doc.text) {
                for def in &index.defs {
                    if def.kind != DefKind::Builtin
                        // && !def.name.starts_with(PARSER_INTERNAL_PREFIX)
                        && def.name.to_lowercase().contains(&query)
                        && let Some(span) = def.span
                    {
                        let start = byte_offset_to_position(&doc.text, span.0);
                        let end = byte_offset_to_position(&doc.text, span.1);
                        let kind = match def.kind {
                            DefKind::Assignment => SymbolKind::VARIABLE,
                            DefKind::Function => SymbolKind::FUNCTION,
                            DefKind::Parameter => SymbolKind::CONSTANT,
                            DefKind::ImplicitParam => SymbolKind::VARIABLE,
                            DefKind::LoopCounter => SymbolKind::VARIABLE,
                            DefKind::Builtin => unreachable!(),
                        };
                        let container_name = def
                            .parent
                            .and_then(|p| index.defs.get(p).map(|d| d.name.clone()));
                        symbols.push(SymbolInformation {
                            name: def.name.clone(),
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

        if is_in_no_completion_zone(&content, byte_offset) {
            return Ok(None);
        }

        if is_typing_non_ident(&content, byte_offset) {
            return Ok(None);
        }

        let mut items = Vec::new();
        let mut seen = HashMap::new();

        let session = Session::new();

        // Local symbols (from AST when parse succeeds)
        if let Ok(index) = session.analyze_symbols(&content) {
            for def in &index.defs {
                if def.kind == DefKind::Builtin {
                    continue;
                }
                let kind = match def.kind {
                    DefKind::Assignment => CompletionItemKind::VARIABLE,
                    DefKind::Function => CompletionItemKind::FUNCTION,
                    DefKind::Parameter => CompletionItemKind::CONSTANT,
                    DefKind::ImplicitParam => CompletionItemKind::VARIABLE,
                    DefKind::LoopCounter => CompletionItemKind::VARIABLE,
                    DefKind::Builtin => unreachable!(),
                };
                seen.entry(def.name.clone())
                    .or_insert_with(|| CompletionItem {
                        label: def.name.clone(),
                        kind: Some(kind),
                        ..CompletionItem::default()
                    });
            }
        } else {
            // Fallback: extract assignment names from recovery token stream
            let tokens = session.tokenize_recovery(&content);
            for window in tokens.windows(2) {
                if let (
                    wqpl::token::Token {
                        token_type: wqpl::token::TokenType::Identifier(name),
                        ..
                    },
                    wqpl::token::Token {
                        token_type: wqpl::token::TokenType::Colon,
                        ..
                    },
                ) = (&window[0], &window[1])
                {
                    seen.entry(name.clone()).or_insert_with(|| CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::VARIABLE),
                        ..CompletionItem::default()
                    });
                }
            }
        }

        // Builtins
        let builtins = session.builtins();
        for name in builtins.list_functions_all() {
            let detail = builtins
                .get_id(&name)
                .and_then(|id| Builtins::usage_from_id(id as u16).map(|u| u.to_string()));
            seen.entry(name.clone()).or_insert_with(|| CompletionItem {
                label: name,
                kind: Some(CompletionItemKind::FUNCTION),
                detail,
                ..CompletionItem::default()
            });
        }

        items.extend(seen.into_values());
        items.sort_by(|a, b| a.label.cmp(&b.label));

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
            let session = Session::new();
            let builtins = session.builtins();
            if builtins.is_known_name(name)
                && let Some(id) = builtins.get_id(name)
            {
                let usage = Builtins::usage_from_id(id as u16).unwrap_or("");
                let arity = Builtins::arity_from_id(id as u16).unwrap_or("");

                let parameters = parse_params_from_usage(usage);

                let sig = SignatureInformation {
                    label: usage.to_string(),
                    documentation: Some(Documentation::String(format!("arity: `{}`", arity))),
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
/// Returns true if the cursor is inside a zone where completions should not
/// be offered: shebang / `!` directive lines, comments, or plain string
/// literals.  Format-string `{expr}` braced expressions are allowed.
fn is_in_no_completion_zone(content: &str, byte_offset: usize) -> bool {
    let clamped = byte_offset.min(content.len());

    // 1. Shebang or ! directive on the original source line.
    let line_start = content[..clamped].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = content[clamped..]
        .find('\n')
        .map(|i| clamped + i)
        .unwrap_or(content.len());
    let line = &content[line_start..line_end];
    if line_start == 0 && line.starts_with("#!") {
        return true;
    }
    if line.trim_start().starts_with('!') {
        return true;
    }

    // 2. Comments or plain strings in the preprocessed token stream.
    let session = Session::new();
    let tokens = session.tokenize_recovery(content);

    let Some(tok) = tokens
        .iter()
        .find(|t| t.byte_start <= byte_offset && byte_offset < t.byte_end)
    else {
        return false;
    };

    match &tok.token_type {
        wqpl::token::TokenType::Comment(_)
        | wqpl::token::TokenType::String(_)
        | wqpl::token::TokenType::Character(_) => true,
        wqpl::token::TokenType::FormatString(parts, _, _) => {
            let in_expr = parts.iter().any(|p| matches!(p,
                wqpl::token::FmtPart::Expr { start, end, .. } if *start <= byte_offset && byte_offset < *end
            ));
            !in_expr
        }
        _ => false,
    }
}

/// Returns true if the user is currently typing something that is not a
/// valid identifier (e.g. a number literal), in which case completions
/// should be suppressed.
fn is_typing_non_ident(content: &str, byte_offset: usize) -> bool {
    // Case 1: cursor is inside a word that starts with a digit.
    if let Some(word) = extract_word_at(content, byte_offset)
        && word.chars().next().is_some_and(|c| c.is_numeric())
    {
        return true;
    }

    // Case 2: cursor is immediately after a number/string/character literal
    // token with only whitespace in between (e.g. `1[` or `1 `).
    let session = Session::new();
    let tokens = session.tokenize_recovery(content);

    if let Some(tok) = tokens.iter().rev().find(|t| t.byte_end <= byte_offset) {
        match &tok.token_type {
            wqpl::token::TokenType::Integer(_)
            | wqpl::token::TokenType::BigInteger(_)
            | wqpl::token::TokenType::Float(_)
            | wqpl::token::TokenType::Imaginary(_)
            | wqpl::token::TokenType::Character(_)
            | wqpl::token::TokenType::String(_) => {
                let between = &content[tok.byte_end..byte_offset];
                if between.chars().all(|c| c.is_whitespace()) {
                    return true;
                }
            }
            _ => {}
        }
    }

    false
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

fn extract_word_at(src: &str, offset: usize) -> Option<String> {
    let offset = offset.min(src.len());
    let at_ident = src[offset..]
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '?');
    let prev_ident = offset > 0
        && src[..offset]
            .chars()
            .last()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '?');
    if !(at_ident || (offset == src.len() && prev_ident)) {
        return None;
    }

    let start = src[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '?')
        .last()
        .map(|(i, _)| i)
        .unwrap_or(offset);
    let end = src[offset..]
        .char_indices()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '?')
        .last()
        .map(|(i, c)| offset + i + c.len_utf8())
        .unwrap_or(offset);
    let word = &src[start..end];
    if word.is_empty() {
        None
    } else {
        Some(word.to_string())
    }
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
fn parse_params_from_usage(usage: &str) -> Vec<ParameterInformation> {
    let inner = usage.split('[').nth(1).unwrap_or("");
    let inner = inner.split(']').next().unwrap_or("");
    inner
        .split(';')
        .map(|p| ParameterInformation {
            label: ParameterLabel::Simple(p.trim().to_string()),
            documentation: None,
        })
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
    fn test_find_call_context() {
        assert_eq!(find_call_context("sum[1;2;3]", 5), Some(("sum", 0)));
        assert_eq!(find_call_context("fib[n-1]", 6), Some(("fib", 0)));
        assert_eq!(find_call_context("map[xs;f]", 7), Some(("map", 1)));
    }
}
