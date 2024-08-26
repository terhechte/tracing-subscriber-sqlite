use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use rusqlite::Connection;
use time::OffsetDateTime;

pub const SQL_SCHEMA: &str = include_str!("../schema/log.sql");

pub fn prepare_database(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(SQL_SCHEMA, ()).map(|_| {})
}

#[derive(Debug, Clone)]
// Here we are using Mutex instead of RwLock because Connection did not implement Sync
pub struct LogHandle(pub(crate) Arc<Mutex<Connection>>);

#[derive(Debug)]
pub struct LogEntry {
    pub time: String,
    pub level: String,
    pub module: Option<String>,
    pub file: Option<String>,
    pub line: Option<i32>,
    pub message: String,
    pub structured: String,
}

impl LogHandle {
    pub fn new(connection: Connection) -> Self {
        Self(Arc::new(Mutex::new(connection)))
    }

    pub fn read_logs_v0(&self) -> rusqlite::Result<Vec<LogEntry>> {
        let conn = self.0.lock().unwrap();

        let mut stmt = conn.prepare("SELECT * FROM logs_v0")?;
        let log_iter = stmt.query_map([], |row| {
            Ok(LogEntry {
                time: row.get(0)?,
                level: row.get(1)?,
                module: row.get(2)?,
                file: row.get(3)?,
                line: row.get(4)?,
                message: row.get(5)?,
                structured: row.get(6)?,
            })
        })?;

        log_iter.collect()
    }

    pub fn log_v0(
        &self,
        level: &str,
        module: Option<&str>,
        file: Option<&str>,
        line: Option<u32>,
        message: &str,
        kvs: HashMap<&str, String>,
    ) {
        let conn = self.0.lock().unwrap();
        let now = OffsetDateTime::now_utc();
        conn.execute(
                "INSERT INTO logs_v0 (time, level, module, file, line, message, structured) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (now, level, module, file, line, message, serde_json::to_string(&kvs).unwrap()),
            ).unwrap();
    }
}
