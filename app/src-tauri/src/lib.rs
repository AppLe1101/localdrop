mod settings;

use anyhow::Context;
use ld_core::{Core, Decision, DeviceInfo, Direction, Event, Payload, SendOutcome, Visibility, PROTOCOL_VERSION};
use ld_crypto::{fp_hex, fp_short, Identity, TrustStore};
use ld_discovery::{Discovery, DiscoveryEvent, Peer};
use ld_storage::{History, NewRecord, Record};
use serde::Serialize;
use settings::Settings;
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::WindowEvent;
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tokio::sync::broadcast::error::RecvError;

struct TransferMeta {
    direction: Direction,
    name: String,
    size: u64,
    peer: String,
    text: Option<String>
}

struct AppState {
    core: Arc<Core>,
    client: quinn::Endpoint,
    web: Arc<ld_web::Hub>,
    name: Mutex<String>,
    fp_hex: String,
    fp_short: String,
    port: u16,
    data_dir: PathBuf,
    peers: Arc<Mutex<HashMap<String, Peer>>>,
    discovery: Discovery,
    history: Arc<History>,
    settings: Mutex<Settings>,
    meta: Arc<Mutex<HashMap<u64, TransferMeta>>>,
}

impl AppState {
    fn persist(&self) -> Result<(), String> {
        self.settings
            .lock()
            .unwrap()
            .save(&self.data_dir.join("settings.json"))
            .map_err(|e| e.to_string())
    }

    fn announce(&self) {
        if self.core.visibility() == Visibility::Off {
            self.discovery.unannounce();
            return;
        }
        let name = self.name.lock().unwrap().clone();
        if let Err(e) = self.discovery.announce(&name, self.port, &self.fp_hex) {
            eprintln!("unable to update announce: {e:#}");
        }
    }

    fn info(&self) -> MyInfo {
        MyInfo {
            name: self.name.lock().unwrap().clone(),
            fp_short: self.fp_short.clone(),
            visibility: self.core.visibility(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PeerDto {
    id: String,
    name: String,
    addr: String,
    fp: Option<String>,
}

impl From<&Peer> for PeerDto {
    fn from(p: &Peer) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            addr: p.addrs.first().map(|a| a.to_string()).unwrap_or_default(),
            fp: p.fp.clone(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MyInfo {
    name: String,
    fp_short: String,
    visibility: Visibility,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    device_name: String,
    download_dir: String,
    visibility: Visibility,
    autostart: bool,
    quit_on_close: bool,
    hotkey: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Queued {
    id: u64,
    peer_id: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SendResult {
    id: u64,
    outcome: &'static str,
}

// COMMANDS: GLOBAL
#[tauri::command]
fn my_info(state: State<'_, AppState>) -> MyInfo {
    state.info()
}

#[tauri::command]
fn list_peers(state: State<'_, AppState>) -> Vec<PeerDto> {
    state.peers.lock().unwrap().values().map(PeerDto::from).collect()
}

#[tauri::command]
fn respond_incoming(id: u64, decision: Decision, state: State<'_, AppState>) -> bool {
    state.core.respond(id, decision)
}

async fn run_send(
    app: &AppHandle,
    state: &AppState,
    peer_id: String,
    payload: Payload,
    label: String,
    text: Option<String>,
    allow_conflict: bool,
) -> Result<SendResult, String> {
    let peer = state
        .peers
        .lock()
        .unwrap()
        .get(&peer_id)
        .cloned()
        .ok_or("Devise not available anymore")?;
    if peer.ver != PROTOCOL_VERSION {
        return Err(format!(
            "«{}» using other version of LocalDrop. Update app on both devices.",
            peer.name
        ));
    }
    let my_name = state.name.lock().unwrap().clone();
    let id = state.core.next_id();
    state.meta.lock().unwrap().insert(
        id,
        TransferMeta { direction: Direction::Send, name: label.clone(), size: 0, peer: peer.name.clone(), text },
    );
    let _ = app.emit("send-queued", Queued { id, peer_id, name: label });

    let r = state
        .core
        .send(&state.client, &peer.addrs, payload, &my_name, Some(&peer.name), allow_conflict, id)
        .await;
    let outcome = match r {
        Ok(SendOutcome::Sent) => "sent",
        Ok(SendOutcome::Rejected) => "rejected",
        Ok(SendOutcome::NameConflict) => {
            state.meta.lock().unwrap().remove(&id);
            "nameConflict"
        }
        Err(_) => "failed",
    };
    Ok(SendResult { id, outcome })
}

#[tauri::command]
async fn send_files(
    app: AppHandle,
    state: State<'_, AppState>,
    peer_id: String,
    paths: Vec<String>,
    allow_conflict: bool,
) -> Result<SendResult, String> {
    if paths.is_empty() {
        return Err("Nothing to send".into());
    }
    let label = if paths.len() == 1 {
        Path::new(&paths[0]).file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string()
    } else {
        format!("Items: {}", paths.len())
    };
    let payload = Payload::Paths(paths.into_iter().map(PathBuf::from).collect());
    run_send(&app, &state, peer_id, payload, label, None, allow_conflict).await
}

#[tauri::command]
async fn send_text(
    app: AppHandle,
    state: State<'_, AppState>,
    peer_id: String,
    text: String,
    allow_conflict: bool,
) -> Result<SendResult, String> {
    if text.trim().is_empty() {
        return Err("Text is empty".into());
    }
    run_send(&app, &state, peer_id, Payload::Text(text.clone()), "Text".into(), Some(text), allow_conflict).await
}

// COMMANDS: DEVICES
#[tauri::command]
fn list_devices(state: State<'_, AppState>) -> Vec<DeviceInfo> {
    state.core.devices()
}

#[tauri::command]
fn forget_device(prefix: String, state: State<'_, AppState>) -> Result<usize, String> {
    state.core.forget(&prefix).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_device_trusted(fingerprint: String, trusted: bool, state: State<'_, AppState>) -> Result<bool, String> {
    state.core.set_trusted(&fingerprint, trusted).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_device(fingerprint: String, name: String, state: State<'_, AppState>) -> Result<bool, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name cannot be empty".into());
    }
    state.core.rename_device(&fingerprint, name).map_err(|e| e.to_string())
}

// COMMANDS: HISTORY
#[tauri::command]
fn list_history(limit: Option<u32>, state: State<'_, AppState>) -> Result<Vec<Record>, String> {
    state.history.list(limit.unwrap_or(200)).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_history(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    state.history.delete(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    state.history.clear().map_err(|e| e.to_string())
}

// COMMANDS: SETTINGS
#[tauri::command]
fn get_settings(app: AppHandle, state: State<'_, AppState>) -> SettingsView {
    let (quit_on_close, hotkey) = {
        let s = state.settings.lock().unwrap();
        (s.quit_on_close, current_hotkey(&s))
    };

    SettingsView {
        device_name: state.name.lock().unwrap().clone(),
        download_dir: state.core.download_dir().display().to_string(),
        visibility: state.core.visibility(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        quit_on_close,
        hotkey,
    }
}

#[tauri::command]
fn set_device_name(name: String, state: State<'_, AppState>) -> Result<MyInfo, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Name cannot be empty".into());
    }
    if name.chars().count() > 40 {
        return Err("Name too long (max 40 symbols)".into());
    }
    state.web.set_device_name(&name);
    *state.name.lock().unwrap() = name.clone();
    state.settings.lock().unwrap().device_name = Some(name);
    state.persist()?;
    state.announce();
    
    Ok(state.info())
}

#[tauri::command]
fn set_visibility(app: AppHandle, mode: Visibility, state: State<'_, AppState>) -> Result<MyInfo, String> {
    apply_visibility(&app, mode)?;
    Ok(state.info())
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let m = app.autolaunch();
    if enabled { m.enable() } else { m.disable() }.map_err(|e| e.to_string())
}

#[tauri::command]
fn set_quit_on_close(quit: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.settings.lock().unwrap().quit_on_close = quit;
    state.persist()
}

#[tauri::command]
fn hide_dropzone(app: AppHandle) {
    if let Some(w) = app.get_webview_window("dropzone") {
        let _ = w.hide();
    }
}

#[tauri::command]
async fn web_share(
    state: State<'_, AppState>,
    paths: Vec<String>,
    ttl_minutes: u32,
    max_downloads: Option<u32>,
    pin: Option<String>,
) -> Result<ld_web::SessionView, String> {
    let ttl = Duration::from_secs(ttl_minutes.clamp(1, 1440) as u64 * 60);
    let paths = paths.into_iter().map(PathBuf::from).collect();
    state
        .web
        .create_share(paths, ttl, max_downloads.map(|m| m.clamp(1, 1000)), pin)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn web_receive(
    state: State<'_, AppState>,
    ttl_minutes: u32,
    max_mb: Option<u64>,
    pin: Option<String>
) -> Result<ld_web::SessionView, String> {
    let ttl = Duration::from_secs(ttl_minutes.clamp(1, 1440) as u64 * 60);
    let dir = state.core.download_dir().join("Из браузера");
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    state
        .web
        .create_receive(dir, ttl, max_mb.map(|m| m.saturating_mul(1024 * 1024)), pin)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn web_sessions(state: State<'_, AppState>) -> Vec<ld_web::SessionView> {
    state.web.list()
}

#[tauri::command]
fn web_close(id: String, state: State<'_, AppState>) -> bool {
    state.web.close(&id)
}

#[tauri::command]
fn qr_svg(text: String) -> Result<String, String> {
    ld_web::qr_svg(&text).map_err(|e| e.to_string())
}

static TRAY_OK: AtomicBool = AtomicBool::new(false);
static DZ_PLACED: AtomicBool = AtomicBool::new(false);

struct TrayItems(Vec<(Visibility, CheckMenuItem<tauri::Wry>)>);

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

const DEFAULT_HOTKEY: &str = "CommandOrControl+Alt+D";

fn current_hotkey(s: &Settings) -> String {
    s.dropzone_hotkey.clone().unwrap_or_else(|| DEFAULT_HOTKEY.into())
}

fn register_hotkey(app: &AppHandle, accel: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    gs.unregister_all().map_err(|e| e.to_string())?;
    if accel.is_empty() {
        return Ok(());
    }
    gs.on_shortcut(accel, |app, _, event| {
        if event.state() == ShortcutState::Pressed {
            toggle_dropzone(app);
        }
    })
    .map_err(|e| format!("Unable to set «{accel}»: {e}"))
}

fn toggle_dropzone(app: &AppHandle) {
    let Some(w) = app.get_webview_window("dropzone") else {return};
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }

    if !DZ_PLACED.swap(true, Ordering::Relaxed) {
        if let (Ok(Some(m)), Ok(win)) = (app.primary_monitor(), w.outer_size()) {
            let (s, p) = (m.size(), m.position());
            let x = p.x + s.width as i32 - win.width as i32 - 24;
            let y = p.y + s.height as i32 - win.height as i32 - 100;
            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
        }
    }
    let _ = w.show();
    let _ = w.set_focus();
}

#[tauri::command]
fn set_hotkey(app: AppHandle, accel: String, state: State<'_, AppState>) -> Result<String, String> {
    let accel = accel.trim().to_string();
    let old = current_hotkey(&state.settings.lock().unwrap());
    if let Err(e) = register_hotkey(&app, &accel) {
        let _ = register_hotkey(&app, &old);
        return Err(e);
    }
    state.settings.lock().unwrap().dropzone_hotkey = Some(accel.clone());
    state.persist()?;
    Ok(accel)
}

fn apply_visibility(app: &AppHandle, mode: Visibility) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.core.set_visibility(mode);
    state.settings.lock().unwrap().visibility = mode;
    state.persist()?;
    state.announce();
    sync_tray(app);
    let _ = app.emit("info-changed", ());
    Ok(())
}

fn sync_tray(app: &AppHandle) {
    let (Some(state), Some(items)) = (app.try_state::<AppState>(), app.try_state::<TrayItems>()) else {
        return;
    };
    let cur = state.core.visibility();
    for (mode, item) in &items.0 {
        let _ = item.set_checked(*mode == cur);
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let cur = app.state::<AppState>().core.visibility();

    let show = MenuItem::with_id(app, "show", "Открыть LocalDrop", true, None::<&str>)?;
    let dz = MenuItem::with_id(app, "dropzone", "Drop Zone", true, None::<&str>)?;
    let mut items = Vec::new();
    for (mode, id, label) in [
        (Visibility::Everyone, "vis_everyone", "Все"),
        (Visibility::TrustedOnly, "vis_trusted", "Только доверенные"),
        (Visibility::Off, "vis_off", "Выключено"),
    ] {
        items.push((mode, CheckMenuItem::with_id(app, id, label, true, mode == cur, None::<&str>)?));
    }
    let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = items.iter().map(|(_, i)| i as &dyn IsMenuItem<tauri::Wry>).collect();
    let submenu = Submenu::with_items(app, "Приём файлов", true, &refs)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &dz, &submenu, &sep, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("LocalDrop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "dropzone" => toggle_dropzone(app),
            "vis_everyone" => { let _ = apply_visibility(app, Visibility::Everyone); }
            "vis_trusted" => { let _ = apply_visibility(app, Visibility::TrustedOnly); }
            "vis_off" => { let _ = apply_visibility(app, Visibility::Off); }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, ..} = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(TrayItems(items));
    Ok(())
}

/// Empty row - fallback to default dir
#[tauri::command]
fn set_download_dir(path: String, state: State<'_, AppState>) -> Result<String, String> {
    let path = path.trim().to_string();
    let dir = if path.is_empty() { default_downloads(&state.data_dir) } else { PathBuf::from(&path) };
    std::fs::create_dir_all(&dir).map_err(|e| format!("Unable to use folder: {e}"))?;
    state.core.set_download_dir(dir.clone());
    state.settings.lock().unwrap().download_dir = if path.is_empty() { None } else { Some(path) };
    state.persist()?;
    Ok(dir.display().to_string())
}

// HISTORY
fn record(history: &History, meta: &Mutex<HashMap<u64, TransferMeta>>, app: &AppHandle, e: &Event) {
    let mut m = meta.lock().unwrap();
    let mut entry: Option<NewRecord> = None;
    match e {
        Event::Incoming { id, sender, name, size, known_name, .. } => {
            m.insert(
                *id,
                TransferMeta {
                    direction: Direction::Receive,
                    name: name.clone(),
                    size: *size,
                    peer: known_name.clone().unwrap_or_else(|| sender.clone()),
                    text: None,
                },
            );
        }
        Event::Started { id, direction, name, size, peer_name } => {
            let text = m.get(id).and_then(|t| t.text.clone());
            m.insert(
                *id,
                TransferMeta { direction: *direction, name: name.clone(), size: *size, peer: peer_name.clone(), text },
            );
        }
        Event::TextReceived { id, text } => {
            if let Some(t) = m.get_mut(id) {
                t.text = Some(text.clone());
            }
        }
        Event::Finished { id, path } => {
            if let Some(t) = m.remove(id) {
                entry = Some(make_record(t, "done", path.clone(), None));
            }
        }
        Event::Rejected { id } => {
            if let Some(t) = m.remove(id) {
                entry = Some(make_record(t, "rejected", None, None));
            }
        }
        Event::Failed { id, error } => {
            if let Some(t) = m.remove(id) {
                entry = Some(make_record(t, "failed", None, Some(error.clone())));
            }
        }
        _ => {}
    }
    drop(m);
    if let Some(r) = entry {
        if history.add(&r).is_ok() {
            let _ = app.emit("history-changed", ());
        }
    }
}

fn record_web(history: &History, app: &AppHandle, e: &ld_web::WebEvent) {
    let r = match e {
        ld_web::WebEvent::Downloaded { name, size, ip, .. } => NewRecord {
            direction: "send".into(),
            name: name.clone(),
            size: *size,
            peer: format!("Браузер ({ip})"),
            status: "done".into(),
            path: None,
            error: None,
            text: None,
        },
        ld_web::WebEvent::Uploaded { name, size, ip, path, .. } => NewRecord {
            direction: "receive".into(),
            name: name.clone(),
            size: *size,
            peer: format!("Браузер ({ip})"),
            status: "done".into(),
            path: Some(path.clone()),
            error: None,
            text: None,
        },
        _ => return,
    };
    if history.add(&r).is_ok() {
        let _ = app.emit("history-changed", ());
    }
}

fn make_record(t: TransferMeta, status: &str, path: Option<String>, error: Option<String>) -> NewRecord {
    let direction = match t.direction {
        Direction::Send => "send",
        Direction::Receive => "receive",
    };
    NewRecord {
        direction: direction.into(),
        name: t.name,
        size: t.size,
        peer: t.peer,
        status: status.into(),
        path,
        error,
        text: t.text,
    }
}

// INITIALIZE
fn default_downloads(dd: &Path) -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| dd.join("downloads")).join("LocalDrop")
}

async fn init(app: AppHandle) -> anyhow::Result<AppState> {
    let profile = std::env::var("LOCALDROP_PROFILE").unwrap_or_else(|_| "app".into());
    let dd = dirs::data_dir()
        .context("unable to initialize data directory")?
        .join("localdrop")
        .join(profile);
    std::fs::create_dir_all(&dd)?;

    let settings = Settings::load(&dd.join("settings.json"));
    let name = settings
        .device_name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| {
            std::env::var("LOCALDROP_NAME")
                .ok()
                .or_else(|| hostname::get().ok().and_then(|h| h.into_string().ok()))
                .unwrap_or_else(|| "LocalDrop".into())
        });

    let id = Identity::load_or_create(&dd.join("identity"))?;
    let store = TrustStore::load(dd.join("trusted.json"))?;
    let core = Core::new(id.fingerprint, store);
    core.set_visibility(settings.visibility);
    let my_fp = fp_hex(&id.fingerprint);

    let server = ld_transport::server_endpoint(SocketAddr::from(([0, 0, 0, 0], 53317)), &id).or_else(|_| ld_transport::server_endpoint(SocketAddr::from(([0, 0, 0, 0], 0)), &id))?;
    let port = server.local_addr()?.port();
    let client = ld_transport::client_endpoint(&id)?;

    let downloads = settings
        .download_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_downloads(&dd));
    {
        let core = core.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = core.run_receiver(server, downloads).await {
                eprintln!("receiver stopped: {e:#}");
            }
        });
    }

    let history = Arc::new(History::open(&dd.join("history.db"))?);
    let meta: Arc<Mutex<HashMap<u64, TransferMeta>>> = Arc::default();

    {
        let mut rx = core.subscribe();
        let app = app.clone();
        let history = history.clone();
        let meta = meta.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(e) => {
                        if let Event::Incoming { .. } = &e {
                            show_main(&app);
                        }
                        record(&history, &meta, &app, &e);
                        let _ = app.emit("core-event", &e);
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }

    let web = ld_web::Hub::new(name.clone());
    tauri::async_runtime::spawn(web.clone().run_reaper());
    {
        let mut rx = web.subscribe();
        let app = app.clone();
        let history = history.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(e) => {
                        record_web(&history, &app, &e);
                        let _ = app.emit("web-event", &e);
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }

    // searching devices
    let discovery = Discovery::new()?;
    if settings.visibility != Visibility::Off {
        discovery.announce(&name, port, &my_fp)?;
    }
    let mut rx = discovery.browse()?;
    let peers: Arc<Mutex<HashMap<String, Peer>>> = Arc::default();
    {
        let peers = peers.clone();
        let app = app.clone();
        let my_fp = my_fp.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(ev) = rx.recv().await {
                match ev {
                    DiscoveryEvent::Found(p) => {
                        if p.fp.as_deref() == Some(my_fp.as_str()) {
                            continue; // this device
                        }
                        let dto = PeerDto::from(&p);
                        peers.lock().unwrap().insert(p.id.clone(), p);
                        let _ = app.emit("peer-found", dto);
                    }
                    DiscoveryEvent::Lost(id) => {
                        let lost_at = Instant::now();
                        let peers = peers.clone();
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(1500)).await;
                            let gone = {
                                let mut g = peers.lock().unwrap();
                                let stale = g.get(&id).map(|p| p.seen <= lost_at);
                                if stale == Some(true) {
                                    g.remove(&id);
                                    true
                                } else {
                                    false
                                }
                            };
                            if gone {
                                let _ = app.emit("peer-lost", id);
                            }
                        });
                    }
                }
            }
        });
    }

    Ok(AppState {
        core,
        client,
        web,
        name: Mutex::new(name),
        fp_hex: my_fp,
        fp_short: fp_short(&id.fingerprint),
        port,
        data_dir: dd,
        peers,
        discovery,
        history,
        settings: Mutex::new(settings),
        meta,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    if std::env::var("LOCALDROP_PROFILE").is_err() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)));
    }
    
    builder
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let state = tauri::async_runtime::block_on(init(app.handle().clone()))?;
            app.manage(state);
            match build_tray(app.handle()) {
                Ok(()) => TRAY_OK.store(true, Ordering::Relaxed),
                Err(e) => eprintln!("tray icon creation failed: {e}"),
            }
            let hk = current_hotkey(&app.state::<AppState>().settings.lock().unwrap());
            if let Err(e) = register_hotkey(app.handle(), &hk) {
                eprintln!("hotkey: {e}");
            }
            let minimized = std::env::args().any(|a| a == "--minimized");
            if !(minimized && TRAY_OK.load(Ordering::Relaxed)) {
                show_main(app.handle());
            }
            #[cfg(target_os = "macos")]
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_decorations(true);
                let _ = w.set_title_bar_style(tauri::TitleBarStyle::Overlay);
                let _ = w.set_title("");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "dropzone" {
                    api.prevent_close();
                    let _ = window.hide();
                    return;
                }
                let quit = window
                    .app_handle()
                    .try_state::<AppState>()
                    .map(|s| s.settings.lock().unwrap().quit_on_close)
                    .unwrap_or(false);
                if TRAY_OK.load(Ordering::Relaxed) && !quit {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            my_info,
            list_peers,
            respond_incoming,
            send_files,
            send_text,
            list_devices,
            forget_device,
            set_device_trusted,
            rename_device,
            list_history,
            delete_history,
            clear_history,
            get_settings,
            set_device_name,
            set_visibility,
            set_download_dir,
            hide_dropzone,
            set_autostart,
            set_quit_on_close,
            set_hotkey,
            web_share,
            web_receive,
            web_sessions,
            web_close,
            qr_svg
        ])
        .build(tauri::generate_context!())
        .expect("LocalDrop startup error")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(s) = app.try_state::<AppState>() {
                    s.discovery.shutdown();
                }
            }
        });
}