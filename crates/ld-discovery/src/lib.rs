use anyhow::{anyhow, Result};
use ld_protocol::SERVICE_TYPE;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

#[derive(Debug, Clone)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub addrs: Vec<SocketAddr>,
    pub fp: Option<String>,
    pub ver: u16,
    pub seen: Instant,
}

#[derive(Debug)]
pub enum DiscoveryEvent {
    Found(Peer),
    Lost(String),
    Reset,
}

pub struct Discovery {
    daemon: Mutex<ServiceDaemon>,
    registered: Mutex<Option<String>>,
    params: Mutex<Option<(String, u16, String)>>,
    tx: UnboundedSender<DiscoveryEvent>,
    rx: Mutex<Option<UnboundedReceiver<DiscoveryEvent>>>,
    browsing: AtomicBool,
}

impl Discovery {
    pub fn new() -> Result<Self> {
        let (tx, rx) = unbounded_channel();
        Ok(Self {
            daemon: Mutex::new(ServiceDaemon::new()?),
            registered: Mutex::new(None),
            params: Mutex::new(None),
            tx,
            rx: Mutex::new(Some(rx)),
            browsing: AtomicBool::new(false),
        })
    }

    fn register(&self, name: &str, port: u16, fp_hex: &str) -> Result<()> {
        let short = fp_hex.get(..16).unwrap_or(fp_hex);
        let instance = format!("ld-{short}");
        let host = format!("{instance}.local.");
        let mut props = HashMap::new();
        props.insert("name".to_string(), name.to_string());
        props.insert("ver".to_string(), ld_protocol::PROTOCOL_VERSION.to_string());
        props.insert("fp".to_string(), fp_hex.to_string());
        let info = ServiceInfo::new(SERVICE_TYPE, &instance, &host, "", port, props)?.enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        self.daemon.lock().unwrap().register(info)?;
        *self.registered.lock().unwrap() = Some(fullname);
        *self.params.lock().unwrap() = Some((name.to_string(), port, fp_hex.to_string()));
        Ok(())
    }

    pub fn announce(&self, name: &str, port: u16, fp_hex: &str) -> Result<()> {
        self.unannounce();
        self.register(name, port, fp_hex)
    }

    pub fn unannounce(&self) {
        *self.params.lock().unwrap() = None;
        let full = self.registered.lock().unwrap().take();
        if let Some(full) = full {
            let _ = self.daemon.lock().unwrap().unregister(&full);
        }
    }

    pub fn browse(&self) -> Result<UnboundedReceiver<DiscoveryEvent>> {
        let out = self.rx.lock().unwrap().take().ok_or_else(|| anyhow!("browse уже вызван"))?;
        self.browsing.store(true, Ordering::Relaxed);
        self.spawn_browse()?;
        Ok(out)
    }

    fn spawn_browse(&self) -> Result<()> {
        let rx = self.daemon.lock().unwrap().browse(SERVICE_TYPE)?;
        let tx = self.tx.clone();
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
                            let name = info.get_property_val_str("name").unwrap_or(&id).to_string();
                            Some(DiscoveryEvent::Found(Peer {
                                id,
                                name,
                                addrs: v4.into_iter().map(|ip| SocketAddr::new(IpAddr::V4(ip), port)).collect(),
                                fp: info.get_property_val_str("fp").map(str::to_string),
                                ver: info.get_property_val_str("ver").and_then(|v| v.parse().ok()).unwrap_or(1),
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
        Ok(())
    }

    pub fn restart(&self) -> Result<()> {
        let fresh = ServiceDaemon::new()?;
        let old = std::mem::replace(&mut *self.daemon.lock().unwrap(), fresh);
        let _ = old.shutdown();
        *self.registered.lock().unwrap() = None;
        let params = self.params.lock().unwrap().clone();
        if let Some((name, port, fp)) = params {
            self.register(&name, port, &fp)?;
        }
        if self.browsing.load(Ordering::Relaxed) {
            self.spawn_browse()?;
        }
        let _ = self.tx.send(DiscoveryEvent::Reset);
        Ok(())
    }

    pub fn shutdown(&self) {
        let full = self.registered.lock().unwrap().take();
        let d = self.daemon.lock().unwrap();
        if let Some(full) = full {
            if let Ok(rx) = d.unregister(&full) {
                let _ = rx.recv_timeout(Duration::from_millis(500));
            }
        }
        if let Ok(rx) = d.shutdown() {
            let _ = rx.recv_timeout(Duration::from_millis(500));
        }
    }
}