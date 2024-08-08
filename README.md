# tracing-subscribe-sqlite

A tracing Subscriber to send log to sqlite database (WIP).

## Usage

```toml
[dependencies]
tracing-subscriber-sqlite = "0.1"
```

```rust
use rusqlite::Connection;
use tracing_subscriber_sqlite::prepare_database;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open("log.db")?;

    prepare_database(&conn)?;

    tracing::subscriber::set_global_default(tracing_subscriber_sqlite::Subscriber::new(conn))?;

    tracing::info!(x = 1, "test"); // structured data is ignored currently

    tracing::debug!("debug");

    Ok(())
}
```

### `log` Compatibility

Use `tracing-log` to send `log`'s records to `tracing` ecosystem.

```toml
[dependencies]
tracing-subscriber-sqlite = { version = "0.1", features = ["tracing-log"]}
```

```rust
use rusqlite::Connection;
use tracing_subscriber_sqlite::prepare_database;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open("log.db")?;

    prepare_database(&conn)?;

    tracing_log::LogTracer::init().unwrap(); // handle `log`'s records
    tracing::subscriber::set_global_default(tracing_subscriber_sqlite::Subscriber::new(conn))?;

    log::warn!("log warning");

    Ok(())
}
```
