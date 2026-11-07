use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use ld_core::{Core, Decision, Direction, Event, SendOutcome};
use ld_crypto::{fp_hex, fp_short, Identity, TrustStore};
use ld_discovery::{Discovery, DiscoveryEvent, Peer};
use std::{
    io::{self, Write},
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::sync::broadcast::error::RecvError;

#[derive(Parser)]
#[command(name = "ld-cli", about = "LocalDrop: console MVP")]
struct Cli {
    #[arg(long, global = true)]
    profile: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Listen {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "downloads")]
        dir: PathBuf,
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    List {
        #[arg(long, default_value_t = 5)]
        timeout: u64,
    },
    Send {
        target: String,
        file: PathBuf,
        #[arg(long, default_value = "LocalDrop CLI")]
        name: String,
    },
    Devices,
    Forget { prefix: String },
}

fn data_dir(profile: &str) -> Result<PathBuf> {
    let safe: String = profile
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let base = dirs::data_dir().context("unable to found data directory")?;
    Ok(base.join("localdrop").join(safe))
}

fn make_core(dir: &PathBuf) -> Result<(Identity, Arc<Core>)> {
    let id = Identity::load_or_create(&dir.join("identity"))?;
    let store = TrustStore::load(dir.join("trusted.json"))?;
    let core = Core::new(id.fingerprint, store);
    Ok((id, core))
}

async fn ask(prompt: String) -> String {
    tokio::task::spawn_blocking(move || {
        print!("{prompt}");
        io::stdout().flush().ok();
        let mut s = String::new();
        io::stdin().read_line(&mut s).ok();
        s.trim().to_lowercase()
    })
    .await
    .unwrap_or_default()
}

fn human(n: u64) -> String {
    let mut v = n as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if v < 1024.0 {
            return format!("{v:.1} {unit}");
        }
        v /= 1024.0;
    }
    format!("{v:.1} TB")
}
fn spawn_console(core: Arc<Core>) {
    let mut rx = core.subscribe();
    tokio::spawn(async move {
        loop {
            let ev = match rx.recv().await {
                Ok(e) => e,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            match ev {
                Event::Incoming { id, sender, name, size, fp_short, code, known_name, .. } => {
                    let core = core.clone();
                    tokio::spawn(async move {
                        let who = match known_name {
                            Some(n) => format!("known device «{n}»"),
                            None => "NEW device".to_string(),
                        };
                        println!(
                            "\nIncoming file: {name}, {}\n  from: «{sender}», {who}\n  fingerprint: {fp_short}\n  confirmation code: {code}",
                            human(size)
                        );
                        let a = ask("Accept? [y] once, [t] accept & true, [N] reject: ".into()).await;
                        let d = match a.as_str() {
                            "t" | "trust" | "т" | "доверять" => Decision::AcceptAndTrust,
                            "y" | "yes" | "д" | "да" => Decision::AcceptOnce,
                            _ => Decision::Reject,
                        };
                        core.respond(id, d);
                    });
                },
                Event::TextReceived { text, .. } => println!("\nПолучен текст:\n{text}"),
                Event::PeerCode { code, trusted, .. } => {
                    if trusted {
                        println!("Sender - trusted device.");
                    } else {
                        println!("New device. Confirmation code: {code}\nCompare it with recipient.");
                    }
                },
                Event::Started { direction, name, size, peer_name, .. } => match direction {
                    Direction::Send => println!("Sending «{name}» ({}) to {peer_name}", human(size)),
                    Direction::Receive => println!("Accepting «{name}» ({}) from {peer_name}", human(size)),
                },
                Event::Progress { done, total, .. } => {
                    let pct = if total > 0 { done * 100 / total } else { 100 };
                    print!("\r  {pct}% ({} / {})   ", human(done), human(total));
                    io::stdout().flush().ok();
                }
                Event::Finished { path, .. } => match path {
                    Some(p) => println!("\nSaved: {p}"),
                    None => println!("\nReady."),
                },
                Event::Rejected { .. } => println!("Transfer rejected."),
                Event::Failed { error, .. } => eprintln!("\nTransfer error: {error}"),
            }
        }
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let profile = cli.profile.clone().unwrap_or_else(|| match &cli.cmd {
        Cmd::Listen { name, .. } => name.clone(),
        _ => "default".to_string(),
    });
    let dd = data_dir(&profile)?;

    match cli.cmd {
        Cmd::Listen { name, dir, port } => {
            let (id, core) = make_core(&dd)?;
            let endpoint = ld_transport::server_endpoint(SocketAddr::from(([0, 0, 0, 0], port)), &id)?;
            let port = endpoint.local_addr()?.port();
            let disc = Discovery::new()?;
            disc.announce(&name, port, &fp_hex(&id.fingerprint))?;
            println!("«{name}» listening on {port} port, files saving to {}", dir.display());
            println!("Device fingerprint: {}", fp_short(&id.fingerprint));
            println!("Profile data: {}", dd.display());
            println!("Exit: Ctrl+C.");
            spawn_console(core.clone());
            tokio::select! {
                r = core.run_receiver(endpoint, dir) => r?,
                _ = tokio::signal::ctrl_c() => {}
            }
            disc.shutdown();
            println!("Stopped");
        }

        Cmd::List { timeout } => {
            let disc = Discovery::new()?;
            let mut rx = disc.browse()?;
            println!("Searching devices ({timeout} sec)...");
            let _ = tokio::time::timeout(Duration::from_secs(timeout), async {
                while let Some(ev) = rx.recv().await {
                    if let DiscoveryEvent::Found(p) = ev {
                        let fp = p.fp.as_deref().map(|f| &f[..16.min(f.len())]).unwrap_or("?");
                        let addrs: Vec<String> = p.addrs.iter().map(|a| a.to_string()).collect();
                        println!("  {}  -  {}  [{}]", p.name, addrs.join(", "), fp);
                    }
                }
            })
            .await;
        }

        Cmd::Send { target, file, name } => {
            let (id, core) = make_core(&dd)?;
            let (addrs, target_name) = match target.parse::<SocketAddr>() {
                Ok(a) => (vec![a], None),
                Err(_) => {
                    let p = find_peer(&target, 10).await?;
                    (p.addrs, Some(p.name))
                }
            };
            let endpoint = ld_transport::client_endpoint(&id)?;
            spawn_console(core.clone());

            let mut outcome = core
                .send_file(&endpoint, &addrs, &file, &name, target_name.as_deref(), false, core.next_id())
                .await?;

            if outcome == SendOutcome::NameConflict {
                println!(
                    "ATTENTION: device with name «{}» has different fingerprint.\n",
                    target_name.as_deref().unwrap_or("")
                );
                let a = ask("Continue anyway? [y/N]: ".into()).await;
                if matches!(a.as_str(), "y" | "yes" | "д" | "да") {
                    outcome = core
                        .send_file(&endpoint, &addrs, &file, &name, target_name.as_deref(), true, core.next_id())
                        .await?;
                } else {
                    println!("Rejected.");
                }
            }
            let _ = outcome;
            endpoint.wait_idle().await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        Cmd::Devices => {
            let store = TrustStore::load(dd.join("trusted.json"))?;
            let mut any = false;
            for (fp, d) in store.list() {
                any = true;
                println!(
                    "{}…  {:<20} {}",
                    &fp[..16],
                    d.name,
                    if d.trusted { "trusted" } else { "known" }
                );
            }
            if !any {
                println!("List is empty.");
            }
        }

        Cmd::Forget { prefix } => {
            let mut store = TrustStore::load(dd.join("trusted.json"))?;
            let n = store.remove_by_prefix(&prefix)?;
            println!("Device deleted: {n}");
        }
    }
    Ok(())
}

async fn find_peer(name: &str, timeout: u64) -> Result<Peer> {
    let disc = Discovery::new()?;
    let mut rx = disc.browse()?;
    let found = tokio::time::timeout(Duration::from_secs(timeout), async {
        while let Some(ev) = rx.recv().await {
            if let DiscoveryEvent::Found(p) = ev {
                if p.name == name {
                    return Some(p);
                }
            }
        }
        None
    })
    .await;
    match found {
        Ok(Some(p)) => Ok(p),
        _ => bail!("device «{name}» not found in {timeout} с"),
    }
}