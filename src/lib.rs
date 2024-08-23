use std::{
    collections::HashMap,
    fmt::Write,
    sync::{atomic::AtomicU64, Mutex},
};

use rusqlite::Connection;
use time::OffsetDateTime;
use tracing::{field::Visit, level_filters::LevelFilter, span};
#[cfg(feature = "tracing-log")]
use tracing_log::NormalizeEvent;

pub const SQL_SCHEMA: &str = include_str!("../schema/log.sql");

pub fn prepare_database(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(SQL_SCHEMA, ()).map(|_| {})
}

/// A `Layer` to write events to a sqlite database.
/// This type can be composed with other `Subscriber`s and `Layer`s.
#[derive(Debug)]
pub struct Layer {
    connection: Mutex<Connection>,
    max_level: LevelFilter,
    black_list: Option<Box<[&'static str]>>,
    white_list: Option<Box<[&'static str]>>,
}

impl Layer {
    pub fn black_list(&self) -> Option<&[&'static str]> {
        self.black_list.as_deref()
    }

    pub fn white_list(&self) -> Option<&[&'static str]> {
        self.white_list.as_deref()
    }

    pub fn max_level(&self) -> &LevelFilter {
        &self.max_level
    }

    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.level() <= self.max_level()
            && metadata.module_path().map_or(true, |m| {
                let starts_with = |module: &&str| m.starts_with(module);
                let has_module = |modules: &[&str]| modules.iter().any(starts_with);
                self.white_list().map_or(true, has_module)
                    && !(self.black_list().map_or(false, has_module))
            })
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(self.max_level)
    }

    fn on_event(&self, event: &tracing::Event<'_>) {
        #[cfg(feature = "tracing-log")]
        let normalized_meta = event.normalized_metadata();
        #[cfg(feature = "tracing-log")]
        let meta = match normalized_meta.as_ref() {
            Some(meta) if self.enabled(meta) => meta,
            None => event.metadata(),
            _ => return,
        };

        #[cfg(not(feature = "tracing-log"))]
        let meta = event.metadata();

        let level = meta.level().as_str();
        let moudle = meta.module_path();
        let file = meta.file();
        let line = meta.line();

        let mut message = String::new();
        let mut kvs = HashMap::new();

        event.record(&mut Visitor {
            message: &mut message,
            kvs: &mut kvs,
        });

        let conn = self.connection.lock().unwrap();
        let now = OffsetDateTime::now_utc();
        conn.execute(
            "INSERT INTO logs_v0 (time, level, module, file, line, message, structured) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (now, level, moudle, file, line, message, serde_json::to_string(&kvs).unwrap()),
        )
        .unwrap();
    }

    pub fn to_subscriber(self) -> Subscriber {
        Subscriber::with_layer(self)
    }
}

#[cfg(feature = "layer")]
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Layer {
    fn enabled(
        &self,
        metadata: &tracing::Metadata<'_>,
        _: tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        self.enabled(metadata)
    }

    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        self.on_event(event)
    }
}

/// A simple `Subscriber` that wraps `Layer`[crate::Layer].
#[derive(Debug)]
pub struct Subscriber {
    id: AtomicU64,
    layer: Layer,
}

impl Subscriber {
    pub fn new(connection: Connection) -> Self {
        Self::with_max_level(connection, LevelFilter::TRACE)
    }

    fn with_layer(layer: Layer) -> Self {
        Self {
            id: AtomicU64::new(1),
            layer,
        }
    }

    pub fn with_max_level(connection: Connection, max_level: LevelFilter) -> Self {
        Self::with_layer(Layer {
            connection: Mutex::new(connection),
            max_level,
            black_list: None,
            white_list: None,
        })
    }

    pub fn black_list(&self) -> Option<&[&'static str]> {
        self.layer.black_list()
    }

    pub fn white_list(&self) -> Option<&[&'static str]> {
        self.layer.white_list()
    }
}

impl tracing::Subscriber for Subscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        self.layer.enabled(metadata)
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        self.layer.max_level_hint()
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        let id = self.id.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        span::Id::from_u64(id)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        self.layer.on_event(event)
    }

    fn enter(&self, _span: &span::Id) {}

    fn exit(&self, _span: &span::Id) {}
}

struct Visitor<'a> {
    pub message: &'a mut String,
    pub kvs: &'a mut HashMap<&'static str, String>, // todo: store structured key-value data
}

impl<'a> Visit for Visitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => write!(self.message, "{value:?}").unwrap(),
            #[cfg(feature = "tracing-log")]
            "log.line" | "log.file" | "log.target" | "log.module_path" => {}
            name => {
                self.kvs.insert(name, format!("{value:?}"));
            }
        }
    }
}

#[derive(Debug)]
pub struct SubscriberBuilder {
    max_level: LevelFilter,
    black_list: Option<Box<[&'static str]>>,
    white_list: Option<Box<[&'static str]>>,
}

impl SubscriberBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_level(self, max_level: LevelFilter) -> Self {
        Self { max_level, ..self }
    }

    /// A log will not be recorded if its module path starts with any of item in the black list.
    pub fn with_black_list(self, black_list: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            black_list: Some(black_list.into_iter().collect()),
            ..self
        }
    }

    /// A log may be recorded only if its module path starts with any of item in the white list.
    pub fn with_white_list(self, white_list: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            white_list: Some(white_list.into_iter().collect()),
            ..self
        }
    }

    pub fn build(self, conn: Connection) -> Subscriber {
        self.build_layer(conn).to_subscriber()
    }

    pub fn build_prepared(self, conn: Connection) -> Result<Subscriber, rusqlite::Error> {
        prepare_database(&conn)?;

        self.build_layer_prepared(conn).map(|l| l.to_subscriber())
    }

    pub fn build_layer(self, conn: Connection) -> Layer {
        Layer {
            connection: Mutex::new(conn),
            max_level: self.max_level,
            black_list: self.black_list,
            white_list: self.white_list,
        }
    }

    pub fn build_layer_prepared(self, conn: Connection) -> Result<Layer, rusqlite::Error> {
        prepare_database(&conn)?;

        Ok(self.build_layer(conn))
    }
}

impl Default for SubscriberBuilder {
    fn default() -> Self {
        Self {
            max_level: LevelFilter::DEBUG,
            black_list: None,
            white_list: None,
        }
    }
}
