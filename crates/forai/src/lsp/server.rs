use std::error::Error;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
    PublishDiagnostics,
};
use lsp_types::request::{
    Completion, DocumentSymbolRequest, Formatting, GotoDefinition, HoverRequest,
    Request as LspRequest,
};
use lsp_types::{
    CompletionOptions, HoverProviderCapability, InitializeParams,
    OneOf, PublishDiagnosticsParams, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Position, Range, Uri,
};

use crate::formatter;

use super::completion;
use super::diagnostics;
use super::document::DocumentStore;
use super::goto_def;
use super::hover;
use super::symbols;

pub fn main_loop() {
    if let Err(e) = run_server() {
        eprintln!("LSP server error: {e}");
    }
}

fn run_server() -> Result<(), Box<dyn Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();

    let server_capabilities = serde_json::to_value(ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
            ..Default::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        ..Default::default()
    })?;

    let init_params = connection.initialize(server_capabilities)?;
    let _init_params: InitializeParams = serde_json::from_value(init_params)?;

    let mut store = DocumentStore::new();

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                handle_request(&connection, &store, req)?;
            }
            Message::Notification(notif) => {
                handle_notification(&connection, &mut store, notif)?;
            }
            Message::Response(_) => {}
        }
    }

    io_threads.join()?;
    Ok(())
}

fn handle_request(
    conn: &Connection,
    store: &DocumentStore,
    req: Request,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    if let Some((id, params)) = cast_request::<Completion>(&req) {
        let items = if let Some(doc) = store.get(&params.text_document_position.text_document.uri) {
            completion::completions(doc, &params)
        } else {
            Vec::new()
        };
        send_response(conn, id, items)?;
    } else if let Some((id, params)) = cast_request::<HoverRequest>(&req) {
        let uri = &params.text_document_position_params.text_document.uri;
        let result = store.get(uri).and_then(|doc| hover::hover(doc, &params));
        send_response(conn, id, result)?;
    } else if let Some((id, params)) = cast_request::<GotoDefinition>(&req) {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        let result = store
            .get(&uri)
            .and_then(|doc| goto_def::goto_definition(doc, &params, &uri));
        send_response(conn, id, result)?;
    } else if let Some((id, params)) = cast_request::<DocumentSymbolRequest>(&req) {
        let uri = &params.text_document.uri;
        let result = if let Some(doc) = store.get(uri) {
            let syms = symbols::document_symbols(doc, &params);
            Some(lsp_types::DocumentSymbolResponse::Nested(syms))
        } else {
            None
        };
        send_response(conn, id, result)?;
    } else if let Some((id, params)) = cast_request::<Formatting>(&req) {
        let uri = &params.text_document.uri;
        let result = store.get(uri).map(|doc| {
            let formatted = formatter::format_source(&doc.text);
            let last_line = doc.line_index.line_count() as u32;
            vec![TextEdit {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: last_line,
                        character: 0,
                    },
                },
                new_text: formatted,
            }]
        });
        send_response(conn, id, result)?;
    }

    Ok(())
}

fn handle_notification(
    conn: &Connection,
    store: &mut DocumentStore,
    notif: Notification,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    if let Some(params) = cast_notification::<DidOpenTextDocument>(&notif) {
        let uri = params.text_document.uri.clone();
        store.open(
            params.text_document.uri,
            params.text_document.text,
            params.text_document.version,
        );
        publish_diagnostics(conn, store, &uri)?;
    } else if let Some(params) = cast_notification::<DidChangeTextDocument>(&notif) {
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.into_iter().last() {
            store.change(&uri, change.text, params.text_document.version);
        }
        publish_diagnostics(conn, store, &uri)?;
    } else if let Some(params) = cast_notification::<DidCloseTextDocument>(&notif) {
        store.close(&params.text_document.uri);
        let params = PublishDiagnosticsParams {
            uri: params.text_document.uri,
            diagnostics: Vec::new(),
            version: None,
        };
        let notif = Notification::new(PublishDiagnostics::METHOD.to_string(), params);
        conn.sender.send(Message::Notification(notif))?;
    }

    Ok(())
}

fn publish_diagnostics(
    conn: &Connection,
    store: &DocumentStore,
    uri: &Uri,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    if let Some(doc) = store.get(uri) {
        let diags = diagnostics::compute_diagnostics(doc, uri);
        let params = PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: diags,
            version: None,
        };
        let notif = Notification::new(PublishDiagnostics::METHOD.to_string(), params);
        conn.sender.send(Message::Notification(notif))?;
    }
    Ok(())
}

fn cast_request<R: LspRequest>(req: &Request) -> Option<(RequestId, R::Params)>
where
    R::Params: serde::de::DeserializeOwned,
{
    if req.method == R::METHOD {
        let params: R::Params = serde_json::from_value(req.params.clone()).ok()?;
        Some((req.id.clone(), params))
    } else {
        None
    }
}

fn cast_notification<N: lsp_types::notification::Notification>(
    notif: &Notification,
) -> Option<N::Params>
where
    N::Params: serde::de::DeserializeOwned,
{
    if notif.method == N::METHOD {
        serde_json::from_value(notif.params.clone()).ok()
    } else {
        None
    }
}

fn send_response<T: serde::Serialize>(
    conn: &Connection,
    id: RequestId,
    result: T,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let resp = Response::new_ok(id, serde_json::to_value(result)?);
    conn.sender.send(Message::Response(resp))?;
    Ok(())
}
