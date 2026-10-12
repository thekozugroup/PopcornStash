//! SQLite pool and migrations.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

use crate::Result;

pub type Db = SqlitePool;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Open (creating if needed) the database at `path` and apply migrations.
pub async fn open(path: &Path) -> Result<Db> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// An in-memory database with migrations applied, for tests.
pub async fn open_memory() -> Result<Db> {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn migrations_apply() {
        let db = super::open_memory().await.unwrap();
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM items").fetch_one(&db).await.unwrap();
        assert_eq!(n, 0);
    }
}
