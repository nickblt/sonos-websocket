pub mod connection;
pub mod discovery;
pub mod error;
pub mod message;
pub mod player;
pub mod system;
pub mod tls;
pub mod types;

// Low-level connection types (for advanced use)
pub use connection::{ConnectionConfig, ConnectionEvent, ConnectionState, PlayerConnection};

// Discovery
pub use discovery::{DiscoveredPlayer, discover_players};

// Error handling
pub use error::{Error, Result};

// Message types (for parsing events)
pub use message::{API_KEY, Message, RequestHeader, ResponseHeader, WEBSOCKET_PROTOCOL};

// Topology types
pub use types::{Group, GroupId, HouseholdId, PlayerId, PlayerInfo, Topology};

// Event types
pub use types::{PlayerEvent, SystemEvent};

// Playback types
pub use types::{
    Album, Artist, GroupVolume, MetadataStatus, MusicObjectId, PlayModes, PlayState,
    PlaybackError, PlaybackStatus, PlayerVolume, QueueItem, Service, Track,
};

// Music service types
pub use types::{MusicService, MusicServiceAccount};

// Cloud queue / session types
pub use types::{CreateSessionRequest, LoadCloudQueueRequest, PlaybackSession};

// High-level API
pub use player::Player;
pub use system::SonosSystem;
