//! Example: Subscribe to topology and playback events
//!
//! This demonstrates the reactive event-driven API:
//! - Listens for topology changes
//! - Connects to all coordinators
//! - Subscribes to playback/volume/metadata for each group
//!
//! Run with: cargo run --example subscribe

use sonos_websocket::{PlayState, Player, PlayerEvent, SonosSystem, SystemEvent};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("sonos_websocket=info".parse()?),
        )
        .init();

    println!("=== Sonos Subscription Example ===\n");

    // Start the system
    println!("Discovering Sonos players...");
    let system = SonosSystem::start(Duration::from_secs(3)).await?;
    println!("Connected to household: {}\n", system.household_id());

    // Track connected coordinators and their event receivers
    let mut coordinators: HashMap<String, (Player, broadcast::Receiver<PlayerEvent>)> =
        HashMap::new();

    // Get system events
    let mut system_events = system.events();

    println!("Listening for events (Ctrl+C to quit)...\n");

    loop {
        tokio::select! {
            // Handle system events (topology changes)
            Ok(event) = system_events.recv() => {
                match event {
                    SystemEvent::TopologyUpdated(topology) => {
                        println!("[Topology] Updated: {} groups, {} players",
                            topology.groups.len(), topology.players.len());

                        // Connect to any new coordinators
                        for group in &topology.groups {
                            if coordinators.contains_key(&group.coordinator_id) {
                                continue;
                            }

                            if let Some(player) = system.get_player(&group.coordinator_id).await {
                                println!("  Connecting to coordinator: {} ({})",
                                    player.name(), group.name);

                                if let Err(e) = player.connect().await {
                                    println!("    Failed to connect: {}", e);
                                    continue;
                                }

                                // Subscribe to playback events
                                if let Err(e) = player.subscribe_all(&group.id).await {
                                    println!("    Failed to subscribe: {}", e);
                                    continue;
                                }

                                println!("    Subscribed to playback events");

                                let events = player.events();
                                coordinators.insert(group.coordinator_id.clone(), (player, events));
                            }
                        }

                        // Remove coordinators that are no longer in topology
                        let current_coordinator_ids: std::collections::HashSet<_> =
                            topology.groups.iter().map(|g| g.coordinator_id.clone()).collect();

                        coordinators.retain(|id, (player, _)| {
                            if current_coordinator_ids.contains(id) {
                                true
                            } else {
                                println!("  Removing coordinator: {}", player.name());
                                false
                            }
                        });
                    }
                    SystemEvent::PlayerAdded(info) => {
                        println!("[Player Added] {} ({})", info.name, info.id);
                    }
                    SystemEvent::PlayerRemoved(id) => {
                        println!("[Player Removed] {}", id);
                    }
                }
            }

            // Poll all coordinator event channels
            _ = poll_coordinator_events(&mut coordinators) => {}

            // Handle Ctrl+C
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutting down...");
                break;
            }
        }
    }

    // Disconnect all coordinators
    for (_, (player, _)) in coordinators {
        player.disconnect().await;
    }

    Ok(())
}

/// Poll all coordinator event channels and print events
async fn poll_coordinator_events(
    coordinators: &mut HashMap<String, (Player, broadcast::Receiver<PlayerEvent>)>,
) {
    for (_, (player, events)) in coordinators.iter_mut() {
        match events.try_recv() {
            Ok(event) => {
                let name = player.name();
                match event {
                    PlayerEvent::Connected => {
                        println!("[{}] Connected", name);
                    }
                    PlayerEvent::Disconnected => {
                        println!("[{}] Disconnected", name);
                    }
                    PlayerEvent::Reconnecting => {
                        println!("[{}] Reconnecting...", name);
                    }
                    PlayerEvent::PlaybackChanged(status) => {
                        let state = match status.state {
                            PlayState::Idle => "Idle",
                            PlayState::Playing => "Playing",
                            PlayState::Paused => "Paused",
                            PlayState::Buffering => "Buffering",
                        };
                        print!("[{}] Playback: {}", name, state);
                        if let Some(pos) = status.position_millis {
                            print!(" @ {}ms", pos);
                        }
                        println!();
                    }
                    PlayerEvent::VolumeChanged(vol) => {
                        println!(
                            "[{}] Group Volume: {}{}",
                            name,
                            vol.volume,
                            if vol.muted { " (muted)" } else { "" }
                        );
                    }
                    PlayerEvent::PlayerVolumeChanged(vol) => {
                        println!(
                            "[{}] Player Volume: {}{}",
                            name,
                            vol.volume,
                            if vol.muted { " (muted)" } else { "" }
                        );
                    }
                    PlayerEvent::MetadataChanged(metadata) => {
                        if let Some(track) = metadata.track_name() {
                            print!("[{}] Now playing: {}", name, track);
                            if let Some(artist) = metadata.artist_name() {
                                print!(" - {}", artist);
                            }
                            println!();
                        }
                    }
                    PlayerEvent::PlaybackError(error) => {
                        println!("Error: {:?}", error);
                    }
                }
            }
            Err(broadcast::error::TryRecvError::Empty) => {}
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                println!("[{}] Lagged {} events", player.name(), n);
            }
            Err(broadcast::error::TryRecvError::Closed) => {}
        }
    }

    // Small sleep to prevent busy loop
    tokio::time::sleep(Duration::from_millis(10)).await;
}
