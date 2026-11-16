use anyhow::{anyhow, Result};
use ld_protocol::SERVICE_TYPE;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
        Arc
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

#[derive(Clone)]
enum Ann {
    Open { name: String, port: u16, fp: String },
    Hidden { port: u16, fp: String, trusted: Vec<String> },
}

fn hide_tag(a_fp: &str, t_fp: &str, none: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"ld-hide-v1");
    h.update(a_fp.as_bytes());
    h.update(b"|");
    h.update(t_fp.as_bytes());
    h.update(b"|");
    h.update(none.as_bytes());
    h.finalize().as_bytes()[..8].iter().map(|x| format!("{x:02x}")).collect()
}

pub struct Discovery {
    daemon: Mutex<ServiceDaemon>,
    registered: Mutex<Option<String>>,
    params: Mutex<Option<Ann>>,
    tx: UnboundedSender<DiscoveryEvent>,
    rx: Mutex<Option<UnboundedReceiver<DiscoveryEvent>>>,
    browsing: AtomicBool,
    known: Arc<Mutex<Vec<(String, String)>>>,
    my_fp: Arc<Mutex<String>>,
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
            known: Arc::default(),
            my_fp: Arc::default()
        })
    }

    pub fn set_identity(&self, my_fp: &str) { *self.my_fp.lock().unwrap() = my_fp.to_string(); }
    pub fn set_known(&self, list: Vec<(String, String)>) { *self.known.lock().unwrap() = list; }

    fn register(&self, a: &Ann) -> Result<()> {
        let mut props = HashMap::new();
        props.insert("ver".to_string(), ld_protocol::PROTOCOL_VERSION.to_string());
        let (instance, port) = match a {
            Ann::Open { name, port, fp } => {
                props.insert("name".to_string(), name.clone());
                props.insert("fp".to_string(), fp.clone());
                (format!("ld-{}", fp.get(..16).unwrap_or(fp)), *port)
            }
            Ann::Hidden { port, fp, trusted } => {
                let mut b = [0u8; 16];
                getrandom::getrandom(&mut b).map_err(|e| anyhow!("генератор случайных чисел: {e}"))?;
                let nonce: String = b.iter().map(|x| format!("{x:02x}")).collect();
                props.insert("h".to_string(), "1".to_string());
                props.insert("n".to_string(), nonce.clone());
                let tags: Vec<String> = trusted.iter().take(40).map(|t| hide_tag(fp, t, &nonce)).collect();
                for (i, chunk) in tags.chunks(12).enumerate() {
                    props.insert(format!("t{i}"), chunk.join(","));
                }
                (format!("ld-h{}", &nonce[..12]), *port)
            }
        };
        let host = format!("{instance}.local.");
        let info = ServiceInfo::new(SERVICE_TYPE, &instance, &host, "", port, props)?.enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        self.daemon.lock().unwrap().register(info)?;
        *self.registered.lock().unwrap() = Some(fullname);
        *self.params.lock().unwrap() = Some(a.clone());
        Ok(())
    }

    pub fn announce(&self, name: &str, port: u16, fp_hex: &str) -> Result<()> {
        self.unannounce();
        self.register(&Ann::Open { name: name.into(), port, fp: fp_hex.into() })
    }

    pub fn announce_hidden(&self, port: u16, fp_hex: &str, trusted: &[String]) -> Result<()> {
        self.unannounce();
        self.register(&Ann::Hidden { port, fp: fp_hex.into(), trusted: trusted.to_vec() })
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
        let known = self.known.clone();
        let my_fp = self.my_fp.clone();
        std::thread::spawn(move || {
            while let Ok(ev) = rx.recv() {
                let msg = match ev {
                    ServiceEvent::ServiceResolved(info) => {
                        let mut v4: Vec<Ipv4Addr> = info
                            .get_addresses()
                            .iter()
                            .filter_map(|a| match a { IpAddr::V4(v) => Some(*v), _ => None })
                            .collect();
                        v4.sort_by_key(|a| (a.is_link_local(), *a));
                        let id = info.get_fullname().to_string();
                        let ident: Option<(String, Option<String>)> = if info.get_property_val_str("h") == Some("1") {
                            let nonce = info.get_property_val_str("n").unwrap_or("").to_string();
                            let mut tags: Vec<String> = Vec::new();
                            for i in 0..4 {
                                if let Some(v) = info.get_property_val_str(&format!("t{i}")) {
                                    tags.extend(v.split(',').map(str::to_string));
                                }
                            }
                            let me = my_fp.lock().unwrap().clone();
                            let list = known.lock().unwrap().clone();
                            list.into_iter()
                                .find(|(kfp, _)| tags.contains(&hide_tag(kfp, &me, &nonce)))
                                .map(|(kfp, kname)| (kname, Some(kfp)))
                        } else {
                            Some((
                                info.get_property_val_str("name").unwrap_or(&id).to_string(),
                                info.get_property_val_str("fp").map(str::to_string),
                            ))
                        };
                        match (ident, v4.is_empty()) {
                            (Some((name, fp)), false) => {
                                let port = info.get_port();
                                Some(DiscoveryEvent::Found(Peer {
                                    id,
                                    name,
                                    addrs: v4.into_iter().map(|ip| SocketAddr::new(IpAddr::V4(ip), port)).collect(),
                                    fp,
                                    ver: info.get_property_val_str("ver").and_then(|v| v.parse().ok()).unwrap_or(1),
                                    seen: Instant::now(),
                                }))
                            }
                            _ => None,
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
        if let Some(a) = params {
            self.register(&a)?;
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