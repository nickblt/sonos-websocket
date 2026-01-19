use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Error, Result};

/// Global command ID counter for unique message IDs
static CMD_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generate a unique command ID
pub fn next_cmd_id() -> String {
    CMD_ID_COUNTER.fetch_add(1, Ordering::Relaxed).to_string()
}

/// WebSocket subprotocol required by Sonos
pub const WEBSOCKET_PROTOCOL: &str = "v1.api.smartspeaker.audio";

/// API key required for Sonos local API
pub const API_KEY: &str = "85f746ce-7307-4acf-bc36-71c9979d7b00";

/// Header for outgoing WebSocket messages
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestHeader {
    pub namespace: String,
    pub command: String,
    pub cmd_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub household_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

impl RequestHeader {
    /// Create a new request header for a household-level command
    pub fn household(namespace: &str, command: &str, household_id: &str) -> Self {
        Self {
            namespace: namespace.to_string(),
            command: command.to_string(),
            cmd_id: next_cmd_id(),
            household_id: Some(household_id.to_string()),
            group_id: None,
            player_id: None,
            session_id: None,
            authorization: None,
            user_id: None,
        }
    }

    /// Create a new request header for a group-level command
    pub fn group(namespace: &str, command: &str, household_id: &str, group_id: &str) -> Self {
        Self {
            namespace: namespace.to_string(),
            command: command.to_string(),
            cmd_id: next_cmd_id(),
            household_id: Some(household_id.to_string()),
            group_id: Some(group_id.to_string()),
            player_id: None,
            session_id: None,
            authorization: None,
            user_id: None,
        }
    }

    /// Create a new request header for a player-level command
    pub fn player(namespace: &str, command: &str, household_id: &str, player_id: &str) -> Self {
        Self {
            namespace: namespace.to_string(),
            command: command.to_string(),
            cmd_id: next_cmd_id(),
            household_id: Some(household_id.to_string()),
            group_id: None,
            player_id: Some(player_id.to_string()),
            session_id: None,
            authorization: None,
            user_id: None,
        }
    }

    /// Create a new request header for a session-level command (playbackSession namespace)
    pub fn session(command: &str, household_id: &str, session_id: &str) -> Self {
        Self {
            namespace: "playbackSession".to_string(),
            command: command.to_string(),
            cmd_id: next_cmd_id(),
            household_id: Some(household_id.to_string()),
            group_id: None,
            player_id: None,
            session_id: Some(session_id.to_string()),
            authorization: None,
            user_id: None,
        }
    }
}

/// Header for incoming WebSocket messages
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseHeader {
    pub namespace: String,

    /// Present for command responses
    #[serde(default)]
    pub response: Option<String>,

    /// Present for events/notifications
    #[serde(default)]
    pub name: Option<String>,

    /// Response type (e.g., "none", "playbackStatus", "groupVolume", "globalError")
    #[serde(rename = "type")]
    pub response_type: Option<String>,

    /// Whether the command succeeded (for command responses)
    #[serde(default)]
    pub success: Option<bool>,

    /// Command ID (echoed back from request)
    #[serde(default)]
    pub cmd_id: Option<String>,

    #[serde(default)]
    pub household_id: Option<String>,

    #[serde(default)]
    pub group_id: Option<String>,

    #[serde(default)]
    pub player_id: Option<String>,

    /// Error code for error responses
    #[serde(default)]
    pub error_code: Option<String>,

    /// Error reason for error responses
    #[serde(default)]
    pub reason: Option<String>,
}

impl ResponseHeader {
    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        self.response_type.as_deref() == Some("globalError")
            || self.error_code.is_some()
            || self.success == Some(false)
    }

    /// Check if this is an event (not a command response)
    pub fn is_event(&self) -> bool {
        self.name.is_some() && self.response.is_none()
    }
}

/// A complete WebSocket message (header + body)
#[derive(Debug, Clone)]
pub struct Message {
    pub header: ResponseHeader,
    pub body: Value,
}

impl Message {
    /// Parse a raw WebSocket text message into a Message
    pub fn parse(text: &str) -> Result<Self> {
        // Messages are JSON arrays: [header, body]
        let arr: Vec<Value> = serde_json::from_str(text)?;

        if arr.len() != 2 {
            return Err(Error::Json(serde_json::Error::io(std::io::Error::other(
                format!("expected 2-element array, got {}", arr.len()),
            ))));
        }

        let header: ResponseHeader = serde_json::from_value(arr[0].clone())?;
        let body = arr[1].clone();

        Ok(Self { header, body })
    }

    /// Get the body as a specific type
    pub fn body_as<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        Ok(serde_json::from_value(self.body.clone())?)
    }
}

/// Build a WebSocket message from header and body
pub fn build_message(header: &RequestHeader, body: &Value) -> Result<String> {
    let arr = serde_json::json!([header, body]);
    Ok(serde_json::to_string(&arr)?)
}

/// Build a WebSocket message with an empty body
pub fn build_message_empty(header: &RequestHeader) -> Result<String> {
    build_message(header, &serde_json::json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_response() {
        let raw = r#"[{"namespace":"groups","householdId":"Sonos_xxx","response":"subscribe","success":true,"type":"none","cmdId":"1"},{}]"#;

        let msg = Message::parse(raw).unwrap();
        assert_eq!(msg.header.namespace, "groups");
        assert_eq!(msg.header.response, Some("subscribe".to_string()));
        assert_eq!(msg.header.success, Some(true));
        assert!(!msg.header.is_error());
        assert!(!msg.header.is_event());
    }

    #[test]
    fn test_parse_event() {
        let raw = r#"[{"namespace":"groups","householdId":"Sonos_xxx","name":"groupsChanged","type":"groups"},{"groups":[]}]"#;

        let msg = Message::parse(raw).unwrap();
        assert_eq!(msg.header.namespace, "groups");
        assert_eq!(msg.header.name, Some("groupsChanged".to_string()));
        assert!(msg.header.is_event());
    }

    #[test]
    fn test_build_message() {
        let header = RequestHeader::household("groups", "subscribe", "Sonos_xxx");
        let msg = build_message_empty(&header).unwrap();

        // Verify it parses as valid JSON array
        let arr: Vec<Value> = serde_json::from_str(&msg).unwrap();
        assert_eq!(arr.len(), 2);
    }

    // ========================================================================
    // Malformed message handling
    // ========================================================================

    #[test]
    fn test_parse_invalid_json() {
        let raw = "not valid json";
        assert!(Message::parse(raw).is_err());
    }

    #[test]
    fn test_parse_empty_array() {
        let raw = "[]";
        assert!(Message::parse(raw).is_err());
    }

    #[test]
    fn test_parse_single_element_array() {
        let raw = r#"[{"namespace":"groups"}]"#;
        assert!(Message::parse(raw).is_err());
    }

    #[test]
    fn test_parse_three_element_array() {
        let raw = r#"[{"namespace":"groups"},{},{}]"#;
        assert!(Message::parse(raw).is_err());
    }

    #[test]
    fn test_parse_missing_namespace() {
        // namespace is required, serde should fail
        let raw = r#"[{"response":"subscribe"},{}]"#;
        assert!(Message::parse(raw).is_err());
    }

    #[test]
    fn test_parse_null_body() {
        let raw = r#"[{"namespace":"groups"},null]"#;
        let msg = Message::parse(raw).unwrap();
        assert_eq!(msg.header.namespace, "groups");
        assert!(msg.body.is_null());
    }

    // ========================================================================
    // ResponseHeader variants
    // ========================================================================

    #[test]
    fn test_parse_error_response_global_error() {
        let raw = r#"[{"namespace":"groups","type":"globalError","errorCode":"ERROR_INVALID","reason":"Invalid request"},{}]"#;

        let msg = Message::parse(raw).unwrap();
        assert!(msg.header.is_error());
        assert_eq!(msg.header.error_code, Some("ERROR_INVALID".to_string()));
        assert_eq!(msg.header.reason, Some("Invalid request".to_string()));
    }

    #[test]
    fn test_parse_error_response_success_false() {
        let raw = r#"[{"namespace":"playback","response":"play","success":false,"cmdId":"5"},{}]"#;

        let msg = Message::parse(raw).unwrap();
        assert!(msg.header.is_error());
        assert_eq!(msg.header.success, Some(false));
    }

    #[test]
    fn test_parse_error_response_with_error_code() {
        let raw = r#"[{"namespace":"playback","errorCode":"NOT_SUPPORTED"},{}]"#;

        let msg = Message::parse(raw).unwrap();
        assert!(msg.header.is_error());
    }

    #[test]
    fn test_parse_playback_status_response() {
        let raw = r#"[{"namespace":"playback","householdId":"HH1","groupId":"GRP1","response":"getPlaybackStatus","success":true,"type":"playbackStatus","cmdId":"10"},{"playbackState":"PLAYBACK_STATE_PLAYING"}]"#;

        let msg = Message::parse(raw).unwrap();
        assert_eq!(msg.header.namespace, "playback");
        assert_eq!(msg.header.group_id, Some("GRP1".to_string()));
        assert_eq!(msg.header.response_type, Some("playbackStatus".to_string()));
        assert!(!msg.header.is_error());
        assert!(!msg.header.is_event());
    }

    #[test]
    fn test_parse_volume_event() {
        let raw = r#"[{"namespace":"groupVolume","householdId":"HH1","groupId":"GRP1","name":"volume","type":"groupVolume"},{"volume":50,"muted":false}]"#;

        let msg = Message::parse(raw).unwrap();
        assert!(msg.header.is_event());
        assert_eq!(msg.header.name, Some("volume".to_string()));
    }

    #[test]
    fn test_parse_player_level_response() {
        let raw = r#"[{"namespace":"playerVolume","householdId":"HH1","playerId":"RINCON_123","response":"setVolume","success":true,"type":"none","cmdId":"15"},{}]"#;

        let msg = Message::parse(raw).unwrap();
        assert_eq!(msg.header.player_id, Some("RINCON_123".to_string()));
        assert!(!msg.header.is_error());
    }

    // ========================================================================
    // Event vs command discrimination edge cases
    // ========================================================================

    #[test]
    fn test_is_event_with_both_name_and_response() {
        // Edge case: has both name and response - should NOT be event
        let raw = r#"[{"namespace":"test","name":"something","response":"something"},{}]"#;

        let msg = Message::parse(raw).unwrap();
        assert!(!msg.header.is_event());
    }

    #[test]
    fn test_is_event_with_only_response() {
        let raw = r#"[{"namespace":"test","response":"subscribe"},{}]"#;

        let msg = Message::parse(raw).unwrap();
        assert!(!msg.header.is_event());
    }

    #[test]
    fn test_is_event_with_only_name() {
        let raw = r#"[{"namespace":"test","name":"notification"},{}]"#;

        let msg = Message::parse(raw).unwrap();
        assert!(msg.header.is_event());
    }

    // ========================================================================
    // body_as deserialization
    // ========================================================================

    #[test]
    fn test_body_as_valid_type() {
        let raw = r#"[{"namespace":"test"},{"value":42}]"#;

        #[derive(Deserialize)]
        struct TestBody {
            value: i32,
        }

        let msg = Message::parse(raw).unwrap();
        let body: TestBody = msg.body_as().unwrap();
        assert_eq!(body.value, 42);
    }

    #[test]
    fn test_body_as_wrong_type() {
        let raw = r#"[{"namespace":"test"},{"value":"not a number"}]"#;

        #[derive(Deserialize)]
        struct TestBody {
            #[allow(dead_code)]
            value: i32,
        }

        let msg = Message::parse(raw).unwrap();
        assert!(msg.body_as::<TestBody>().is_err());
    }

    // ========================================================================
    // RequestHeader builders
    // ========================================================================

    #[test]
    fn test_request_header_household() {
        let header = RequestHeader::household("groups", "subscribe", "HH1");

        assert_eq!(header.namespace, "groups");
        assert_eq!(header.command, "subscribe");
        assert_eq!(header.household_id, Some("HH1".to_string()));
        assert!(header.group_id.is_none());
        assert!(header.player_id.is_none());
    }

    #[test]
    fn test_request_header_group() {
        let header = RequestHeader::group("playback", "play", "HH1", "GRP1");

        assert_eq!(header.namespace, "playback");
        assert_eq!(header.command, "play");
        assert_eq!(header.household_id, Some("HH1".to_string()));
        assert_eq!(header.group_id, Some("GRP1".to_string()));
        assert!(header.player_id.is_none());
    }

    #[test]
    fn test_request_header_player() {
        let header = RequestHeader::player("playerVolume", "setVolume", "HH1", "RINCON_123");

        assert_eq!(header.namespace, "playerVolume");
        assert_eq!(header.command, "setVolume");
        assert_eq!(header.household_id, Some("HH1".to_string()));
        assert!(header.group_id.is_none());
        assert_eq!(header.player_id, Some("RINCON_123".to_string()));
    }

    #[test]
    fn test_request_header_session() {
        let header = RequestHeader::session("loadCloudQueue", "HH1", "SESSION_123");

        assert_eq!(header.namespace, "playbackSession");
        assert_eq!(header.command, "loadCloudQueue");
        assert_eq!(header.household_id, Some("HH1".to_string()));
        assert!(header.group_id.is_none());
        assert!(header.player_id.is_none());
        assert_eq!(header.session_id, Some("SESSION_123".to_string()));
    }

    #[test]
    fn test_cmd_id_unique() {
        let h1 = RequestHeader::household("a", "b", "c");
        let h2 = RequestHeader::household("a", "b", "c");

        assert_ne!(h1.cmd_id, h2.cmd_id);
    }

    // ========================================================================
    // Message building
    // ========================================================================

    #[test]
    fn test_build_message_with_body() {
        let header = RequestHeader::group("playback", "play", "HH1", "GRP1");
        let body = serde_json::json!({"speed": 1.0});
        let msg = build_message(&header, &body).unwrap();

        // Parse it back
        let arr: Vec<Value> = serde_json::from_str(&msg).unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[1]["speed"], 1.0);
    }

    #[test]
    fn test_build_message_serializes_optional_fields() {
        let header = RequestHeader::household("groups", "subscribe", "HH1");
        let msg = build_message_empty(&header).unwrap();

        // Optional None fields should not appear in JSON
        assert!(!msg.contains("groupId"));
        assert!(!msg.contains("playerId"));
        assert!(msg.contains("householdId"));
    }
}
