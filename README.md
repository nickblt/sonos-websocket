# sonos-websocket

A Rust library for controlling Sonos speakers via their local WebSocket API.

> **Work in Progress**: This library is under active development. Expect breaking changes between versions until 1.0.

## Why WebSocket/mDNS?

This library uses Sonos's modern WebSocket API with mDNS discovery instead of the legacy UPnP/SOAP interface:

| | WebSocket API | UPnP/SOAP |
|---|---|---|
| **Discovery** | mDNS (`_sonos._tcp.local`) | SSDP multicast |
| **Communication** | Persistent WebSocket + JSON | HTTP requests + XML |
| **Events** | Push-based subscriptions | Polling or UPnP eventing |
| **TLS** | Built-in (port 1443) | None |
| **Topology** | First-class support | Manual coordination |

The WebSocket API provides real-time event streaming, cleaner message formats, and better support for Sonos groups and households.

## Features

- **mDNS Discovery**: Find Sonos speakers on your network automatically
- **Topology Tracking**: Monitor groups, coordinators, and household changes
- **On-Demand Connections**: Players start disconnected; connect only when needed
- **Playback Control**: Play, pause, skip, seek, volume
- **Event Streaming**: Receive playback state, volume, and metadata changes
- **Cloud Queue Support**: Load cloud-based queues for streaming services
- **TLS**: Secure connections using Sonos's custom CA

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
sonos-websocket = { git = "https://github.com/your-repo/sonos-websocket" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

```rust
use sonos_websocket::{SonosSystem, SystemEvent};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Discover speakers and connect to the system
    let system = SonosSystem::start(Duration::from_secs(3)).await?;
    println!("Connected to household: {}", system.household_id());

    // Wait for topology
    let mut events = system.events();
    let topology = loop {
        if let Ok(SystemEvent::TopologyUpdated(t)) = events.recv().await {
            break t;
        }
    };

    // Control a group via its coordinator
    if let Some(group) = topology.groups.first() {
        if let Some(coordinator) = system.get_coordinator(group).await {
            coordinator.connect().await?;

            // Get current state
            let status = coordinator.get_playback_status(&group.id).await?;
            let volume = coordinator.get_volume(&group.id).await?;

            // Control playback
            coordinator.play(&group.id).await?;
            coordinator.set_volume(&group.id, 30).await?;
        }
    }

    Ok(())
}
```

## Architecture

```
┌─────────────────┐
│  Your App       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐     mDNS discovery      ┌─────────────────┐
│  SonosSystem    │ ◄──────────────────────►│ Sonos Speakers  │
│                 │     WebSocket (TLS)     │                 │
│  - Topology     │ ◄──────────────────────►│ - Groups        │
│  - Players      │     JSON messages       │ - Coordinators  │
└─────────────────┘                         └─────────────────┘
```

**Key concepts:**
- **SonosSystem**: Entry point. Discovers speakers, tracks topology, creates `Player` objects
- **Player**: Represents a single speaker. Starts disconnected; call `connect()` for control
- **Group**: One or more players playing in sync. Has a coordinator that accepts commands
- **Topology**: Current state of all players and groups in a household

## Events

### System Events (topology changes)

```rust
let mut events = system.events();
while let Ok(event) = events.recv().await {
    match event {
        SystemEvent::TopologyUpdated(topology) => { /* groups/players changed */ }
        SystemEvent::PlayerAdded(info) => { /* new player discovered */ }
        SystemEvent::PlayerRemoved(id) => { /* player went offline */ }
    }
}
```

### Player Events (after connecting)

```rust
let mut events = player.events();
while let Ok(event) = events.recv().await {
    match event {
        PlayerEvent::PlaybackChanged(status) => { /* play/pause/position */ }
        PlayerEvent::VolumeChanged(volume) => { /* volume/mute */ }
        PlayerEvent::MetadataChanged(meta) => { /* track info */ }
        PlayerEvent::Connected | PlayerEvent::Disconnected => { /* connection state */ }
        _ => {}
    }
}
```

## Examples

```bash
# Discover speakers on the network
cargo run --example discover

# Connect and show topology
cargo run --example connect

# Control playback
cargo run --example playback

# Subscribe to events
cargo run --example subscribe
```

## Network Requirements

- Same network/VLAN as Sonos speakers
- mDNS traffic allowed (port 5353 UDP)
- Access to speaker port 1443 (WebSocket over TLS)

## Limitations

- Single household support (first discovered)
- No S1 (legacy) speaker support
- Cloud queue server implementation not included (bring your own)

## License

MIT
