use serde_json::Value;
use sonos_websocket::{ConnectionEvent, ConnectionState, PlayerConnection, discover_players};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

fn print_topology(body: &Value) {
    if let Some(groups) = body.get("groups").and_then(|v| v.as_array()) {
        println!("Groups ({}):", groups.len());
        for group in groups {
            let name = group.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let coordinator = group
                .get("coordinatorId")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let player_count = group
                .get("playerIds")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            println!(
                "  - {} ({} players, coordinator: {})",
                name, player_count, coordinator
            );
        }
    }

    if let Some(players) = body.get("players").and_then(|v| v.as_array()) {
        println!("Players ({}):", players.len());
        for player in players {
            let name = player.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let id = player.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            println!("  - {} ({})", name, id);
        }
    }
    println!();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("sonos_websocket=debug".parse()?),
        )
        .init();

    println!("Discovering Sonos players...");
    let players = discover_players(Duration::from_secs(3))?;

    if players.is_empty() {
        println!("No players found!");
        return Ok(());
    }

    println!("\nFound {} player(s):", players.len());
    for player in &players {
        println!(
            "  - {} ({}) - household: {}",
            player.name, player.hostname, player.txt_mhhid
        );
    }

    // Pick the first player
    let player = &players[0];
    println!(
        "\nConnecting to {} ({}:{}{})...",
        player.name, player.hostname, player.txt_sslport, player.txt_wss
    );

    // Create connection
    let conn = PlayerConnection::new(&player.hostname, player.txt_sslport, &player.txt_wss);

    // Subscribe to events before connecting
    let mut events = conn.events();

    // Connect
    conn.connect().await?;

    // Wait for connected state
    loop {
        if conn.state().await == ConnectionState::Connected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    println!("Connected! Subscribing to groups...");

    // Subscribe to groups namespace
    let response = conn
        .subscribe("groups", &player.txt_mhhid, None, None)
        .await?;

    if response.header.success != Some(true) {
        println!("Subscribe failed: {:?}", response.header);
        return Ok(());
    }
    println!("Subscribed! Listening for events (Ctrl+C to quit)...\n");

    // Listen for events - initial topology comes as first event after subscribe
    loop {
        tokio::select! {
            Ok(event) = events.recv() => {
                match event {
                    ConnectionEvent::Message(msg) => {
                        match msg.header.namespace.as_str() {
                            "groups" => print_topology(&msg.body),
                            _ => {
                                if let Some(name) = &msg.header.name {
                                    println!("[Event] {} - {}", msg.header.namespace, name);
                                }
                            }
                        }
                    }
                    ConnectionEvent::Disconnected => {
                        println!("[Disconnected]");
                    }
                    ConnectionEvent::Reconnecting { attempt } => {
                        println!("[Reconnecting] attempt {}", attempt);
                    }
                    ConnectionEvent::Connected => {
                        println!("[Connected]");
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutting down...");
                break;
            }
        }
    }

    conn.disconnect().await;
    Ok(())
}
