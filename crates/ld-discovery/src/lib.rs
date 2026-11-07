use anyhow::Result;
use ld_protocol::SERVICE_TYPE;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

#[derive(Debug, Clone)]
pub struct Peer {
    pub id: String,
    pub ver: u16,
    pub name: String,
    pub addrs: Vec<SocketAddr>,
    pub fp: Option<String>,
    pub seen: Instant,
}

#[derive(Debug)]
pub enum DiscoveryEvent {
    Found(Peer),
    Lost(String),
}

pub struct Discovery {
    daemon: ServiceDaemon,
    registered: Mutex<Option<String>>,
}

impl Discovery {
    pub fn new() -> Result<Self> {
        Ok(Self { daemon: ServiceDaemon::new()?, registered: Mutex::new(None) })
    }

    pub fn unannounce(&self) {
        if let Some(full) = self.registered.lock().unwrap().take() {
            let _ = self.daemon.unregister(&full);
        }
    }

    pub fn announce(&self, name: &str, port: u16, fp_hex: &str) -> Result<()> {
        self.unannounce();
        let short = fp_hex.get(..16).unwrap_or(fp_hex);
        let instance = format!("ld-{short}");
        let host = format!("{instance}.local.");
        let mut props = HashMap::new();
        props.insert("name".to_string(), name.to_string());
        props.insert("ver".to_string(), ld_protocol::PROTOCOL_VERSION.to_string());
        props.insert("fp".to_string(), fp_hex.to_string());
        let info = ServiceInfo::new(SERVICE_TYPE, &instance, &host, "", port, props)?
            .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        self.daemon.register(info)?;
        *self.registered.lock().unwrap() = Some(fullname);
        Ok(())
    }

    pub fn shutdown(&self) {
        if let Some(full) = self.registered.lock().unwrap().take() {
            if let Ok(rx) = self.daemon.unregister(&full) {
                let _ = rx.recv_timeout(Duration::from_millis(500));
            }
        }
        if let Ok(rx) = self.daemon.shutdown() {
            let _ = rx.recv_timeout(Duration::from_millis(500));
        }
    }

    pub fn browse(&self) -> Result<UnboundedReceiver<DiscoveryEvent>> {
        let rx = self.daemon.browse(SERVICE_TYPE)?;
        let (tx, out) = unbounded_channel();
        std::thread::spawn(move || {
            while let Ok(ev) = rx.recv() {
                let msg = match ev {
                    ServiceEvent::ServiceResolved(info) => {
                        let mut v4: Vec<Ipv4Addr> = info
                            .get_addresses()
                            .iter()
                            .filter_map(|a| match a {
                                IpAddr::V4(v) => Some(*v),
                                _ => None,
                            })
                            .collect();
                        v4.sort_by_key(|a| (a.is_link_local(), *a));
                        if v4.is_empty() {
                            None
                        } else {
                            let port = info.get_port();
                            let id = info.get_fullname().to_string();
                            let name = info
                                .get_property_val_str("name")
                                .unwrap_or(&id)
                                .to_string();
                            Some(DiscoveryEvent::Found(Peer {
                                id,
                                ver: info.get_property_val_str("ver").and_then(|v| v.parse().ok()).unwrap_or(1),
                                name,
                                addrs: v4
                                    .into_iter()
                                    .map(|ip| SocketAddr::new(IpAddr::V4(ip), port))
                                    .collect(),
                                fp: info.get_property_val_str("fp").map(str::to_string),
                                seen: Instant::now(),
                            }))
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => Some(DiscoveryEvent::Lost(fullname)),
                    _ => None,
                };
                if let Some(m) = msg {
                    if tx.send(m).is_err() {
                        break;
                    }
                }
            }
        });
        Ok(out)
    }
}