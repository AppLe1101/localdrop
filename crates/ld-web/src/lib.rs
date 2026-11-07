use anyhow::{anyhow, bail, Result};
use axum::{
    body::{Body, Bytes},
    extract::{ConnectInfo, DefaultBodyLimit, Path as UrlPath, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{Stream, StreamExt};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    future::IntoFuture,
    io,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicUsize, Ordering::Relaxed},
        Arc, Mutex,
    },
    task::{Context, Poll},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{fs::File, io::AsyncWriteExt, net::TcpListener, sync::{broadcast, oneshot}};
use tokio_util::io::ReaderStream;

const PAGE: &str = include_str!("page.html");
const PREFERRED_PORT: u16 = 53318;
const MAX_NAME: usize = 180;

const PIN_MIN: usize = 4;
const PIN_MAX: usize = 8;
const FAILS_PER_LOCK: u32 = 5;
const FAILS_MAX: u32 = 20;

// Public types
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum WebEvent {
    Changed,
    Downloaded { session: String, name: String, size: u64, ip: String },
    Uploaded { session: String, name: String, size: u64, path: String, ip: String },
    Closed { session: String, title: String, reason: String },
    PinLocked { session: String, title: String, ip: String, failures: u32 },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: String,
    pub kind: &'static str, // "share" | "receive"
    pub title: String,
    pub files: u32,
    pub total_size: u64,
    pub expires_at: i64, // unix, мс
    pub max_downloads: Option<u32>,
    pub downloads: u32,
    pub uploads: u32,
    pub received_bytes: u64,
    pub max_bytes: Option<u64>,
    pub port: u16,
    pub hosts: Vec<String>,
    pub pin: Option<String>,
}

#[derive(Serialize)]
struct FileView {
    name: String,
    size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InfoView {
    kind: &'static str,
    device: String,
    files: Vec<FileView>,
    expires_in: u64,
    max_bytes: Option<u64>,
}

// Internal state
struct SharedFile {
    path: PathBuf,
    name: String,
    size: u64,
}

enum Kind {
    Share(Vec<SharedFile>),
    Receive { dir: PathBuf, max_bytes: Option<u64>, received: Arc<AtomicU64> },
}

struct Session {
    kind: Kind,
    title: String,
    deadline: Instant,
    expires_at: i64,
    max_downloads: Option<u32>,
    started: Vec<u32>,
    completed: Vec<u32>,
    uploads: u32,
    closed: Arc<AtomicBool>,
    pin: Option<String>,
    auths: HashSet<String>,
    fails: u32,
    locked_until: Option<Instant>,
}

impl Session {
    fn alive(&self) -> bool {
        !self.closed.load(Relaxed) && Instant::now() < self.deadline
    }
}

enum InfoRes {
    Gone,
    Pin { length: usize, retry_in: u64 },
    Ok(InfoView),
}

struct Server {
    _stop: oneshot::Sender<()>,
}

enum PinRes {
    Gone,
    Locked(u64),
    Wrong(u32),
    Ok(String),
}

fn auth_ok(s: &Session, cookie: Option<&str>) -> bool {
    s.pin.is_none() || cookie.map_or(false, |c| s.auths.contains(c))
}

fn cookie_of(h: &HeaderMap) -> Option<String> {
    h.get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|p| p.trim().strip_prefix("ld=").map(str::to_string))
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn check_pin(pin: Option<String>) -> Result<Option<String>> {
    match pin.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
        None => Ok(None),
        Some(p) => {
            if !(PIN_MIN..=PIN_MAX).contains(&p.len()) || !p.bytes().all(|b| b.is_ascii_digit()) {
                bail!("PIN-code must be 4-8 numbers");
            }
            Ok(Some(p))
        }
    }
}

pub struct Hub {
    sessions: Mutex<HashMap<String, Session>>,
    events: broadcast::Sender<WebEvent>,
    inflight: AtomicUsize,
    port: AtomicU16,
    device: Mutex<String>,
    server: tokio::sync::Mutex<Option<Server>>,
}

fn unix_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn new_token() -> Result<String> {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).map_err(|e| anyhow!("random number generator: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

impl Hub {
    pub fn new(device: String) -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Self {
            sessions: Mutex::new(HashMap::new()),
            events,
            inflight: AtomicUsize::new(0),
            port: AtomicU16::new(0),
            device: Mutex::new(device),
            server: tokio::sync::Mutex::new(None),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WebEvent> {
        self.events.subscribe()
    }

    fn emit(&self, e: WebEvent) {
        let _ = self.events.send(e);
    }

    pub fn set_device_name(&self, name: &str) {
        *self.device.lock().unwrap() = name.to_string();
    }

    // Session creation & control
    pub async fn create_share(
        self: &Arc<Self>,
        paths: Vec<PathBuf>,
        ttl: Duration,
        max_downloads: Option<u32>,
        pin: Option<String>,
    ) -> Result<SessionView> {
        let mut files = Vec::new();
        for p in paths {
            let m = std::fs::metadata(&p).map_err(|e| anyhow!("{}: {e}", p.display()))?;
            if !m.is_file() {
                bail!("«{}» - Not a file. Folders cannot currently be transferred via the link.", p.display());
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
            files.push(SharedFile { path: p, name, size: m.len() });
        }
        if files.is_empty() {
            bail!("No file selected");
        }
        let title = if files.len() == 1 { files[0].name.clone() } else { format!("Files: {}", files.len()) };
        let n = files.len();
        self.insert(Kind::Share(files), title, ttl, max_downloads, n, pin).await
    }

    pub async fn create_receive(
        self: &Arc<Self>,
        dir: PathBuf,
        ttl: Duration,
        max_bytes: Option<u64>,
        pin: Option<String>,
    ) -> Result<SessionView> {
        let kind = Kind::Receive { dir, max_bytes, received: Arc::new(AtomicU64::new(0)) };
        self.insert(kind, "Receiving from a browser".into(), ttl, None, 0, pin).await
    }

    async fn insert(
        self: &Arc<Self>,
        kind: Kind,
        title: String,
        ttl: Duration,
        max_downloads: Option<u32>,
        nfiles: usize,
        pin: Option<String>,
    ) -> Result<SessionView> {
        let mut srv = self.server.lock().await;
        if srv.is_none() {
            *srv = Some(self.start_server().await?);
        }
        let pin = check_pin(pin)?;
        let token = new_token()?;
        let s = Session {
            kind,
            title,
            deadline: Instant::now() + ttl,
            expires_at: unix_ms() + ttl.as_millis() as i64,
            max_downloads,
            started: vec![0; nfiles],
            completed: vec![0; nfiles],
            uploads: 0,
            closed: Arc::new(AtomicBool::new(false)),
            pin,
            auths: HashSet::new(),
            fails: 0,
            locked_until: None,
        };
        let view = self.view(&token, &s);
        self.sessions.lock().unwrap().insert(token, s);
        drop(srv);
        self.emit(WebEvent::Changed);
        Ok(view)
    }

    fn view(&self, id: &str, s: &Session) -> SessionView {
        let (kind, files, total, received, max_bytes): (&'static str, u32, u64, u64, Option<u64>) = match &s.kind {
            Kind::Share(f) => ("share", f.len() as u32, f.iter().map(|x| x.size).sum(), 0, None),
            Kind::Receive { max_bytes, received, .. } => ("receive", 0, 0, received.load(Relaxed), *max_bytes),
        };
        SessionView {
            id: id.to_string(),
            kind,
            title: s.title.clone(),
            files,
            total_size: total,
            expires_at: s.expires_at,
            max_downloads: s.max_downloads,
            downloads: s.completed.iter().sum(),
            uploads: s.uploads,
            received_bytes: received,
            max_bytes,
            port: self.port.load(Relaxed),
            hosts: lan_ips(),
            pin: s.pin.clone()
        }
    }

    pub fn list(&self) -> Vec<SessionView> {
        let g = self.sessions.lock().unwrap();
        let mut v: Vec<SessionView> = g.iter().filter(|(_, s)| s.alive()).map(|(k, s)| self.view(k, s)).collect();
        v.sort_by(|a, b| b.expires_at.cmp(&a.expires_at));
        v
    }

    pub fn close(&self, id: &str) -> bool {
        let removed = self.sessions.lock().unwrap().remove(id);
        match removed {
            Some(s) => {
                s.closed.store(true, Relaxed);
                self.emit(WebEvent::Closed { session: id.to_string(), title: s.title, reason: "manual".into() });
                self.emit(WebEvent::Changed);
                true
            }
            None => false,
        }
    }

    pub async fn run_reaper(self: Arc<Self>) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            let expired: Vec<(String, Session)> = {
                let mut g = self.sessions.lock().unwrap();
                let now = Instant::now();
                let ids: Vec<String> = g.iter().filter(|(_, s)| now >= s.deadline).map(|(k, _)| k.clone()).collect();
                ids.into_iter().filter_map(|k| g.remove(&k).map(|s| (k, s))).collect()
            };
            for (id, s) in &expired {
                self.emit(WebEvent::Closed { session: id.clone(), title: s.title.clone(), reason: "expired".into() });
            }
            if !expired.is_empty() {
                self.emit(WebEvent::Changed);
            }
            let mut srv = self.server.lock().await;
            if srv.is_some() && self.sessions.lock().unwrap().is_empty() && self.inflight.load(Relaxed) == 0 {
                *srv = None;
            }
        }
    }

    async fn start_server(self: &Arc<Self>) -> Result<Server> {
        let listener = match TcpListener::bind(("0.0.0.0", PREFERRED_PORT)).await {
            Ok(l) => l,
            Err(_) => TcpListener::bind(("0.0.0.0", 0)).await?,
        };
        self.port.store(listener.local_addr()?.port(), Relaxed);
        let app = router(self.clone());
        let (stop, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let serve = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).into_future();
            tokio::select! {
                _ = serve => {}
                _ = rx => {}
            }
        });
        Ok(Server { _stop: stop })
    }

    // Request handlers
    fn is_alive(&self, token: &str) -> bool {
        self.sessions.lock().unwrap().get(token).map_or(false, |s| s.alive())
    }

    fn info(&self, token: &str, cookie: Option<&str>) -> InfoRes {
        let g = self.sessions.lock().unwrap();
        let Some(s) = g.get(token) else { return InfoRes::Gone };
        if !s.alive() {
            return InfoRes::Gone;
        }
        if !auth_ok(s, cookie) {
            let retry_in = s
                .locked_until
                .map(|t| t.saturating_duration_since(Instant::now()).as_secs_f64().ceil() as u64)
                .unwrap_or(0);
            return InfoRes::Pin { length: s.pin.as_ref().map_or(0, |p| p.len()), retry_in };
        }
        let (kind, files, max_bytes) = match &s.kind {
            Kind::Share(f) => (
                "share",
                f.iter().map(|x| FileView { name: x.name.clone(), size: x.size }).collect::<Vec<_>>(),
                None,
            ),
            Kind::Receive { max_bytes, .. } => ("receive", Vec::new(), *max_bytes),
        };
        InfoRes::Ok(InfoView {
            kind,
            device: self.device.lock().unwrap().clone(),
            files,
            expires_in: s.deadline.saturating_duration_since(Instant::now()).as_secs(),
            max_bytes,
        })
    }

    fn try_pin(&self, token: &str, attempt: &str, ip: &str) -> PinRes {
        let mut events: Vec<WebEvent> = Vec::new();
        let res = {
            let mut g = self.sessions.lock().unwrap();
            let Some(s) = g.get_mut(token) else { return PinRes::Gone };
            if !s.alive() {
                return PinRes::Gone;
            }
            let Some(pin) = s.pin.clone() else { return PinRes::Gone };
            let now = Instant::now();
            if let Some(t) = s.locked_until {
                if now < t {
                    return PinRes::Locked((t - now).as_secs_f64().ceil() as u64);
                }
            }
            if ct_eq(pin.as_bytes(), attempt.as_bytes()) {
                s.fails = 0;
                s.locked_until = None;
                let Ok(auth) = new_token() else { return PinRes::Gone };
                if s.auths.len() >= 100 {
                    s.auths.clear();
                }
                s.auths.insert(auth.clone());
                PinRes::Ok(auth)
            } else {
                s.fails += 1;
                let fails = s.fails;
                if fails >= FAILS_MAX {
                    let title = s.title.clone();
                    s.closed.store(true, Relaxed);
                    g.remove(token);
                    events.push(WebEvent::Closed { session: token.to_string(), title, reason: "attempts".into() });
                    events.push(WebEvent::Changed);
                    PinRes::Gone
                } else if fails % FAILS_PER_LOCK == 0 {
                    let secs = 30u64 << (fails / FAILS_PER_LOCK - 1);
                    s.locked_until = Some(now + Duration::from_secs(secs));
                    events.push(WebEvent::PinLocked {
                        session: token.to_string(),
                        title: s.title.clone(),
                        ip: ip.to_string(),
                        failures: fails,
                    });
                    PinRes::Locked(secs)
                } else {
                    PinRes::Wrong(FAILS_PER_LOCK - fails % FAILS_PER_LOCK)
                }
            }
        };
        for e in events {
            self.emit(e);
        }
        res
    }

    fn begin_download(&self, token: &str, idx: usize, cookie: Option<&str>) -> Result<(PathBuf, String, Arc<AtomicBool>), StatusCode> {
        let mut g = self.sessions.lock().unwrap();
        let s = g.get_mut(token).ok_or(StatusCode::NOT_FOUND)?;
        if !s.alive() {
            return Err(StatusCode::NOT_FOUND);
        }
        if !auth_ok(s, cookie) { return Err(StatusCode::UNAUTHORIZED); }
        let (path, name) = match &s.kind {
            Kind::Share(files) => {
                let f = files.get(idx).ok_or(StatusCode::NOT_FOUND)?;
                (f.path.clone(), f.name.clone())
            }
            _ => return Err(StatusCode::NOT_FOUND),
        };
        if let Some(max) = s.max_downloads {
            if s.started[idx] >= max {
                return Err(StatusCode::GONE);
            }
        }
        s.started[idx] += 1;
        Ok((path, name, s.closed.clone()))
    }

    fn abort_download(&self, token: &str, idx: usize) {
        if let Some(s) = self.sessions.lock().unwrap().get_mut(token) {
            s.started[idx] = s.started[idx].saturating_sub(1);
        }
    }

    fn end_download(&self, token: &str, idx: usize, finished: bool, name: &str, size: u64, ip: &str) {
        let mut limit_title: Option<String> = None;
        {
            let mut g = self.sessions.lock().unwrap();
            if let Some(s) = g.get_mut(token) {
                if finished {
                    s.completed[idx] += 1;
                    if let Some(max) = s.max_downloads {
                        if s.completed.iter().all(|c| *c >= max) {
                            limit_title = Some(s.title.clone());
                        }
                    }
                } else {
                    s.started[idx] = s.started[idx].saturating_sub(1);
                }
            }
            if limit_title.is_some() {
                g.remove(token);
            }
        }
        if finished {
            self.emit(WebEvent::Downloaded {
                session: token.to_string(),
                name: name.to_string(),
                size,
                ip: ip.to_string(),
            });
        }
        if let Some(title) = limit_title {
            self.emit(WebEvent::Closed { session: token.to_string(), title, reason: "limit".into() });
        }
        self.emit(WebEvent::Changed);
    }

    fn begin_upload(&self, token: &str, cookie: Option<&str>) -> Result<UploadTicket, StatusCode> {
        let g = self.sessions.lock().unwrap();
        let s = g.get(token).ok_or(StatusCode::NOT_FOUND)?;
        if !s.alive() {
            return Err(StatusCode::NOT_FOUND);
        }
        if !auth_ok(s, cookie) { return Err(StatusCode::UNAUTHORIZED); }
        match &s.kind {
            Kind::Receive { dir, max_bytes, received } => Ok(UploadTicket {
                dir: dir.clone(),
                max_bytes: *max_bytes,
                received: received.clone(),
                closed: s.closed.clone(),
            }),
            _ => Err(StatusCode::NOT_FOUND),
        }
    }

    fn finish_upload(&self, token: &str) {
        if let Some(s) = self.sessions.lock().unwrap().get_mut(token) {
            s.uploads += 1;
        }
        self.emit(WebEvent::Changed);
    }
}

struct UploadTicket {
    dir: PathBuf,
    max_bytes: Option<u64>,
    received: Arc<AtomicU64>,
    closed: Arc<AtomicBool>,
}

struct Busy(Arc<Hub>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.inflight.fetch_sub(1, Relaxed);
    }
}

// HTTP
fn router(hub: Arc<Hub>) -> Router {
    Router::new()
        .route("/s/:token", get(page))
        .route("/s/:token/info", get(info))
        .route("/s/:token/f/:idx", get(download))
        .route("/s/:token/u", post(upload).layer(DefaultBodyLimit::disable()))
        .route("/s/:token/pin", post(pin_submit).layer(DefaultBodyLimit::max(64)))
        .layer(axum::middleware::map_response(harden))
        .with_state(hub)
}

async fn harden(mut res: Response) -> Response {
    let h = res.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("cache-control", HeaderValue::from_static("no-store"));
    res
}

async fn page(State(hub): State<Arc<Hub>>, UrlPath(token): UrlPath<String>) -> Response {
    if !hub.is_alive(&token) {
        return (StatusCode::NOT_FOUND, "The link is invalid or expired").into_response();
    }
    (
        [(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; img-src data:; form-action 'none'; base-uri 'none'",
        )],
        Html(PAGE),
    )
        .into_response()
}

async fn info(State(hub): State<Arc<Hub>>, UrlPath(token): UrlPath<String>, headers: HeaderMap) -> Response {
    let cookie = cookie_of(&headers);
    match hub.info(&token, cookie.as_deref()) {
        InfoRes::Ok(v) => Json(v).into_response(),
        InfoRes::Pin { length, retry_in } => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "pin": true, "length": length, "retryIn": retry_in })),
        )
            .into_response(),
        InfoRes::Gone => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn pin_submit(
    State(hub): State<Arc<Hub>>,
    UrlPath(token): UrlPath<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: String,
) -> Response {
    match hub.try_pin(&token, body.trim(), &addr.ip().to_string()) {
        PinRes::Ok(auth) => {
            let c = format!("ld={auth}; Path=/s/{token}; HttpOnly; SameSite=Strict; Max-Age=86400");
            match HeaderValue::from_str(&c) {
                Ok(v) => ([(header::SET_COOKIE, v)], "ok").into_response(),
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
        PinRes::Wrong(left) => {
            (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "left": left }))).into_response()
        }
        PinRes::Locked(secs) => {
            (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({ "retryIn": secs }))).into_response()
        }
        PinRes::Gone => StatusCode::NOT_FOUND.into_response(),
    }
}

struct DlGuard {
    hub: Arc<Hub>,
    token: String,
    idx: usize,
    name: String,
    size: u64,
    ip: String,
    finished: bool,
}

impl Drop for DlGuard {
    fn drop(&mut self) {
        self.hub.end_download(&self.token, self.idx, self.finished, &self.name, self.size, &self.ip);
        self.hub.inflight.fetch_sub(1, Relaxed);
    }
}

struct Tracked {
    inner: ReaderStream<File>,
    closed: Arc<AtomicBool>,
    guard: DlGuard,
}

impl Stream for Tracked {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.closed.load(Relaxed) {
            return Poll::Ready(Some(Err(io::Error::new(io::ErrorKind::Other, "ссылка закрыта"))));
        }
        let p = Pin::new(&mut self.inner).poll_next(cx);
        if let Poll::Ready(None) = &p {
            self.guard.finished = true;
        }
        p
    }
}

async fn download(
    State(hub): State<Arc<Hub>>,
    UrlPath((token, idx)): UrlPath<(String, usize)>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let cookie = cookie_of(&headers);
    let (path, name, closed) = match hub.begin_download(&token, idx, cookie.as_deref()) {
        Ok(v) => v,
        Err(c) => return c.into_response(),
    };
    let file = match File::open(&path).await {
        Ok(f) => f,
        Err(_) => {
            hub.abort_download(&token, idx);
            return (StatusCode::NOT_FOUND, "File unavailable").into_response();
        }
    };
    let size = match file.metadata().await {
        Ok(m) => m.len(),
        Err(_) => {
            hub.abort_download(&token, idx);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    hub.inflight.fetch_add(1, Relaxed);
    let stream = Tracked {
        inner: ReaderStream::with_capacity(file, 256 * 1024),
        closed,
        guard: DlGuard {
            hub: hub.clone(),
            token,
            idx,
            name: name.clone(),
            size,
            ip: addr.ip().to_string(),
            finished: false,
        },
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, size)
        .header(header::CONTENT_DISPOSITION, disposition(&name))
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn upload(
    State(hub): State<Arc<Hub>>,
    UrlPath(token): UrlPath<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let raw = headers.get("x-file-name").and_then(|v| v.to_str().ok()).unwrap_or("file");
    let name = safe_name(&pct_decode(raw));
    let t = match hub.begin_upload(&token, cookie_of(&headers).as_deref()) {
        Ok(t) => t,
        Err(c) => return c.into_response(),
    };
    hub.inflight.fetch_add(1, Relaxed);
    let _busy = Busy(hub.clone());

    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if let (Some(max), Some(len)) = (t.max_bytes, declared) {
        if t.received.load(Relaxed).saturating_add(len) > max {
            return (StatusCode::PAYLOAD_TOO_LARGE, "Size limit exceeded").into_response();
        }
    }

    let (mut file, path) = match create_unique(&t.dir, &name).await {
        Ok(v) => v,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create file").into_response(),
    };

    let mut written = 0u64;
    let mut stream = body.into_data_stream();
    let result: Result<(), (StatusCode, &'static str)> = loop {
        match stream.next().await {
            None => break Ok(()),
            Some(Err(_)) => break Err((StatusCode::BAD_REQUEST, "Transfer interrupted")),
            Some(Ok(chunk)) => {
                if t.closed.load(Relaxed) {
                    break Err((StatusCode::GONE, "Link closed"));
                }
                let n = chunk.len() as u64;
                let total = t.received.fetch_add(n, Relaxed) + n;
                written += n;
                if let Some(max) = t.max_bytes {
                    if total > max {
                        break Err((StatusCode::PAYLOAD_TOO_LARGE, "Size limit exceeded"));
                    }
                }
                if file.write_all(&chunk).await.is_err() {
                    break Err((StatusCode::INTERNAL_SERVER_ERROR, "Disk write error"));
                }
            }
        }
    };

    match result {
        Ok(()) => {
            let _ = file.flush().await;
            drop(file);
            hub.finish_upload(&token);
            hub.emit(WebEvent::Uploaded {
                session: token,
                name: path.file_name().and_then(|n| n.to_str()).unwrap_or(&name).to_string(),
                size: written,
                path: path.display().to_string(),
                ip: addr.ip().to_string(),
            });
            (StatusCode::OK, "ok").into_response()
        }
        Err((code, msg)) => {
            drop(file);
            let _ = tokio::fs::remove_file(&path).await;
            t.received.fetch_sub(written, Relaxed);
            (code, msg).into_response()
        }
    }
}

// Additional
async fn create_unique(dir: &Path, name: &str) -> io::Result<(File, PathBuf)> {
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = p.extension().and_then(|s| s.to_str());
    for i in 0..10_000u32 {
        let cand = match (i, ext) {
            (0, _) => name.to_string(),
            (_, Some(e)) => format!("{stem} ({i}).{e}"),
            (_, None) => format!("{stem} ({i})"),
        };
        let path = dir.join(&cand);
        match tokio::fs::OpenOptions::new().write(true).create_new(true).open(&path).await {
            Ok(f) => return Ok((f, path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::Other, "could not determine a filename"))
}

fn safe_name(raw: &str) -> String {
    let base = raw.rsplit(|c| c == '/' || c == '\\').next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if "<>:\"|?*".contains(c) { '_' } else { c })
        .collect();
    let mut s = cleaned.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if s.chars().count() > MAX_NAME {
        s = s.chars().take(MAX_NAME).collect();
    }
    if s.is_empty() {
        return "file".into();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        s.insert(0, '_');
    }
    s
}

fn pct(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut o = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                o.push(v);
                i += 3;
                continue;
            }
        }
        o.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&o).into_owned()
}

fn disposition(name: &str) -> HeaderValue {
    let ascii: String = name
        .chars()
        .map(|c| if (c.is_ascii_graphic() || c == ' ') && c != '"' && c != '\\' { c } else { '_' })
        .collect();
    HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{}", pct(name)))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

pub fn lan_ips() -> Vec<String> {
    let mut v: Vec<(u8, String)> = Vec::new();
    if let Ok(list) = local_ip_address::list_afinet_netifas() {
        for (name, ip) in list {
            let IpAddr::V4(a) = ip else { continue };
            if a.is_loopback() || a.is_link_local() || a.is_unspecified() {
                continue;
            }
            let n = name.to_lowercase();
            let virt = ["vethernet", "wsl", "docker", "virtual", "vmware", "vbox", "hyper-v", "vpn", "tailscale", "zerotier", "veth", "br-"]
                .iter()
                .any(|k| n.contains(k));
            let rank = match (a.is_private(), virt) {
                (true, false) => 0,
                (false, false) => 1,
                (true, true) => 2,
                (false, true) => 3,
            };
            v.push((rank, a.to_string()));
        }
    }
    v.sort();
    v.dedup_by(|a, b| a.1 == b.1);
    v.into_iter().map(|(_, ip)| ip).collect()
}

pub fn qr_svg(text: &str) -> Result<String> {
    use qrcode::{render::svg, QrCode};
    let code = QrCode::new(text.as_bytes())?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(220, 220)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .quiet_zone(true)
        .build())
}