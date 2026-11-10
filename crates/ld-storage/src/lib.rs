use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const KEEP: i64 = 1000;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: i64,
    pub ts: i64,
    pub direction: String, // "send" | "receive"
    pub name: String,
    pub size: i64,
    pub peer: String,
    pub status: String, // "done" | "rejected" | "failed"
    pub path: Option<String>,
    pub error: Option<String>,
    pub text: Option<String>,
    pub batch: Option<String>
}

pub struct NewRecord {
    pub direction: String,
    pub name: String,
    pub size: u64,
    pub peer: String,
    pub status: String,
    pub path: Option<String>,
    pub error: Option<String>,
    pub text: Option<String>,
    pub batch: Option<String>
}

pub struct History {
    conn: Mutex<Connection>,
}

impl History {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS history (
                 id        INTEGER PRIMARY KEY AUTOINCREMENT,
                 ts        INTEGER NOT NULL,
                 direction TEXT NOT NULL,
                 name      TEXT NOT NULL,
                 size      INTEGER NOT NULL,
                 peer      TEXT NOT NULL,
                 status    TEXT NOT NULL,
                 path      TEXT,
                 error     TEXT,
                 text      TEXT,
                 batch     TEXT
             );",
        )?;
        //let _ = conn.execute("ALTER TABLE history ADD COLUMN batch TEXT", []);
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn add(&self, r: &NewRecord) -> Result<()> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO history (ts, direction, name, size, peer, status, path, error, text, batch)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![ts, r.direction, r.name, r.size as i64, r.peer, r.status, r.path, r.error, r.text, r.batch],
        )?;
        conn.execute(
            "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT ?1)",
            params![KEEP],
        )?;
        Ok(())
    }

    pub fn list(&self, limit: u32) -> Result<Vec<Record>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ts, direction, name, size, peer, status, path, error, text, batch
            FROM history ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(Record {
                id: row.get(0)?,
                ts: row.get(1)?,
                direction: row.get(2)?,
                name: row.get(3)?,
                size: row.get(4)?,
                peer: row.get(5)?,
                status: row.get(6)?,
                path: row.get(7)?,
                error: row.get(8)?,
                text: row.get(9)?,
                batch: row.get(10)?
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM history WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM history", [])?;
        Ok(())
    }
}