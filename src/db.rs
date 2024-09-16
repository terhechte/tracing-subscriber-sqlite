use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use rusqlite::Connection;
use time::OffsetDateTime;
use tracing::Level;

pub const SQL_SCHEMA: &str = include_str!("../schema/log.sql");

pub fn prepare_database(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(SQL_SCHEMA, ()).map(|_| {})
}

#[derive(Debug, Clone)]
// Here we are using Mutex instead of RwLock because Connection did not implement Sync
pub struct LogHandle(pub(crate) Arc<Mutex<Connection>>);

#[derive(Debug)]
pub struct LogEntry<S = String> {
    pub time: OffsetDateTime,
    pub level: Level,
    pub module: Option<S>,
    pub file: Option<S>,
    pub line: Option<u32>,
    pub message: String,
    pub structured: HashMap<S, String>,
}

impl LogHandle {
    pub fn new(connection: Connection) -> Self {
        Self(Arc::new(Mutex::new(connection)))
    }

    pub fn read_logs(&self) -> rusqlite::Result<Vec<LogEntry>> {
        let conn = self.0.lock().unwrap();

        let mut stmt = conn.prepare("SELECT * FROM logs_v0")?;
        let log_iter = stmt.query_map([], |row| {
            Ok(LogEntry {
                time: row.get(0)?,
                level: {
                    let level: String = row.get(1)?;
                    level.parse().unwrap()
                },
                module: row.get(2)?,
                file: row.get(3)?,
                line: row.get(4)?,
                message: row.get(5)?,
                structured: {
                    let structured: String = row.get(6)?;
                    serde_json::from_str(&structured).unwrap()
                },
            })
        })?;

        log_iter.collect()
    }
}

pub trait Connect {
    fn log(&self, entry: LogEntry<&str>);
}

impl Connect for Connection {
    fn log(&self, entry: LogEntry<&str>) {
        self.execute("INSERT INTO logs_v0 (time, level, module, file, line, message, structured) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", 
        (entry.time, entry.level.as_str(), entry.module, entry.file, entry.line, entry.message, serde_json::to_string(&entry.structured).unwrap())).unwrap();
    }
}

impl Connect for Mutex<Connection> {
    fn log(&self, entry: LogEntry<&str>) {
        let conn = self.lock().unwrap();
        conn.log(entry);
    }
}

impl Connect for Arc<Mutex<Connection>> {
    fn log(&self, entry: LogEntry<&str>) {
        self.as_ref().log(entry)
    }
}

impl Connect for LogHandle {
    fn log(&self, entry: LogEntry<&str>) {
        self.0.log(entry)
    }
}
