use sonos_websocket::discover_players;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let players = discover_players(Duration::from_secs(3))?;

    println!("Found {} players:", players.len());
    for player in &players {
        println!("{:#?}", player);
    }

    Ok(())
}
