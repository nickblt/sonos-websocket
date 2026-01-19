//! mDNS-based Sonos player discovery.

use mdns_sd::{ScopedIp, ServiceDaemon, ServiceEvent};
use std::collections::HashSet;
use std::time::Duration;
use tracing::trace;

/// Information about a discovered Sonos player.
#[derive(Debug, Clone)]
pub struct DiscoveredPlayer {
    pub name: String,
    pub hostname: String,
    pub addresses: HashSet<ScopedIp>,
    pub txt_info: String,
    pub txt_vers: String,
    pub txt_mhhid: String,
    pub txt_protovers: String,
    pub txt_sslport: u16,
    pub txt_min_api_version: String,
    pub txt_uuid: String,
    pub txt_bootseq: String,
    pub txt_hhid: String,
    pub txt_location: String,
    pub txt_wss: String,
    pub txt_hhsslport: u16,
    pub txt_infohash: String,
    pub txt_variant: String,
    pub txt_mdnssequence: String,
    pub txt_locationid: String,
}

/// Discover Sonos players on the network via mDNS.
///
/// Returns a list of discovered players after the timeout period.
pub fn discover_players(
    timeout: Duration,
) -> Result<Vec<DiscoveredPlayer>, Box<dyn std::error::Error>> {
    let mdns = ServiceDaemon::new()?;
    let service_type = "_sonos._tcp.local.";
    let receiver = mdns.browse(service_type)?;

    let mut seen_uuids: HashSet<String> = HashSet::new();
    let mut players: Vec<DiscoveredPlayer> = Vec::new();
    let deadline = std::time::Instant::now() + timeout;

    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                trace!(
                    fullname = info.get_fullname(),
                    hostname = info.get_hostname(),
                    addresses = ?info.get_addresses(),
                    port = info.get_port(),
                    properties = ?info.get_properties(),
                    "mDNS service resolved"
                );

                let props = info.get_properties();

                let txt_uuid = props
                    .get_property_val_str("uuid")
                    .unwrap_or_default()
                    .to_string();
                if txt_uuid.is_empty() || !seen_uuids.insert(txt_uuid.clone()) {
                    continue;
                }

                // Parse room name from service fullname: "RINCON_xxx@Room Name._sonos._tcp.local."
                let fullname = info.get_fullname();
                let name = fullname
                    .split('@')
                    .nth(1)
                    .and_then(|s| s.split("._sonos").next())
                    .unwrap_or("Unknown")
                    .to_string();

                let hostname = info.get_hostname().trim_end_matches('.').to_string();
                let addresses = info.get_addresses().clone();

                let txt_sslport = props
                    .get_property_val_str("sslport")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1443);

                let txt_hhsslport = props
                    .get_property_val_str("hhsslport")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1843);

                let txt_wss = props
                    .get_property_val_str("wss")
                    .unwrap_or_default()
                    .to_string();

                let txt_info = props
                    .get_property_val_str("info")
                    .unwrap_or_default()
                    .to_string();
                let txt_vers = props
                    .get_property_val_str("vers")
                    .unwrap_or_default()
                    .to_string();
                let txt_mhhid = props
                    .get_property_val_str("mhhid")
                    .unwrap_or_default()
                    .to_string();
                let txt_protovers = props
                    .get_property_val_str("protovers")
                    .unwrap_or_default()
                    .to_string();
                let txt_min_api_version = props
                    .get_property_val_str("minApiVersion")
                    .unwrap_or_default()
                    .to_string();
                let txt_bootseq = props
                    .get_property_val_str("bootseq")
                    .unwrap_or_default()
                    .to_string();
                let txt_hhid = props
                    .get_property_val_str("hhid")
                    .unwrap_or_default()
                    .to_string();
                let txt_location = props
                    .get_property_val_str("location")
                    .unwrap_or_default()
                    .to_string();
                let txt_infohash = props
                    .get_property_val_str("infohash")
                    .unwrap_or_default()
                    .to_string();
                let txt_variant = props
                    .get_property_val_str("variant")
                    .unwrap_or_default()
                    .to_string();
                let txt_mdnssequence = props
                    .get_property_val_str("mdnssequence")
                    .unwrap_or_default()
                    .to_string();
                let txt_locationid = props
                    .get_property_val_str("locationid")
                    .unwrap_or_default()
                    .to_string();

                players.push(DiscoveredPlayer {
                    name,
                    hostname,
                    addresses,
                    txt_info,
                    txt_vers,
                    txt_mhhid,
                    txt_protovers,
                    txt_sslport,
                    txt_min_api_version,
                    txt_uuid,
                    txt_bootseq,
                    txt_hhid,
                    txt_location,
                    txt_wss,
                    txt_hhsslport,
                    txt_infohash,
                    txt_variant,
                    txt_mdnssequence,
                    txt_locationid,
                });
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    mdns.shutdown()?;
    Ok(players)
}
