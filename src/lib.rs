use std::{
    fmt::Write,
    sync::{atomic::AtomicU64, Mutex},
};

use rusqlite::Connection;
use tracing::{field::Visit, level_filters::LevelFilter, span};
#[cfg(feature = "tracing-log")]
use tracing_log::NormalizeEvent;

pub const SQL_SCHEMA: &str = include_str!("../schema/log.sql");

pub fn prepare_database(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(SQL_SCHEMA, ()).map(|_| {})
}

pub struct Subscriber {
    id: AtomicU64,
    connection: Mutex<Connection>,
    max_level: LevelFilter,
}

impl Subscriber {
    pub fn new(connection: Connection) -> Self {
        Self::with_max_level(connection, LevelFilter::TRACE)
    }

    pub fn with_max_level(connection: Connection, max_level: LevelFilter) -> Self {
        Self {
            id: AtomicU64::new(1),
            connection: Mutex::new(connection),
            max_level,
        }
    }
}

impl tracing::Subscriber for Subscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.level() <= &self.max_level
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(self.max_level)
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        let id = self.id.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        span::Id::from_u64(id)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut message = String::new();

        event.record(&mut Visitor {
            message: &mut message,
        });

        #[cfg(feature = "tracing-log")]
        let normalized_meta = event.normalized_metadata();
        #[cfg(feature = "tracing-log")]
        let meta = normalized_meta.as_ref().unwrap_or_else(|| event.metadata());
        #[cfg(not(feature = "tracing-log"))]
        let meta = event.metadata();

        let level = meta.level().as_str();
        let moudle = meta.module_path();
        let file = meta.file();
        let line = meta.line();

        let conn = self.connection.lock().unwrap();
        conn.execute(
            "INSERT INTO logs (level, module, file, line, message) VALUES (?1, ?2, ?3, ?4, ?5)",
            (level, moudle, file, line, message),
        )
        .unwrap();
    }

    fn enter(&self, _span: &span::Id) {}

    fn exit(&self, _span: &span::Id) {}
}

struct Visitor<'a> {
    pub message: &'a mut String,
    // todo: store structured key-value data
}

impl<'a> Visit for Visitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => write!(self.message, "{value:?}").unwrap(),
            _ => {} // todo: store structured key-value data
        }
    }
}
