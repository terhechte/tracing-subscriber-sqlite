mod db;

use std::{
    collections::HashMap,
    fmt::Write,
    sync::{atomic::AtomicU64, Arc, RwLock},
};

use db::{prepare_database, LogHandle};
use rusqlite::Connection;
use tracing::{field::Visit, level_filters::LevelFilter, span};
#[cfg(feature = "tracing-log")]
use tracing_log::NormalizeEvent;

pub struct Subscriber {
    id: AtomicU64,
    logger: LogHandle,
    max_level: LevelFilter,
    black_list: Option<Box<[&'static str]>>,
    white_list: Option<Box<[&'static str]>>,
}

impl Subscriber {
    pub fn new(connection: Connection) -> Self {
        Self::with_max_level(connection, LevelFilter::TRACE)
    }

    fn with_details(
        logger: RwLock<Connection>,
        max_level: LevelFilter,
        black_list: Option<Box<[&'static str]>>,
        white_list: Option<Box<[&'static str]>>,
    ) -> Self {
        Self {
            id: AtomicU64::new(1),
            logger: LogHandle(Arc::new(logger)),
            max_level,
            black_list,
            white_list,
        }
    }

    pub fn with_max_level(connection: Connection, max_level: LevelFilter) -> Self {
        Self::with_details(RwLock::new(connection), max_level, None, None)
    }

    pub fn black_list(&self) -> Option<&[&'static str]> {
        self.black_list.as_deref()
    }

    pub fn white_list(&self) -> Option<&[&'static str]> {
        self.white_list.as_deref()
    }

    pub fn log_handle(&self) -> LogHandle {
        self.logger.clone()
    }
}

impl tracing::Subscriber for Subscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.level() <= &self.max_level
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

    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        let id = self.id.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        span::Id::from_u64(id)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
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
        let module = meta.module_path();
        let file = meta.file();
        let line = meta.line();

        let mut message = String::new();
        let mut kvs = HashMap::new();

        event.record(&mut Visitor {
            message: &mut message,
            kvs: &mut kvs,
        });

        self.logger.log_v0(level, module, file, line, &message, kvs);
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
        Subscriber::with_details(
            RwLock::new(conn),
            self.max_level,
            self.black_list,
            self.white_list,
        )
    }

    pub fn build_prepared(self, conn: Connection) -> Result<Subscriber, rusqlite::Error> {
        prepare_database(&conn)?;

        Ok(self.build(conn))
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
