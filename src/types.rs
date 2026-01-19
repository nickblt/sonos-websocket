use serde::{Deserialize, Serialize};

/// Unique identifier for a player
pub type PlayerId = String;

// ============================================================================
// Playback Types
// ============================================================================

/// Playback state of a group
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayState {
    /// Not playing
    #[serde(rename = "PLAYBACK_STATE_IDLE")]
    Idle,
    /// Currently playing
    #[serde(rename = "PLAYBACK_STATE_PLAYING")]
    Playing,
    /// Paused
    #[serde(rename = "PLAYBACK_STATE_PAUSED")]
    Paused,
    /// Buffering (transitional state)
    #[serde(rename = "PLAYBACK_STATE_BUFFERING")]
    Buffering,
}

/// Play mode settings for a group
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayModes {
    /// Repeat the queue
    #[serde(default)]
    pub repeat: bool,
    /// Repeat the current track
    #[serde(default)]
    pub repeat_one: bool,
    /// Shuffle the queue
    #[serde(default)]
    pub shuffle: bool,
    /// Crossfade between tracks
    #[serde(default)]
    pub crossfade: bool,
}

/// Current playback status for a group
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatus {
    /// Current play state
    #[serde(rename = "playbackState")]
    pub state: PlayState,
    /// Position in the current track in milliseconds
    pub position_millis: Option<u64>,
    /// Current play modes
    #[serde(default)]
    pub play_modes: PlayModes,
}

/// Volume state for a group
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupVolume {
    /// Volume level (0-100)
    pub volume: u8,
    /// Whether the group is muted
    pub muted: bool,
    /// Whether the volume is fixed (e.g., for a fixed-output device)
    #[serde(default)]
    pub fixed: bool,
}

/// Volume state for an individual player (matches schema.json #/components/schemas/playerVolume)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerVolume {
    /// Volume level (0-100)
    pub volume: u8,
    /// Whether the player is muted
    #[serde(default)]
    pub muted: bool,
    /// Whether the volume is fixed (e.g., for a fixed line-out device)
    #[serde(default)]
    pub fixed: bool,
}

// ============================================================================
// Music Object Types (schema-aligned)
// ============================================================================

/// Universal music object identifier (matches schema.json #/components/schemas/universalMusicObjectId)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicObjectId {
    /// Required: The object ID from the music service
    pub object_id: String,
    /// The service ID for resolving content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_id: Option<String>,
    /// Account ID in format sn_[household id] or mhhid_[household id]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

/// Music service info for tracks (matches schema.json #/components/schemas/service)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    /// Service name (e.g., "Qobuz")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Service ID (e.g., "31" for Qobuz)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Artist metadata (matches schema.json #/components/schemas/artist)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    /// Artist name
    pub name: String,
    /// Universal music object ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<MusicObjectId>,
}

/// Album metadata (matches schema.json #/components/schemas/album)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    /// Album name
    pub name: String,
    /// Album artist (can differ from track artist)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<Artist>,
    /// Universal music object ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<MusicObjectId>,
}

/// Track metadata (matches schema.json #/components/schemas/track)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// Track name/title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Track artist
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<Artist>,
    /// Track album
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<Album>,
    /// Track duration in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_millis: Option<i32>,
    /// URL to album/track artwork (deprecated in schema but still works)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    /// Direct URL to the audio stream
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_url: Option<String>,
    /// MIME type of the audio (e.g., "audio/flac")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// Universal music object ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<MusicObjectId>,
    /// Music service info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<Service>,
    /// Track type (e.g., "track", "episode")
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub track_type: Option<String>,
}

/// A queue item (current or next track)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    /// Track metadata
    pub track: Option<Track>,
    /// Whether this item has been deleted from the queue
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<bool>,
}

/// Metadata status response from the API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataStatus {
    /// Currently playing item
    pub current_item: Option<QueueItem>,
    /// Next item in queue
    pub next_item: Option<QueueItem>,
    /// Stream info for radio/live streams
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_info: Option<String>,
}

impl MetadataStatus {
    /// Get the current track name
    pub fn track_name(&self) -> Option<&str> {
        self.current_item.as_ref()?.track.as_ref()?.name.as_deref()
    }

    /// Get the current artist name
    pub fn artist_name(&self) -> Option<&str> {
        self.current_item
            .as_ref()?
            .track
            .as_ref()?
            .artist
            .as_ref()
            .map(|a| a.name.as_str())
    }

    /// Get the current album name
    pub fn album_name(&self) -> Option<&str> {
        self.current_item
            .as_ref()?
            .track
            .as_ref()?
            .album
            .as_ref()
            .map(|a| a.name.as_str())
    }

    /// Get the current track duration in milliseconds
    pub fn duration_millis(&self) -> Option<i32> {
        self.current_item.as_ref()?.track.as_ref()?.duration_millis
    }

    /// Get the current track image URL
    pub fn image_url(&self) -> Option<&str> {
        self.current_item.as_ref()?.track.as_ref()?.image_url.as_deref()
    }
}

// ============================================================================
// Topology Types
// ============================================================================

/// Unique identifier for a group
pub type GroupId = String;

/// Unique identifier for a household
pub type HouseholdId = String;

/// Metadata about a Sonos player (from topology)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerInfo {
    pub id: PlayerId,
    pub name: String,
    pub websocket_url: Option<String>,
}

/// A group of players with a coordinator
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub coordinator_id: PlayerId,
    pub player_ids: Vec<PlayerId>,
}

/// The complete topology of all players and groups
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topology {
    pub players: Vec<PlayerInfo>,
    pub groups: Vec<Group>,
    pub household_id: Option<HouseholdId>,
}

// ============================================================================
// System Events (from SonosSystem.events())
// ============================================================================

/// System-wide events for topology changes
#[derive(Debug, Clone)]
pub enum SystemEvent {
    /// A new player appeared in the topology
    PlayerAdded(PlayerInfo),

    /// A player was removed from the topology
    PlayerRemoved(PlayerId),

    /// Topology updated (groups/players changed)
    TopologyUpdated(Topology),
}

// ============================================================================
// Player Events (from Player.events())
// ============================================================================

/// Per-player events for connection and playback state
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    /// Successfully connected to the player
    Connected,

    /// Disconnected from the player
    Disconnected,

    /// Attempting to reconnect
    Reconnecting,

    /// Playback state changed
    PlaybackChanged(PlaybackStatus),

    /// Group volume changed
    VolumeChanged(GroupVolume),

    /// Individual player volume changed
    PlayerVolumeChanged(PlayerVolume),

    /// Metadata (current track) changed
    MetadataChanged(MetadataStatus),

    /// Playback error occurred (e.g., cloud queue server error)
    PlaybackError(PlaybackError),
}

// ============================================================================
// Music Service Types
// ============================================================================

/// Music service info from account match response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicService {
    pub id: String,
    pub name: String,
}

/// Music service account from match response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicServiceAccount {
    /// Format: "sn_N" where N is the account number
    pub id: String,
    pub user_id_hash_code: String,
    pub nickname: String,
    pub is_guest: bool,
    pub service: MusicService,
}

impl MusicServiceAccount {
    /// Extract the service account number (e.g., "sn_8" -> 8)
    pub fn service_account_number(&self) -> Option<u32> {
        self.id.strip_prefix("sn_").and_then(|n| n.parse().ok())
    }
}

// ============================================================================
// Playback Session Types (for cloud queue)
// ============================================================================

/// Request parameters for creating a playback session.
///
/// # Example
/// ```
/// use sonos_websocket::CreateSessionRequest;
///
/// let request = CreateSessionRequest {
///     app_id: "com.example.myapp".to_string(),
///     app_context: "user_123".to_string(),
///     account_id: Some("sn_8".to_string()),  // Link to music service account
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    /// App identifier in reverse DNS format (e.g., "com.example.myapp").
    /// Combined with app_context, determines if sessions can be joined.
    pub app_id: String,

    /// Instance data for your app (e.g., hashed user ID).
    /// Combined with app_id, determines if sessions can be joined.
    pub app_context: String,

    /// Music service account ID (e.g., "sn_8").
    /// Links the session to a specific music service account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,

    /// Custom data string (max 1023 chars).
    /// Persisted across session joins; useful for state synchronization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_data: Option<String>,
}

/// Playback session info returned from createSession
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSession {
    /// Unique identifier for the playback session
    pub session_id: String,
    /// Current session state (e.g., "SESSION_STATE_CONNECTED")
    #[serde(default)]
    pub session_state: Option<String>,
}

/// Request parameters for loading a cloud queue.
///
/// All fields except `queue_base_url` are optional. Use `Default::default()` and
/// set only the fields you need.
///
/// # Example
/// ```
/// use sonos_websocket::{LoadCloudQueueRequest, Track, Artist};
///
/// let request = LoadCloudQueueRequest {
///     queue_base_url: "http://192.168.1.100:9443/queues/my-queue/v2.3/".to_string(),
///     item_id: Some("1".to_string()),
///     play_on_completion: Some(true),
///     track_metadata: Some(Track {
///         name: Some("My Song".to_string()),
///         artist: Some(Artist { name: "Artist".to_string(), id: None }),
///         ..Default::default()
///     }),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadCloudQueueRequest {
    /// The base URL for the cloud queue server.
    /// Must end with a version specification like `/v2.3/`.
    pub queue_base_url: String,

    /// The track ID to start playing. If empty string or omitted,
    /// starts from the beginning of the queue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,

    /// If true, start playback immediately after loading.
    /// Default is false (requires separate play command).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play_on_completion: Option<bool>,

    /// Track metadata to display immediately before fetching from queue server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_metadata: Option<Track>,

    /// HTTP Authorization header value for cloud queue requests.
    /// If not set and session matches a SMAPI account, uses that account's OAuth token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_authorization: Option<String>,

    /// If true, also send http_authorization to HTTPS media requests.
    /// Never sent to HTTP (insecure) requests. Default is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_http_authorization_for_media: Option<bool>,

    /// Opaque version identifier for queue state synchronization.
    /// Passed back to the queue server in GET /itemWindow requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_version: Option<String>,

    /// Position in milliseconds to start playback within the track.
    /// Default is 0. If not provided and item_id matches current item,
    /// playback continues from current position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_millis: Option<i32>,
}

/// Playback error reported by Sonos.
///
/// Received when playback fails, e.g., cloud queue server errors.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackError {
    /// Error state: ERROR_CLOUD_QUEUE_SERVER, ERROR_DISALLOWED_BY_POLICY,
    /// ERROR_PLAYBACK_FAILED, ERROR_PLAYBACK_NO_CONTENT
    pub error_code: String,
    /// Optional debug message
    pub reason: Option<String>,
    /// Track ID from cloud queue
    pub item_id: Option<String>,
    /// Last cloud queue state identifier
    pub queue_version: Option<String>,
    /// HTTP status code if server-related error
    pub http_status: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // PlayState enum
    // ========================================================================

    #[test]
    fn test_play_state_deserialize() {
        assert_eq!(
            serde_json::from_str::<PlayState>(r#""PLAYBACK_STATE_IDLE""#).unwrap(),
            PlayState::Idle
        );
        assert_eq!(
            serde_json::from_str::<PlayState>(r#""PLAYBACK_STATE_PLAYING""#).unwrap(),
            PlayState::Playing
        );
        assert_eq!(
            serde_json::from_str::<PlayState>(r#""PLAYBACK_STATE_PAUSED""#).unwrap(),
            PlayState::Paused
        );
        assert_eq!(
            serde_json::from_str::<PlayState>(r#""PLAYBACK_STATE_BUFFERING""#).unwrap(),
            PlayState::Buffering
        );
    }

    #[test]
    fn test_play_state_serialize() {
        assert_eq!(
            serde_json::to_string(&PlayState::Playing).unwrap(),
            r#""PLAYBACK_STATE_PLAYING""#
        );
    }

    #[test]
    fn test_play_state_unknown_variant() {
        assert!(serde_json::from_str::<PlayState>(r#""PLAYBACK_STATE_UNKNOWN""#).is_err());
    }

    // ========================================================================
    // PlayModes
    // ========================================================================

    #[test]
    fn test_play_modes_defaults() {
        let modes: PlayModes = serde_json::from_str("{}").unwrap();
        assert!(!modes.repeat);
        assert!(!modes.repeat_one);
        assert!(!modes.shuffle);
        assert!(!modes.crossfade);
    }

    #[test]
    fn test_play_modes_round_trip() {
        let modes = PlayModes {
            repeat: true,
            repeat_one: false,
            shuffle: true,
            crossfade: false,
        };
        let json = serde_json::to_string(&modes).unwrap();
        let parsed: PlayModes = serde_json::from_str(&json).unwrap();
        assert_eq!(modes, parsed);
    }

    // ========================================================================
    // PlaybackStatus
    // ========================================================================

    #[test]
    fn test_playback_status_full() {
        let json = r#"{
            "playbackState": "PLAYBACK_STATE_PLAYING",
            "positionMillis": 12345,
            "playModes": {"repeat": true, "shuffle": false}
        }"#;

        let status: PlaybackStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status.state, PlayState::Playing);
        assert_eq!(status.position_millis, Some(12345));
        assert!(status.play_modes.repeat);
        assert!(!status.play_modes.shuffle);
    }

    #[test]
    fn test_playback_status_minimal() {
        let json = r#"{"playbackState": "PLAYBACK_STATE_IDLE"}"#;

        let status: PlaybackStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status.state, PlayState::Idle);
        assert!(status.position_millis.is_none());
        // play_modes should default
        assert!(!status.play_modes.repeat);
    }

    // ========================================================================
    // GroupVolume
    // ========================================================================

    #[test]
    fn test_group_volume_round_trip() {
        let vol = GroupVolume {
            volume: 75,
            muted: false,
            fixed: true,
        };
        let json = serde_json::to_string(&vol).unwrap();
        let parsed: GroupVolume = serde_json::from_str(&json).unwrap();
        assert_eq!(vol, parsed);
    }

    #[test]
    fn test_group_volume_fixed_defaults() {
        let json = r#"{"volume": 50, "muted": false}"#;
        let vol: GroupVolume = serde_json::from_str(json).unwrap();
        assert_eq!(vol.volume, 50);
        assert!(!vol.muted);
        assert!(!vol.fixed); // default
    }

    // ========================================================================
    // Track / Artist / Album
    // ========================================================================

    #[test]
    fn test_track_full() {
        let json = r#"{
            "name": "Test Song",
            "artist": {"name": "Test Artist"},
            "album": {"name": "Test Album"},
            "durationMillis": 180000,
            "imageUrl": "https://example.com/image.jpg"
        }"#;

        let track: Track = serde_json::from_str(json).unwrap();
        assert_eq!(track.name, Some("Test Song".to_string()));
        assert_eq!(track.artist.as_ref().unwrap().name, "Test Artist");
        assert_eq!(track.album.as_ref().unwrap().name, "Test Album");
        assert_eq!(track.duration_millis, Some(180000));
        assert_eq!(
            track.image_url,
            Some("https://example.com/image.jpg".to_string())
        );
    }

    #[test]
    fn test_track_empty() {
        let json = "{}";
        let track: Track = serde_json::from_str(json).unwrap();
        assert!(track.name.is_none());
        assert!(track.artist.is_none());
        assert!(track.album.is_none());
    }

    // ========================================================================
    // MetadataStatus and helper methods
    // ========================================================================

    #[test]
    fn test_metadata_status_helpers_with_data() {
        let json = r#"{
            "currentItem": {
                "track": {
                    "name": "Current Song",
                    "artist": {"name": "Artist Name"},
                    "album": {"name": "Album Name"},
                    "durationMillis": 200000,
                    "imageUrl": "https://example.com/art.jpg"
                }
            },
            "nextItem": null
        }"#;

        let status: MetadataStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status.track_name(), Some("Current Song"));
        assert_eq!(status.artist_name(), Some("Artist Name"));
        assert_eq!(status.album_name(), Some("Album Name"));
        assert_eq!(status.duration_millis(), Some(200000));
        assert_eq!(status.image_url(), Some("https://example.com/art.jpg"));
    }

    #[test]
    fn test_metadata_status_helpers_empty() {
        let json = r#"{"currentItem": null, "nextItem": null}"#;

        let status: MetadataStatus = serde_json::from_str(json).unwrap();
        assert!(status.track_name().is_none());
        assert!(status.artist_name().is_none());
        assert!(status.album_name().is_none());
        assert!(status.duration_millis().is_none());
        assert!(status.image_url().is_none());
    }

    #[test]
    fn test_metadata_status_partial_track() {
        let json = r#"{
            "currentItem": {
                "track": {
                    "name": "Song Name"
                }
            }
        }"#;

        let status: MetadataStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status.track_name(), Some("Song Name"));
        assert!(status.artist_name().is_none());
        assert!(status.album_name().is_none());
    }

    // ========================================================================
    // Topology types: PlayerInfo, Group, Topology
    // ========================================================================

    #[test]
    fn test_player_info_round_trip() {
        let info = PlayerInfo {
            id: "RINCON_123".to_string(),
            name: "Living Room".to_string(),
            websocket_url: Some("wss://192.168.1.100:1443/websocket/api".to_string()),
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: PlayerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info.id, parsed.id);
        assert_eq!(info.name, parsed.name);
        assert_eq!(info.websocket_url, parsed.websocket_url);
    }

    #[test]
    fn test_player_info_missing_websocket_url() {
        let json = r#"{"id": "RINCON_123", "name": "Kitchen"}"#;
        let info: PlayerInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.id, "RINCON_123");
        assert_eq!(info.name, "Kitchen");
        assert!(info.websocket_url.is_none());
    }

    #[test]
    fn test_group_round_trip() {
        let group = Group {
            id: "RINCON_123:1".to_string(),
            name: "Living Room".to_string(),
            coordinator_id: "RINCON_123".to_string(),
            player_ids: vec!["RINCON_123".to_string(), "RINCON_456".to_string()],
        };

        let json = serde_json::to_string(&group).unwrap();
        let parsed: Group = serde_json::from_str(&json).unwrap();
        assert_eq!(group.id, parsed.id);
        assert_eq!(group.coordinator_id, parsed.coordinator_id);
        assert_eq!(group.player_ids.len(), 2);
    }

    #[test]
    fn test_group_empty_player_ids() {
        let json = r#"{
            "id": "GRP1",
            "name": "Empty Group",
            "coordinatorId": "RINCON_123",
            "playerIds": []
        }"#;

        let group: Group = serde_json::from_str(json).unwrap();
        assert!(group.player_ids.is_empty());
    }

    #[test]
    fn test_topology_full() {
        let json = r#"{
            "players": [
                {"id": "RINCON_123", "name": "Living Room"},
                {"id": "RINCON_456", "name": "Kitchen"}
            ],
            "groups": [
                {
                    "id": "GRP1",
                    "name": "Living Room",
                    "coordinatorId": "RINCON_123",
                    "playerIds": ["RINCON_123"]
                }
            ],
            "householdId": "HH_ABC"
        }"#;

        let topology: Topology = serde_json::from_str(json).unwrap();
        assert_eq!(topology.players.len(), 2);
        assert_eq!(topology.groups.len(), 1);
        assert_eq!(topology.household_id, Some("HH_ABC".to_string()));
    }

    #[test]
    fn test_topology_empty() {
        let json = r#"{"players": [], "groups": []}"#;
        let topology: Topology = serde_json::from_str(json).unwrap();
        assert!(topology.players.is_empty());
        assert!(topology.groups.is_empty());
        assert!(topology.household_id.is_none());
    }

    // ========================================================================
    // Forward compatibility: unknown fields ignored
    // ========================================================================

    #[test]
    fn test_player_info_unknown_fields_ignored() {
        let json = r#"{
            "id": "RINCON_123",
            "name": "Living Room",
            "unknownField": "should be ignored",
            "anotherUnknown": 42
        }"#;

        let info: PlayerInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.id, "RINCON_123");
    }

    #[test]
    fn test_group_unknown_fields_ignored() {
        let json = r#"{
            "id": "GRP1",
            "name": "Group",
            "coordinatorId": "RINCON_123",
            "playerIds": [],
            "futureField": true
        }"#;

        let group: Group = serde_json::from_str(json).unwrap();
        assert_eq!(group.id, "GRP1");
    }

    // ========================================================================
    // MusicServiceAccount
    // ========================================================================

    #[test]
    fn test_music_service_account_deserialize() {
        let json = r#"{
            "id": "sn_8",
            "userIdHashCode": "qobuz:user:HwyJtEEf1kazy",
            "nickname": "Nick",
            "isGuest": false,
            "service": {"id": "31", "name": "Qobuz"}
        }"#;

        let account: MusicServiceAccount = serde_json::from_str(json).unwrap();
        assert_eq!(account.id, "sn_8");
        assert_eq!(account.nickname, "Nick");
        assert!(!account.is_guest);
        assert_eq!(account.service.id, "31");
        assert_eq!(account.service.name, "Qobuz");
    }

    #[test]
    fn test_service_account_number() {
        let account = MusicServiceAccount {
            id: "sn_8".to_string(),
            user_id_hash_code: "test".to_string(),
            nickname: "Test".to_string(),
            is_guest: false,
            service: MusicService {
                id: "31".to_string(),
                name: "Qobuz".to_string(),
            },
        };
        assert_eq!(account.service_account_number(), Some(8));
    }

    #[test]
    fn test_service_account_number_invalid() {
        let account = MusicServiceAccount {
            id: "invalid".to_string(),
            user_id_hash_code: "test".to_string(),
            nickname: "Test".to_string(),
            is_guest: false,
            service: MusicService {
                id: "31".to_string(),
                name: "Qobuz".to_string(),
            },
        };
        assert_eq!(account.service_account_number(), None);
    }

    // ========================================================================
    // PlaybackSession
    // ========================================================================

    #[test]
    fn test_playback_session_deserialize_full() {
        let json = r#"{
            "sessionId": "session_abc123",
            "sessionState": "SESSION_STATE_CONNECTED"
        }"#;

        let session: PlaybackSession = serde_json::from_str(json).unwrap();
        assert_eq!(session.session_id, "session_abc123");
        assert_eq!(
            session.session_state,
            Some("SESSION_STATE_CONNECTED".to_string())
        );
    }

    #[test]
    fn test_playback_session_deserialize_minimal() {
        let json = r#"{"sessionId": "session_xyz"}"#;

        let session: PlaybackSession = serde_json::from_str(json).unwrap();
        assert_eq!(session.session_id, "session_xyz");
        assert!(session.session_state.is_none());
    }
}
