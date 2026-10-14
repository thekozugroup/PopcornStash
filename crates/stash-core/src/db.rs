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
    let pool = connect(path).await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// The pool every connection of the server uses: WAL, foreign keys on.
async fn connect(path: &Path) -> Result<Db> {
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    Ok(SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?)
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

    /// Migration 0002 on a database that has lived at 0001 (section 6, Migration of
    /// existing data): run through the real pool, foreign keys on, inside sqlx's
    /// transaction. Every progress, trash, download link and favourite survives, every
    /// item gets its place, library and adder, and no foreign key is left dangling.
    #[tokio::test]
    async fn migration_0002_keeps_every_row() {
        let dir = tempfile::tempdir().unwrap();
        let only_first = dir.path().join("migrations");
        std::fs::create_dir(&only_first).unwrap();
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/migrations/0001_init.sql"),
            only_first.join("0001_init.sql"),
        )
        .unwrap();
        let path = dir.path().join("popcornstash.db");
        let db = super::connect(&path).await.unwrap();
        sqlx::migrate::Migrator::new(only_first.as_path())
            .await
            .unwrap()
            .run(&db)
            .await
            .unwrap();
        sqlx::raw_sql(
            "INSERT INTO users (id, display_name, role, webauthn_id, created_at) VALUES
                ('owner', 'Ana', 'owner', 'w1', 0), ('kid', 'Bo', 'member', 'w2', 0);
             INSERT INTO downloads (id, kind, name, source, state, created_by, created_at, updated_at) VALUES
                ('d-kid', 'torrent', 'Heat', 'magnet:?xt=urn:btih:a', 'sorted', 'kid', 1, 1),
                ('d-owner', 'upload', 'beach.jpg', 'beach.jpg', 'sorted', 'owner', 2, 2),
                ('d-gone', 'http', 'x', 'https://x', 'sorted', NULL, 3, 3);
             INSERT INTO items (id, rel_path, kind, category, status, title, sort_title, size, mtime,
                                favorite, source_download_id, added_at, updated_at) VALUES
                ('movie', 'Movies/Heat (1995)/Heat (1995).mkv', 'video', 'movie', 'ready', 'Heat', 'heat', 1, 1, 1, 'd-kid', 1, 1),
                ('episode', 'Shows/Bluey/Season 01/Bluey - S01E01.mkv', 'video', 'episode', 'ready', 'Bluey', 'bluey', 1, 1, 0, NULL, 1, 1),
                ('home', 'Home Videos/2024/06/clip.mp4', 'video', 'home_video', 'missing', 'clip', 'clip', 1, 1, 1, NULL, 1, 1),
                ('photo', 'Photos/2024/06/beach.jpg', 'photo', 'photo', 'ready', 'beach', 'beach', 1, 1, 0, NULL, 1, 1),
                ('doc', 'Other/recipe.pdf', 'other', 'other', 'ready', 'recipe', 'recipe', 1, 1, 0, NULL, 1, 1),
                ('waiting', 'Inbox/odd.mkv', 'video', 'movie', 'needs_review', 'odd', 'odd', 1, 1, 0, NULL, 1, 1),
                ('completed', 'Downloads/Completed/x/a.mkv', 'video', 'movie', 'needs_review', 'a', 'a', 1, 1, 0, NULL, 1, 1),
                ('family', 'Family 2019/beach.mp4', 'video', 'home_video', 'ready', 'beach', 'beach', 1, 1, 1, NULL, 1, 1),
                ('loose', 'loose.mkv', 'video', 'movie', 'ready', 'loose', 'loose', 1, 1, 0, NULL, 1, 1),
                ('lower', 'movies/lower.mkv', 'video', 'movie', 'ready', 'lower', 'lower', 1, 1, 0, NULL, 1, 1),
                ('trashed', '.popcornstash/Trash/trashed/old.mkv', 'video', 'movie', 'trashed', 'old', 'old', 1, 1, 0, NULL, 1, 1);
             INSERT INTO download_items (download_id, item_id) VALUES
                ('d-kid', 'movie'), ('d-owner', 'photo'), ('d-gone', 'doc');
             INSERT INTO item_progress (user_id, item_id, position_s, duration_s, finished, updated_at) VALUES
                ('owner', 'movie', 600, 7000, 0, 1), ('kid', 'movie', 60, 7000, 0, 1),
                ('kid', 'episode', 100, 600, 0, 1), ('owner', 'family', 5, 50, 0, 1),
                ('owner', 'trashed', 5, 50, 0, 1);
             INSERT INTO trash (item_id, original_rel_path, trashed_rel_path, trashed_at, trashed_by) VALUES
                ('trashed', 'Movies/Old/old.mkv', '.popcornstash/Trash/trashed/old.mkv', 1, 'owner');",
        )
        .execute(&db)
        .await
        .unwrap();
        db.close().await;

        // The real start: every migration, through the server's pool.
        let db = super::open(&path).await.unwrap();
        let count = |table: &'static str| {
            let db = db.clone();
            async move {
                sqlx::query_scalar::<_, i64>(&format!("SELECT count(*) FROM {table}"))
                    .fetch_one(&db)
                    .await
                    .unwrap()
            }
        };
        assert_eq!(count("items").await, 11);
        assert_eq!(count("item_progress").await, 5, "resume positions survive");
        assert_eq!(count("trash").await, 1, "the trash survives");
        assert_eq!(count("download_items").await, 3, "download links survive");
        let favourites: Vec<String> =
            sqlx::query_scalar("SELECT id FROM items WHERE favorite = 1 ORDER BY id")
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(favourites, ["family", "home", "movie"]);
        let rows: Vec<(String, String, Option<String>, Option<String>)> =
            sqlx::query_as("SELECT id, place, library_id, added_by FROM items ORDER BY id")
                .fetch_all(&db)
                .await
                .unwrap();
        let movies = Some(crate::library::defs::STARTING_MOVIES.to_string());
        let expected: Vec<(&str, &str, Option<String>, Option<&str>)> = vec![
            ("completed", "incoming", None, None),
            ("doc", "other_files", None, None),
            (
                "episode",
                "library",
                Some(crate::library::defs::STARTING_SHOWS.into()),
                None,
            ),
            ("family", "folder", None, None),
            (
                "home",
                "library",
                Some(crate::library::defs::STARTING_HOME_VIDEOS.into()),
                None,
            ),
            ("loose", "folder", None, None),
            ("lower", "folder", None, None),
            ("movie", "library", movies, Some("kid")),
            (
                "photo",
                "library",
                Some(crate::library::defs::STARTING_PHOTOS.into()),
                Some("owner"),
            ),
            ("trashed", "trash", None, None),
            ("waiting", "incoming", None, None),
        ];
        let got: Vec<(&str, &str, Option<String>, Option<&str>)> = rows
            .iter()
            .map(|(id, place, library, by)| {
                (id.as_str(), place.as_str(), library.clone(), by.as_deref())
            })
            .collect();
        assert_eq!(got, expected);
        let dangling: Vec<(String, i64, String, i64)> = sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(&db)
            .await
            .unwrap();
        assert!(dangling.is_empty(), "{dangling:?}");
        // The four starting libraries, seen by everyone, with their claims.
        let libraries: Vec<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT l.name, l.icon, l.folder, l.visibility, l.position FROM libraries l ORDER BY position",
        )
        .fetch_all(&db)
        .await
        .unwrap();
        assert_eq!(
            libraries
                .iter()
                .map(|(n, i, f, v, p)| (n.as_str(), i.as_str(), f.as_str(), v.as_str(), *p))
                .collect::<Vec<_>>(),
            vec![
                ("Movies", "film", "Movies", "everyone", 0),
                ("Shows", "tv", "Shows", "everyone", 1),
                ("Home Videos", "clapperboard", "Home Videos", "everyone", 2),
                ("Photos", "image", "Photos", "everyone", 3),
            ]
        );
        assert_eq!(count("library_claims").await, 4);
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
