use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, trace};

use crate::connection::{ConnectionConfig, ConnectionEvent, PlayerConnection};
use crate::error::{Error, Result};
use crate::message::RequestHeader;
use crate::types::{
    CreateSessionRequest, GroupId, GroupVolume, HouseholdId, LoadCloudQueueRequest, MetadataStatus,
    MusicServiceAccount, PlayState, PlaybackError, PlaybackSession, PlaybackStatus, PlayerEvent,
    PlayerId, PlayerVolume,
};

/// A Sonos player that can be connected on-demand.
///
/// Players are created from topology and start disconnected. Call `connect()`
/// to establish a WebSocket connection for playback control.
///
/// This type is cheaply cloneable - clones share the same connection state.
#[derive(Clone)]
pub struct Player {
    id: PlayerId,
    name: String,
    websocket_url: String,
    household_id: HouseholdId,
    connection: Arc<RwLock<Option<PlayerConnection>>>,
    event_tx: broadcast::Sender<PlayerEvent>,
}

impl Player {
    /// Create a new Player from topology info (starts disconnected).
    pub fn new(
        id: PlayerId,
        name: String,
        websocket_url: String,
        household_id: HouseholdId,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            id,
            name,
            websocket_url,
            household_id,
            connection: Arc::new(RwLock::new(None)),
            event_tx,
        }
    }

    /// Get the player ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the player name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the household ID
    pub fn household_id(&self) -> &str {
        &self.household_id
    }

    /// Get the WebSocket URL for this player
    pub fn websocket_url(&self) -> &str {
        &self.websocket_url
    }

    /// Check if the player is currently connected
    pub async fn is_connected(&self) -> bool {
        self.connection.read().await.is_some()
    }

    /// Subscribe to player events (connection state, playback, volume, metadata)
    pub fn events(&self) -> broadcast::Receiver<PlayerEvent> {
        self.event_tx.subscribe()
    }

    /// Connect to this player's WebSocket endpoint.
    ///
    /// Uses the .local hostname derived from player ID for TLS verification.
    /// If already connected, this is a no-op.
    pub async fn connect(&self) -> Result<()> {
        // Check if already connected
        if self.connection.read().await.is_some() {
            debug!("Player {} already connected", self.id);
            return Ok(());
        }

        // Parse websocket URL for port and path, but use .local hostname for TLS
        let (_ip_host, port, path) = Self::parse_websocket_url(&self.websocket_url)?;

        // Derive hostname from player ID: RINCON_38420B91F87E01400 -> Sonos-38420B91F87E.local
        let hostname = Self::derive_hostname(&self.id)?;

        debug!(
            "Connecting to player {} at {}:{}{}",
            self.id, hostname, port, path
        );

        let conn =
            PlayerConnection::with_config(&hostname, port, &path, ConnectionConfig::default());

        conn.connect().await?;

        // Wait for connection to be established
        Self::wait_for_connection(&conn, std::time::Duration::from_secs(10)).await?;

        // Start forwarding connection events to player events
        let event_tx = self.event_tx.clone();
        let mut conn_events = conn.events();
        let player_id = self.id.clone();

        tokio::spawn(async move {
            while let Ok(event) = conn_events.recv().await {
                match event {
                    ConnectionEvent::Connected => {
                        let _ = event_tx.send(PlayerEvent::Connected);
                    }
                    ConnectionEvent::Disconnected => {
                        let _ = event_tx.send(PlayerEvent::Disconnected);
                    }
                    ConnectionEvent::Reconnecting { attempt } => {
                        debug!("Player {} reconnecting (attempt {})", player_id, attempt);
                        let _ = event_tx.send(PlayerEvent::Reconnecting);
                    }
                    ConnectionEvent::Message(msg) => {
                        // Parse and forward playback/volume/metadata events
                        Self::handle_message(&event_tx, *msg);
                    }
                }
            }
        });

        *self.connection.write().await = Some(conn);
        let _ = self.event_tx.send(PlayerEvent::Connected);

        Ok(())
    }

    /// Disconnect from this player
    pub async fn disconnect(&self) {
        if let Some(conn) = self.connection.write().await.take() {
            conn.disconnect().await;
            let _ = self.event_tx.send(PlayerEvent::Disconnected);
        }
    }

    /// Wait for a connection to be established
    async fn wait_for_connection(
        conn: &PlayerConnection,
        timeout: std::time::Duration,
    ) -> Result<()> {
        use crate::connection::ConnectionState;

        let start = std::time::Instant::now();
        loop {
            if conn.state().await == ConnectionState::Connected {
                return Ok(());
            }

            if start.elapsed() > timeout {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "timed out waiting for connection",
                )));
            }

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// Derive the .local hostname from player ID
    /// RINCON_38420B91F87E01400 -> Sonos-38420B91F87E.local
    fn derive_hostname(player_id: &str) -> Result<String> {
        // Player ID format: RINCON_XXXXXXXXXXXX01400
        // The MAC address is the 12 hex chars after RINCON_
        let mac = player_id
            .strip_prefix("RINCON_")
            .and_then(|s| s.get(..12))
            .ok_or_else(|| {
                Error::InvalidUrl(format!(
                    "cannot derive hostname from player ID: {}",
                    player_id
                ))
            })?;

        Ok(format!("Sonos-{}.local", mac))
    }

    /// Parse a websocket URL into (hostname, port, path)
    fn parse_websocket_url(url: &str) -> Result<(String, u16, String)> {
        // Expected format: wss://hostname:port/path
        let url = url
            .strip_prefix("wss://")
            .or_else(|| url.strip_prefix("ws://"))
            .ok_or_else(|| Error::InvalidUrl(format!("invalid websocket URL: {}", url)))?;

        // Split host:port from path
        let (host_port, path) = url
            .split_once('/')
            .map(|(hp, p)| (hp, format!("/{}", p)))
            .unwrap_or((url, "/websocket/api".to_string()));

        // Split host from port
        let (hostname, port_str) = host_port
            .rsplit_once(':')
            .ok_or_else(|| Error::InvalidUrl(format!("missing port in URL: {}", url)))?;

        let port: u16 = port_str
            .parse()
            .map_err(|_| Error::InvalidUrl(format!("invalid port: {}", port_str)))?;

        Ok((hostname.to_string(), port, path))
    }

    /// Handle incoming WebSocket messages and emit appropriate PlayerEvents
    fn handle_message(event_tx: &broadcast::Sender<PlayerEvent>, msg: crate::message::Message) {
        let namespace = &msg.header.namespace;

        trace!(
            namespace = %namespace,
            name = ?msg.header.name,
            response_type = ?msg.header.response_type,
            "Received WebSocket message"
        );

        match namespace.as_str() {
            "playback" => {
                // Check the event name to differentiate playbackError from playbackStatus
                match msg.header.name.as_deref() {
                    Some("playbackError") => {
                        debug!("Received playbackError event: {:?}", msg.body);
                        if let Ok(error) = serde_json::from_value::<PlaybackError>(msg.body) {
                            let _ = event_tx.send(PlayerEvent::PlaybackError(error));
                        }
                    }
                    _ => {
                        if let Ok(status) = serde_json::from_value::<PlaybackStatus>(msg.body) {
                            let _ = event_tx.send(PlayerEvent::PlaybackChanged(status));
                        }
                    }
                }
            }
            "groupVolume" => {
                if let Ok(volume) = serde_json::from_value::<GroupVolume>(msg.body) {
                    let _ = event_tx.send(PlayerEvent::VolumeChanged(volume));
                }
            }
            "playerVolume" => {
                if let Ok(volume) = serde_json::from_value::<PlayerVolume>(msg.body) {
                    let _ = event_tx.send(PlayerEvent::PlayerVolumeChanged(volume));
                }
            }
            "playbackMetadata" => {
                if let Ok(metadata) = serde_json::from_value::<MetadataStatus>(msg.body) {
                    let _ = event_tx.send(PlayerEvent::MetadataChanged(metadata));
                }
            }
            _ => {}
        }
    }

    /// Get the connection, returning an error if not connected
    async fn require_connection(&self) -> Result<PlayerConnection> {
        self.connection
            .read()
            .await
            .clone()
            .ok_or_else(|| Error::NotConnected(self.id.clone()))
    }

    // =========================================================================
    // Playback Commands (require connection + group_id)
    // =========================================================================

    /// Start or resume playback
    pub async fn play(&self, group_id: &GroupId) -> Result<()> {
        let conn = self.require_connection().await?;
        let header = RequestHeader::group("playback", "play", &self.household_id, group_id);
        conn.send_command_empty(header).await?;
        Ok(())
    }

    /// Pause playback
    pub async fn pause(&self, group_id: &GroupId) -> Result<()> {
        let conn = self.require_connection().await?;
        let header = RequestHeader::group("playback", "pause", &self.household_id, group_id);
        conn.send_command_empty(header).await?;
        Ok(())
    }

    /// Stop playback
    pub async fn stop(&self, group_id: &GroupId) -> Result<()> {
        let conn = self.require_connection().await?;
        let header = RequestHeader::group("playback", "stop", &self.household_id, group_id);
        conn.send_command_empty(header).await?;
        Ok(())
    }

    /// Skip to the next track
    pub async fn skip_next(&self, group_id: &GroupId) -> Result<()> {
        let conn = self.require_connection().await?;
        let header =
            RequestHeader::group("playback", "skipToNextTrack", &self.household_id, group_id);
        conn.send_command_empty(header).await?;
        Ok(())
    }

    /// Skip to the previous track
    pub async fn skip_prev(&self, group_id: &GroupId) -> Result<()> {
        let conn = self.require_connection().await?;
        let header = RequestHeader::group(
            "playback",
            "skipToPreviousTrack",
            &self.household_id,
            group_id,
        );
        conn.send_command_empty(header).await?;
        Ok(())
    }

    /// Toggle play/pause based on current state
    pub async fn toggle_play_pause(&self, group_id: &GroupId) -> Result<()> {
        let conn = self.require_connection().await?;
        let header =
            RequestHeader::group("playback", "togglePlayPause", &self.household_id, group_id);
        conn.send_command_empty(header).await?;
        Ok(())
    }

    // =========================================================================
    // Volume Commands (require connection + group_id)
    // =========================================================================

    /// Get the current group volume
    pub async fn get_volume(&self, group_id: &GroupId) -> Result<GroupVolume> {
        let conn = self.require_connection().await?;
        let header = RequestHeader::group("groupVolume", "getVolume", &self.household_id, group_id);
        let response = conn.send_command_empty(header).await?;
        let volume: GroupVolume = serde_json::from_value(response.body)?;
        Ok(volume)
    }

    /// Set the group volume (0-100)
    pub async fn set_volume(&self, group_id: &GroupId, volume: u8) -> Result<()> {
        let conn = self.require_connection().await?;
        let header = RequestHeader::group("groupVolume", "setVolume", &self.household_id, group_id);
        let body = serde_json::json!({ "volume": volume.min(100) });
        conn.send_command(header, body).await?;
        Ok(())
    }

    /// Set the group mute state
    pub async fn set_mute(&self, group_id: &GroupId, muted: bool) -> Result<()> {
        let conn = self.require_connection().await?;
        let header = RequestHeader::group("groupVolume", "setMute", &self.household_id, group_id);
        let body = serde_json::json!({ "muted": muted });
        conn.send_command(header, body).await?;
        Ok(())
    }

    /// Adjust volume relative to current level
    pub async fn set_relative_volume(&self, group_id: &GroupId, delta: i8) -> Result<()> {
        let conn = self.require_connection().await?;
        let header = RequestHeader::group(
            "groupVolume",
            "setRelativeVolume",
            &self.household_id,
            group_id,
        );
        let body = serde_json::json!({ "volumeDelta": delta });
        conn.send_command(header, body).await?;
        Ok(())
    }

    // =========================================================================
    // Individual Player Volume Commands (require connection only)
    // =========================================================================

    /// Get the volume of this individual player
    pub async fn get_player_volume(&self) -> Result<PlayerVolume> {
        let conn = self.require_connection().await?;
        let header =
            RequestHeader::player("playerVolume", "getVolume", &self.household_id, &self.id);
        let response = conn.send_command_empty(header).await?;
        let volume: PlayerVolume = serde_json::from_value(response.body)?;
        Ok(volume)
    }

    /// Set the volume of this individual player (0-100)
    pub async fn set_player_volume(&self, volume: u8) -> Result<()> {
        let conn = self.require_connection().await?;
        let header =
            RequestHeader::player("playerVolume", "setVolume", &self.household_id, &self.id);
        let body = serde_json::json!({ "volume": volume });
        conn.send_command(header, body).await?;
        Ok(())
    }

    /// Set the mute state of this individual player
    pub async fn set_player_mute(&self, muted: bool) -> Result<()> {
        let conn = self.require_connection().await?;
        let header =
            RequestHeader::player("playerVolume", "setMute", &self.household_id, &self.id);
        let body = serde_json::json!({ "muted": muted });
        conn.send_command(header, body).await?;
        Ok(())
    }

    /// Adjust this player's volume relative to current level
    pub async fn set_player_relative_volume(&self, delta: i8) -> Result<()> {
        let conn = self.require_connection().await?;
        let header = RequestHeader::player(
            "playerVolume",
            "setRelativeVolume",
            &self.household_id,
            &self.id,
        );
        let body = serde_json::json!({ "volumeDelta": delta });
        conn.send_command(header, body).await?;
        Ok(())
    }

    /// Subscribe to volume changes for this individual player.
    /// Events will be received via the `events()` channel as `PlayerEvent::PlayerVolumeChanged`.
    pub async fn subscribe_player_volume(&self) -> Result<()> {
        let conn = self.require_connection().await?;
        conn.subscribe("playerVolume", &self.household_id, None, Some(&self.id))
            .await?;
        Ok(())
    }

    // =========================================================================
    // Status Commands (require connection + group_id)
    // =========================================================================

    /// Get current playback status
    pub async fn get_playback_status(&self, group_id: &GroupId) -> Result<PlaybackStatus> {
        let conn = self.require_connection().await?;
        let header = RequestHeader::group(
            "playback",
            "getPlaybackStatus",
            &self.household_id,
            group_id,
        );
        let response = conn.send_command_empty(header).await?;
        let status: PlaybackStatus = serde_json::from_value(response.body)?;
        Ok(status)
    }

    /// Get current track metadata
    pub async fn get_metadata(&self, group_id: &GroupId) -> Result<MetadataStatus> {
        let conn = self.require_connection().await?;
        let header = RequestHeader::group(
            "playbackMetadata",
            "getMetadataStatus",
            &self.household_id,
            group_id,
        );
        let response = conn.send_command_empty(header).await?;
        let metadata: MetadataStatus = serde_json::from_value(response.body)?;
        Ok(metadata)
    }

    /// Check if currently playing
    pub async fn is_playing(&self, group_id: &GroupId) -> Result<bool> {
        let status = self.get_playback_status(group_id).await?;
        Ok(status.state == PlayState::Playing)
    }

    // =========================================================================
    // Music Service Accounts
    // =========================================================================

    /// Match a music service user to their Sonos account.
    ///
    /// Returns the service account info including the `sn` value needed for queue URIs.
    pub async fn match_music_service_account(
        &self,
        user_id_hash_code: &str,
        service_id: &str,
        nickname: &str,
    ) -> Result<MusicServiceAccount> {
        let conn = self.require_connection().await?;

        let header = RequestHeader::household("musicServiceAccounts", "match", &self.household_id);

        let body = serde_json::json!({
            "userIdHashCode": user_id_hash_code,
            "serviceId": service_id,
            "nickname": nickname
        });

        let response = conn.send_command(header, body).await?;
        let account: MusicServiceAccount = serde_json::from_value(response.body)?;

        Ok(account)
    }

    // =========================================================================
    // Cloud Queue / Playback Session Commands
    // =========================================================================

    /// Create a playback session for a group.
    ///
    /// This establishes a session that can be used to control cloud queue playback.
    /// The returned `PlaybackSession` contains a `session_id` needed for subsequent
    /// cloud queue operations like `load_cloud_queue`.
    ///
    /// # Example
    /// ```no_run
    /// use sonos_websocket::CreateSessionRequest;
    ///
    /// # async fn example(player: sonos_websocket::Player) -> sonos_websocket::Result<()> {
    /// let group_id = "group_123".to_string();
    /// let session = player.create_playback_session(&group_id, CreateSessionRequest {
    ///     app_id: "com.example.myapp".to_string(),
    ///     app_context: "user_123".to_string(),
    ///     account_id: Some("sn_8".to_string()),  // Link to music service account
    ///     ..Default::default()
    /// }).await?;
    /// println!("Session ID: {}", session.session_id);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_playback_session(
        &self,
        group_id: &GroupId,
        request: CreateSessionRequest,
    ) -> Result<PlaybackSession> {
        let conn = self.require_connection().await?;

        let header = RequestHeader::group(
            "playbackSession",
            "createSession",
            &self.household_id,
            group_id,
        );

        let body = serde_json::to_value(&request)?;
        let response = conn.send_command(header, body).await?;
        let session: PlaybackSession = serde_json::from_value(response.body)?;

        Ok(session)
    }

    /// Load a cloud queue into the playback session.
    ///
    /// # Example
    /// ```no_run
    /// use sonos_websocket::{LoadCloudQueueRequest, Track, Artist};
    ///
    /// # async fn example(player: sonos_websocket::Player) -> sonos_websocket::Result<()> {
    /// player.load_cloud_queue("session_123", LoadCloudQueueRequest {
    ///     queue_base_url: "http://192.168.1.100:9443/queues/my-queue/v2.3/".to_string(),
    ///     item_id: Some("1".to_string()),
    ///     play_on_completion: Some(true),
    ///     ..Default::default()
    /// }).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn load_cloud_queue(
        &self,
        session_id: &str,
        request: LoadCloudQueueRequest,
    ) -> Result<()> {
        let conn = self.require_connection().await?;

        let header = RequestHeader::session("loadCloudQueue", &self.household_id, session_id);
        let body = serde_json::to_value(&request)?;

        conn.send_command(header, body).await?;
        Ok(())
    }

    // =========================================================================
    // Subscriptions (require connection + group_id)
    // =========================================================================

    /// Subscribe to playback state changes.
    /// Events will be received via the `events()` channel as `PlayerEvent::PlaybackChanged`.
    pub async fn subscribe_playback(&self, group_id: &GroupId) -> Result<()> {
        let conn = self.require_connection().await?;
        conn.subscribe("playback", &self.household_id, Some(group_id), None)
            .await?;
        Ok(())
    }

    /// Subscribe to volume changes.
    /// Events will be received via the `events()` channel as `PlayerEvent::VolumeChanged`.
    pub async fn subscribe_volume(&self, group_id: &GroupId) -> Result<()> {
        let conn = self.require_connection().await?;
        conn.subscribe("groupVolume", &self.household_id, Some(group_id), None)
            .await?;
        Ok(())
    }

    /// Subscribe to metadata (now playing) changes.
    /// Events will be received via the `events()` channel as `PlayerEvent::MetadataChanged`.
    pub async fn subscribe_metadata(&self, group_id: &GroupId) -> Result<()> {
        let conn = self.require_connection().await?;
        conn.subscribe("playbackMetadata", &self.household_id, Some(group_id), None)
            .await?;
        Ok(())
    }

    /// Subscribe to all playback-related events (playback, volume, metadata)
    pub async fn subscribe_all(&self, group_id: &GroupId) -> Result<()> {
        self.subscribe_playback(group_id).await?;
        self.subscribe_volume(group_id).await?;
        self.subscribe_metadata(group_id).await?;
        Ok(())
    }
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("websocket_url", &self.websocket_url)
            .field("household_id", &self.household_id)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Player creation and basic accessors
    // ========================================================================

    #[test]
    fn test_player_new() {
        let player = Player::new(
            "RINCON_123456789ABC01400".to_string(),
            "Living Room".to_string(),
            "wss://192.168.1.100:1443/websocket/api".to_string(),
            "Sonos_HH123".to_string(),
        );

        assert_eq!(player.id(), "RINCON_123456789ABC01400");
        assert_eq!(player.name(), "Living Room");
        assert_eq!(player.household_id(), "Sonos_HH123");
        assert_eq!(
            player.websocket_url(),
            "wss://192.168.1.100:1443/websocket/api"
        );
    }

    #[tokio::test]
    async fn test_player_starts_disconnected() {
        let player = Player::new(
            "RINCON_123".to_string(),
            "Test".to_string(),
            "wss://test:1443/ws".to_string(),
            "HH1".to_string(),
        );

        assert!(!player.is_connected().await);
    }

    #[tokio::test]
    async fn test_player_clone_shares_connection() {
        let player1 = Player::new(
            "RINCON_123".to_string(),
            "Test".to_string(),
            "wss://test:1443/ws".to_string(),
            "HH1".to_string(),
        );
        let player2 = player1.clone();

        // Both start disconnected
        assert!(!player1.is_connected().await);
        assert!(!player2.is_connected().await);

        // Simulate setting a connection through internal state
        *player1.connection.write().await = Some(PlayerConnection::new("test.local", 1443, "/ws"));

        // Both should now see connected
        assert!(player1.is_connected().await);
        assert!(player2.is_connected().await);
    }

    // ========================================================================
    // derive_hostname
    // ========================================================================

    #[test]
    fn test_derive_hostname_valid() {
        // Standard RINCON format
        let result = Player::derive_hostname("RINCON_38420B91F87E01400");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Sonos-38420B91F87E.local");
    }

    #[test]
    fn test_derive_hostname_different_mac() {
        let result = Player::derive_hostname("RINCON_AABBCCDDEEFF01400");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Sonos-AABBCCDDEEFF.local");
    }

    #[test]
    fn test_derive_hostname_lowercase_mac() {
        // MAC could be lowercase
        let result = Player::derive_hostname("RINCON_aabbccddeeff01400");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Sonos-aabbccddeeff.local");
    }

    #[test]
    fn test_derive_hostname_missing_prefix() {
        let result = Player::derive_hostname("38420B91F87E01400");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_hostname_wrong_prefix() {
        let result = Player::derive_hostname("PLAYER_38420B91F87E01400");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_hostname_too_short() {
        // Less than 12 chars after RINCON_
        let result = Player::derive_hostname("RINCON_12345");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_hostname_empty() {
        let result = Player::derive_hostname("");
        assert!(result.is_err());
    }

    #[test]
    fn test_derive_hostname_just_prefix() {
        let result = Player::derive_hostname("RINCON_");
        assert!(result.is_err());
    }

    // ========================================================================
    // parse_websocket_url
    // ========================================================================

    #[test]
    fn test_parse_websocket_url_full() {
        let result = Player::parse_websocket_url("wss://192.168.1.100:1443/websocket/api");
        assert!(result.is_ok());
        let (host, port, path) = result.unwrap();
        assert_eq!(host, "192.168.1.100");
        assert_eq!(port, 1443);
        assert_eq!(path, "/websocket/api");
    }

    #[test]
    fn test_parse_websocket_url_hostname() {
        let result = Player::parse_websocket_url("wss://Sonos-AABBCCDDEEFF.local:1443/ws");
        assert!(result.is_ok());
        let (host, port, path) = result.unwrap();
        assert_eq!(host, "Sonos-AABBCCDDEEFF.local");
        assert_eq!(port, 1443);
        assert_eq!(path, "/ws");
    }

    #[test]
    fn test_parse_websocket_url_no_path() {
        // No path should default to /websocket/api
        let result = Player::parse_websocket_url("wss://192.168.1.100:1443");
        assert!(result.is_ok());
        let (host, port, path) = result.unwrap();
        assert_eq!(host, "192.168.1.100");
        assert_eq!(port, 1443);
        assert_eq!(path, "/websocket/api");
    }

    #[test]
    fn test_parse_websocket_url_ws_scheme() {
        // ws:// should also work
        let result = Player::parse_websocket_url("ws://192.168.1.100:80/api");
        assert!(result.is_ok());
        let (host, port, path) = result.unwrap();
        assert_eq!(host, "192.168.1.100");
        assert_eq!(port, 80);
        assert_eq!(path, "/api");
    }

    #[test]
    fn test_parse_websocket_url_deep_path() {
        let result = Player::parse_websocket_url("wss://host.local:443/api/v1/websocket");
        assert!(result.is_ok());
        let (_, _, path) = result.unwrap();
        assert_eq!(path, "/api/v1/websocket");
    }

    #[test]
    fn test_parse_websocket_url_invalid_scheme() {
        let result = Player::parse_websocket_url("https://192.168.1.100:1443/ws");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_websocket_url_no_scheme() {
        let result = Player::parse_websocket_url("192.168.1.100:1443/ws");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_websocket_url_no_port() {
        let result = Player::parse_websocket_url("wss://192.168.1.100/ws");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_websocket_url_invalid_port() {
        let result = Player::parse_websocket_url("wss://192.168.1.100:notaport/ws");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_websocket_url_port_out_of_range() {
        let result = Player::parse_websocket_url("wss://192.168.1.100:99999/ws");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_websocket_url_empty() {
        let result = Player::parse_websocket_url("");
        assert!(result.is_err());
    }

    // ========================================================================
    // require_connection (error when disconnected)
    // ========================================================================

    #[tokio::test]
    async fn test_require_connection_when_disconnected() {
        let player = Player::new(
            "RINCON_123".to_string(),
            "Test".to_string(),
            "wss://test:1443/ws".to_string(),
            "HH1".to_string(),
        );

        let result = player.require_connection().await;
        assert!(result.is_err());
        match result {
            Err(Error::NotConnected(id)) => assert_eq!(id, "RINCON_123"),
            Err(e) => panic!("Expected NotConnected, got {:?}", e),
            Ok(_) => panic!("Expected error, got Ok"),
        }
    }

    // ========================================================================
    // playback commands when disconnected
    // ========================================================================

    #[tokio::test]
    async fn test_play_when_disconnected() {
        let player = Player::new(
            "RINCON_123".to_string(),
            "Test".to_string(),
            "wss://test:1443/ws".to_string(),
            "HH1".to_string(),
        );

        let group_id = "GROUP1".to_string();
        let result = player.play(&group_id).await;
        assert!(matches!(result, Err(Error::NotConnected(_))));
    }

    #[tokio::test]
    async fn test_pause_when_disconnected() {
        let player = Player::new(
            "RINCON_123".to_string(),
            "Test".to_string(),
            "wss://test:1443/ws".to_string(),
            "HH1".to_string(),
        );

        let group_id = "GROUP1".to_string();
        let result = player.pause(&group_id).await;
        assert!(matches!(result, Err(Error::NotConnected(_))));
    }

    #[tokio::test]
    async fn test_set_volume_when_disconnected() {
        let player = Player::new(
            "RINCON_123".to_string(),
            "Test".to_string(),
            "wss://test:1443/ws".to_string(),
            "HH1".to_string(),
        );

        let group_id = "GROUP1".to_string();
        let result = player.set_volume(&group_id, 50).await;
        assert!(matches!(result, Err(Error::NotConnected(_))));
    }

    // ========================================================================
    // event subscription
    // ========================================================================

    #[tokio::test]
    async fn test_event_subscription() {
        let player = Player::new(
            "RINCON_123".to_string(),
            "Test".to_string(),
            "wss://test:1443/ws".to_string(),
            "HH1".to_string(),
        );

        let mut rx = player.events();

        // Send event through internal sender
        let _ = player.event_tx.send(PlayerEvent::Connected);

        let event = rx.recv().await.unwrap();
        assert!(matches!(event, PlayerEvent::Connected));
    }

    // ========================================================================
    // Debug formatting
    // ========================================================================

    #[test]
    fn test_player_debug() {
        let player = Player::new(
            "RINCON_123".to_string(),
            "Living Room".to_string(),
            "wss://test:1443/ws".to_string(),
            "HH1".to_string(),
        );

        let debug_str = format!("{:?}", player);
        assert!(debug_str.contains("RINCON_123"));
        assert!(debug_str.contains("Living Room"));
    }

    // ========================================================================
    // cloud queue commands when disconnected
    // ========================================================================

    #[tokio::test]
    async fn test_create_playback_session_when_disconnected() {
        let player = Player::new(
            "RINCON_123".to_string(),
            "Test".to_string(),
            "wss://test:1443/ws".to_string(),
            "HH1".to_string(),
        );

        let group_id = "GROUP1".to_string();
        let result = player
            .create_playback_session(
                &group_id,
                CreateSessionRequest {
                    app_id: "qobuz".to_string(),
                    app_context: "qobuz".to_string(),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(result, Err(Error::NotConnected(_))));
    }

    #[tokio::test]
    async fn test_load_cloud_queue_when_disconnected() {
        let player = Player::new(
            "RINCON_123".to_string(),
            "Test".to_string(),
            "wss://test:1443/ws".to_string(),
            "HH1".to_string(),
        );

        let result = player
            .load_cloud_queue(
                "SESSION_123",
                LoadCloudQueueRequest {
                    queue_base_url: "http://localhost:9443/queue/".to_string(),
                    item_id: Some("1".to_string()),
                    play_on_completion: Some(true),
                    ..Default::default()
                },
            )
            .await;
        assert!(matches!(result, Err(Error::NotConnected(_))));
    }
}
