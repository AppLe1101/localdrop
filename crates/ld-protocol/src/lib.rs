use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const PROTOCOL_VERSION: u16 = 2;
pub const SERVICE_TYPE: &str = "_localdrop._udp.local.";
pub const MAX_ITEMS: usize = 50_000;
pub const MAX_TEXT: u64 = 4 * 1024 * 1024;
const MAX_MSG: usize = 8 * 1024 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Files,
    Text,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Item {
    pub path: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Message {
    LegacyOffer { version: u16, sender: String, name: String, size: u64 }, // Offer from protocol v1 
    Offer { version: u16, sender: String, kind: Kind, items: Vec<Item>, preview: Option<String> },
    Accept,
    Reject,
    Done,
    Ack { ok: bool },
    ItemEnd { hash: [u8; 32] },
}

pub async fn write_msg<W: AsyncWrite + Unpin>(w: &mut W, msg: &Message) -> Result<()> {
    let bytes = postcard::to_stdvec(msg)?;
    w.write_u32(bytes.len() as u32).await?;
    w.write_all(&bytes).await?;
    Ok(())
}

pub async fn read_msg<R: AsyncRead + Unpin>(r: &mut R) -> Result<Message> {
    let len = r.read_u32().await? as usize;
    if len > MAX_MSG {
        bail!("message too large: {len}");
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(postcard::from_bytes(&buf)?)
}

