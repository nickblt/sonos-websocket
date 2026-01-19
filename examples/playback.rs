//! Example: Control playback using SonosSystem
//!
//! This demonstrates the high-level API for controlling Sonos playback.
//! Run with: cargo run --example playback

use sonos_websocket::{PlayState, SonosSystem, SystemEvent};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("sonos_websocket=info".parse()?),
        )
        .init();

    println!("=== Sonos Playback Control Example ===\n");

    // Discover and connect to Sonos system
    println!("Discovering Sonos players...");
    let system = SonosSystem::start(Duration::from_secs(3)).await?;

    println!("Connected! Household: {}\n", system.household_id());

    // Wait for initial topology
    let mut events = system.events();
    println!("Waiting for topology...");

    let topology = loop {
        if let Ok(SystemEvent::TopologyUpdated(topology)) = events.recv().await {
            break topology;
        }
    };

    println!("Found {} player(s):", topology.players.len());
    for player_info in &topology.players {
        println!("  - {} ({})", player_info.name, player_info.id);
    }
    println!();

    println!("Found {} group(s):", topology.groups.len());
    for group in &topology.groups {
        println!("  - {} (coordinator: {})", group.name, group.coordinator_id);
    }
    println!();

    // Get the first group and its coordinator
    if let Some(group) = topology.groups.first() {
        if let Some(coordinator) = system.get_coordinator(group).await {
            println!("=== Controlling group: {} ===\n", group.name);

            // Connect to the coordinator
            println!("Connecting to coordinator {}...", coordinator.name());
            coordinator.connect().await?;
            println!("Connected!\n");

            // Get current playback status
            match coordinator.get_playback_status(&group.id).await {
                Ok(status) => {
                    let state = match status.state {
                        PlayState::Idle => "Idle",
                        PlayState::Playing => "Playing",
                        PlayState::Paused => "Paused",
                        PlayState::Buffering => "Buffering",
                    };
                    println!("Current state: {}", state);
                    if let Some(pos) = status.position_millis {
                        println!("Position: {} ms", pos);
                    }
                }
                Err(e) => println!("Could not get playback status: {}", e),
            }

            // Get current volume
            match coordinator.get_volume(&group.id).await {
                Ok(vol) => {
                    println!("Volume: {}, Muted: {}", vol.volume, vol.muted);
                }
                Err(e) => println!("Could not get volume: {}", e),
            }

            // Get current track metadata
            match coordinator.get_metadata(&group.id).await {
                Ok(metadata) => {
                    if let Some(name) = metadata.track_name() {
                        println!("Now playing: {}", name);
                    }
                    if let Some(artist) = metadata.artist_name() {
                        println!("Artist: {}", artist);
                    }
                    if let Some(album) = metadata.album_name() {
                        println!("Album: {}", album);
                    }
                }
                Err(e) => println!("Could not get metadata: {}", e),
            }
            println!();

            // Example: you could control playback here
            // coordinator.play(&group.id).await?;
            // coordinator.pause(&group.id).await?;
            // coordinator.set_volume(&group.id, 30).await?;
        } else {
            println!("Could not find coordinator for group");
        }
    } else {
        println!("No groups found!");
    }

    println!("Done.");
    Ok(())
}
