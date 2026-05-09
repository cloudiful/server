mod http;

#[cfg(test)]
mod tests;

pub use http::{BoundServer, Server, ServerConfig};

pub use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt, prompt, prompt_handler,
    prompt_router, tool, tool_handler, tool_router, transport,
};

use std::error::Error as StdError;

use rmcp::{
    RmcpError,
    service::{RunningService, ServerInitializeError},
    transport::IntoTransport,
};

pub async fn serve_stdio<S>(
    service: S,
) -> Result<RunningService<rmcp::service::RoleServer, S>, McpServerError>
where
    S: ServerHandler,
{
    serve(service, rmcp::transport::stdio()).await
}

pub async fn serve<S, T, E, A>(
    service: S,
    transport: T,
) -> Result<RunningService<rmcp::service::RoleServer, S>, McpServerError>
where
    S: ServerHandler,
    T: IntoTransport<rmcp::service::RoleServer, E, A>,
    E: StdError + Send + Sync + 'static,
{
    service.serve(transport).await.map_err(McpServerError::from)
}

#[derive(Debug)]
pub enum McpServerError {
    Config(crate::ServerError),
    Mcp(RmcpError),
    InvalidServicePath(String),
}

impl std::fmt::Display for McpServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(err) => write!(f, "{err}"),
            Self::Mcp(err) => write!(f, "{err}"),
            Self::InvalidServicePath(path) => {
                write!(f, "invalid MCP service path: {path}")
            }
        }
    }
}

impl StdError for McpServerError {}

impl From<RmcpError> for McpServerError {
    fn from(value: RmcpError) -> Self {
        Self::Mcp(value)
    }
}

impl From<ServerInitializeError> for McpServerError {
    fn from(value: ServerInitializeError) -> Self {
        Self::Mcp(value.into())
    }
}

impl From<crate::ServerError> for McpServerError {
    fn from(value: crate::ServerError) -> Self {
        Self::Config(value)
    }
}

impl McpServerError {
    pub(crate) fn invalid_service_path(path: String) -> Self {
        Self::InvalidServicePath(path)
    }
}
