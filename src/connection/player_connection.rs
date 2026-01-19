use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc, oneshot};
use tokio::time::sleep;
use tokio_rustls::client::TlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tracing::{debug, error, info, warn};

use crate::error::{Error, Result};
use crate::message::{self, API_KEY, Message, RequestHeader, WEBSOCKET_PROTOCOL};
use crate::tls;

type WsWriter = SplitSink<WebSocketStream<TlsStream<TcpStream>>, WsMessage>;
type WsReader = SplitStream<WebSocketStream<TlsStream<TcpStream>>>;

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
}

/// Events emitted by the connection
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    /// Connection established
    Connected,
    /// Connection lost, will attempt reconnect
    Disconnected,
    /// Reconnection attempt starting
    Reconnecting { attempt: u32 },
    /// Received a message (event or unsolicited response)
    Message(Box<Message>),
}

/// Pending command waiting for response
struct PendingCommand {
    response_tx: oneshot::Sender<Result<Message>>,
}

/// Configuration for a player connection
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Base delay for exponential backoff (default: 1 second)
    pub reconnect_base_delay: Duration,
    /// Maximum delay for exponential backoff (default: 30 seconds)
    pub reconnect_max_delay: Duration,
    /// Maximum reconnection attempts (None = infinite)
    pub max_reconnect_attempts: Option<u32>,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            reconnect_base_delay: Duration::from_secs(1),
            reconnect_max_delay: Duration::from_secs(30),
            max_reconnect_attempts: None,
        }
    }
}

/// A WebSocket connection to a single Sonos player.
///
/// This type is cheaply cloneable - clones share the same underlying connection.
#[derive(Clone)]
pub struct PlayerConnection {
    /// Player hostname (e.g., "Sonos-XXXX.local")
    hostname: String,
    /// Player port (typically 1443)
    port: u16,
    /// WebSocket path (typically "/websocket/api")
    ws_path: String,
    /// Connection configuration
    config: ConnectionConfig,
    /// Current connection state
    state: Arc<RwLock<ConnectionState>>,
    /// Channel to send messages to the write task
    write_tx: Arc<Mutex<Option<mpsc::Sender<WsMessage>>>>,
    /// Pending commands waiting for responses
    pending_commands: Arc<Mutex<HashMap<String, PendingCommand>>>,
    /// Broadcast channel for connection events
    event_tx: broadcast::Sender<ConnectionEvent>,
    /// Handle to the connection task
    task_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl PlayerConnection {
    /// Create a new player connection (does not connect immediately)
    pub fn new(hostname: &str, port: u16, ws_path: &str) -> Self {
        Self::with_config(hostname, port, ws_path, ConnectionConfig::default())
    }

    /// Create a new player connection with custom configuration
    pub fn with_config(hostname: &str, port: u16, ws_path: &str, config: ConnectionConfig) -> Self {
        let (event_tx, _) = broadcast::channel(64);

        Self {
            hostname: hostname.to_string(),
            port,
            ws_path: ws_path.to_string(),
            config,
            state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
            write_tx: Arc::new(Mutex::new(None)),
            pending_commands: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            task_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the current connection state
    pub async fn state(&self) -> ConnectionState {
        *self.state.read().await
    }

    /// Subscribe to connection events
    pub fn events(&self) -> broadcast::Receiver<ConnectionEvent> {
        self.event_tx.subscribe()
    }

    /// Connect to the player
    pub async fn connect(&self) -> Result<()> {
        // Check if already connected or connecting
        {
            let state = self.state.read().await;
            if *state == ConnectionState::Connected || *state == ConnectionState::Connecting {
                return Ok(());
            }
        }

        // Set state to connecting
        *self.state.write().await = ConnectionState::Connecting;

        // Spawn the connection task
        let conn = self.clone_internals();
        let handle = tokio::spawn(async move {
            connection_task(conn).await;
        });

        *self.task_handle.lock().await = Some(handle);

        Ok(())
    }

    /// Send a command and wait for the response
    pub async fn send_command(&self, header: RequestHeader, body: Value) -> Result<Message> {
        let cmd_id = header.cmd_id.clone();
        let msg_text = message::build_message(&header, &body)?;

        // Create response channel
        let (tx, rx) = oneshot::channel();

        // Register pending command
        {
            let mut pending = self.pending_commands.lock().await;
            pending.insert(cmd_id.clone(), PendingCommand { response_tx: tx });
        }

        // Send message
        {
            let write_tx = self.write_tx.lock().await;
            if let Some(tx) = write_tx.as_ref() {
                tx.send(WsMessage::Text(msg_text.into()))
                    .await
                    .map_err(|_| Error::ConnectionClosed)?;
            } else {
                // Clean up pending command
                self.pending_commands.lock().await.remove(&cmd_id);
                return Err(Error::ConnectionClosed);
            }
        }

        // Wait for response with timeout
        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                // Channel closed
                self.pending_commands.lock().await.remove(&cmd_id);
                Err(Error::ConnectionClosed)
            }
            Err(_) => {
                // Timeout
                self.pending_commands.lock().await.remove(&cmd_id);
                Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "command timed out",
                )))
            }
        }
    }

    /// Send a command with empty body
    pub async fn send_command_empty(&self, header: RequestHeader) -> Result<Message> {
        self.send_command(header, serde_json::json!({})).await
    }

    /// Subscribe to a namespace
    pub async fn subscribe(
        &self,
        namespace: &str,
        household_id: &str,
        group_id: Option<&str>,
        player_id: Option<&str>,
    ) -> Result<Message> {
        let header = if let Some(gid) = group_id {
            RequestHeader::group(namespace, "subscribe", household_id, gid)
        } else if let Some(pid) = player_id {
            RequestHeader::player(namespace, "subscribe", household_id, pid)
        } else {
            RequestHeader::household(namespace, "subscribe", household_id)
        };

        self.send_command_empty(header).await
    }

    /// Disconnect from the player gracefully
    pub async fn disconnect(&self) {
        // Send WebSocket close frame if connected
        {
            let write_tx = self.write_tx.lock().await;
            if let Some(tx) = write_tx.as_ref() {
                let _ = tx.send(WsMessage::Close(None)).await;
                // Give it a moment to send
                drop(write_tx);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        // Cancel the task
        if let Some(handle) = self.task_handle.lock().await.take() {
            handle.abort();
        }

        // Clear the write channel
        *self.write_tx.lock().await = None;

        // Clear pending commands
        let mut pending = self.pending_commands.lock().await;
        for (_, cmd) in pending.drain() {
            let _ = cmd.response_tx.send(Err(Error::ConnectionClosed));
        }

        *self.state.write().await = ConnectionState::Disconnected;
    }

    /// Clone internal state for the connection task
    fn clone_internals(&self) -> ConnectionInternals {
        ConnectionInternals {
            hostname: self.hostname.clone(),
            port: self.port,
            ws_path: self.ws_path.clone(),
            config: self.config.clone(),
            state: Arc::clone(&self.state),
            write_tx: Arc::clone(&self.write_tx),
            pending_commands: Arc::clone(&self.pending_commands),
            event_tx: self.event_tx.clone(),
        }
    }
}

/// Internal state passed to the connection task
struct ConnectionInternals {
    hostname: String,
    port: u16,
    ws_path: String,
    config: ConnectionConfig,
    state: Arc<RwLock<ConnectionState>>,
    write_tx: Arc<Mutex<Option<mpsc::Sender<WsMessage>>>>,
    pending_commands: Arc<Mutex<HashMap<String, PendingCommand>>>,
    event_tx: broadcast::Sender<ConnectionEvent>,
}

/// Main connection task that handles connect, read loop, and reconnection
async fn connection_task(conn: ConnectionInternals) {
    let mut attempt = 0u32;

    loop {
        // Calculate backoff delay
        if attempt > 0 {
            let delay = calculate_backoff(
                attempt,
                conn.config.reconnect_base_delay,
                conn.config.reconnect_max_delay,
            );

            *conn.state.write().await = ConnectionState::Reconnecting;
            let _ = conn
                .event_tx
                .send(ConnectionEvent::Reconnecting { attempt });

            info!(
                "Reconnecting to {} in {:?} (attempt {})",
                conn.hostname, delay, attempt
            );
            sleep(delay).await;
        }

        // Check max attempts
        if let Some(max) = conn.config.max_reconnect_attempts
            && attempt >= max
        {
            error!(
                "Max reconnection attempts ({}) reached for {}",
                max, conn.hostname
            );
            *conn.state.write().await = ConnectionState::Disconnected;
            return;
        }

        attempt += 1;

        // Attempt connection
        match establish_connection(&conn.hostname, conn.port, &conn.ws_path).await {
            Ok((writer, reader)) => {
                info!("Connected to {}", conn.hostname);
                attempt = 0; // Reset attempt counter on successful connection

                *conn.state.write().await = ConnectionState::Connected;
                let _ = conn.event_tx.send(ConnectionEvent::Connected);

                // Set up write channel
                let (write_tx, write_rx) = mpsc::channel(32);
                *conn.write_tx.lock().await = Some(write_tx);

                // Run read/write loops
                run_connection(
                    writer,
                    reader,
                    write_rx,
                    &conn.pending_commands,
                    &conn.event_tx,
                )
                .await;

                // Connection lost
                warn!("Connection to {} lost", conn.hostname);
                *conn.write_tx.lock().await = None;
                let _ = conn.event_tx.send(ConnectionEvent::Disconnected);

                // Fail any pending commands
                let mut pending = conn.pending_commands.lock().await;
                for (_, cmd) in pending.drain() {
                    let _ = cmd.response_tx.send(Err(Error::ConnectionClosed));
                }
            }
            Err(e) => {
                warn!("Failed to connect to {}: {}", conn.hostname, e);
            }
        }
    }
}

/// Establish TLS + WebSocket connection
async fn establish_connection(
    hostname: &str,
    port: u16,
    ws_path: &str,
) -> Result<(WsWriter, WsReader)> {
    // Connect with TLS
    let tls_stream = tls::connect_tls(hostname, port).await?;

    // Build WebSocket request
    let ws_url = format!("wss://{}:{}{}", hostname, port, ws_path);
    debug!("WebSocket handshake to {}", ws_url);

    let mut request = ws_url.into_client_request()?;

    // Add required headers
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static(WEBSOCKET_PROTOCOL),
    );
    request
        .headers_mut()
        .insert("X-Sonos-Api-Key", HeaderValue::from_static(API_KEY));

    // Perform WebSocket handshake
    let (ws_stream, _response) = tokio_tungstenite::client_async(request, tls_stream).await?;

    // Split into read/write
    let (writer, reader) = ws_stream.split();

    Ok((writer, reader))
}

/// Run the read and write loops for an established connection
async fn run_connection(
    mut writer: WsWriter,
    mut reader: WsReader,
    mut write_rx: mpsc::Receiver<WsMessage>,
    pending_commands: &Arc<Mutex<HashMap<String, PendingCommand>>>,
    event_tx: &broadcast::Sender<ConnectionEvent>,
) {
    loop {
        tokio::select! {
            // Handle outgoing messages
            Some(msg) = write_rx.recv() => {
                let is_close = matches!(msg, WsMessage::Close(_));
                if let Err(e) = writer.send(msg).await {
                    error!("Failed to send WebSocket message: {}", e);
                    return;
                }
                // Exit after sending close frame
                if is_close {
                    debug!("Sent WebSocket close frame");
                    return;
                }
            }

            // Handle incoming messages
            msg = reader.next() => {
                match msg {
                    Some(Ok(WsMessage::Text(text))) => {
                        handle_incoming_message(&text, pending_commands, event_tx).await;
                    }
                    Some(Ok(WsMessage::Ping(data))) => {
                        if let Err(e) = writer.send(WsMessage::Pong(data)).await {
                            error!("Failed to send pong: {}", e);
                            return;
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) => {
                        debug!("Received WebSocket close");
                        return;
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error: {}", e);
                        return;
                    }
                    None => {
                        debug!("WebSocket stream ended");
                        return;
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Handle an incoming WebSocket message
async fn handle_incoming_message(
    text: &str,
    pending_commands: &Arc<Mutex<HashMap<String, PendingCommand>>>,
    event_tx: &broadcast::Sender<ConnectionEvent>,
) {
    let msg = match Message::parse(text) {
        Ok(m) => m,
        Err(e) => {
            warn!("Failed to parse message: {}", e);
            return;
        }
    };

    // Check if this is a response to a pending command
    if let Some(cmd_id) = &msg.header.cmd_id {
        let mut pending = pending_commands.lock().await;
        if let Some(cmd) = pending.remove(cmd_id) {
            let _ = cmd.response_tx.send(Ok(msg));
            return;
        }
    }

    // Otherwise, broadcast as an event
    let _ = event_tx.send(ConnectionEvent::Message(Box::new(msg)));
}

/// Calculate exponential backoff with jitter
fn calculate_backoff(attempt: u32, base: Duration, max: Duration) -> Duration {
    let exp_delay = base.saturating_mul(2u32.saturating_pow(attempt.saturating_sub(1)));
    let capped = exp_delay.min(max);

    // Add jitter (±25%)
    let jitter_range = capped.as_millis() as u64 / 4;
    if jitter_range > 0 {
        use std::time::SystemTime;
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64;
        let jitter = (nanos % (jitter_range * 2)) as i64 - jitter_range as i64;
        Duration::from_millis((capped.as_millis() as i64 + jitter).max(0) as u64)
    } else {
        capped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // ConnectionState
    // ========================================================================

    #[test]
    fn test_connection_state_equality() {
        assert_eq!(ConnectionState::Disconnected, ConnectionState::Disconnected);
        assert_ne!(ConnectionState::Disconnected, ConnectionState::Connected);
    }

    #[test]
    fn test_connection_state_clone() {
        let state = ConnectionState::Reconnecting;
        let cloned = state;
        assert_eq!(state, cloned);
    }

    // ========================================================================
    // ConnectionConfig
    // ========================================================================

    #[test]
    fn test_connection_config_default() {
        let config = ConnectionConfig::default();
        assert_eq!(config.reconnect_base_delay, Duration::from_secs(1));
        assert_eq!(config.reconnect_max_delay, Duration::from_secs(30));
        assert!(config.max_reconnect_attempts.is_none());
    }

    #[test]
    fn test_connection_config_custom() {
        let config = ConnectionConfig {
            reconnect_base_delay: Duration::from_millis(500),
            reconnect_max_delay: Duration::from_secs(60),
            max_reconnect_attempts: Some(5),
        };
        assert_eq!(config.reconnect_base_delay, Duration::from_millis(500));
        assert_eq!(config.max_reconnect_attempts, Some(5));
    }

    // ========================================================================
    // calculate_backoff
    // ========================================================================

    #[test]
    fn test_backoff_first_attempt() {
        let base = Duration::from_secs(1);
        let max = Duration::from_secs(30);

        // First attempt (attempt=1): base * 2^0 = 1 sec (with jitter)
        let delay = calculate_backoff(1, base, max);
        // Should be within 750ms - 1250ms (±25% jitter)
        assert!(delay >= Duration::from_millis(750));
        assert!(delay <= Duration::from_millis(1250));
    }

    #[test]
    fn test_backoff_exponential_growth() {
        let base = Duration::from_secs(1);
        let max = Duration::from_secs(30);

        // Second attempt: base * 2^1 = 2 sec (±25% jitter = 1.5-2.5)
        let delay2 = calculate_backoff(2, base, max);
        assert!(delay2 >= Duration::from_millis(1500));
        assert!(delay2 <= Duration::from_millis(2500));

        // Third attempt: base * 2^2 = 4 sec (±25% jitter = 3-5)
        let delay3 = calculate_backoff(3, base, max);
        assert!(delay3 >= Duration::from_millis(3000));
        assert!(delay3 <= Duration::from_millis(5000));
    }

    #[test]
    fn test_backoff_capped_at_max() {
        let base = Duration::from_secs(1);
        let max = Duration::from_secs(10);

        // Very high attempt should be capped at max (10 sec ± 25% = 7.5-12.5)
        let delay = calculate_backoff(100, base, max);
        assert!(delay >= Duration::from_millis(7500));
        assert!(delay <= Duration::from_millis(12500));
    }

    #[test]
    fn test_backoff_zero_base() {
        let base = Duration::ZERO;
        let max = Duration::from_secs(30);

        // Zero base should result in zero delay (no jitter range)
        let delay = calculate_backoff(5, base, max);
        assert_eq!(delay, Duration::ZERO);
    }

    #[test]
    fn test_backoff_attempt_zero() {
        let base = Duration::from_secs(1);
        let max = Duration::from_secs(30);

        // Attempt 0 uses saturating_sub(1) which becomes 0, so 2^0 = 1
        // Result: base * 1 = 1 sec (with jitter)
        let delay = calculate_backoff(0, base, max);
        assert!(delay >= Duration::from_millis(750));
        assert!(delay <= Duration::from_millis(1250));
    }

    // ========================================================================
    // PlayerConnection creation
    // ========================================================================

    #[test]
    fn test_player_connection_new() {
        let conn = PlayerConnection::new("Sonos-123.local", 1443, "/websocket/api");
        assert_eq!(conn.hostname, "Sonos-123.local");
        assert_eq!(conn.port, 1443);
        assert_eq!(conn.ws_path, "/websocket/api");
    }

    #[test]
    fn test_player_connection_with_config() {
        let config = ConnectionConfig {
            reconnect_base_delay: Duration::from_millis(100),
            reconnect_max_delay: Duration::from_secs(5),
            max_reconnect_attempts: Some(3),
        };
        let conn = PlayerConnection::with_config("test.local", 8080, "/ws", config);
        assert_eq!(conn.hostname, "test.local");
        assert_eq!(conn.port, 8080);
        assert_eq!(conn.config.max_reconnect_attempts, Some(3));
    }

    #[tokio::test]
    async fn test_player_connection_initial_state() {
        let conn = PlayerConnection::new("test.local", 1443, "/ws");
        assert_eq!(conn.state().await, ConnectionState::Disconnected);
    }

    #[tokio::test]
    async fn test_player_connection_clone_shares_state() {
        let conn1 = PlayerConnection::new("test.local", 1443, "/ws");
        let conn2 = conn1.clone();

        // Both should see same initial state
        assert_eq!(conn1.state().await, ConnectionState::Disconnected);
        assert_eq!(conn2.state().await, ConnectionState::Disconnected);

        // Modify state through one reference
        *conn1.state.write().await = ConnectionState::Connected;

        // Both should see the change
        assert_eq!(conn1.state().await, ConnectionState::Connected);
        assert_eq!(conn2.state().await, ConnectionState::Connected);
    }

    // ========================================================================
    // ConnectionEvent
    // ========================================================================

    #[test]
    fn test_connection_event_debug() {
        let event = ConnectionEvent::Reconnecting { attempt: 3 };
        let debug_str = format!("{:?}", event);
        assert!(debug_str.contains("Reconnecting"));
        assert!(debug_str.contains("3"));
    }

    #[tokio::test]
    async fn test_event_subscription() {
        let conn = PlayerConnection::new("test.local", 1443, "/ws");
        let mut rx = conn.events();

        // Send event through the internal sender
        let _ = conn.event_tx.send(ConnectionEvent::Connected);

        // Subscriber should receive it
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, ConnectionEvent::Connected));
    }

    // ========================================================================
    // send_command without connection (error path)
    // ========================================================================

    #[tokio::test]
    async fn test_send_command_when_disconnected() {
        let conn = PlayerConnection::new("test.local", 1443, "/ws");

        let header = RequestHeader::household("test", "test", "HH1");
        let result = conn.send_command(header, serde_json::json!({})).await;

        // Should fail because write_tx is None (not connected)
        assert!(result.is_err());
        match result {
            Err(Error::ConnectionClosed) => {} // expected
            other => panic!("Expected ConnectionClosed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_send_command_cleans_up_pending_on_failure() {
        let conn = PlayerConnection::new("test.local", 1443, "/ws");

        let header = RequestHeader::household("test", "test", "HH1");
        let cmd_id = header.cmd_id.clone();

        let _ = conn.send_command(header, serde_json::json!({})).await;

        // Verify pending command was cleaned up
        let pending = conn.pending_commands.lock().await;
        assert!(!pending.contains_key(&cmd_id));
    }

    // ========================================================================
    // disconnect
    // ========================================================================

    #[tokio::test]
    async fn test_disconnect_clears_state() {
        let conn = PlayerConnection::new("test.local", 1443, "/ws");

        // Simulate connected state
        *conn.state.write().await = ConnectionState::Connected;

        conn.disconnect().await;

        assert_eq!(conn.state().await, ConnectionState::Disconnected);
        assert!(conn.write_tx.lock().await.is_none());
        assert!(conn.pending_commands.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_disconnect_fails_pending_commands() {
        let conn = PlayerConnection::new("test.local", 1443, "/ws");

        // Manually add a pending command
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = conn.pending_commands.lock().await;
            pending.insert("test_cmd".to_string(), PendingCommand { response_tx: tx });
        }

        conn.disconnect().await;

        // The pending command should have received an error
        let result = rx.await;
        assert!(result.is_ok()); // Channel received something
        let inner = result.unwrap();
        assert!(matches!(inner, Err(Error::ConnectionClosed)));
    }
}
