mod db;

pub use db::*;
use time::OffsetDateTime;

use std::{
    collections::HashMap,
    fmt::Write,
    sync::{atomic::AtomicU64, Arc, Mutex},
};

use rusqlite::Connection;
use tracing::{field::Visit, level_filters::LevelFilter, span};
#[cfg(feature = "tracing-log")]
use tracing_log::NormalizeEvent;

/// A `Layer` to write events to a sqlite database.
/// This type can be composed with other `Subscriber`s and `Layer`s.
#[derive(Debug)]
pub struct Layer<C> {
    logger: C,
    max_level: LevelFilter,
    black_list: Option<Box<[&'static str]>>,
    white_list: Option<Box<[&'static str]>>,
}

impl<C> Layer<C> {
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

    pub fn to_subscriber(self) -> Subscriber<C> {
        Subscriber::with_layer(self)
    }
}

impl<C: Connect> Layer<C> {
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

        let level = *meta.level();
        let module = meta.module_path();
        let file = meta.file();
        let line = meta.line();

        let mut message = String::new();
        let mut structured = HashMap::new();

        event.record(&mut Visitor {
            message: &mut message,
            kvs: &mut structured,
        });

        self.logger.log(LogEntry {
            time: OffsetDateTime::now_utc(),
            level,
            module,
            file,
            line,
            message,
            structured,
        });
    }
}

#[cfg(feature = "layer")]
impl<S: tracing::Subscriber, C: Connect + 'static> tracing_subscriber::Layer<S> for Layer<C> {
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
pub struct Subscriber<C> {
    id: AtomicU64,
    layer: Layer<C>,
}

impl<C> Subscriber<C> {
    pub fn new(connection: C) -> Self {
        Self::with_max_level(connection, LevelFilter::TRACE)
    }

    fn with_layer(layer: Layer<C>) -> Self {
        Self {
            id: AtomicU64::new(1),
            layer,
        }
    }

    pub fn with_max_level(connection: C, max_level: LevelFilter) -> Self {
        Self::with_layer(Layer {
            logger: connection,
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

impl<C: Connect + 'static> tracing::Subscriber for Subscriber<C> {
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

    pub fn build<C>(self, conn: C) -> Subscriber<C> {
        self.build_layer(conn).to_subscriber()
    }

    pub fn build_prepared(
        self,
        conn: Arc<Mutex<Connection>>,
    ) -> Result<Subscriber<Arc<Mutex<Connection>>>, rusqlite::Error> {
        self.build_layer_prepared(conn).map(|l| l.to_subscriber())
    }

    pub fn build_layer<C>(self, conn: C) -> Layer<C> {
        Layer {
            logger: conn,
            max_level: self.max_level,
            black_list: self.black_list,
            white_list: self.white_list,
        }
    }

    pub fn build_layer_prepared(
        self,
        conn: Arc<Mutex<Connection>>,
    ) -> Result<Layer<Arc<Mutex<Connection>>>, rusqlite::Error> {
        prepare_database(&*conn.lock().unwrap())?;

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
