use anyhow::{anyhow, bail, Context, Result};
use ld_crypto::{fp_short, short_code, Fingerprint, TrustStore};
use ld_protocol::{read_msg, write_msg, Item, Message, MAX_ITEMS, MAX_TEXT};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{broadcast, oneshot},
};

pub use ld_protocol::{Kind, PROTOCOL_VERSION};

const CHUNK: usize = 1024 * 1024;
const PROGRESS_EVERY: Duration = Duration::from_millis(100);
pub const DECISION_TIMEOUT: Duration = Duration::from_secs(60);

// Pub types
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Send,
    Receive,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    Reject,
    AcceptOnce,
    AcceptAndTrust,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Visibility {
    Off,
    TrustedOnly,
    #[default]
    Everyone,
}

pub enum Payload {
    Paths(Vec<PathBuf>),
    Text(String),
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    Incoming {
        id: u64,
        sender: String,
        kind: Kind,
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
    Started { id: u64, direction: Direction, name: String, size: u64, peer_name: String },
    Progress { id: u64, done: u64, total: u64 },
    TextReceived { id: u64, text: String },
    Finished { id: u64, path: Option<String> },
    Rejected { id: u64 },
    Failed { id: u64, error: String },
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendOutcome {
    Sent,
    Rejected,
    NameConflict,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub fingerprint: String,
    pub name: String,
    pub trusted: bool,
}

// Core
pub struct Core {
    my_fp: Fingerprint,
    store: Mutex<TrustStore>,
    events: broadcast::Sender<Event>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Decision>>>,
    next_id: AtomicU64,
    visibility: Mutex<Visibility>,
    download_dir: Mutex<PathBuf>,
}

impl Core {
    pub fn new(my_fp: Fingerprint, store: TrustStore) -> Arc<Self> {
        let (events, _) = broadcast::channel(512);
        Arc::new(Self {
            my_fp,
            store: Mutex::new(store),
            events,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            visibility: Mutex::new(Visibility::Everyone),
            download_dir: Mutex::new(PathBuf::from("downloads")),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    pub fn respond(&self, id: u64, decision: Decision) -> bool {
        self.pending
            .lock()
            .unwrap()
            .remove(&id)
            .map(|tx| tx.send(decision).is_ok())
            .unwrap_or(false)
    }

    pub fn visibility(&self) -> Visibility {
        *self.visibility.lock().unwrap()
    }

    pub fn set_visibility(&self, v: Visibility) {
        *self.visibility.lock().unwrap() = v;
    }

    pub fn download_dir(&self) -> PathBuf {
        self.download_dir.lock().unwrap().clone()
    }

    pub fn set_download_dir(&self, dir: PathBuf) {
        *self.download_dir.lock().unwrap() = dir;
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.store
            .lock()
            .unwrap()
            .list()
            .map(|(fp, d)| DeviceInfo { fingerprint: fp.clone(), name: d.name.clone(), trusted: d.trusted })
            .collect()
    }

    pub fn forget(&self, prefix: &str) -> Result<usize> {
        self.store.lock().unwrap().remove_by_prefix(prefix)
    }

    pub fn set_trusted(&self, key: &str, trusted: bool) -> Result<bool> {
        self.store.lock().unwrap().set_trusted(key, trusted)
    }

    pub fn rename_device(&self, key: &str, name: &str) -> Result<bool> {
        self.store.lock().unwrap().rename(key, name)
    }

    pub async fn send_file(
        &self,
        endpoint: &quinn::Endpoint,
        addrs: &[SocketAddr],
        path: &Path,
        sender_name: &str,
        target_name: Option<&str>,
        allow_conflict: bool,
        id: u64,
    ) -> Result<SendOutcome> {
        self.send(
            endpoint,
            addrs,
            Payload::Paths(vec![path.to_path_buf()]),
            sender_name,
            target_name,
            allow_conflict,
            id,
        )
        .await
    }

    pub async fn send(
        &self,
        endpoint: &quinn::Endpoint,
        addrs: &[SocketAddr],
        payload: Payload,
        sender_name: &str,
        target_name: Option<&str>,
        allow_conflict: bool,
        id: u64,
    ) -> Result<SendOutcome> {
        let r = self
            .send_inner(endpoint, addrs, payload, sender_name, target_name, allow_conflict, id)
            .await;
        if let Err(e) = &r {
            self.emit(Event::Failed { id, error: format!("{e:#}") });
        }
        r
    }

    async fn send_inner(
        &self,
        endpoint: &quinn::Endpoint,
        addrs: &[SocketAddr],
        payload: Payload,
        sender_name: &str,
        target_name: Option<&str>,
        allow_conflict: bool,
        id: u64,
    ) -> Result<SendOutcome> {
        let (kind, sources, text_bytes, preview) = match payload {
            Payload::Paths(paths) => {
                let sources = tokio::task::spawn_blocking(move || collect(&paths)).await??;
                (Kind::Files, sources, None, None)
            }
            Payload::Text(t) => {
                if t.len() as u64 > MAX_TEXT {
                    bail!("текст слишком большой (максимум 4 МБ)");
                }
                let preview: String = t.chars().take(300).collect();
                let bytes = t.into_bytes();
                let item = Item { path: String::new(), size: bytes.len() as u64 };
                (Kind::Text, vec![Source { file: PathBuf::new(), item }], Some(bytes), Some(preview))
            }
        };
        let items: Vec<Item> = sources.iter().map(|s| s.item.clone()).collect();
        let total: u64 = items.iter().map(|i| i.size).sum();
        let title = make_title(kind, &items);

        let conn = connect_any(endpoint, addrs).await?;
        let peer_fp = ld_transport::peer_fingerprint(&conn)
            .context("получатель не предъявил сертификат")?;
        let code = short_code(&self.my_fp, &peer_fp);

        let known = self.store.lock().unwrap().get(&peer_fp).cloned();

        if known.is_none() && !allow_conflict {
            if let Some(n) = target_name {
                let conflict = self.store.lock().unwrap().name_conflict(n, &peer_fp);
                if conflict {
                    conn.close(0u32.into(), b"aborted");
                    return Ok(SendOutcome::NameConflict);
                }
            }
        }

        let trusted = known.as_ref().map_or(false, |d| d.trusted);
        let peer_label = known
            .as_ref()
            .map(|d| d.name.clone())
            .or_else(|| target_name.map(str::to_string))
            .unwrap_or_else(|| addrs.first().map(|a| a.to_string()).unwrap_or_default());

        self.emit(Event::PeerCode { id, code, new_device: known.is_none(), trusted });
        self.emit(Event::Started {
            id,
            direction: Direction::Send,
            name: title,
            size: total,
            peer_name: peer_label.clone(),
        });

        let (mut send, mut recv) = conn.open_bi().await?;
        write_msg(
            &mut send,
            &Message::Offer {
                version: PROTOCOL_VERSION,
                sender: sender_name.to_string(),
                kind,
                items,
                preview,
            },
        )
        .await?;

        match read_msg(&mut recv).await? {
            Message::Accept => {}
            Message::Reject => {
                self.emit(Event::Rejected { id });
                conn.close(0u32.into(), b"rejected");
                return Ok(SendOutcome::Rejected);
            }
            other => bail!("неожиданный ответ: {other:?}"),
        }

        if known.is_none() {
            self.store.lock().unwrap().upsert(&peer_fp, &peer_label, false)?;
        }

        let mut buf = vec![0u8; CHUNK];
        let mut sent = 0u64;
        let mut last = Instant::now();
        for src in &sources {
            let mut hasher = blake3::Hasher::new();
            if let Some(bytes) = &text_bytes {
                hasher.update(bytes);
                send.write_all(bytes).await?;
                sent += bytes.len() as u64;
            } else {
                let mut file = File::open(&src.file)
                    .await
                    .with_context(|| format!("не удалось открыть {}", src.file.display()))?;
                let mut remaining = src.item.size;
                while remaining > 0 {
                    let want = remaining.min(CHUNK as u64) as usize;
                    let n = file.read(&mut buf[..want]).await?;
                    if n == 0 {
                        bail!("файл «{}» изменился во время отправки", src.item.path);
                    }
                    hasher.update(&buf[..n]);
                    send.write_all(&buf[..n]).await?;
                    remaining -= n as u64;
                    sent += n as u64;
                    if last.elapsed() >= PROGRESS_EVERY {
                        last = Instant::now();
                        self.emit(Event::Progress { id, done: sent, total });
                    }
                }
            }
            write_msg(&mut send, &Message::ItemEnd { hash: *hasher.finalize().as_bytes() }).await?;
        }
        self.emit(Event::Progress { id, done: sent, total });

        write_msg(&mut send, &Message::Done).await?;
        send.finish()?;

        match read_msg(&mut recv).await? {
            Message::Ack { ok: true } => {}
            Message::Ack { ok: false } => bail!("получатель сообщил об ошибке при приёме"),
            other => bail!("неожиданный ответ: {other:?}"),
        }
        conn.close(0u32.into(), b"done");
        self.emit(Event::Finished { id, path: None });
        Ok(SendOutcome::Sent)
    }

    pub async fn run_receiver(self: Arc<Self>, endpoint: quinn::Endpoint, dir: PathBuf) -> Result<()> {
        self.set_download_dir(dir);
        tokio::fs::create_dir_all(self.download_dir()).await.ok();
        while let Some(incoming) = endpoint.accept().await {
            let core = self.clone();
            tokio::spawn(async move {
                let id = core.next_id();
                let dir = core.download_dir();
                if let Err(e) = core.handle(id, incoming, &dir).await {
                    core.pending.lock().unwrap().remove(&id);
                    core.emit(Event::Failed { id, error: format!("{e:#}") });
                }
            });
        }
        Ok(())
    }

    async fn handle(&self, id: u64, incoming: quinn::Incoming, dir: &Path) -> Result<()> {
        let conn = incoming.await?;
        let peer_fp = ld_transport::peer_fingerprint(&conn)
            .context("отправитель не предъявил сертификат")?;
        let (mut send, mut recv) = conn.accept_bi().await?;

        let (sender, kind, items, preview) = match read_msg(&mut recv).await? {
            Message::Offer { version, sender, kind, items, preview } => {
                if version != PROTOCOL_VERSION {
                    bail!("несовместимая версия протокола: {version}");
                }
                (sender, kind, items, preview)
            }
            Message::LegacyOffer { sender, .. } => {
                let _ = write_msg(&mut send, &Message::Reject).await;
                let _ = send.finish();
                bail!("«{sender}» использует устаревшую версию LocalDrop: обновите приложение на нём");
            }
            _ => bail!("ожидался Offer"),
        };

        if items.is_empty() || items.len() > MAX_ITEMS {
            bail!("некорректный список файлов");
        }
        let total = items.iter().fold(0u64, |a, i| a.saturating_add(i.size));
        if kind == Kind::Text && (items.len() != 1 || total > MAX_TEXT) {
            bail!("некорректная передача текста");
        }
        let rels: Vec<Vec<String>> = if kind == Kind::Files {
            items
                .iter()
                .map(|i| sanitize(&i.path).ok_or_else(|| anyhow!("некорректный путь в передаче: «{}»", i.path)))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let title = make_title(kind, &items);

        let known = self.store.lock().unwrap().get(&peer_fp).cloned();
        let trusted = known.as_ref().map_or(false, |d| d.trusted);

        let blocked = match self.visibility() {
            Visibility::Off => true,
            Visibility::TrustedOnly => !trusted,
            Visibility::Everyone => false,
        };

        let decision = if blocked {
            Decision::Reject
        } else if trusted {
            Decision::AcceptOnce
        } else {
            let (tx, rx) = oneshot::channel();
            self.pending.lock().unwrap().insert(id, tx);
            self.emit(Event::Incoming {
                id,
                sender: sender.clone(),
                kind,
                name: title.clone(),
                count: items.len() as u32,
                size: total,
                preview: preview.clone(),
                fp_short: fp_short(&peer_fp),
                code: short_code(&self.my_fp, &peer_fp),
                known_name: known.as_ref().map(|d| d.name.clone()),
                timeout_secs: DECISION_TIMEOUT.as_secs(),
            });
            let d = tokio::select! {
                r = tokio::time::timeout(DECISION_TIMEOUT, rx) => match r {
                    Ok(Ok(d)) => d,
                    _ => Decision::Reject,
                },
                _ = conn.closed() => Decision::Reject,
            };
            self.pending.lock().unwrap().remove(&id);
            d
        };

        if decision == Decision::Reject {
            let _ = write_msg(&mut send, &Message::Reject).await;
            let _ = send.finish();
            self.emit(Event::Rejected { id });
            return Ok(());
        }

        if !trusted {
            let mut store = self.store.lock().unwrap();
            store.upsert(&peer_fp, &sender, decision == Decision::AcceptAndTrust)?;
        }
        let peer_name = known.map(|d| d.name).unwrap_or(sender);

        tokio::fs::create_dir_all(dir).await?;

        let mut roots: Vec<(PathBuf, bool)> = Vec::new();
        let mut dests: Vec<PathBuf> = Vec::new();
        if kind == Kind::Files {
            let mut top: HashMap<String, PathBuf> = HashMap::new();
            let mut reserved: HashSet<PathBuf> = HashSet::new();
            for rel in &rels {
                let dest = if rel.len() == 1 {
                    let p = unique_path(dir, &rel[0], &reserved);
                    reserved.insert(p.clone());
                    roots.push((p.clone(), false));
                    p
                } else {
                    let root = top
                        .entry(rel[0].clone())
                        .or_insert_with(|| {
                            let p = unique_path(dir, &rel[0], &reserved);
                            reserved.insert(p.clone());
                            roots.push((p.clone(), true));
                            p
                        })
                        .clone();
                    rel[1..].iter().fold(root, |acc, c| acc.join(c))
                };
                dests.push(dest);
            }
        }

        write_msg(&mut send, &Message::Accept).await?;
        self.emit(Event::Started {
            id,
            direction: Direction::Receive,
            name: title,
            size: total,
            peer_name,
        });

        let mut text_buf: Vec<u8> = Vec::new();
        let res = if kind == Kind::Text {
            self.recv_items(id, &mut recv, &items, &[], total, Some(&mut text_buf)).await
        } else {
            self.recv_items(id, &mut recv, &items, &dests, total, None).await
        };
        let text = match res {
            Ok(()) if kind == Kind::Text => String::from_utf8(text_buf)
                .map(Some)
                .map_err(|_| anyhow!("получен некорректный текст")),
            Ok(()) => Ok(None),
            Err(e) => Err(e),
        };
        let text = match text {
            Ok(t) => t,
            Err(e) => {
                cleanup(&roots).await;
                let _ = write_msg(&mut send, &Message::Ack { ok: false }).await;
                let _ = send.finish();
                return Err(e);
            }
        };

        write_msg(&mut send, &Message::Ack { ok: true }).await?;
        send.finish()?;

        match text {
            Some(t) => {
                self.emit(Event::TextReceived { id, text: t });
                self.emit(Event::Finished { id, path: None });
            }
            None => {
                let path = if roots.len() == 1 { roots[0].0.clone() } else { dir.to_path_buf() };
                self.emit(Event::Finished { id, path: Some(path.display().to_string()) });
            }
        }
        conn.closed().await;
        Ok(())
    }

    async fn recv_items(
        &self,
        id: u64,
        recv: &mut quinn::RecvStream,
        items: &[Item],
        dests: &[PathBuf],
        total: u64,
        mut text: Option<&mut Vec<u8>>,
    ) -> Result<()> {
        let mut buf = vec![0u8; CHUNK];
        let mut done = 0u64;
        let mut last = Instant::now();

        for (idx, item) in items.iter().enumerate() {
            let mut file = if text.is_none() {
                let dest = dests.get(idx).context("внутренняя ошибка: нет пути назначения")?;
                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let f = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(dest)
                    .await
                    .with_context(|| format!("не удалось создать {}", dest.display()))?;
                Some(f)
            } else {
                None
            };

            let mut hasher = blake3::Hasher::new();
            let mut remaining = item.size;
            while remaining > 0 {
                let want = remaining.min(CHUNK as u64) as usize;
                match recv.read(&mut buf[..want]).await? {
                    Some(n) => {
                        if let Some(f) = file.as_mut() {
                            f.write_all(&buf[..n]).await?;
                        }
                        if let Some(t) = text.as_mut() {
                            t.extend_from_slice(&buf[..n]);
                        }
                        hasher.update(&buf[..n]);
                        remaining -= n as u64;
                        done += n as u64;
                        if last.elapsed() >= PROGRESS_EVERY {
                            last = Instant::now();
                            self.emit(Event::Progress { id, done, total });
                        }
                    }
                    None => bail!("поток оборвался раньше времени"),
                }
            }
            if let Some(mut f) = file.take() {
                f.flush().await?;
            }

            let Message::ItemEnd { hash } = read_msg(recv).await? else {
                bail!("ожидался конец файла");
            };
            if hash != *hasher.finalize().as_bytes() {
                bail!("хеш не совпал: «{}»", item.path);
            }
        }
        self.emit(Event::Progress { id, done: total, total });

        let Message::Done = read_msg(recv).await? else {
            bail!("ожидался Done");
        };
        Ok(())
    }
}

// Additional
struct Source {
    file: PathBuf,
    item: Item,
}

fn collect(paths: &[PathBuf]) -> Result<Vec<Source>> {
    let mut out = Vec::new();
    for p in paths {
        let meta = std::fs::metadata(p).with_context(|| format!("не удалось открыть {}", p.display()))?;
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .context("некорректное имя файла")?
            .to_string();
        if meta.is_dir() {
            walk(p, &name, &mut out)?;
        } else if meta.is_file() {
            out.push(Source { file: p.clone(), item: Item { path: name, size: meta.len() } });
        }
        if out.len() > MAX_ITEMS {
            bail!("слишком много файлов (максимум {MAX_ITEMS})");
        }
    }
    if out.is_empty() {
        bail!("нечего отправлять: папка пуста");
    }
    Ok(out)
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<Source>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("не удалось прочитать {}", dir.display()))? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel2 = format!("{rel}/{name}");
        if ft.is_dir() {
            walk(&entry.path(), &rel2, out)?;
        } else if ft.is_file() {
            out.push(Source { file: entry.path(), item: Item { path: rel2, size: entry.metadata()?.len() } });
            if out.len() > MAX_ITEMS {
                bail!("слишком много файлов (максимум {MAX_ITEMS})");
            }
        }
    }
    Ok(())
}

fn plural(n: usize, one: &str, few: &str, many: &str) -> String {
    let (m10, m100) = (n % 10, n % 100);
    let w = if m10 == 1 && m100 != 11 {
        one
    } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
        few
    } else {
        many
    };
    format!("{n} {w}")
}

fn make_title(kind: Kind, items: &[Item]) -> String {
    if kind == Kind::Text {
        return "Текст".into();
    }
    if items.len() == 1 {
        return items[0].path.clone();
    }
    let tops: HashSet<&str> = items.iter().map(|i| i.path.split('/').next().unwrap_or("")).collect();
    let count = plural(items.len(), "файл", "файла", "файлов");
    if tops.len() == 1 && items.iter().all(|i| i.path.contains('/')) {
        format!("{} ({count})", tops.iter().next().unwrap())
    } else {
        count
    }
}

fn sanitize(path: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for raw in path.split('/') {
        let c: String = raw
            .chars()
            .map(|ch| if ch.is_control() || "<>:\"\\|?*".contains(ch) { '_' } else { ch })
            .collect();
        let mut c = c.trim_end_matches(|ch: char| ch == '.' || ch == ' ').to_string();
        if c.is_empty() {
            return None;
        }
        let stem = c.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit());
        if reserved {
            c.insert(0, '_');
        }
        out.push(c);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn unique_path(dir: &Path, name: &str, reserved: &HashSet<PathBuf>) -> PathBuf {
    let free = |p: &PathBuf| !p.exists() && !reserved.contains(p);
    let first = dir.join(name);
    if free(&first) {
        return first;
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = p.extension().and_then(|s| s.to_str());
    for i in 1.. {
        let candidate = match ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        let path = dir.join(candidate);
        if free(&path) {
            return path;
        }
    }
    unreachable!()
}

async fn cleanup(roots: &[(PathBuf, bool)]) {
    for (p, is_dir) in roots {
        if *is_dir {
            let _ = tokio::fs::remove_dir_all(p).await;
        } else {
            let _ = tokio::fs::remove_file(p).await;
        }
    }
}

async fn connect_any(endpoint: &quinn::Endpoint, addrs: &[SocketAddr]) -> Result<quinn::Connection> {
    let mut last = anyhow!("нет адресов для подключения");
    for a in addrs {
        match tokio::time::timeout(Duration::from_secs(3), endpoint.connect(*a, "localdrop")?).await {
            Ok(Ok(c)) => return Ok(c),
            Ok(Err(e)) => last = anyhow!("{a}: {e}"),
            Err(_) => last = anyhow!("{a}: таймаут подключения"),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn rejects_traversal() {
        assert!(sanitize("../x").is_none());
        assert!(sanitize("a/../b").is_none());
        assert!(sanitize("/etc/passwd").is_none());
        assert!(sanitize("a//b").is_none());
        assert!(sanitize("").is_none());
    }

    #[test]
    fn keeps_normal_paths() {
        assert_eq!(sanitize("dir/file.txt"), Some(vec!["dir".into(), "file.txt".into()]));
    }

    #[test]
    fn cleans_names() {
        assert_eq!(sanitize("a:b/c?.txt"), Some(vec!["a_b".into(), "c_.txt".into()]));
        assert_eq!(sanitize("NUL.txt"), Some(vec!["_NUL.txt".into()]));
    }
}