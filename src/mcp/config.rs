use std::{sync::Arc, time::Duration};

use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig,
    session::{EventStore, SessionStore},
};
use tokio_util::sync::CancellationToken;

use super::McpServerError;

/// Default maximum Streamable HTTP POST body size (4 MiB), matching rmcp.
pub const DEFAULT_MAX_REQUEST_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Runtime configuration for MCP Streamable HTTP servers.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    service_path: String,
    legacy_session_mode: bool,
    json_response: bool,
    sse_keep_alive: Option<Duration>,
    sse_retry: Option<Duration>,
    allowed_hosts: Vec<String>,
    allowed_origins: Vec<String>,
    hosts_validation_disabled: bool,
    cancellation_token: Option<CancellationToken>,
    session_store: Option<Arc<dyn SessionStore>>,
    event_store: Option<Arc<dyn EventStore>>,
    max_request_body_bytes: usize,
    stateless_protocol_metadata_required: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            service_path: "/mcp".to_string(),
            legacy_session_mode: true,
            json_response: false,
            sse_keep_alive: None,
            sse_retry: None,
            allowed_hosts: Vec::new(),
            allowed_origins: Vec::new(),
            hosts_validation_disabled: false,
            cancellation_token: None,
            session_store: None,
            event_store: None,
            max_request_body_bytes: DEFAULT_MAX_REQUEST_BODY_BYTES,
            stateless_protocol_metadata_required: false,
        }
    }
}

impl ServerConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_service_path(mut self, service_path: impl Into<String>) -> Self {
        self.service_path = service_path.into();
        self
    }

    /// Control whether protocol versions before `2026-07-28` use MCP sessions.
    ///
    /// The `2026-07-28` protocol is always served statelessly; this option only
    /// affects legacy clients that negotiate an older protocol version.
    pub fn with_legacy_session_mode(mut self, legacy_session_mode: bool) -> Self {
        self.legacy_session_mode = legacy_session_mode;
        self
    }

    /// Deprecated alias for [`Self::with_legacy_session_mode`].
    #[deprecated(
        since = "0.3.0",
        note = "renamed to with_legacy_session_mode: sessions only apply to protocol versions before 2026-07-28"
    )]
    pub fn with_stateful_mode(self, stateful_mode: bool) -> Self {
        self.with_legacy_session_mode(stateful_mode)
    }

    /// Prefer plain JSON responses over `text/event-stream` for simple
    /// request-response tools in stateless mode.
    pub fn with_json_response(mut self, json_response: bool) -> Self {
        self.json_response = json_response;
        self
    }

    pub fn with_sse_keep_alive(mut self, sse_keep_alive: Duration) -> Self {
        self.sse_keep_alive = Some(sse_keep_alive);
        self
    }

    pub fn with_sse_retry(mut self, sse_retry: Duration) -> Self {
        self.sse_retry = Some(sse_retry);
        self
    }

    /// Restrict inbound `Host` headers to the given hostnames or `host:port`
    /// authorities, preventing DNS rebinding attacks.
    ///
    /// Without an explicit allowlist the server only accepts loopback hosts
    /// (`localhost`, `127.0.0.1`, `::1`). Public deployments must call this
    /// with their own hostnames.
    ///
    /// Calls are last-call-wins: `with_allowed_hosts([])` keeps the loopback
    /// default, while [`Self::disable_allowed_hosts`] allows any host.
    pub fn with_allowed_hosts<I, S>(mut self, allowed_hosts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_hosts = allowed_hosts.into_iter().map(Into::into).collect();
        self.hosts_validation_disabled = false;
        self
    }

    /// Allow requests with any `Host` header. Not recommended for public
    /// deployments.
    pub fn disable_allowed_hosts(mut self) -> Self {
        self.allowed_hosts.clear();
        self.hosts_validation_disabled = true;
        self
    }

    /// Restrict inbound browser `Origin` headers. Requests without an `Origin`
    /// header are unaffected.
    pub fn with_allowed_origins<I, S>(mut self, allowed_origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_origins = allowed_origins.into_iter().map(Into::into).collect();
        self
    }

    /// Disable `Origin` validation, reverting to the default ignore-Origin
    /// behavior.
    pub fn disable_allowed_origins(mut self) -> Self {
        self.allowed_origins.clear();
        self
    }

    /// Cancel all active sessions and stop accepting new requests when the
    /// token is cancelled.
    pub fn with_cancellation_token(mut self, cancellation_token: CancellationToken) -> Self {
        self.cancellation_token = Some(cancellation_token);
        self
    }

    /// Persist legacy session state in an external store for cross-instance
    /// recovery.
    pub fn with_session_store<T>(mut self, session_store: Arc<T>) -> Self
    where
        T: SessionStore,
    {
        self.session_store = Some(session_store);
        self
    }

    pub fn without_session_store(mut self) -> Self {
        self.session_store = None;
        self
    }

    /// Persist SSE events in a shared store so stateless clients can resume
    /// streams with `Last-Event-ID` across server instances.
    pub fn with_event_store<T>(mut self, event_store: Arc<T>) -> Self
    where
        T: EventStore,
    {
        self.event_store = Some(event_store);
        self
    }

    pub fn without_event_store(mut self) -> Self {
        self.event_store = None;
        self
    }

    /// Set the maximum POST request body size in bytes.
    pub fn with_max_request_body_bytes(mut self, bytes: usize) -> Self {
        self.max_request_body_bytes = bytes;
        self
    }

    /// Require stateless JSON-RPC request POSTs to carry per-request protocol
    /// signals (`MCP-Protocol-Version` header and `_meta` metadata).
    pub fn with_stateless_protocol_metadata_required(mut self, required: bool) -> Self {
        self.stateless_protocol_metadata_required = required;
        self
    }

    pub(crate) fn service_path(&self) -> Result<String, McpServerError> {
        let path = self.service_path.trim();
        if path.is_empty() {
            return Err(McpServerError::invalid_service_path(
                self.service_path.clone(),
            ));
        }

        let mut normalized = path.trim_end_matches('/').to_string();
        if normalized.is_empty() {
            normalized.push('/');
        }
        if !normalized.starts_with('/') {
            normalized.insert(0, '/');
        }

        Ok(normalized)
    }

    pub(crate) fn event_store(&self) -> Option<Arc<dyn EventStore>> {
        self.event_store.clone()
    }

    pub(crate) fn to_rmcp_config(&self) -> StreamableHttpServerConfig {
        let mut config = StreamableHttpServerConfig::default()
            .with_sse_keep_alive(self.sse_keep_alive)
            .with_sse_retry(self.sse_retry)
            .with_legacy_session_mode(self.legacy_session_mode)
            .with_json_response(self.json_response)
            .with_max_request_body_bytes(self.max_request_body_bytes)
            .with_stateless_protocol_metadata_required(self.stateless_protocol_metadata_required);

        if !self.allowed_hosts.is_empty() {
            config = config.with_allowed_hosts(self.allowed_hosts.clone());
        } else if self.hosts_validation_disabled {
            config = config.disable_allowed_hosts();
        }

        if !self.allowed_origins.is_empty() {
            config = config.with_allowed_origins(self.allowed_origins.clone());
        }

        if let Some(cancellation_token) = &self.cancellation_token {
            config = config.with_cancellation_token(cancellation_token.clone());
        }

        config.session_store = self.session_store.clone();
        config
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rmcp::transport::streamable_http_server::session::{
        EventId, EventStore, EventStoreError, EventStream, ServerSseMessage, SessionState,
        SessionStore, SessionStoreError,
    };

    use super::*;

    struct NoopSessionStore;

    #[async_trait::async_trait]
    impl SessionStore for NoopSessionStore {
        async fn load(&self, _id: &str) -> Result<Option<SessionState>, SessionStoreError> {
            Ok(None)
        }

        async fn store(&self, _id: &str, _state: &SessionState) -> Result<(), SessionStoreError> {
            Ok(())
        }

        async fn delete(&self, _id: &str) -> Result<(), SessionStoreError> {
            Ok(())
        }
    }

    struct NoopEventStore;

    #[async_trait::async_trait]
    impl EventStore for NoopEventStore {
        async fn store_event(
            &self,
            _stream_id: &str,
            _event: &ServerSseMessage,
        ) -> Result<EventId, EventStoreError> {
            Ok("event-1".to_string())
        }

        async fn replay_events_after(
            &self,
            _last_event_id: &str,
        ) -> Result<EventStream, EventStoreError> {
            Err("not implemented".into())
        }
    }

    #[test]
    fn defaults_map_to_secure_rmcp_config() {
        let config = ServerConfig::default().to_rmcp_config();

        assert!(config.legacy_session_mode);
        assert!(!config.json_response);
        assert_eq!(config.allowed_hosts, ["localhost", "127.0.0.1", "::1"]);
        assert!(config.allowed_origins.is_empty());
        assert_eq!(
            config.max_request_body_bytes,
            DEFAULT_MAX_REQUEST_BODY_BYTES
        );
        assert!(!config.stateless_protocol_metadata_required);
        assert!(config.session_store.is_none());
    }

    #[test]
    fn legacy_session_mode_controls_legacy_protocol_sessions() {
        let config = ServerConfig::new()
            .with_legacy_session_mode(false)
            .to_rmcp_config();

        assert!(!config.legacy_session_mode);
    }

    #[allow(deprecated)]
    #[test]
    fn stateful_mode_alias_forwards_to_legacy_session_mode() {
        let config = ServerConfig::new()
            .with_stateful_mode(false)
            .to_rmcp_config();

        assert!(!config.legacy_session_mode);
    }

    #[test]
    fn explicit_hosts_replace_loopback_default() {
        let config = ServerConfig::new()
            .with_allowed_hosts(["mcp.example.com"])
            .to_rmcp_config();

        assert_eq!(config.allowed_hosts, ["mcp.example.com"]);
    }

    #[test]
    fn disabling_host_validation_allows_any_host() {
        let config = ServerConfig::new().disable_allowed_hosts().to_rmcp_config();

        assert!(config.allowed_hosts.is_empty());
    }

    #[test]
    fn empty_hosts_list_keeps_loopback_default() {
        let config = ServerConfig::new()
            .with_allowed_hosts(Vec::<String>::new())
            .to_rmcp_config();

        assert_eq!(config.allowed_hosts, ["localhost", "127.0.0.1", "::1"]);
    }

    #[test]
    fn allowed_origins_are_passed_through() {
        let config = ServerConfig::new()
            .with_allowed_origins(["https://app.example.com"])
            .to_rmcp_config();

        assert_eq!(config.allowed_origins, ["https://app.example.com"]);
    }

    #[test]
    fn request_body_limit_and_strict_metadata_are_configurable() {
        let config = ServerConfig::new()
            .with_max_request_body_bytes(1024)
            .with_stateless_protocol_metadata_required(true)
            .to_rmcp_config();

        assert_eq!(config.max_request_body_bytes, 1024);
        assert!(config.stateless_protocol_metadata_required);
    }

    #[test]
    fn session_store_and_event_store_are_retained() {
        let config = ServerConfig::new()
            .with_session_store(Arc::new(NoopSessionStore))
            .with_event_store(Arc::new(NoopEventStore));

        assert!(config.session_store.is_some());
        assert!(config.event_store().is_some());

        let rmcp_config = config.to_rmcp_config();
        assert!(rmcp_config.session_store.is_some());
    }
}
