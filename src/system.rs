use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast};
use tracing::{debug, info, warn};

use serde::Deserialize;

use crate::connection::{ConnectionConfig, ConnectionEvent, PlayerConnection};
use crate::discovery::{DiscoveredPlayer, discover_players};
use crate::error::{Error, Result};
use crate::message::RequestHeader;
use crate::player::Player;
use crate::types::{Group, HouseholdId, PlayerId, PlayerInfo, SystemEvent, Topology};

/// Groups response from the API
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupsResponse {
    groups: Vec<Group>,
    players: Vec<PlayerInfo>,
}

/// Main entry point for the Sonos system.
///
/// Handles discovery, topology tracking, and provides access to players.
/// Players start disconnected - call `player.connect()` to control them.
#[derive(Clone)]
pub struct SonosSystem {
    household_id: HouseholdId,
    players: Arc<RwLock<HashMap<PlayerId, Player>>>,
    topology: Arc<RwLock<Topology>>,
    event_tx: broadcast::Sender<SystemEvent>,
    /// The one connection used for topology subscription (kept alive)
    #[allow(dead_code)]
    topology_connection: PlayerConnection,
    _task_handle: Arc<TaskHandle>,
}

struct TaskHandle {
    task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Drop for TaskHandle {
    fn drop(&mut self) {
        if let Some(handle) = self.task.lock().unwrap().take() {
            handle.abort();
        }
    }
}

impl SonosSystem {
    /// Discover Sonos players and start the system.
    ///
    /// Performs mDNS discovery for the specified duration, connects to ONE
    /// player for topology subscription, then creates Player objects for
    /// all players in the household.
    pub async fn start(timeout: Duration) -> Result<Self> {
        info!("Discovering Sonos players for {:?}...", timeout);
        let discovered = discover_players(timeout)
            .map_err(|e| Error::Io(std::io::Error::other(format!("discovery failed: {}", e))))?;

        if discovered.is_empty() {
            return Err(Error::PlayerNotFound("no Sonos players found".to_string()));
        }

        info!("Found {} players via mDNS", discovered.len());
        Self::from_discovered(discovered).await
    }

    /// Create a SonosSystem from pre-discovered players.
    pub async fn from_discovered(discovered: Vec<DiscoveredPlayer>) -> Result<Self> {
        if discovered.is_empty() {
            return Err(Error::PlayerNotFound("no players provided".to_string()));
        }

        // Get household ID from first player with one
        let household_id = discovered
            .iter()
            .find(|p| !p.txt_mhhid.is_empty())
            .map(|p| p.txt_mhhid.clone())
            .unwrap_or_default();

        // Connect to first player for topology subscription
        let first = &discovered[0];
        let hostname = first.hostname.trim_end_matches('.').to_string();
        let ws_path = if first.txt_wss.is_empty() {
            "/websocket/api".to_string()
        } else {
            first.txt_wss.clone()
        };

        info!("Connecting to {} for topology subscription", first.name);

        let topology_connection = PlayerConnection::with_config(
            &hostname,
            first.txt_sslport,
            &ws_path,
            ConnectionConfig::default(),
        );

        topology_connection.connect().await?;

        // Wait for connection to be established
        Self::wait_for_connection(&topology_connection, Duration::from_secs(10)).await?;

        // Subscribe to groups namespace for topology
        let header = RequestHeader::household("groups", "subscribe", &household_id);
        let response = topology_connection.send_command_empty(header).await?;

        if response.header.success != Some(true) {
            return Err(Error::Io(std::io::Error::other(format!(
                "topology subscribe failed: {:?}",
                response.header
            ))));
        }

        info!("Subscribed to topology on {}", first.name);

        // Create event channel
        let (event_tx, _) = broadcast::channel(64);

        // Initialize empty state - will be populated from first topology response
        let players = Arc::new(RwLock::new(HashMap::new()));
        let topology = Arc::new(RwLock::new(Topology {
            players: vec![],
            groups: vec![],
            household_id: Some(household_id.clone()),
        }));

        // Spawn background task to handle topology updates
        let task_handle = Self::spawn_topology_task(
            topology_connection.clone(),
            players.clone(),
            topology.clone(),
            event_tx.clone(),
            household_id.clone(),
        );

        Ok(Self {
            household_id,
            players,
            topology,
            event_tx,
            topology_connection,
            _task_handle: Arc::new(task_handle),
        })
    }

    /// Get the household ID
    pub fn household_id(&self) -> &str {
        &self.household_id
    }

    /// Get a player by ID (returns a clone, they're cheap)
    pub async fn get_player(&self, id: &str) -> Option<Player> {
        self.players.read().await.get(id).cloned()
    }

    /// Get all players
    pub async fn players(&self) -> Vec<Player> {
        self.players.read().await.values().cloned().collect()
    }

    /// Get the current topology
    pub async fn get_topology(&self) -> Topology {
        self.topology.read().await.clone()
    }

    /// Subscribe to system events (player added/removed, topology updated)
    pub fn events(&self) -> broadcast::Receiver<SystemEvent> {
        self.event_tx.subscribe()
    }

    /// Find the group containing a player
    pub async fn find_group_for_player(&self, player_id: &str) -> Option<Group> {
        let topology = self.topology.read().await;
        topology
            .groups
            .iter()
            .find(|g| g.player_ids.contains(&player_id.to_string()))
            .cloned()
    }

    /// Get coordinator player for a group
    pub async fn get_coordinator(&self, group: &Group) -> Option<Player> {
        self.get_player(&group.coordinator_id).await
    }

    /// Wait for a connection to be established
    async fn wait_for_connection(conn: &PlayerConnection, timeout: Duration) -> Result<()> {
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

            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Spawn the background task that handles topology updates
    fn spawn_topology_task(
        connection: PlayerConnection,
        players: Arc<RwLock<HashMap<PlayerId, Player>>>,
        topology: Arc<RwLock<Topology>>,
        event_tx: broadcast::Sender<SystemEvent>,
        household_id: HouseholdId,
    ) -> TaskHandle {
        let mut conn_events = connection.events();

        let task = tokio::spawn(async move {
            loop {
                match conn_events.recv().await {
                    Ok(ConnectionEvent::Message(msg)) => {
                        if msg.header.namespace == "groups"
                            && let Ok(response) = serde_json::from_value::<GroupsResponse>(msg.body)
                        {
                            Self::handle_topology_update(
                                &response,
                                &players,
                                &topology,
                                &event_tx,
                                &household_id,
                            )
                            .await;
                        }
                    }
                    Ok(ConnectionEvent::Disconnected) => {
                        warn!("Topology connection lost");
                        // TODO: Could implement failover to another player here
                    }
                    Ok(ConnectionEvent::Reconnecting { attempt }) => {
                        debug!("Topology connection reconnecting (attempt {})", attempt);
                    }
                    Ok(ConnectionEvent::Connected) => {
                        debug!("Topology connection restored");
                    }
                    Err(_) => {
                        // Channel closed, exit task
                        break;
                    }
                }
            }
        });

        TaskHandle {
            task: std::sync::Mutex::new(Some(task)),
        }
    }

    /// Handle a topology update from the groups namespace
    async fn handle_topology_update(
        response: &GroupsResponse,
        players: &Arc<RwLock<HashMap<PlayerId, Player>>>,
        topology: &Arc<RwLock<Topology>>,
        event_tx: &broadcast::Sender<SystemEvent>,
        household_id: &str,
    ) {
        let mut players_guard = players.write().await;
        let mut topology_guard = topology.write().await;

        let old_player_ids: std::collections::HashSet<_> = players_guard.keys().cloned().collect();
        let new_player_ids: std::collections::HashSet<_> =
            response.players.iter().map(|p| p.id.clone()).collect();

        // Detect added players
        for player_info in &response.players {
            if !old_player_ids.contains(&player_info.id) {
                let websocket_url = player_info
                    .websocket_url
                    .clone()
                    .unwrap_or_else(|| format!("wss://{}:1443/websocket/api", player_info.id));

                let player = Player::new(
                    player_info.id.clone(),
                    player_info.name.clone(),
                    websocket_url,
                    household_id.to_string(),
                );

                debug!("Player added: {} ({})", player_info.name, player_info.id);
                players_guard.insert(player_info.id.clone(), player.clone());

                let _ = event_tx.send(SystemEvent::PlayerAdded(player_info.clone()));
            }
        }

        // Detect removed players
        for player_id in &old_player_ids {
            if !new_player_ids.contains(player_id) {
                debug!("Player removed: {}", player_id);
                if let Some(player) = players_guard.remove(player_id) {
                    // Disconnect the player if it was connected
                    player.disconnect().await;
                }
                let _ = event_tx.send(SystemEvent::PlayerRemoved(player_id.clone()));
            }
        }

        // Update topology
        let new_topology = Topology {
            players: response.players.clone(),
            groups: response.groups.clone(),
            household_id: Some(household_id.to_string()),
        };

        *topology_guard = new_topology.clone();

        // Always emit topology updated
        let _ = event_tx.send(SystemEvent::TopologyUpdated(new_topology));
    }
}

impl std::fmt::Debug for SonosSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SonosSystem")
            .field("household_id", &self.household_id)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // GroupsResponse deserialization
    // ========================================================================

    #[test]
    fn test_groups_response_full() {
        let json = r#"{
            "groups": [
                {
                    "id": "RINCON_123:1",
                    "name": "Living Room",
                    "coordinatorId": "RINCON_123",
                    "playerIds": ["RINCON_123", "RINCON_456"]
                }
            ],
            "players": [
                {"id": "RINCON_123", "name": "Living Room", "websocketUrl": "wss://192.168.1.100:1443/ws"},
                {"id": "RINCON_456", "name": "Kitchen"}
            ]
        }"#;

        let response: GroupsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.groups.len(), 1);
        assert_eq!(response.players.len(), 2);
        assert_eq!(response.groups[0].coordinator_id, "RINCON_123");
        assert_eq!(
            response.players[0].websocket_url,
            Some("wss://192.168.1.100:1443/ws".to_string())
        );
        assert!(response.players[1].websocket_url.is_none());
    }

    #[test]
    fn test_groups_response_empty() {
        let json = r#"{"groups": [], "players": []}"#;

        let response: GroupsResponse = serde_json::from_str(json).unwrap();
        assert!(response.groups.is_empty());
        assert!(response.players.is_empty());
    }

    #[test]
    fn test_groups_response_unknown_fields_ignored() {
        let json = r#"{
            "groups": [],
            "players": [],
            "unknownField": "should be ignored",
            "partial": true
        }"#;

        let response: GroupsResponse = serde_json::from_str(json).unwrap();
        assert!(response.groups.is_empty());
    }

    // ========================================================================
    // handle_topology_update
    // ========================================================================

    #[tokio::test]
    async fn test_topology_update_adds_players() {
        let players = Arc::new(RwLock::new(HashMap::new()));
        let topology = Arc::new(RwLock::new(Topology {
            players: vec![],
            groups: vec![],
            household_id: Some("HH1".to_string()),
        }));
        let (event_tx, mut event_rx) = broadcast::channel(64);

        let response = GroupsResponse {
            groups: vec![],
            players: vec![PlayerInfo {
                id: "RINCON_123".to_string(),
                name: "Living Room".to_string(),
                websocket_url: Some("wss://192.168.1.100:1443/ws".to_string()),
            }],
        };

        SonosSystem::handle_topology_update(&response, &players, &topology, &event_tx, "HH1").await;

        // Check player was added
        let players_guard = players.read().await;
        assert!(players_guard.contains_key("RINCON_123"));
        assert_eq!(
            players_guard.get("RINCON_123").unwrap().name(),
            "Living Room"
        );

        // Check topology was updated
        let topology_guard = topology.read().await;
        assert_eq!(topology_guard.players.len(), 1);

        // Check events were emitted
        let event = event_rx.recv().await.unwrap();
        assert!(matches!(event, SystemEvent::PlayerAdded(_)));

        let event = event_rx.recv().await.unwrap();
        assert!(matches!(event, SystemEvent::TopologyUpdated(_)));
    }

    #[tokio::test]
    async fn test_topology_update_removes_players() {
        // Start with one player
        let mut initial_players = HashMap::new();
        initial_players.insert(
            "RINCON_123".to_string(),
            Player::new(
                "RINCON_123".to_string(),
                "Living Room".to_string(),
                "wss://test:1443/ws".to_string(),
                "HH1".to_string(),
            ),
        );
        let players = Arc::new(RwLock::new(initial_players));
        let topology = Arc::new(RwLock::new(Topology {
            players: vec![PlayerInfo {
                id: "RINCON_123".to_string(),
                name: "Living Room".to_string(),
                websocket_url: None,
            }],
            groups: vec![],
            household_id: Some("HH1".to_string()),
        }));
        let (event_tx, mut event_rx) = broadcast::channel(64);

        // Update with empty players (player removed)
        let response = GroupsResponse {
            groups: vec![],
            players: vec![],
        };

        SonosSystem::handle_topology_update(&response, &players, &topology, &event_tx, "HH1").await;

        // Check player was removed
        let players_guard = players.read().await;
        assert!(!players_guard.contains_key("RINCON_123"));

        // Check events
        let event = event_rx.recv().await.unwrap();
        assert!(matches!(event, SystemEvent::PlayerRemoved(id) if id == "RINCON_123"));
    }

    #[tokio::test]
    async fn test_topology_update_with_groups() {
        let players = Arc::new(RwLock::new(HashMap::new()));
        let topology = Arc::new(RwLock::new(Topology {
            players: vec![],
            groups: vec![],
            household_id: Some("HH1".to_string()),
        }));
        let (event_tx, _) = broadcast::channel(64);

        let response = GroupsResponse {
            groups: vec![Group {
                id: "RINCON_123:1".to_string(),
                name: "Living Room".to_string(),
                coordinator_id: "RINCON_123".to_string(),
                player_ids: vec!["RINCON_123".to_string()],
            }],
            players: vec![PlayerInfo {
                id: "RINCON_123".to_string(),
                name: "Living Room".to_string(),
                websocket_url: None,
            }],
        };

        SonosSystem::handle_topology_update(&response, &players, &topology, &event_tx, "HH1").await;

        // Check topology has groups
        let topology_guard = topology.read().await;
        assert_eq!(topology_guard.groups.len(), 1);
        assert_eq!(topology_guard.groups[0].coordinator_id, "RINCON_123");
    }

    #[tokio::test]
    async fn test_topology_update_existing_player_not_readded() {
        // Start with a player already in the map
        let mut initial_players = HashMap::new();
        initial_players.insert(
            "RINCON_123".to_string(),
            Player::new(
                "RINCON_123".to_string(),
                "Living Room".to_string(),
                "wss://original:1443/ws".to_string(),
                "HH1".to_string(),
            ),
        );
        let players = Arc::new(RwLock::new(initial_players));
        let topology = Arc::new(RwLock::new(Topology {
            players: vec![],
            groups: vec![],
            household_id: Some("HH1".to_string()),
        }));
        let (event_tx, mut event_rx) = broadcast::channel(64);

        // Update with same player (should not trigger PlayerAdded)
        let response = GroupsResponse {
            groups: vec![],
            players: vec![PlayerInfo {
                id: "RINCON_123".to_string(),
                name: "Living Room Updated".to_string(), // Name changed but ID same
                websocket_url: Some("wss://new:1443/ws".to_string()),
            }],
        };

        SonosSystem::handle_topology_update(&response, &players, &topology, &event_tx, "HH1").await;

        // Player should still be in map with ORIGINAL data (not re-added)
        let players_guard = players.read().await;
        assert_eq!(
            players_guard.get("RINCON_123").unwrap().websocket_url(),
            "wss://original:1443/ws"
        );

        // Should only get TopologyUpdated, not PlayerAdded
        let event = event_rx.recv().await.unwrap();
        assert!(matches!(event, SystemEvent::TopologyUpdated(_)));
    }

    #[tokio::test]
    async fn test_topology_update_websocket_url_fallback() {
        let players = Arc::new(RwLock::new(HashMap::new()));
        let topology = Arc::new(RwLock::new(Topology {
            players: vec![],
            groups: vec![],
            household_id: Some("HH1".to_string()),
        }));
        let (event_tx, _) = broadcast::channel(64);

        // Player without websocket_url - should get fallback
        let response = GroupsResponse {
            groups: vec![],
            players: vec![PlayerInfo {
                id: "RINCON_123".to_string(),
                name: "Test".to_string(),
                websocket_url: None,
            }],
        };

        SonosSystem::handle_topology_update(&response, &players, &topology, &event_tx, "HH1").await;

        let players_guard = players.read().await;
        let player = players_guard.get("RINCON_123").unwrap();
        // Fallback URL uses player ID
        assert_eq!(
            player.websocket_url(),
            "wss://RINCON_123:1443/websocket/api"
        );
    }

    // ========================================================================
    // find_group_for_player
    // ========================================================================

    #[tokio::test]
    async fn test_find_group_for_player() {
        let players = Arc::new(RwLock::new(HashMap::new()));
        let topology = Arc::new(RwLock::new(Topology {
            players: vec![],
            groups: vec![
                Group {
                    id: "GROUP1".to_string(),
                    name: "Living Room".to_string(),
                    coordinator_id: "RINCON_123".to_string(),
                    player_ids: vec!["RINCON_123".to_string(), "RINCON_456".to_string()],
                },
                Group {
                    id: "GROUP2".to_string(),
                    name: "Bedroom".to_string(),
                    coordinator_id: "RINCON_789".to_string(),
                    player_ids: vec!["RINCON_789".to_string()],
                },
            ],
            household_id: Some("HH1".to_string()),
        }));
        let (event_tx, _) = broadcast::channel(64);
        let conn = PlayerConnection::new("test.local", 1443, "/ws");

        let system = SonosSystem {
            household_id: "HH1".to_string(),
            players,
            topology,
            event_tx,
            topology_connection: conn,
            _task_handle: Arc::new(TaskHandle {
                task: std::sync::Mutex::new(None),
            }),
        };

        // Find group for RINCON_456
        let group = system.find_group_for_player("RINCON_456").await;
        assert!(group.is_some());
        assert_eq!(group.unwrap().id, "GROUP1");

        // Find group for RINCON_789
        let group = system.find_group_for_player("RINCON_789").await;
        assert!(group.is_some());
        assert_eq!(group.unwrap().id, "GROUP2");

        // Non-existent player
        let group = system.find_group_for_player("RINCON_NOTFOUND").await;
        assert!(group.is_none());
    }
}
