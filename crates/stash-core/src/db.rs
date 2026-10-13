//! SQLite pool and migrations.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use crate::Result;

pub type Db = SqlitePool;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Open (creating if needed) the database at `path` and apply migrations.
///
/// The database (with its `-wal` and `-shm` files) is private to the web server's user
/// (0600): it holds the library index, device tokens waiting to be collected and the
/// audit log, and the torrent engine shares the web server's group.
pub async fn open(path: &Path) -> Result<Db> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    make_private(path).await?;
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

/// Create the database file 0600 when it is missing (SQLite gives its `-wal` and
/// `-shm` files the database file's mode) and narrow any existing file to 0600.
async fn make_private(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if tokio::fs::symlink_metadata(path).await.is_err() {
        // Created empty; SQLite initialises it on first use.
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .await
        {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    let mut names = vec![path.as_os_str().to_owned()];
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        names.push(name);
    }
    for name in names {
        let file = Path::new(&name);
        match tokio::fs::symlink_metadata(file).await {
            Ok(meta) if meta.is_file() && meta.permissions().mode() & 0o077 != 0 => {
                tokio::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).await?;
            }
            _ => {}
        }
    }
    Ok(())
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
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// Nobody but the web server's user may read the database, not even its group
    /// (the torrent engine runs in the same group).
    #[tokio::test]
    async fn database_files_are_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("popcornstash.db");
        let db = super::open(&path).await.unwrap();
        sqlx::query("CREATE TABLE t (x)")
            .execute(&db)
            .await
            .unwrap();
        assert_eq!(mode(&path), 0o600);
        let wal = dir.path().join("popcornstash.db-wal");
        assert!(wal.exists());
        assert_eq!(mode(&wal), 0o600);
        db.close().await;

        // A database left readable by an older version is narrowed on the next start.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let db = super::open(&path).await.unwrap();
        assert_eq!(mode(&path), 0o600);
        db.close().await;
    }

    #[tokio::test]
    async fn migrations_apply() {
        let db = super::open_memory().await.unwrap();
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM items")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
