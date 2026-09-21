//! The transport-agnostic server main loop.

use std::path::PathBuf;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit, Initialized,
    Notification as _, PublishDiagnostics,
};
use lsp_types::{
    CompletionOptions, HoverProviderCapability, InitializeParams, OneOf, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, Uri,
};

use crate::features;
use crate::position::PositionEncoding;
use crate::world::World;

/// Capability set advertised by sigil-lsp. Providers are advertised only
/// for implemented features.
pub fn server_capabilities(encoding: PositionEncoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding.as_kind()),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                will_save: None,
                will_save_wait_until: None,
                save: None,
            },
        )),
        document_symbol_provider: Some(OneOf::Left(true)),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: None,
            trigger_characters: Some(vec!["[".to_string(), " ".to_string()]),
            all_commit_characters: None,
            completion_item: None,
            work_done_progress_options: Default::default(),
        }),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        semantic_tokens_provider: Some(crate::semantic_tokens::server_capability()),
        ..ServerCapabilities::default()
    }
}

/// Run the server loop until shutdown/exit or transport disconnect.
pub fn run_server(
    connection: Connection,
    mut world: World,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (id, params) = connection.initialize_start()?;
    let init: InitializeParams =
        serde_json::from_value(params).map_err(|e| format!("invalid initialize params: {e}"))?;

    let offered = init
        .capabilities
        .general
        .as_ref()
        .and_then(|g| g.position_encodings.as_deref());
    world.set_encoding(PositionEncoding::negotiate(offered));
    if world.root().is_none() {
        #[allow(deprecated)]
        let root = init
            .workspace_folders
            .as_ref()
            .and_then(|fs| fs.first())
            .map(|f| uri_to_path(f.uri.clone()))
            .or_else(|| init.root_uri.clone().map(uri_to_path))
            .flatten();
        world.set_root(root);
    }
    world.scan_workspace();
    world.reanalyze();

    let initialize_result = serde_json::json!({
        "capabilities": server_capabilities(world.encoding()),
        "serverInfo": {
            "name": "sigil-lsp",
            "version": env!("CARGO_PKG_VERSION"),
        },
    });
    connection.initialize_finish(id, initialize_result)?;

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                on_request(&connection, &world, req);
            }
            Message::Notification(n) => {
                if n.method == Exit::METHOD {
                    return Ok(());
                }
                on_notification(&connection, &mut world, n);
            }
            Message::Response(_) => {}
        }
    }
    Ok(())
}

fn on_request(connection: &Connection, world: &World, req: Request) {
    use lsp_types::request::Request as _;
    let response = match req.method.as_str() {
        crate::DocumentTextRequest::METHOD => {
            match serde_json::from_value::<crate::DocumentTextParams>(req.params.clone()) {
                Ok(params) => {
                    let key = params.text_document.uri.to_string();
                    Response::new_ok(req.id.clone(), world.snapshot_text(&key))
                }
                Err(e) => Response::new_err(
                    req.id.clone(),
                    ErrorCode::InvalidParams as i32,
                    format!("invalid params: {e}"),
                ),
            }
        }
        lsp_types::request::DocumentSymbolRequest::METHOD => handle(
            req.id,
            req.params,
            |params: lsp_types::DocumentSymbolParams| {
                let uri = params.text_document.uri.to_string();
                serde_json::to_value(features::document_symbols(world, &uri))
            },
        ),
        lsp_types::request::GotoDefinition::METHOD => handle(
            req.id,
            req.params,
            |params: lsp_types::GotoDefinitionParams| {
                let uri = params
                    .text_document_position_params
                    .text_document
                    .uri
                    .to_string();
                serde_json::to_value(features::definition(
                    world,
                    &uri,
                    params.text_document_position_params.position,
                ))
            },
        ),
        lsp_types::request::HoverRequest::METHOD => {
            handle(req.id, req.params, |params: lsp_types::HoverParams| {
                let uri = params
                    .text_document_position_params
                    .text_document
                    .uri
                    .to_string();
                serde_json::to_value(features::hover(
                    world,
                    &uri,
                    params.text_document_position_params.position,
                ))
            })
        }
        lsp_types::request::Completion::METHOD => {
            handle(req.id, req.params, |params: lsp_types::CompletionParams| {
                let uri = params.text_document_position.text_document.uri.to_string();
                serde_json::to_value(features::completion(
                    world,
                    &uri,
                    params.text_document_position.position,
                ))
            })
        }
        lsp_types::request::References::METHOD => {
            handle(req.id, req.params, |params: lsp_types::ReferenceParams| {
                let uri = params.text_document_position.text_document.uri.to_string();
                serde_json::to_value(features::references(
                    world,
                    &uri,
                    params.text_document_position.position,
                    params.context.include_declaration,
                ))
            })
        }
        lsp_types::request::Rename::METHOD => {
            match serde_json::from_value::<lsp_types::RenameParams>(req.params.clone()) {
                Ok(params) => {
                    let uri = params.text_document_position.text_document.uri.to_string();
                    match features::rename(
                        world,
                        &uri,
                        params.text_document_position.position,
                        &params.new_name,
                    ) {
                        Ok(edit) => match serde_json::to_value(edit) {
                            Ok(value) => Response::new_ok(req.id, value),
                            Err(e) => Response::new_err(
                                req.id,
                                ErrorCode::InternalError as i32,
                                format!("serialize response: {e}"),
                            ),
                        },
                        // Invalid names / unrenamable targets: the request
                        // was syntactically fine, so RequestFailed (LSP
                        // 3.17) with a human-readable reason.
                        Err(message) => Response::new_err(
                            req.id,
                            ErrorCode::RequestFailed as i32,
                            format!("cannot rename: {message}"),
                        ),
                    }
                }
                Err(e) => Response::new_err(
                    req.id,
                    ErrorCode::InvalidParams as i32,
                    format!("invalid params: {e}"),
                ),
            }
        }
        lsp_types::request::WorkspaceSymbolRequest::METHOD => handle(
            req.id,
            req.params,
            |params: lsp_types::WorkspaceSymbolParams| {
                serde_json::to_value(features::workspace_symbols(world, &params.query))
            },
        ),
        lsp_types::request::SemanticTokensFullRequest::METHOD => handle(
            req.id,
            req.params,
            |params: lsp_types::SemanticTokensParams| {
                let uri = params.text_document.uri.to_string();
                serde_json::to_value(crate::semantic_tokens::semantic_tokens(world, &uri))
            },
        ),
        _ => Response::new_err(
            req.id.clone(),
            ErrorCode::MethodNotFound as i32,
            format!("method not supported: {}", req.method),
        ),
    };
    let _ = connection.sender.send(response.into());
}

/// Deserialize params, run a feature, and turn the outcome into a Response.
fn handle<P: serde::de::DeserializeOwned, F>(
    id: lsp_server::RequestId,
    params: serde_json::Value,
    f: F,
) -> Response
where
    F: FnOnce(P) -> serde_json::Result<serde_json::Value>,
{
    match serde_json::from_value::<P>(params) {
        Ok(p) => match f(p) {
            Ok(value) => Response::new_ok(id, value),
            Err(e) => Response::new_err(
                id,
                ErrorCode::InternalError as i32,
                format!("serialize response: {e}"),
            ),
        },
        Err(e) => Response::new_err(
            id,
            ErrorCode::InvalidParams as i32,
            format!("invalid params: {e}"),
        ),
    }
}

fn on_notification(connection: &Connection, world: &mut World, n: Notification) {
    match n.method.as_str() {
        DidOpenTextDocument::METHOD => {
            let Ok(params) =
                serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(n.params)
            else {
                return;
            };
            let key = params.text_document.uri.to_string();
            world.open_document(
                &key,
                params.text_document.text,
                params.text_document.version,
            );
            publish_workspace(connection, world);
        }
        DidChangeTextDocument::METHOD => {
            let Ok(params) =
                serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(n.params)
            else {
                return;
            };
            let key = params.text_document.uri.to_string();
            if world.change_document(&key, params.text_document.version, params.content_changes) {
                publish_workspace(connection, world);
            }
        }
        DidCloseTextDocument::METHOD => {
            let Ok(params) =
                serde_json::from_value::<lsp_types::DidCloseTextDocumentParams>(n.params)
            else {
                return;
            };
            let key = params.text_document.uri.to_string();
            if world.close_document(&key) {
                // Clear the closed document first, then refresh the rest.
                let _ = connection.sender.send(
                    lsp_server::Notification::new(
                        PublishDiagnostics::METHOD.to_string(),
                        serde_json::to_value(lsp_types::PublishDiagnosticsParams {
                            uri: params.text_document.uri,
                            diagnostics: Vec::new(),
                            version: None,
                        })
                        .unwrap(),
                    )
                    .into(),
                );
                publish_workspace(connection, world);
            }
        }
        Initialized::METHOD => {}
        _ => {}
    }
}

/// Re-publish diagnostics for every document in the workspace.
fn publish_workspace(connection: &Connection, world: &mut World) {
    world.reanalyze();
    let analysis = world.analysis();
    let encoding = world.encoding();
    for (uri, doc) in world.docs() {
        let diagnostics = analysis.diagnostics_for(uri, doc, encoding);
        let Ok(params) = serde_json::to_value(lsp_types::PublishDiagnosticsParams {
            uri: parse_uri(uri),
            diagnostics,
            version: Some(doc.version),
        }) else {
            continue;
        };
        let _ = connection.sender.send(
            lsp_server::Notification::new(PublishDiagnostics::METHOD.to_string(), params).into(),
        );
    }
}

fn parse_uri(s: &str) -> Uri {
    serde_json::from_value(serde_json::Value::String(s.to_string())).expect("stored URIs are valid")
}

fn uri_to_path(u: Uri) -> Option<PathBuf> {
    let s = u.to_string();
    s.strip_prefix("file://").map(PathBuf::from)
}
