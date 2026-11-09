use ld_core::{Core, Decision, Direction, Event, Kind, Payload, SendOutcome, Visibility, PROTOCOL_VERSION};
use ld_crypto::{fp_hex, fp_short, Identity, TrustStore};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast::error::RecvError;

uniffi::setup_scaffolding!();

// Types for swift
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum LdError {
    #[error("{msg}")]
    Failed { msg: String },
}

impl From<anyhow::Error> for LdError {
    fn from(e: anyhow::Error) -> Self {
        LdError::Failed { msg: format!("{e:#}") }
    }
}

fn fail(msg: &str) -> LdError {
    LdError::Failed { msg: msg.to_string() }
}

#[derive(Clone, uniffi::Record)]
pub struct FfiPeer {
    pub id: String,
    pub name: String,
    pub addrs: Vec<String>,
    pub fp: Option<String>,
    pub ver: u16,
}

#[derive(uniffi::Record)]
pub struct FfiDevice {
    pub fingerprint: String,
    pub name: String,
    pub trusted: bool,
}

#[derive(uniffi::Enum)]
pub enum FfiKind {
    Files,
    Text,
}

#[derive(uniffi::Enum)]
pub enum FfiDirection {
    Send,
    Receive,
}

#[derive(uniffi::Enum)]
pub enum FfiDecision {
    Reject,
    AcceptOnce,
    AcceptAndTrust,
}

#[derive(uniffi::Enum)]
pub enum FfiVisibility {
    Off,
    TrustedOnly,
    Everyone,
}

#[derive(uniffi::Enum)]
pub enum FfiEvent {
    Incoming {
        id: u64,
        sender: String,
        kind: FfiKind,
        name: String,
        count: u32,
        size: u64,
        preview: Option<String>,
        fp_short: String,
        code: String,
        known_name: Option<String>,
        timeout_secs: u64,
    },
    PeerCode { id: u64, code: String, new_device: bool, trusted: bool },
    Started { id: u64, direction: FfiDirection, name: String, size: u64, peer_name: String },
    Progress { id: u64, done: u64, total: u64 },
    TextReceived { id: u64, text: String },
    Finished { id: u64, path: Option<String> },
    Rejected { id: u64 },
    Failed { id: u64, error: String },
    NameConflict { id: u64 },
}

#[uniffi::export(with_foreign)]
pub trait EventListener: Send + Sync {
    fn on_event(&self, event: FfiEvent);
}

fn convert(e: Event) -> FfiEvent {
    match e {
        Event::Incoming { id, sender, kind, name, count, size, preview, fp_short, code, known_name, timeout_secs } => {
            FfiEvent::Incoming {
                id,
                sender,
                kind: match kind {
                    Kind::Files => FfiKind::Files,
                    Kind::Text => FfiKind::Text,
                },
                name,
                count,
                size,
                preview,
                fp_short,
                code,
                known_name,
                timeout_secs,
            }
        }
        Event::PeerCode { id, code, new_device, trusted } => FfiEvent::PeerCode { id, code, new_device, trusted },
        Event::Started { id, direction, name, size, peer_name } => FfiEvent::Started {
            id,
            direction: match direction {
                Direction::Send => FfiDirection::Send,
                Direction::Receive => FfiDirection::Receive,
            },
            name,
            size,
            peer_name,
        },
        Event::Progress { id, done, total } => FfiEvent::Progress { id, done, total },
        Event::TextReceived { id, text } => FfiEvent::TextReceived { id, text },
        Event::Finished { id, path } => FfiEvent::Finished { id, path },
        Event::Rejected { id } => FfiEvent::Rejected { id },
        Event::Failed { id, error } => FfiEvent::Failed { id, error },
    }
}

// app obj
#[derive(uniffi::Object)]
pub struct LocalDrop {
    rt: tokio::runtime::Runtime,
    core: Arc<Core>,
    client: quinn::Endpoint,
    server: quinn::Endpoint,
    port: u16,
    name: Mutex<String>,
    fp_hex: String,
    fp_short: String,
    download_dir: PathBuf,
    peers: Mutex<HashMap<String, FfiPeer>>,
    listener: Mutex<Option<Arc<dyn EventListener>>>,
}

impl LocalDrop {
    fn build(data_dir: String, download_dir: String, device_name: String) -> anyhow::Result<Arc<Self>> {
        let dd = PathBuf::from(data_dir);
        std::fs::create_dir_all(&dd)?;
        let download_dir = PathBuf::from(download_dir);
        std::fs::create_dir_all(&download_dir)?;

        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
        let id = Identity::load_or_create(&dd.join("identity"))?;
        let store = TrustStore::load(dd.join("trusted.json"))?;
        let core = Core::new(id.fingerprint, store);

        let (server, client) = {
            let _guard = rt.enter();
            let server = ld_transport::server_endpoint(SocketAddr::from(([0, 0, 0, 0], 53317)), &id)
                .or_else(|_| ld_transport::server_endpoint(SocketAddr::from(([0, 0, 0, 0], 0)), &id))?;
            (server, ld_transport::client_endpoint(&id)?)
        };
        let port = server.local_addr()?.port();

        Ok(Arc::new(Self {
            rt,
            core,
            client,
            server,
            port,
            name: Mutex::new(device_name),
            fp_hex: fp_hex(&id.fingerprint),
            fp_short: fp_short(&id.fingerprint),
            download_dir,
            peers: Mutex::new(HashMap::new()),
            listener: Mutex::new(None),
        }))
    }

    fn spawn_send(&self, peer_id: String, payload: Payload, allow_conflict: bool) -> Result<u64, LdError> {
        let peer = self
            .peers
            .lock()
            .unwrap()
            .get(&peer_id)
            .cloned()
            .ok_or_else(|| fail("Устройство больше не доступно"))?;
        if peer.ver != PROTOCOL_VERSION {
            return Err(LdError::Failed {
                msg: format!("«{}» использует другую версию LocalDrop. Обновите приложение на обоих устройствах.", peer.name),
            });
        }
        let addrs: Vec<SocketAddr> = peer.addrs.iter().filter_map(|a| a.parse().ok()).collect();
        if addrs.is_empty() {
            return Err(fail("У устройства нет доступных адресов"));
        }

        let id = self.core.next_id();
        let core = self.core.clone();
        let client = self.client.clone();
        let my_name = self.name.lock().unwrap().clone();
        let listener = self.listener.lock().unwrap().clone();

        self.rt.spawn(async move {
            let r = core
                .send(&client, &addrs, payload, &my_name, Some(&peer.name), allow_conflict, id)
                .await;
            if let Ok(SendOutcome::NameConflict) = r {
                if let Some(l) = listener {
                    l.on_event(FfiEvent::NameConflict { id });
                }
            }
        });
        Ok(id)
    }
}

#[uniffi::export]
pub fn protocol_version() -> u16 {
    PROTOCOL_VERSION
}

#[uniffi::export]
impl LocalDrop {
    #[uniffi::constructor]
    pub fn new(data_dir: String, download_dir: String, device_name: String) -> Result<Arc<Self>, LdError> {
        Ok(Self::build(data_dir, download_dir, device_name)?)
    }

    pub fn start(&self, listener: Arc<dyn EventListener>) {
        *self.listener.lock().unwrap() = Some(listener.clone());

        let mut rx = self.core.subscribe();
        self.rt.spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(e) => listener.on_event(convert(e)),
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        });

        let core = self.core.clone();
        let server = self.server.clone();
        let dir = self.download_dir.clone();
        self.rt.spawn(async move {
            let _ = core.run_receiver(server, dir).await;
        });
    }

    pub fn shutdown(&self) {
        self.server.close(0u32.into(), b"bye");
    }

    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn fingerprint_hex(&self) -> String {
        self.fp_hex.clone()
    }
    pub fn fingerprint_short(&self) -> String {
        self.fp_short.clone()
    }
    pub fn device_name(&self) -> String {
        self.name.lock().unwrap().clone()
    }
    pub fn set_device_name(&self, name: String) {
        *self.name.lock().unwrap() = name;
    }

    pub fn peer_found(&self, peer: FfiPeer) {
        self.peers.lock().unwrap().insert(peer.id.clone(), peer);
    }
    pub fn peer_lost(&self, id: String) {
        self.peers.lock().unwrap().remove(&id);
    }

    pub fn send_files(&self, peer_id: String, paths: Vec<String>, allow_conflict: bool) -> Result<u64, LdError> {
        if paths.is_empty() {
            return Err(fail("Нечего отправлять"));
        }
        self.spawn_send(peer_id, Payload::Paths(paths.into_iter().map(PathBuf::from).collect()), allow_conflict)
    }

    pub fn send_text(&self, peer_id: String, text: String, allow_conflict: bool) -> Result<u64, LdError> {
        if text.trim().is_empty() {
            return Err(fail("Текст пуст"));
        }
        self.spawn_send(peer_id, Payload::Text(text), allow_conflict)
    }

    pub fn respond(&self, id: u64, decision: FfiDecision) -> bool {
        self.core.respond(
            id,
            match decision {
                FfiDecision::Reject => Decision::Reject,
                FfiDecision::AcceptOnce => Decision::AcceptOnce,
                FfiDecision::AcceptAndTrust => Decision::AcceptAndTrust,
            },
        )
    }

    pub fn devices(&self) -> Vec<FfiDevice> {
        self.core
            .devices()
            .into_iter()
            .map(|d| FfiDevice { fingerprint: d.fingerprint, name: d.name, trusted: d.trusted })
            .collect()
    }
    pub fn set_trusted(&self, fingerprint: String, trusted: bool) -> Result<bool, LdError> {
        Ok(self.core.set_trusted(&fingerprint, trusted)?)
    }
    pub fn rename_device(&self, fingerprint: String, name: String) -> Result<bool, LdError> {
        Ok(self.core.rename_device(&fingerprint, &name)?)
    }
    pub fn forget_device(&self, prefix: String) -> Result<u32, LdError> {
        Ok(self.core.forget(&prefix)? as u32)
    }

    pub fn set_visibility(&self, v: FfiVisibility) {
        self.core.set_visibility(match v {
            FfiVisibility::Off => Visibility::Off,
            FfiVisibility::TrustedOnly => Visibility::TrustedOnly,
            FfiVisibility::Everyone => Visibility::Everyone,
        });
    }
}