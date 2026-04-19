// ServerHandler uses `fn f() -> impl Future<...>` signatures that we can't
// rewrite as `async fn` because the trait demands a named return type.
// Clippy's manual_async_fn doesn't notice that constraint — silence it
// for the whole module.
#![allow(clippy::manual_async_fn)]
//! The MCP server — an [`rmcp::ServerHandler`] backed by our tool
//! registry.
//!
//! Responsibilities (kept tight; all the real work lives in
//! [`crate::tools`] + sibling modules):
//!   - Advertise capabilities (`tools`, `prompts`, `resources`).
//!   - Dispatch `tools/call` to [`crate::tools::dispatch`] with
//!     audit-log bookkeeping.
//!   - Serve `prompts/list` + `prompts/get` from [`crate::prompts`].
//!   - Serve `resources/list` + `resources/read` from
//!     [`crate::resources`].

use std::borrow::Cow;
use std::future::Future;
use std::sync::Arc;

use rmcp::ErrorData as McpRpcError;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorCode, GetPromptRequestParams,
    GetPromptResult, Implementation, JsonObject, ListPromptsResult, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, Prompt, PromptArgument, PromptMessage,
    PromptMessageContent, PromptMessageRole, RawResource, ReadResourceRequestParams,
    ReadResourceResult, Resource, ResourceContents, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};

use crate::audit;
use crate::context::McpContext;
use crate::prompts as prompt_registry;
use crate::resources as resource_registry;
use crate::tools;

/// The concrete handler type.
#[derive(Debug, Clone)]
pub struct VersionxServer {
    ctx: McpContext,
    info: ServerInfo,
}

impl VersionxServer {
    pub fn new(ctx: McpContext) -> Self {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_prompts()
            .enable_resources()
            .build();
        let mut impl_info = Implementation::new("versionx-mcp", env!("CARGO_PKG_VERSION"));
        impl_info.title = Some("Versionx MCP".into());
        impl_info.website_url = Some("https://versionx.dev".into());
        info.server_info = impl_info;
        info.instructions = Some(
            "Versionx MCP server. Mutating tools (release_propose, release_apply, \
             changelog_draft) end with `_propose` / `_apply` / `_draft`; read-only tools end \
             with `_list` / `_status` / `_graph` / `_read` / `_check`. Call `tools/list` for \
             the authoritative schema."
                .into(),
        );
        Self { ctx, info }
    }

    fn client_info_string(&self, rc: &RequestContext<RoleServer>) -> Option<String> {
        rc.peer
            .peer_info()
            .map(|info| format!("{}/{}", info.client_info.name, info.client_info.version))
    }
}

impl ServerHandler for VersionxServer {
    fn get_info(&self) -> ServerInfo {
        self.info.clone()
    }

    fn list_tools(
        &self,
        _req: Option<PaginatedRequestParams>,
        _rc: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpRpcError>> + Send + '_ {
        async move {
            let tools: Vec<Tool> = tools::descriptors()
                .into_iter()
                .map(|d| {
                    let schema: JsonObject =
                        serde_json::from_value(d.input_schema.clone()).unwrap_or_default();
                    let mut t = Tool::new(
                        Cow::Borrowed(d.name),
                        Cow::Borrowed(d.description),
                        Arc::new(schema),
                    );
                    t.title = Some(d.title.into());
                    t
                })
                .collect();
            Ok(ListToolsResult::with_all_items(tools))
        }
    }

    fn call_tool(
        &self,
        req: CallToolRequestParams,
        rc: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, McpRpcError>> + Send + '_ {
        async move {
            let timer = audit::start_call(req.name.to_string(), self.client_info_string(&rc));
            let params =
                req.arguments.map(serde_json::Value::Object).unwrap_or(serde_json::Value::Null);
            match tools::dispatch(&req.name, params, &self.ctx).await {
                Ok(out) => {
                    self.ctx.audit.record(&timer.finish(!out.is_error, None));
                    let content = vec![Content::text(out.summary.clone())];
                    let mut result = if out.is_error {
                        CallToolResult::error(content)
                    } else {
                        CallToolResult::success(content)
                    };
                    result.structured_content = Some(out.structured);
                    Ok(result)
                }
                Err(e) => {
                    self.ctx.audit.record(&timer.finish(false, Some(e.to_string())));
                    Err(e.into_rpc_error())
                }
            }
        }
    }

    fn list_prompts(
        &self,
        _req: Option<PaginatedRequestParams>,
        _rc: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListPromptsResult, McpRpcError>> + Send + '_ {
        async move {
            let prompts: Vec<Prompt> = prompt_registry::descriptors()
                .into_iter()
                .map(|d| {
                    let args = if d.arguments.is_empty() {
                        None
                    } else {
                        Some(
                            d.arguments
                                .iter()
                                .map(|a| {
                                    let mut pa = PromptArgument::default();
                                    pa.name = a.name.into();
                                    pa.description = Some(a.description.into());
                                    pa.required = Some(a.required);
                                    pa
                                })
                                .collect(),
                        )
                    };
                    Prompt::new(d.name, Some(d.description), args)
                })
                .collect();
            Ok(ListPromptsResult::with_all_items(prompts))
        }
    }

    fn get_prompt(
        &self,
        req: GetPromptRequestParams,
        _rc: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<GetPromptResult, McpRpcError>> + Send + '_ {
        async move {
            let name = req.name.to_string();
            let d = prompt_registry::descriptors()
                .into_iter()
                .find(|d| d.name == name)
                .ok_or_else(|| {
                    McpRpcError::new(
                        ErrorCode::METHOD_NOT_FOUND,
                        format!("unknown prompt: {name}"),
                        None,
                    )
                })?;
            let messages: Vec<PromptMessage> = d
                .messages
                .iter()
                .map(|m| {
                    let role = match m.role {
                        "assistant" => PromptMessageRole::Assistant,
                        _ => PromptMessageRole::User,
                    };
                    PromptMessage::new(role, PromptMessageContent::text(m.text))
                })
                .collect();
            let mut out = GetPromptResult::new(messages);
            out.description = Some(d.description.into());
            Ok(out)
        }
    }

    fn list_resources(
        &self,
        _req: Option<PaginatedRequestParams>,
        _rc: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, McpRpcError>> + Send + '_ {
        async move {
            let resources: Vec<Resource> = resource_registry::descriptors()
                .into_iter()
                .map(|d| {
                    let raw = RawResource {
                        uri: d.uri.into(),
                        name: d.name.into(),
                        title: None,
                        description: Some(d.description.into()),
                        mime_type: Some(d.mime_type.into()),
                        size: None,
                        icons: None,
                        meta: None,
                    };
                    Resource { raw, annotations: None }
                })
                .collect();
            Ok(ListResourcesResult::with_all_items(resources))
        }
    }

    fn read_resource(
        &self,
        req: ReadResourceRequestParams,
        _rc: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResult, McpRpcError>> + Send + '_ {
        async move {
            let uri = req.uri.to_string();
            let (body, mime) =
                resource_registry::read(&uri, &self.ctx.workspace_root).ok_or_else(|| {
                    McpRpcError::new(
                        ErrorCode::METHOD_NOT_FOUND,
                        format!("unknown resource: {uri}"),
                        None,
                    )
                })?;
            Ok(ReadResourceResult::new(vec![ResourceContents::TextResourceContents {
                uri: uri.into(),
                mime_type: Some(mime),
                meta: None,
                text: body,
            }]))
        }
    }
}

// --------- transport entry points ---------------------------------------

/// Serve on stdio. Blocks until the peer closes the transport.
pub async fn serve_stdio(server: VersionxServer) -> anyhow::Result<()> {
    use rmcp::service::ServiceExt;
    use rmcp::transport::io::stdio;
    let transport = stdio();
    let running = server.serve(transport).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    running.waiting().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}
