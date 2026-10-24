//! Integration tests: a real `Library` over a temporary stash folder and an in-memory database.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::broadcast;

use super::repo::{self, ItemQuery};
use super::watcher::{WatchOptions, spawn_with};
use super::{Library, OrganizeRequest, ReviewAction};
use crate::config::{Config, HwAccel};
use crate::events::{Event, EventBus, LibraryChange};
use crate::jobs::JobQueue;
use crate::media::MediaService;
use crate::models::{Category, DownloadState, ItemStatus};
use crate::paths::StashLayout;
use crate::settings::SettingsStore;

struct Harness {
    _dir: tempfile::TempDir,
    root: PathBuf,
    lib: Arc<Library>,
    events: broadcast::Receiver<Event>,
}

async fn harness() -> Harness {
    harness_in(tempfile::tempdir().unwrap()).await
}

async fn harness_in(dir: tempfile::TempDir) -> Harness {
    let root = dir.path().join("stash");
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&root).unwrap();
    let config = Arc::new(Config::for_tests(&root, &config_dir));
    let db = crate::db::open_memory().await.unwrap();
    let settings = SettingsStore::load(db.clone(), HwAccel::None)
        .await
        .unwrap();
    let layout = Arc::new(StashLayout::new(&root));
    layout.ensure().await.unwrap();
    crate::library::defs::ensure_folders(&db, &layout)
        .await
        .unwrap();
    let events = EventBus::new();
    let rx = events.subscribe();
    let media = Arc::new(MediaService::new(
        config,
        db.clone(),
        settings.clone(),
        layout.clone(),
        events.clone(),
    ));
    let jobs = JobQueue::new(db.clone(), events.clone());
    let lib = Arc::new(Library::new(db, layout, events, jobs, settings, media));
    Harness {
        _dir: dir,
        root,
        lib,
        events: rx,
    }
}

impl Harness {
    fn write(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Write a file whose mtime is `age` in the past (so it counts as stable).
    fn write_old(&self, rel: &str, bytes: &[u8], age: Duration) -> PathBuf {
        let path = self.write(rel, bytes);
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
        path
    }

    fn exists(&self, rel: &str) -> bool {
        self.root.join(rel).exists()
    }

    async fn item(&self, id: &str) -> crate::models::ItemRow {
        repo::get(&self.lib.db, id).await.unwrap().unwrap()
    }

    async fn jobs(&self, kind: &str) -> Vec<(Option<String>, serde_json::Value)> {
        let rows: Vec<(Option<String>, String)> =
            sqlx::query_as("SELECT item_id, payload FROM jobs WHERE kind = ? ORDER BY id")
                .bind(kind)
                .fetch_all(&self.lib.db)
                .await
                .unwrap();
        rows.into_iter()
            .map(|(id, p)| (id, serde_json::from_str(&p).unwrap()))
            .collect()
    }

    /// A full owner of this stash.
    fn owner(&self) -> crate::library::access::Viewer {
        crate::library::access::Viewer {
            user_id: "u".into(),
            role: crate::models::Role::Owner,
            full: true,
        }
    }

    /// The item's suggestion as a full owner reads it now.
    async fn suggestion(&self, item: &crate::models::ItemRow) -> crate::models::Suggestion {
        let access = crate::library::access::Access::load(&self.lib.db, &self.owner())
            .await
            .unwrap();
        let defs = crate::library::defs::load_all(&self.lib.db).await.unwrap();
        let states = crate::library::defs::folder_states(&self.lib.db, &self.lib.layout, &defs)
            .await
            .unwrap();
        crate::library::organizer::suggestion_view(item, &access, &defs, &states).unwrap()
    }

    /// Where the item's suggestion would put it now.
    async fn destination(&self, item: &crate::models::ItemRow) -> String {
        self.suggestion(item).await.destination.unwrap_or_default()
    }

    fn drain_events(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(e) = self.events.try_recv() {
            out.push(e);
        }
        out
    }
}

/// Unique bytes so quick hashes differ between files.
fn bytes(tag: &str) -> Vec<u8> {
    format!("popcornstash test file {tag}").into_bytes()
}

#[tokio::test]
async fn inbox_movie_is_sorted_with_its_sidecars() {
    let mut h = harness().await;
    h.write("Inbox/Heat.1995.1080p.BluRay.x264-GRP.mkv", &bytes("heat"));
    h.write(
        "Inbox/Heat.1995.1080p.BluRay.x264-GRP.srt",
        b"1\n00:00:01,000 --> 00:00:02,000\nHi\n",
    );
    h.write(
        "Inbox/Heat.1995.1080p.BluRay.x264-GRP.en.forced.srt",
        b"subs",
    );
    h.write("Inbox/Heat.1995.1080p.BluRay.x264-GRP.nfo", b"nfo");

    let ids = h
        .lib
        .sort_path("Inbox/Heat.1995.1080p.BluRay.x264-GRP.mkv", None)
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);
    let item = h.item(&ids[0]).await;
    assert_eq!(item.rel_path, "Movies/Heat (1995)/Heat (1995).mkv");
    assert_eq!(
        (item.category, item.status),
        (Category::Movie, ItemStatus::Ready)
    );
    assert_eq!((item.title.as_str(), item.year), ("Heat", Some(1995)));
    assert_eq!(item.sort_title, "heat");
    assert!(item.quick_hash.is_some());
    assert!(item.suggestion_json.is_none());
    assert!(h.exists("Movies/Heat (1995)/Heat (1995).mkv"));
    assert!(h.exists("Movies/Heat (1995)/Heat (1995).srt"));
    assert!(h.exists("Movies/Heat (1995)/Heat (1995).en.forced.srt"));
    assert!(h.exists("Movies/Heat (1995)/Heat (1995).nfo"));
    assert_eq!(
        std::fs::read_dir(h.root.join("Inbox")).unwrap().count(),
        0,
        "Inbox is empty again"
    );
    assert!(h.exists("Inbox"), "the Inbox folder itself stays");

    let probes = h.jobs("probe").await;
    let thumbs = h.jobs("thumb").await;
    assert_eq!(probes.len(), 1);
    assert_eq!(thumbs.len(), 1);
    assert_eq!(probes[0].0.as_deref(), Some(item.id.as_str()));
    let events = h.drain_events();
    assert!(events.iter().any(|e| matches!(e, Event::Library { r#type: LibraryChange::Added, item_id, .. } if *item_id == item.id)));
    assert!(
        !events.iter().any(|e| matches!(e, Event::Inbox { .. })),
        "review count did not change"
    );

    // Sorting the same name again collides and gets " (2)".
    h.write("Inbox/Heat (1995).mkv", &bytes("heat two"));
    let ids = h
        .lib
        .sort_path("Inbox/Heat (1995).mkv", None)
        .await
        .unwrap();
    assert_eq!(
        h.item(&ids[0]).await.rel_path,
        "Movies/Heat (1995)/Heat (1995) (2).mkv"
    );
    assert_eq!(
        std::fs::read(h.root.join("Movies/Heat (1995)/Heat (1995).mkv")).unwrap(),
        bytes("heat")
    );
}

#[tokio::test]
async fn low_confidence_files_wait_for_review() {
    let mut h = harness().await;
    h.write("Inbox/Our Wedding.mp4", &bytes("wedding"));
    let ids = h
        .lib
        .sort_path("Inbox/Our Wedding.mp4", None)
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);
    let item = h.item(&ids[0]).await;
    assert_eq!(item.status, ItemStatus::NeedsReview);
    assert_eq!(item.rel_path, "Inbox/Our Wedding.mp4");
    assert!(
        h.exists("Inbox/Our Wedding.mp4"),
        "low confidence files stay in place"
    );
    assert_eq!(h.destination(&item).await, "Other/Our Wedding.mp4");
    let events = h.drain_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Inbox { count: 1 }))
    );
    let access = crate::library::access::Access::load(&h.lib.db, &h.owner())
        .await
        .unwrap();
    let inbox = repo::inbox(&h.lib.db, &h.lib.layout, &access)
        .await
        .unwrap()
        .items;
    assert_eq!(inbox.len(), 1);
    assert_eq!(
        inbox[0]
            .suggestion
            .as_ref()
            .unwrap()
            .classification
            .category,
        Category::Other
    );

    // Sorting again changes nothing: the item is already waiting.
    assert!(
        h.lib
            .sort_path("Inbox/Our Wedding.mp4", None)
            .await
            .unwrap()
            .is_empty()
    );

    // Accept with an edit: file it as a home video.
    let accepted = h
        .lib
        .review(
            &item.id,
            ReviewAction::Accept(OrganizeRequest {
                category: Some(Category::HomeVideo),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(accepted.id, item.id);
    assert_eq!(accepted.status, ItemStatus::Ready);
    assert_eq!(accepted.category, Category::HomeVideo);
    assert!(
        accepted.rel_path.starts_with("Home Videos/"),
        "{}",
        accepted.rel_path
    );
    assert!(accepted.rel_path.ends_with("/Our Wedding.mp4"));
    assert!(h.exists(&accepted.rel_path));
    assert!(!h.exists("Inbox/Our Wedding.mp4"));
    assert!(
        h.drain_events()
            .iter()
            .any(|e| matches!(e, Event::Inbox { count: 0 }))
    );
    assert!(
        h.lib.review(&item.id, ReviewAction::Ignore).await.is_err(),
        "already reviewed"
    );

    // Ignore: reviewed, stays where it is.
    h.write("Inbox/Some.Movie.1080p.WEB-DL.mkv", &bytes("nameless"));
    let ids = h
        .lib
        .sort_path("Inbox/Some.Movie.1080p.WEB-DL.mkv", None)
        .await
        .unwrap();
    let pending = h.item(&ids[0]).await;
    assert_eq!(pending.status, ItemStatus::NeedsReview);
    assert_eq!(pending.category, Category::Movie);
    let ignored = h
        .lib
        .review(&pending.id, ReviewAction::Ignore)
        .await
        .unwrap();
    assert_eq!(ignored.status, ItemStatus::Ready);
    assert_eq!(ignored.category, Category::Movie);
    assert_eq!(ignored.rel_path, "Inbox/Some.Movie.1080p.WEB-DL.mkv");
    // A later sort leaves the reviewed file alone.
    assert!(
        h.lib
            .sort_path("Inbox/Some.Movie.1080p.WEB-DL.mkv", None)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(h.item(&pending.id).await.status, ItemStatus::Ready);
}

#[tokio::test]
async fn accept_uses_the_suggestion_and_edits() {
    let h = harness().await;
    h.write(
        "Downloads/Completed/Some.Movie.1080p.WEB-DL.mkv",
        &bytes("movie"),
    );
    let ids = h
        .lib
        .sort_path("Downloads/Completed/Some.Movie.1080p.WEB-DL.mkv", None)
        .await
        .unwrap();
    let item = h.item(&ids[0]).await;
    let accepted = h
        .lib
        .review(
            &item.id,
            ReviewAction::Accept(OrganizeRequest {
                year: Some(2021),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        accepted.rel_path,
        "Movies/Some Movie (2021)/Some Movie (2021).mkv"
    );
    assert_eq!(
        (accepted.title.as_str(), accepted.year),
        ("Some Movie", Some(2021))
    );
    assert!(accepted.suggestion_json.is_none());

    // An episode without a show name is refused with a plain sentence.
    h.write("Inbox/S01E02.mkv", &bytes("ep"));
    let ids = h.lib.sort_path("Inbox/S01E02.mkv", None).await.unwrap();
    let err = h
        .lib
        .review(&ids[0], ReviewAction::Accept(OrganizeRequest::default()))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("show name"), "{err}");
    let ok = h
        .lib
        .review(
            &ids[0],
            ReviewAction::Accept(OrganizeRequest {
                show_name: Some("Andor".into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(ok.rel_path, "Shows/Andor/Season 01/Andor - S01E02.mkv");
    assert_eq!(ok.show_name.as_deref(), Some("Andor"));
}

#[tokio::test]
async fn auto_sort_off_sends_everything_to_review() {
    let h = harness().await;
    h.lib
        .settings
        .update(serde_json::json!({"auto_sort": false}))
        .await
        .unwrap();
    h.write("Inbox/Heat.1995.mkv", &bytes("heat"));
    let ids = h.lib.sort_path("Inbox/Heat.1995.mkv", None).await.unwrap();
    let item = h.item(&ids[0]).await;
    assert_eq!(item.status, ItemStatus::NeedsReview);
    assert_eq!(
        h.destination(&item).await,
        "Movies/Heat (1995)/Heat (1995).mkv"
    );
    assert!(h.exists("Inbox/Heat.1995.mkv"));
}

#[tokio::test]
async fn completed_download_folder_is_sorted_and_linked() {
    let mut h = harness().await;
    let folder = "Downloads/Completed/The.Office.US.S02.720p.WEB-DL";
    h.write(
        &format!("{folder}/The.Office.US.S02E01.720p.WEB-DL.mkv"),
        &bytes("e1"),
    );
    h.write(
        &format!("{folder}/The.Office.US.S02E02.720p.WEB-DL.mkv"),
        &bytes("e2"),
    );
    h.write(
        &format!("{folder}/Sample/the.office.s02e01.sample.mkv"),
        &bytes("sample"),
    );
    h.write(&format!("{folder}/Extras/Bloopers.mkv"), &bytes("extras"));
    h.write(&format!("{folder}/release.txt"), b"txt");
    sqlx::query(
        "INSERT INTO downloads (id, kind, name, source, state, created_at, updated_at, rel_path)
         VALUES ('dl1', 'torrent', 'The Office', 'magnet:?xt=urn:btih:abc', 'sorting', 1, 1, ?)",
    )
    .bind(folder)
    .execute(&h.lib.db)
    .await
    .unwrap();

    let ids = h.lib.sort_path(folder, Some("dl1")).await.unwrap();
    assert_eq!(ids.len(), 2);
    assert!(h.exists("Shows/The Office US/Season 02/The Office US - S02E01.mkv"));
    assert!(h.exists("Shows/The Office US/Season 02/The Office US - S02E02.mkv"));
    // Samples, extras and non-media stay behind in Completed.
    assert!(h.exists(&format!("{folder}/Sample/the.office.s02e01.sample.mkv")));
    assert!(h.exists(&format!("{folder}/Extras/Bloopers.mkv")));
    assert!(h.exists(&format!("{folder}/release.txt")));

    let (state, sorted_at): (String, Option<i64>) =
        sqlx::query_as("SELECT state, sorted_at FROM downloads WHERE id = 'dl1'")
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
    assert_eq!(state, "sorted");
    assert!(sorted_at.is_some());
    let linked: Vec<String> =
        sqlx::query_scalar("SELECT item_id FROM download_items WHERE download_id = 'dl1'")
            .fetch_all(&h.lib.db)
            .await
            .unwrap();
    assert_eq!(linked.len(), 2);
    for id in &ids {
        assert_eq!(h.item(id).await.source_download_id.as_deref(), Some("dl1"));
    }
    let events = h.drain_events();
    let download = events
        .iter()
        .find_map(|e| match e {
            Event::Download(d) => Some(d.clone()),
            _ => None,
        })
        .expect("download event");
    assert_eq!(download.state, DownloadState::Sorted);
    assert_eq!(download.item_ids.len(), 2);

    // The next episode for the same show (different spelling) joins the existing folder.
    h.write("Inbox/the office us s02e03.mkv", &bytes("e3"));
    let ids = h
        .lib
        .sort_path("Inbox/the office us s02e03.mkv", None)
        .await
        .unwrap();
    let e3 = h.item(&ids[0]).await;
    assert_eq!(e3.show_name.as_deref(), Some("The Office US"));
    assert_eq!(
        e3.rel_path,
        "Shows/The Office US/Season 02/The Office US - S02E03.mkv"
    );
    let show = repo::show(
        &h.lib.db,
        crate::library::defs::STARTING_SHOWS,
        "The Office US",
        "u",
    )
    .await
    .unwrap();
    assert_eq!(show.seasons[0].episodes.len(), 3);
}

#[tokio::test]
async fn sorter_finds_the_download_by_path_and_cleans_empty_folders() {
    let h = harness().await;
    let folder = "Downloads/Completed/Arrival.2016.1080p";
    h.write(
        &format!("{folder}/nested/deeper/Arrival.2016.1080p.BluRay.mkv"),
        &bytes("arrival"),
    );
    sqlx::query(
        "INSERT INTO downloads (id, kind, name, source, state, created_at, updated_at, rel_path)
         VALUES ('dl2', 'http', 'Arrival', 'https://example.com/a.mkv', 'completed', 1, 1, ?)",
    )
    .bind(folder)
    .execute(&h.lib.db)
    .await
    .unwrap();
    let ids = h.lib.sort_path(folder, None).await.unwrap();
    assert_eq!(ids.len(), 1);
    assert!(h.exists("Movies/Arrival (2016)/Arrival (2016).mkv"));
    assert!(!h.exists(folder), "emptied download folder is removed");
    assert!(
        h.exists("Downloads/Completed"),
        "the Completed folder stays"
    );
    assert!(h.exists("Downloads/In Progress"));
    let state: String = sqlx::query_scalar("SELECT state FROM downloads WHERE id = 'dl2'")
        .fetch_one(&h.lib.db)
        .await
        .unwrap();
    assert_eq!(state, "sorted");

    // A download whose files vanished is reported, not left sorting forever.
    sqlx::query(
        "INSERT INTO downloads (id, kind, name, source, state, created_at, updated_at, rel_path)
         VALUES ('dl3', 'http', 'Gone', 'https://example.com/g.mkv', 'sorting', 1, 1, 'Downloads/Completed/gone.mkv')",
    )
    .execute(&h.lib.db)
    .await
    .unwrap();
    assert!(
        h.lib
            .sort_path("Downloads/Completed/gone.mkv", Some("dl3"))
            .await
            .unwrap()
            .is_empty()
    );
    let (state, error): (String, Option<String>) =
        sqlx::query_as("SELECT state, error FROM downloads WHERE id = 'dl3'")
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
    assert_eq!(state, "failed");
    assert!(error.unwrap().contains("not found"));
}

#[tokio::test]
async fn sort_refuses_paths_outside_the_drop_zones() {
    let h = harness().await;
    h.write("Movies/Heat (1995)/Heat (1995).mkv", &bytes("heat"));
    assert!(
        h.lib
            .sort_path("Movies/Heat (1995)/Heat (1995).mkv", None)
            .await
            .is_err()
    );
    assert!(
        h.lib
            .enqueue_sort("Movies/Heat (1995)", None)
            .await
            .is_err()
    );
    assert!(
        h.lib
            .enqueue_sort("Downloads/In Progress/x", None)
            .await
            .is_err()
    );
    assert!(h.lib.enqueue_sort("../outside", None).await.is_err());
    assert!(h.exists("Movies/Heat (1995)/Heat (1995).mkv"));
}

#[tokio::test]
async fn enqueue_sort_deduplicates_and_links_downloads() {
    let h = harness().await;
    h.lib.enqueue_sort("Inbox/a.mkv", None).await.unwrap();
    h.lib.enqueue_sort("/Inbox/a.mkv/", None).await.unwrap();
    assert_eq!(h.jobs("sort").await.len(), 1);
    h.lib
        .enqueue_sort("Inbox/a.mkv", Some("dl9"))
        .await
        .unwrap();
    let jobs = h.jobs("sort").await;
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        jobs[0].1,
        serde_json::json!({"rel_path": "Inbox/a.mkv", "download_id": "dl9"})
    );
    // Once that job is done, a new one can be queued.
    sqlx::query("UPDATE jobs SET state = 'done'")
        .execute(&h.lib.db)
        .await
        .unwrap();
    h.lib.enqueue_sort("Inbox/a.mkv", None).await.unwrap();
    assert_eq!(h.jobs("sort").await.len(), 2);
}

#[tokio::test]
async fn custom_folders_are_indexed_but_never_moved() {
    let h = harness().await;
    h.write("Family 2019/Heat.1995.1080p.mkv", &bytes("heat"));
    h.write("Family 2019/beach.jpg", &bytes("beach"));
    h.write("Family 2019/notes.txt", b"notes");
    h.write("Movies/Up (2009)/Up (2009).mkv", &bytes("up"));
    h.write("Movies/Up (2009)/poster.jpg", &bytes("poster"));
    h.write("Shows/Andor/Season 01/Andor - S01E01.mkv", &bytes("andor"));
    h.write("Photos/2020/01/IMG_0001.jpg", &bytes("photo"));
    h.write("Downloads/In Progress/partial/movie.mkv", &bytes("partial"));
    h.write(".popcornstash/Trash/x/old.mkv", &bytes("old"));

    let report = h.lib.rescan().await.unwrap();
    assert_eq!(report.added, 5, "{report:?}");
    assert_eq!(
        (report.updated, report.moved, report.missing, report.removed),
        (0, 0, 0, 0)
    );
    let custom = repo::get_by_path(&h.lib.db, "Family 2019/Heat.1995.1080p.mkv")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(custom.category, Category::Movie);
    assert_eq!(custom.status, ItemStatus::Ready);
    assert_eq!(
        h.destination(&custom).await,
        "Movies/Heat (1995)/Heat (1995).mkv"
    );
    assert!(
        h.exists("Family 2019/Heat.1995.1080p.mkv"),
        "custom folders are never moved automatically"
    );
    let photo = repo::get_by_path(&h.lib.db, "Family 2019/beach.jpg")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(photo.category, Category::Photo);
    assert!(
        repo::get_by_path(&h.lib.db, "Family 2019/notes.txt")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        repo::get_by_path(&h.lib.db, "Movies/Up (2009)/poster.jpg")
            .await
            .unwrap()
            .is_none()
    );
    let up = repo::get_by_path(&h.lib.db, "Movies/Up (2009)/Up (2009).mkv")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (up.title.as_str(), up.year, up.category),
        ("Up", Some(2009), Category::Movie)
    );
    let andor = repo::get_by_path(&h.lib.db, "Shows/Andor/Season 01/Andor - S01E01.mkv")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (andor.show_name.as_deref(), andor.season, andor.episode),
        (Some("Andor"), Some(1), Some(1))
    );
    assert!(
        repo::get_by_path(&h.lib.db, "Downloads/In Progress/partial/movie.mkv")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        repo::get_by_path(&h.lib.db, ".popcornstash/Trash/x/old.mkv")
            .await
            .unwrap()
            .is_none()
    );

    // A second rescan finds nothing new.
    let again = h.lib.rescan().await.unwrap();
    assert_eq!(again, super::ScanReport::default());

    // Organizing is the explicit action that moves it.
    let organized = h
        .lib
        .organize(&custom.id, OrganizeRequest::default())
        .await
        .unwrap();
    assert_eq!(organized.rel_path, "Movies/Heat (1995)/Heat (1995).mkv");
    assert!(!h.exists("Family 2019/Heat.1995.1080p.mkv"));
    assert!(
        h.exists("Family 2019"),
        "the owner's folders are never removed"
    );
    let organized = h
        .lib
        .organize(
            &custom.id,
            OrganizeRequest {
                title: Some("Heat: Director's Cut".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        organized.rel_path,
        "Movies/Heat - Director's Cut (1995)/Heat - Director's Cut (1995).mkv"
    );
    assert_eq!(organized.title, "Heat: Director's Cut");
}

/// Store a capture time the way the probe would after reading EXIF or QuickTime tags.
async fn set_taken(h: &Harness, id: &str, at: i64, local: &str, offset: Option<i32>) {
    sqlx::query("UPDATE items SET taken_at = ?, taken_local = ?, taken_offset = ? WHERE id = ?")
        .bind(at)
        .bind(local)
        .bind(offset)
        .bind(id)
        .execute(&h.lib.db)
        .await
        .unwrap();
}

fn taken_of(item: &crate::models::ItemRow) -> (Option<i64>, Option<&str>, Option<i32>) {
    (
        item.taken_at,
        item.taken_local.as_deref(),
        item.taken_offset,
    )
}

fn move_file(h: &Harness, from: &str, to: &str) {
    std::fs::create_dir_all(h.root.join(to).parent().unwrap()).unwrap();
    std::fs::rename(h.root.join(from), h.root.join(to)).unwrap();
}

/// A photo or clip moved by hand between Home Videos and Photos becomes the other kind
/// but keeps the time its EXIF or QuickTime tags gave: the name and the file time
/// (which the move does not change) never replace it.
#[tokio::test]
async fn a_file_moved_between_photos_and_home_videos_keeps_its_capture_time() {
    let h = harness().await;
    h.write("Home Videos/2024/06/IMG_0001.MOV", &bytes("clip"));
    h.write("Photos/2024/06/june30_p0900.jpg", &bytes("photo"));
    h.lib.rescan().await.unwrap();
    let clip = repo::get_by_path(&h.lib.db, "Home Videos/2024/06/IMG_0001.MOV")
        .await
        .unwrap()
        .unwrap();
    let photo = repo::get_by_path(&h.lib.db, "Photos/2024/06/june30_p0900.jpg")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(clip.category, Category::HomeVideo);
    assert_eq!(photo.category, Category::Photo);
    // 23:30 in Tokyo and 09:00 in Los Angeles, as the probe read them from the files.
    set_taken(
        &h,
        &clip.id,
        1_719_757_800,
        "2024-06-30T23:30:00",
        Some(540),
    )
    .await;
    set_taken(
        &h,
        &photo.id,
        1_719_763_200,
        "2024-06-30T09:00:00",
        Some(-420),
    )
    .await;

    move_file(
        &h,
        "Home Videos/2024/06/IMG_0001.MOV",
        "Photos/Moved/IMG_0001.MOV",
    );
    move_file(
        &h,
        "Photos/2024/06/june30_p0900.jpg",
        "Home Videos/Moved/june30_p0900.jpg",
    );
    let report = h.lib.rescan().await.unwrap();
    assert_eq!(report.moved, 2, "{report:?}");

    let clip = h.item(&clip.id).await;
    assert_eq!(clip.rel_path, "Photos/Moved/IMG_0001.MOV");
    assert_eq!(clip.category, Category::Photo);
    assert_eq!(
        taken_of(&clip),
        (Some(1_719_757_800), Some("2024-06-30T23:30:00"), Some(540))
    );
    let photo = h.item(&photo.id).await;
    assert_eq!(photo.rel_path, "Home Videos/Moved/june30_p0900.jpg");
    assert_eq!(photo.category, Category::HomeVideo);
    assert_eq!(
        taken_of(&photo),
        (Some(1_719_763_200), Some("2024-06-30T09:00:00"), Some(-420))
    );

    // And back again, and once more through a rescan that finds nothing new.
    move_file(
        &h,
        "Photos/Moved/IMG_0001.MOV",
        "Home Videos/2024/06/IMG_0001.MOV",
    );
    h.lib.rescan().await.unwrap();
    h.lib.rescan().await.unwrap();
    let clip = h.item(&clip.id).await;
    assert_eq!(clip.category, Category::HomeVideo);
    assert_eq!(
        taken_of(&clip),
        (Some(1_719_757_800), Some("2024-06-30T23:30:00"), Some(540))
    );
}

/// A movie moved by hand into Photos had no capture time: it is dated from its name or
/// file time, and the probe is queued to read its tags.
#[tokio::test]
async fn a_movie_moved_into_photos_is_dated_and_probed() {
    let h = harness().await;
    h.write("Movies/Heat (1995)/Heat (1995).mkv", &bytes("heat"));
    // Something stays in Photos, so its folder never looks like an unplugged disk.
    h.write("Photos/2020/01/IMG_0001.jpg", &bytes("photo"));
    h.lib.rescan().await.unwrap();
    let heat = repo::get_by_path(&h.lib.db, "Movies/Heat (1995)/Heat (1995).mkv")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(heat.taken_at, None);
    sqlx::query("DELETE FROM jobs")
        .execute(&h.lib.db)
        .await
        .unwrap();

    move_file(&h, "Movies/Heat (1995)/Heat (1995).mkv", "Photos/heat.mkv");
    h.lib.rescan().await.unwrap();
    let heat = h.item(&heat.id).await;
    assert_eq!(heat.category, Category::Photo);
    assert_eq!(heat.taken_at, Some(heat.mtime), "dated from the file time");
    let probes = h.jobs("probe").await;
    assert_eq!(probes, vec![(Some(heat.id.clone()), serde_json::json!({}))]);

    // Back into Movies: a movie has no capture time.
    move_file(&h, "Photos/heat.mkv", "Movies/Heat (1995)/Heat (1995).mkv");
    h.lib.rescan().await.unwrap();
    let heat = h.item(&heat.id).await;
    assert_eq!(heat.category, Category::Movie);
    assert_eq!(taken_of(&heat), (None, None, None));
}

#[tokio::test]
async fn moved_and_missing_files_are_reconciled() {
    let h = harness().await;
    h.write("Movies/Heat (1995)/Heat (1995).mkv", &bytes("heat"));
    h.write("Movies/Up (2009)/Up (2009).mkv", &bytes("up"));
    let report = h.lib.rescan().await.unwrap();
    assert_eq!(report.added, 2);
    let heat = repo::get_by_path(&h.lib.db, "Movies/Heat (1995)/Heat (1995).mkv")
        .await
        .unwrap()
        .unwrap();
    repo::update_metadata(
        &h.lib.db,
        &heat.id,
        &repo::ItemPatch {
            favorite: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Renamed outside PopcornStash: same item, favorite kept.
    std::fs::create_dir_all(h.root.join("Movies/Heat")).unwrap();
    std::fs::rename(
        h.root.join("Movies/Heat (1995)/Heat (1995).mkv"),
        h.root.join("Movies/Heat/heat.mkv"),
    )
    .unwrap();
    // Deleted outside PopcornStash.
    std::fs::remove_file(h.root.join("Movies/Up (2009)/Up (2009).mkv")).unwrap();
    let report = h.lib.rescan().await.unwrap();
    assert_eq!(
        (report.moved, report.missing, report.added),
        (1, 1, 0),
        "{report:?}"
    );
    let moved = h.item(&heat.id).await;
    assert_eq!(moved.rel_path, "Movies/Heat/heat.mkv");
    assert_eq!(moved.status, ItemStatus::Ready);
    assert!(moved.favorite);
    assert_eq!(
        moved.title, "Heat",
        "user metadata survives a move within Movies"
    );
    let up = repo::get_by_path(&h.lib.db, "Movies/Up (2009)/Up (2009).mkv")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(up.status, ItemStatus::Missing);
    assert!(up.missing_since.is_some());
    let page = repo::list(&h.lib.db, &ItemQuery::default(), &h.owner())
        .await
        .unwrap();
    assert_eq!(
        page.items.len(),
        1,
        "missing items are hidden from the default list"
    );

    // Missing for a day: kept. Missing for over a week: forgotten.
    let report = h.lib.rescan().await.unwrap();
    assert_eq!((report.missing, report.removed), (0, 0));
    sqlx::query("UPDATE items SET missing_since = ? WHERE id = ?")
        .bind(crate::now() - 8 * 86_400)
        .bind(&up.id)
        .execute(&h.lib.db)
        .await
        .unwrap();
    let report = h.lib.rescan().await.unwrap();
    assert_eq!(report.removed, 1);
    assert!(repo::get(&h.lib.db, &up.id).await.unwrap().is_none());

    // Changed in place: same id, refreshed size.
    h.write("Movies/Heat/heat.mkv", b"a longer version of the file");
    let report = h.lib.rescan().await.unwrap();
    assert_eq!(report.updated, 1);
    let changed = h.item(&heat.id).await;
    assert_eq!(changed.size, 28);
    assert!(changed.favorite);

    // index_path on a deleted file marks it missing; it comes back when the file does.
    let content = std::fs::read(h.root.join("Movies/Heat/heat.mkv")).unwrap();
    std::fs::remove_file(h.root.join("Movies/Heat/heat.mkv")).unwrap();
    assert!(
        h.lib
            .index_path("Movies/Heat/heat.mkv")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(h.item(&heat.id).await.status, ItemStatus::Missing);
    std::fs::write(h.root.join("Movies/Heat/heat.mkv"), &content).unwrap();
    let back = h
        .lib
        .index_path("Movies/Heat/heat.mkv")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(back.id, heat.id);
    assert_eq!(back.status, ItemStatus::Ready);
}

#[tokio::test]
async fn rescan_queues_stable_inbox_entries() {
    let h = harness().await;
    h.write_old("Inbox/old.mkv", &bytes("old"), Duration::from_secs(60));
    h.write("Inbox/fresh.mkv", &bytes("fresh"));
    h.write_old(
        "Inbox/Folder/inner.mp4",
        &bytes("inner"),
        Duration::from_secs(60),
    );
    let folder = h.root.join("Inbox/Folder");
    std::fs::File::open(&folder)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(60))
        .unwrap();
    h.write_old("Inbox/readme.txt", b"not media", Duration::from_secs(60));
    h.write_old(
        "Downloads/Completed/done.mkv",
        &bytes("done"),
        Duration::from_secs(60),
    );
    h.write_old(
        "Downloads/Completed/tax-form.pdf",
        b"%PDF-1.4",
        Duration::from_secs(60),
    );
    h.write_old(
        "Downloads/loose.mkv",
        &bytes("loose"),
        Duration::from_secs(60),
    );
    h.write_old(
        "Downloads/In Progress/active.mkv",
        &bytes("active"),
        Duration::from_secs(60),
    );

    let report = h.lib.rescan().await.unwrap();
    assert_eq!(report.queued_for_sort, 5, "{report:?}");
    assert_eq!(report.added, 0, "the sorter owns Inbox and Completed");
    let mut queued: Vec<String> = h
        .jobs("sort")
        .await
        .into_iter()
        .map(|(_, p)| p["rel_path"].as_str().unwrap().to_string())
        .collect();
    queued.sort();
    // Files that are not media are sorted too (to Other/, after review); loose files
    // in Downloads/ are not PopcornStash's to move.
    assert_eq!(
        queued,
        vec![
            "Downloads/Completed/done.mkv",
            "Downloads/Completed/tax-form.pdf",
            "Inbox/Folder",
            "Inbox/old.mkv",
            "Inbox/readme.txt"
        ]
    );
    // Already queued: not queued twice.
    assert_eq!(h.lib.rescan().await.unwrap().queued_for_sort, 0);
}

#[tokio::test]
async fn trash_and_restore_roundtrip() {
    let mut h = harness().await;
    h.add_owner().await;
    h.write("Movies/Heat (1995)/Heat (1995).mkv", &bytes("heat"));
    h.write("Movies/Heat (1995)/Heat (1995).srt", b"subs");
    h.lib.rescan().await.unwrap();
    let heat = repo::get_by_path(&h.lib.db, "Movies/Heat (1995)/Heat (1995).mkv")
        .await
        .unwrap()
        .unwrap();
    h.drain_events();

    h.lib.trash_item(&heat.id, Some("owner")).await.unwrap();
    let trashed = h.item(&heat.id).await;
    assert_eq!(trashed.status, ItemStatus::Trashed);
    assert_eq!(
        trashed.rel_path,
        format!(".popcornstash/Trash/{}/Heat (1995).mkv", heat.id)
    );
    assert!(h.exists(&trashed.rel_path));
    assert!(h.exists(&format!(".popcornstash/Trash/{}/Heat (1995).srt", heat.id)));
    assert!(!h.exists("Movies/Heat (1995)/Heat (1995).mkv"));
    assert!(h.drain_events().iter().any(|e| matches!(
        e,
        Event::Library {
            r#type: LibraryChange::Removed,
            ..
        }
    )));
    let list = repo::trash_list(&h.lib.db).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(
        list[0].original_rel_path,
        "Movies/Heat (1995)/Heat (1995).mkv"
    );
    assert_eq!(list[0].trashed_by.as_deref(), Some("owner"));
    assert!(
        h.lib.trash_item(&heat.id, None).await.is_err(),
        "already in the trash"
    );

    // Rescans leave trashed items alone.
    let report = h.lib.rescan().await.unwrap();
    assert_eq!(report, super::ScanReport::default());
    assert_eq!(h.item(&heat.id).await.status, ItemStatus::Trashed);
    // Organizing a trashed item is refused.
    assert!(
        h.lib
            .organize(&heat.id, OrganizeRequest::default())
            .await
            .is_err()
    );

    // Restore puts it back (with its subtitles) and empties its trash folder.
    let restored = h.lib.restore_item(&heat.id).await.unwrap();
    assert_eq!(restored.rel_path, "Movies/Heat (1995)/Heat (1995).mkv");
    assert_eq!(restored.status, ItemStatus::Ready);
    assert!(h.exists("Movies/Heat (1995)/Heat (1995).srt"));
    assert!(!h.exists(&format!(".popcornstash/Trash/{}", heat.id)));
    assert!(repo::trash_list(&h.lib.db).await.unwrap().is_empty());

    // Restoring onto an occupied name picks a free one.
    h.lib.trash_item(&heat.id, None).await.unwrap();
    h.write("Movies/Heat (1995)/Heat (1995).mkv", &bytes("replacement"));
    let restored = h.lib.restore_item(&heat.id).await.unwrap();
    assert_eq!(restored.rel_path, "Movies/Heat (1995)/Heat (1995) (2).mkv");
    assert_eq!(
        std::fs::read(h.root.join(&restored.rel_path)).unwrap(),
        bytes("heat")
    );

    // Empty the trash: files and rows are gone.
    h.lib.trash_item(&heat.id, Some("owner")).await.unwrap();
    assert_eq!(
        h.lib.empty_trash(Some(30)).await.unwrap(),
        0,
        "too recent for the retention sweep"
    );
    assert_eq!(h.lib.empty_trash(None).await.unwrap(), 1);
    assert!(repo::get(&h.lib.db, &heat.id).await.unwrap().is_none());
    assert!(!h.exists(&format!(".popcornstash/Trash/{}", heat.id)));
    assert!(h.exists(".popcornstash/Trash"));
    assert!(matches!(
        h.lib.restore_item(&heat.id).await,
        Err(crate::CoreError::NotFound)
    ));
}

#[tokio::test]
async fn trashing_an_inbox_item_updates_the_review_count() {
    let mut h = harness().await;
    h.write("Inbox/Folder/Our Wedding.mp4", &bytes("wedding"));
    let ids = h.lib.sort_path("Inbox/Folder", None).await.unwrap();
    assert!(h.exists("Inbox/Folder/Our Wedding.mp4"));
    h.drain_events();
    h.lib.trash_item(&ids[0], None).await.unwrap();
    assert!(
        h.drain_events()
            .iter()
            .any(|e| matches!(e, Event::Inbox { count: 0 }))
    );
    assert!(
        !h.exists("Inbox/Folder"),
        "the emptied Inbox folder is cleaned up"
    );
    let restored = h.lib.restore_item(&ids[0]).await.unwrap();
    assert_eq!(
        restored.status,
        ItemStatus::NeedsReview,
        "back in the Inbox, back in review"
    );
}

#[tokio::test]
async fn watcher_indexes_sorts_and_marks_missing() {
    let h = harness().await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let options = WatchOptions {
        tick: Duration::from_millis(100),
        stable_for: Duration::from_millis(400),
        rescan_every: Duration::from_secs(3600),
    };
    let task = spawn_with(h.lib.clone(), cancel.clone(), options);
    // Give inotify a moment to register the watches.
    tokio::time::sleep(Duration::from_millis(300)).await;

    h.write("Movies/Up (2009)/Up (2009).mkv", &bytes("up"));
    h.write("Inbox/drop.mkv", &bytes("drop"));
    let mut indexed = None;
    for _ in 0..80 {
        indexed = repo::get_by_path(&h.lib.db, "Movies/Up (2009)/Up (2009).mkv")
            .await
            .unwrap();
        let sorts = h.jobs("sort").await;
        if indexed.is_some() && !sorts.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let up = indexed.expect("the watcher indexed the new movie");
    assert_eq!(up.title, "Up");
    let sorts = h.jobs("sort").await;
    assert_eq!(sorts.len(), 1);
    assert_eq!(sorts[0].1["rel_path"], "Inbox/drop.mkv");

    std::fs::remove_file(h.root.join("Movies/Up (2009)/Up (2009).mkv")).unwrap();
    let mut status = ItemStatus::Ready;
    for _ in 0..50 {
        status = h.item(&up.id).await.status;
        if status == ItemStatus::Missing {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(status, ItemStatus::Missing);
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
}

impl Harness {
    async fn add_owner(&self) {
        repo::tests::add_user(&self.lib.db, "owner").await;
    }
}

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
        && std::process::Command::new("ffprobe")
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success())
}

/// A one-second clip with iPhone tags, generated with ffmpeg.
fn make_phone_clip(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=1:size=64x48:rate=10",
        ])
        .args([
            "-metadata",
            "com.apple.quicktime.make=Apple",
            "-metadata",
            "com.apple.quicktime.model=iPhone 15 Pro",
        ])
        .args([
            "-metadata",
            "creation_time=2024-06-13T10:21:00Z",
            "-movflags",
            "use_metadata_tags",
        ])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn phone_video_is_sorted_by_its_metadata() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg/ffprobe not installed");
        return;
    }
    let h = harness().await;
    // The name says nothing; the tags say iPhone.
    make_phone_clip(&h.root.join("Inbox/Beach day.mov"));
    let ids = h.lib.sort_path("Inbox/Beach day.mov", None).await.unwrap();
    let item = h.item(&ids[0]).await;
    assert_eq!(item.category, Category::HomeVideo);
    assert_eq!(item.status, ItemStatus::Ready);
    assert_eq!(item.rel_path, "Home Videos/2024/06/Beach day.mov");
    assert_eq!(item.camera.as_deref(), Some("Apple iPhone 15 Pro"));
    assert_eq!(item.taken_at, Some(1_718_274_060));
    assert_eq!((item.width, item.height), (Some(64), Some(48)));
    assert!(item.duration_s.is_some_and(|d| d > 0.5));
    assert!(item.media().is_some_and(|m| m.video.is_some()));
}

#[tokio::test]
async fn names_held_by_missing_items_are_not_reused() {
    let h = harness().await;
    // An item whose file vanished still owns its path (it may come back).
    h.write("Movies/Heat (1995)/Heat (1995).mkv", &bytes("original"));
    h.write("Movies/Up (2009)/Up (2009).mkv", &bytes("up"));
    h.lib.rescan().await.unwrap();
    std::fs::remove_file(h.root.join("Movies/Heat (1995)/Heat (1995).mkv")).unwrap();
    assert_eq!(h.lib.rescan().await.unwrap().missing, 1);

    h.write("Inbox/Heat.1995.mkv", &bytes("another copy"));
    let ids = h.lib.sort_path("Inbox/Heat.1995.mkv", None).await.unwrap();
    assert_eq!(
        h.item(&ids[0]).await.rel_path,
        "Movies/Heat (1995)/Heat (1995) (2).mkv"
    );
}

#[tokio::test]
async fn a_changed_inbox_file_goes_back_to_the_sorter() {
    let h = harness().await;
    h.write("Inbox/Our Wedding.mp4", &bytes("wedding"));
    let ids = h
        .lib
        .sort_path("Inbox/Our Wedding.mp4", None)
        .await
        .unwrap();
    assert_eq!(h.item(&ids[0]).await.status, ItemStatus::NeedsReview);
    sqlx::query("UPDATE jobs SET state = 'done'")
        .execute(&h.lib.db)
        .await
        .unwrap();

    h.write_old(
        "Inbox/Our Wedding.mp4",
        b"a re-exported, longer file",
        Duration::from_secs(60),
    );
    let report = h.lib.rescan().await.unwrap();
    assert_eq!(
        (report.queued_for_sort, report.updated),
        (1, 0),
        "{report:?}"
    );
    let queued = h.jobs("sort").await;
    assert_eq!(
        queued.last().unwrap().1["rel_path"],
        "Inbox/Our Wedding.mp4"
    );

    // The sorter looks at it again (same id) instead of skipping it.
    let again = h
        .lib
        .sort_path("Inbox/Our Wedding.mp4", None)
        .await
        .unwrap();
    assert_eq!(again, ids);
    assert_eq!(h.item(&ids[0]).await.size, 26);
}

// ---------------------------------------------------------------------------
// Review regressions

#[tokio::test]
async fn rescan_refuses_to_forget_a_library_that_looks_unmounted() {
    let h = harness().await;
    h.write("Movies/Heat (1995)/Heat (1995).mkv", &bytes("heat"));
    h.write("Photos/2020/01/IMG_0001.jpg", &bytes("photo"));
    assert_eq!(h.lib.rescan().await.unwrap().added, 2);

    // The share drops away: the folder is still there, but every file is gone.
    std::fs::remove_dir_all(h.root.join("Movies")).unwrap();
    std::fs::remove_dir_all(h.root.join("Photos")).unwrap();
    let err = h.lib.rescan().await.unwrap_err();
    assert!(err.to_string().contains("mounted"), "{err}");
    let statuses: Vec<String> = sqlx::query_scalar("SELECT status FROM items")
        .fetch_all(&h.lib.db)
        .await
        .unwrap();
    assert_eq!(
        statuses,
        vec!["ready", "ready"],
        "nothing was marked missing"
    );

    // Even long-missing items are not forgotten while the stash looks empty.
    sqlx::query("UPDATE items SET status = 'missing', missing_since = ?")
        .bind(crate::now() - 30 * 86_400)
        .execute(&h.lib.db)
        .await
        .unwrap();
    assert!(h.lib.rescan().await.is_err());
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM items")
        .fetch_one(&h.lib.db)
        .await
        .unwrap();
    assert_eq!(n, 2);
}

#[tokio::test]
async fn files_that_reappear_before_marking_are_not_marked_missing() {
    let h = harness().await;
    h.write("Movies/Up (2009)/Up (2009).mkv", &bytes("up"));
    h.lib.rescan().await.unwrap();
    let up = repo::get_by_path(&h.lib.db, "Movies/Up (2009)/Up (2009).mkv")
        .await
        .unwrap()
        .unwrap();
    // The rescan's walk did not see the file (it was being moved), but it is there now.
    let marked = super::scanner::mark_vanished(&h.lib, &[(up.id.clone(), up.rel_path.clone())])
        .await
        .unwrap();
    assert!(marked.is_empty());
    assert_eq!(h.item(&up.id).await.status, ItemStatus::Ready);
    // A row that moved on since the walk is left alone as well.
    std::fs::remove_file(h.root.join("Movies/Up (2009)/Up (2009).mkv")).unwrap();
    let marked = super::scanner::mark_vanished(&h.lib, &[(up.id.clone(), "Movies/old.mkv".into())])
        .await
        .unwrap();
    assert!(marked.is_empty());
    let marked = super::scanner::mark_vanished(&h.lib, &[(up.id.clone(), up.rel_path.clone())])
        .await
        .unwrap();
    assert_eq!(
        marked.into_iter().map(|(id, _)| id).collect::<Vec<_>>(),
        vec![up.id.clone()]
    );
    assert_eq!(h.item(&up.id).await.status, ItemStatus::Missing);
}

#[tokio::test]
async fn rescans_enforce_the_trash_retention() {
    let h = harness().await;
    h.write("Movies/Heat (1995)/Heat (1995).mkv", &bytes("heat"));
    h.write("Movies/Up (2009)/Up (2009).mkv", &bytes("up"));
    h.lib.rescan().await.unwrap();
    let heat = repo::get_by_path(&h.lib.db, "Movies/Heat (1995)/Heat (1995).mkv")
        .await
        .unwrap()
        .unwrap();
    h.lib.trash_item(&heat.id, None).await.unwrap();
    let trashed_rel = h.item(&heat.id).await.rel_path;
    assert!(h.exists(&trashed_rel));

    // Recently trashed: kept.
    h.lib.rescan().await.unwrap();
    assert!(repo::get(&h.lib.db, &heat.id).await.unwrap().is_some());
    // Older than the retention (30 days by default): deleted for good.
    sqlx::query("UPDATE trash SET trashed_at = ?")
        .bind(crate::now() - 31 * 86_400)
        .execute(&h.lib.db)
        .await
        .unwrap();
    h.lib.rescan().await.unwrap();
    assert!(repo::get(&h.lib.db, &heat.id).await.unwrap().is_none());
    assert!(!h.exists(&trashed_rel));
    assert!(h.exists("Movies/Up (2009)/Up (2009).mkv"));
}

#[tokio::test]
async fn sidecars_go_with_the_video_they_belong_to() {
    let h = harness().await;
    h.write("Inbox/Heat.1995.mkv", &bytes("heat"));
    h.write("Inbox/Heat.1995.srt", b"plain subs");
    h.write("Inbox/Heat.1995.1080p.mkv", &bytes("heat hd"));
    h.write("Inbox/Heat.1995.1080p.srt", b"hd subs");
    h.write("Inbox/Heat.1995.1080p.en.forced.srt", b"hd forced");

    let ids = h.lib.sort_path("Inbox/Heat.1995.mkv", None).await.unwrap();
    assert_eq!(
        h.item(&ids[0]).await.rel_path,
        "Movies/Heat (1995)/Heat (1995).mkv"
    );
    assert_eq!(
        std::fs::read(h.root.join("Movies/Heat (1995)/Heat (1995).srt")).unwrap(),
        b"plain subs"
    );
    assert!(
        h.exists("Inbox/Heat.1995.1080p.srt") && h.exists("Inbox/Heat.1995.1080p.en.forced.srt"),
        "the 1080p file's subtitles stay with it"
    );

    let ids = h
        .lib
        .sort_path("Inbox/Heat.1995.1080p.mkv", None)
        .await
        .unwrap();
    assert_eq!(
        h.item(&ids[0]).await.rel_path,
        "Movies/Heat (1995)/Heat (1995) (2).mkv"
    );
    assert_eq!(
        std::fs::read(h.root.join("Movies/Heat (1995)/Heat (1995) (2).srt")).unwrap(),
        b"hd subs"
    );
    assert!(h.exists("Movies/Heat (1995)/Heat (1995) (2).en.forced.srt"));

    // `name.ext.xmp` belongs to that exact file, not to a sibling with the same stem.
    h.write_old("Inbox/IMG_1234.JPG", &bytes("jpg"), Duration::from_secs(60));
    h.write_old(
        "Inbox/IMG_1234.HEIC",
        &bytes("heic"),
        Duration::from_secs(60),
    );
    h.write("Inbox/IMG_1234.HEIC.xmp", b"heic edits");
    let ids = h.lib.sort_path("Inbox/IMG_1234.JPG", None).await.unwrap();
    let jpg = h.item(&ids[0]).await;
    assert!(jpg.rel_path.starts_with("Photos/"), "{}", jpg.rel_path);
    assert!(h.exists("Inbox/IMG_1234.HEIC.xmp"));
    let ids = h.lib.sort_path("Inbox/IMG_1234.HEIC", None).await.unwrap();
    let heic = h.item(&ids[0]).await;
    assert!(
        h.exists(&format!("{}.xmp", heic.rel_path)),
        "{}",
        heic.rel_path
    );
}

#[tokio::test]
async fn inbox_files_with_sample_in_the_name_are_sorted() {
    let h = harness().await;
    h.write("Inbox/Sample of grandmas cooking.mp4", &bytes("cooking"));
    let ids = h
        .lib
        .sort_path("Inbox/Sample of grandmas cooking.mp4", None)
        .await
        .unwrap();
    assert_eq!(
        ids.len(),
        1,
        "only torrent samples in Completed are skipped"
    );
    // The rescan sees it too.
    std::fs::remove_file(h.root.join("Inbox/Sample of grandmas cooking.mp4")).unwrap();
    h.write_old(
        "Inbox/sample day.mp4",
        &bytes("day"),
        Duration::from_secs(60),
    );
    assert_eq!(h.lib.rescan().await.unwrap().queued_for_sort, 1);
}

#[tokio::test]
async fn a_moved_back_file_keeps_its_own_name_and_id() {
    let h = harness().await;
    h.write("Movies/Heat (1995)/Heat (1995).mkv", &bytes("heat"));
    h.lib.rescan().await.unwrap();
    let heat = repo::get_by_path(&h.lib.db, "Movies/Heat (1995)/Heat (1995).mkv")
        .await
        .unwrap()
        .unwrap();
    // The owner drags it into the Inbox to have it sorted again.
    std::fs::rename(
        h.root.join("Movies/Heat (1995)/Heat (1995).mkv"),
        h.root.join("Inbox/Heat.1995.mkv"),
    )
    .unwrap();
    assert_eq!(h.lib.rescan().await.unwrap().missing, 1);
    let ids = h.lib.sort_path("Inbox/Heat.1995.mkv", None).await.unwrap();
    assert_eq!(ids, vec![heat.id.clone()]);
    assert_eq!(
        h.item(&heat.id).await.rel_path,
        "Movies/Heat (1995)/Heat (1995).mkv",
        "its old name is free again, no ' (2)'"
    );
}

#[tokio::test]
async fn sorting_does_not_undo_a_concurrent_organize() {
    let h = harness().await;
    h.write("Inbox/Our Wedding.mp4", &bytes("wedding"));
    let ids = h
        .lib
        .sort_path("Inbox/Our Wedding.mp4", None)
        .await
        .unwrap();
    let id = ids[0].clone();
    // The file changes, so the sorter will look at it again.
    h.write_old(
        "Inbox/Our Wedding.mp4",
        b"re-exported wedding video",
        Duration::from_secs(60),
    );

    // An organize (simulated by hand while holding the move lock) lands while the sorter works.
    let guard = h.lib.lock_moves().await.unwrap();
    let lib = h.lib.clone();
    let sort = tokio::spawn(async move { lib.sort_path("Inbox/Our Wedding.mp4", None).await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    std::fs::create_dir_all(h.root.join("Home Videos/2024/06")).unwrap();
    std::fs::rename(
        h.root.join("Inbox/Our Wedding.mp4"),
        h.root.join("Home Videos/2024/06/Our Wedding.mp4"),
    )
    .unwrap();
    sqlx::query(
        "UPDATE items SET rel_path = 'Home Videos/2024/06/Our Wedding.mp4', status = 'ready',
            category = 'home_video', updated_at = updated_at + 1 WHERE id = ?",
    )
    .bind(&id)
    .execute(&h.lib.db)
    .await
    .unwrap();
    drop(guard);
    sort.await.unwrap().unwrap();

    let item = h.item(&id).await;
    assert_eq!(item.rel_path, "Home Videos/2024/06/Our Wedding.mp4");
    assert_eq!(item.status, ItemStatus::Ready);
    assert_eq!(item.category, Category::HomeVideo);
}

#[tokio::test]
async fn organize_refuses_overlong_titles() {
    let h = harness().await;
    h.write("Family/clip.mp4", &bytes("clip"));
    h.lib.rescan().await.unwrap();
    let clip = repo::get_by_path(&h.lib.db, "Family/clip.mp4")
        .await
        .unwrap()
        .unwrap();
    let err = h
        .lib
        .organize(
            &clip.id,
            OrganizeRequest {
                category: Some(Category::Movie),
                title: Some("x".repeat(600)),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, crate::CoreError::Invalid(_)), "{err}");
    assert!(h.exists("Family/clip.mp4"));
}

#[tokio::test]
async fn a_sorted_download_has_no_leftover_error() {
    let h = harness().await;
    h.write("Downloads/Completed/Arrival.2016.mkv", &bytes("arrival"));
    sqlx::query(
        "INSERT INTO downloads (id, kind, name, source, state, error, created_at, updated_at, rel_path)
         VALUES ('dl', 'http', 'Arrival', 'https://example.com/a.mkv', 'failed', 'An older failure.', 1, 1,
                 'Downloads/Completed/Arrival.2016.mkv')",
    )
    .execute(&h.lib.db)
    .await
    .unwrap();
    h.lib
        .sort_path("Downloads/Completed/Arrival.2016.mkv", Some("dl"))
        .await
        .unwrap();
    let (state, error): (String, Option<String>) =
        sqlx::query_as("SELECT state, error FROM downloads WHERE id = 'dl'")
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
    assert_eq!((state.as_str(), error), ("sorted", None));
}

#[tokio::test]
async fn a_flood_of_changes_falls_back_to_a_rescan() {
    let h = harness().await;
    let mut pending = std::collections::HashMap::new();
    let mut event = notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::File));
    for n in 0..20_001 {
        event = event.add_path(h.root.join(format!("Movies/file{n}.mkv")));
    }
    super::watcher::handle_event(&h.lib, &mut pending, Ok(event)).await;
    assert!(pending.is_empty(), "single paths are no longer tracked");
    assert_eq!(h.jobs("scan").await.len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn unreadable_paths_are_not_marked_missing() {
    let h = harness().await;
    // `Movies/loop` is a symlink to itself: anything below it cannot be read (ELOOP).
    std::os::unix::fs::symlink("loop", h.root.join("Movies/loop")).unwrap();
    let mut row = repo::tests::row("u1", "Unreadable");
    row.rel_path = "Movies/loop/x.mkv".into();
    assert!(repo::insert_item(&h.lib.db, &row).await.unwrap());

    let mut pending = std::collections::HashMap::new();
    let event = notify::Event::new(notify::EventKind::Remove(notify::event::RemoveKind::File))
        .add_path(h.root.join("Movies/loop/x.mkv"));
    super::watcher::handle_event(&h.lib, &mut pending, Ok(event)).await;
    super::watcher::process_pending(&h.lib, &mut pending, Duration::ZERO).await;
    assert_eq!(h.item("u1").await.status, ItemStatus::Ready);
    assert_eq!(
        super::scanner::mark_missing(&h.lib, "Movies/loop/x.mkv")
            .await
            .unwrap(),
        0
    );
    assert_eq!(h.item("u1").await.status, ItemStatus::Ready);
}

#[tokio::test]
async fn the_inbox_count_follows_review_items_that_vanish_and_return() {
    let mut h = harness().await;
    h.write("Inbox/Our Wedding.mp4", &bytes("wedding"));
    let ids = h
        .lib
        .sort_path("Inbox/Our Wedding.mp4", None)
        .await
        .unwrap();
    let item = h.item(&ids[0]).await;
    h.drain_events();

    std::fs::remove_file(h.root.join("Inbox/Our Wedding.mp4")).unwrap();
    assert_eq!(
        super::scanner::mark_missing(&h.lib, "Inbox/Our Wedding.mp4")
            .await
            .unwrap(),
        1
    );
    assert!(
        h.drain_events()
            .iter()
            .any(|e| matches!(e, Event::Inbox { count: 0 }))
    );

    // The same file comes back: it waits for review again and the count says so.
    let path = h.write("Inbox/Our Wedding.mp4", &bytes("wedding"));
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(item.mtime as u64))
        .unwrap();
    let back = h
        .lib
        .index_path("Inbox/Our Wedding.mp4")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((back.id, back.status), (item.id, ItemStatus::NeedsReview));
    assert!(
        h.drain_events()
            .iter()
            .any(|e| matches!(e, Event::Inbox { count: 1 }))
    );
}

#[tokio::test]
async fn an_ignored_file_stays_put_even_when_it_changes() {
    let h = harness().await;
    h.lib
        .settings
        .update(serde_json::json!({"auto_sort": false}))
        .await
        .unwrap();
    h.write("Inbox/Heat.1995.mkv", &bytes("heat"));
    let ids = h.lib.sort_path("Inbox/Heat.1995.mkv", None).await.unwrap();
    h.lib.review(&ids[0], ReviewAction::Ignore).await.unwrap();
    h.lib
        .settings
        .update(serde_json::json!({"auto_sort": true}))
        .await
        .unwrap();

    // The file changes after the owner said "leave it".
    h.write_old(
        "Inbox/Heat.1995.mkv",
        b"a longer heat file",
        Duration::from_secs(60),
    );
    let report = h.lib.rescan().await.unwrap();
    assert_eq!(
        (report.updated, report.queued_for_sort),
        (1, 0),
        "{report:?}"
    );
    assert!(
        h.lib
            .sort_path("Inbox/Heat.1995.mkv", None)
            .await
            .unwrap()
            .is_empty()
    );
    let item = h.item(&ids[0]).await;
    assert_eq!(item.rel_path, "Inbox/Heat.1995.mkv");
    assert_eq!(item.status, ItemStatus::Ready);
    assert_eq!(item.size, 18, "the file facts are refreshed in place");
    assert!(h.exists("Inbox/Heat.1995.mkv"));
}

// ---------------------------------------------------------------------------
// What gets sorted (audit S2, S3, S4)

/// A document, archive or anything else that is not a video, photo or song is filed
/// under Other/ after one review, so the family can reach it. Inside a download that
/// holds a video, the release's other files stay behind, and subtitles still travel
/// with their video.
#[tokio::test]
async fn files_that_are_not_media_wait_for_review_under_other() {
    let mut h = harness().await;
    h.write("Inbox/bank-statement.pdf", b"%PDF-1.7 statement");
    h.write("Inbox/Family archive.zip", b"PK\x03\x04 archive");
    h.write("Downloads/Completed/tax-form.pdf", b"%PDF-1.4 form");
    sqlx::query(
        "INSERT INTO downloads (id, kind, name, source, state, created_at, updated_at, rel_path)
         VALUES ('up1', 'upload', 'tax-form.pdf', 'tax-form.pdf', 'sorting', 1, 1, 'Downloads/Completed/tax-form.pdf')",
    )
    .execute(&h.lib.db)
    .await
    .unwrap();

    let mut waiting = Vec::new();
    for (rel, download) in [
        ("Inbox/bank-statement.pdf", None),
        ("Inbox/Family archive.zip", None),
        ("Downloads/Completed/tax-form.pdf", Some("up1")),
    ] {
        let ids = h.lib.sort_path(rel, download).await.unwrap();
        assert_eq!(ids.len(), 1, "{rel} becomes an item");
        let item = h.item(&ids[0]).await;
        assert_eq!(item.rel_path, rel, "it waits where it is");
        assert_eq!(item.status, ItemStatus::NeedsReview);
        assert_eq!(item.category, Category::Other);
        let suggestion = item.suggestion().unwrap();
        assert_eq!(suggestion.confidence, crate::models::Confidence::Low);
        let name = rel.rsplit('/').next().unwrap();
        assert_eq!(h.destination(&item).await, format!("Other/{name}"));
        waiting.push(item);
    }
    assert!(
        h.drain_events()
            .iter()
            .any(|e| matches!(e, Event::Inbox { count: 3 }))
    );
    let linked: Vec<String> =
        sqlx::query_scalar("SELECT item_id FROM download_items WHERE download_id = 'up1'")
            .fetch_all(&h.lib.db)
            .await
            .unwrap();
    assert_eq!(
        linked,
        vec![waiting[2].id.clone()],
        "the upload shows its item"
    );

    // One tap files it under Other/.
    let accepted = h
        .lib
        .review(
            &waiting[0].id,
            ReviewAction::Accept(OrganizeRequest::default()),
        )
        .await
        .unwrap();
    assert_eq!(
        (accepted.rel_path.as_str(), accepted.status),
        ("Other/bank-statement.pdf", ItemStatus::Ready)
    );
    assert!(h.exists("Other/bank-statement.pdf"));

    // A folder with no media at all: each document is its own item, release clutter is not.
    h.write("Inbox/Tax 2024/return.pdf", b"%PDF return");
    h.write("Inbox/Tax 2024/receipts.docx", b"docx");
    h.write("Inbox/Tax 2024/checksums.sfv", b"sfv");
    let ids = h.lib.sort_path("Inbox/Tax 2024", None).await.unwrap();
    let mut paths = Vec::new();
    for id in &ids {
        paths.push(h.item(id).await.rel_path);
    }
    paths.sort();
    assert_eq!(
        paths,
        vec!["Inbox/Tax 2024/receipts.docx", "Inbox/Tax 2024/return.pdf"]
    );

    // A subtitle copied in next to its video is not an item: it travels with the video,
    // whichever of the two is looked at first.
    h.write("Inbox/Heat.1995.srt", b"subs");
    h.write("Inbox/Heat.1995.mkv", &bytes("heat"));
    assert!(
        h.lib
            .sort_path("Inbox/Heat.1995.srt", None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(h.exists("Inbox/Heat.1995.srt"));
    h.lib.sort_path("Inbox/Heat.1995.mkv", None).await.unwrap();
    assert!(h.exists("Movies/Heat (1995)/Heat (1995).srt"));

    // Inside a download with a video, only the video is sorted.
    let folder = "Downloads/Completed/Arrival.2016.1080p.BluRay.x264-GRP";
    h.write(
        &format!("{folder}/Arrival.2016.1080p.BluRay.x264-GRP.mkv"),
        &bytes("arrival"),
    );
    h.write(
        &format!("{folder}/Arrival.2016.1080p.BluRay.x264-GRP.en.srt"),
        b"subs",
    );
    h.write(&format!("{folder}/RARBG.txt"), b"release group");
    h.write(&format!("{folder}/arrival.sfv"), b"sfv");
    h.write(&format!("{folder}/Visit the site.url"), b"url");
    sqlx::query(
        "INSERT INTO downloads (id, kind, name, source, state, created_at, updated_at, rel_path)
         VALUES ('dl1', 'torrent', 'Arrival', 'magnet:?xt=urn:btih:abc', 'sorting', 1, 1, ?)",
    )
    .bind(folder)
    .execute(&h.lib.db)
    .await
    .unwrap();
    let ids = h.lib.sort_path(folder, Some("dl1")).await.unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(
        h.item(&ids[0]).await.rel_path,
        "Movies/Arrival (2016)/Arrival (2016).mkv"
    );
    assert!(h.exists("Movies/Arrival (2016)/Arrival (2016).en.srt"));
    // The folder changed, so it is sorted again; without its video the leftovers are
    // still the release's, not documents of their own.
    assert!(h.lib.sort_path(folder, None).await.unwrap().is_empty());
    for left in ["RARBG.txt", "arrival.sfv", "Visit the site.url"] {
        let rel = format!("{folder}/{left}");
        assert!(h.exists(&rel), "{left} stays in the download");
        assert!(repo::get_by_path(&h.lib.db, &rel).await.unwrap().is_none());
    }
}

/// Only Inbox/ and Downloads/Completed/ are sorted. Anything else in Downloads/ may
/// belong to another download client (seeding, or waiting for an import), so it is
/// never moved and never indexed.
#[tokio::test]
async fn other_folders_in_downloads_are_left_alone() {
    let h = harness().await;
    let other = "Downloads/Other Client/Some.Movie.2019.1080p.BluRay.x264.mkv";
    h.write_old(other, &bytes("other client"), Duration::from_secs(60));
    std::fs::File::open(h.root.join("Downloads/Other Client"))
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(60))
        .unwrap();
    h.write_old(
        "Downloads/loose.mkv",
        &bytes("loose"),
        Duration::from_secs(60),
    );

    let report = h.lib.rescan().await.unwrap();
    assert_eq!((report.queued_for_sort, report.added), (0, 0), "{report:?}");
    assert!(h.jobs("sort").await.is_empty());
    for rel in [other, "Downloads/loose.mkv", "Downloads/Other Client"] {
        assert!(h.lib.enqueue_sort(rel, None).await.is_err(), "{rel}");
        assert!(h.lib.sort_path(rel, None).await.is_err(), "{rel}");
    }
    assert!(h.lib.index_path(other).await.unwrap().is_none());
    assert!(h.exists(other) && h.exists("Downloads/loose.mkv"));
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM items")
        .fetch_one(&h.lib.db)
        .await
        .unwrap();
    assert_eq!(count, 0);

    // Downloads/Completed is still looked after: a file waiting for review there that
    // changes goes back to the sorter.
    h.write(
        "Downloads/Completed/Some.Movie.1080p.WEB-DL.mkv",
        &bytes("nameless"),
    );
    let ids = h
        .lib
        .sort_path("Downloads/Completed/Some.Movie.1080p.WEB-DL.mkv", None)
        .await
        .unwrap();
    assert_eq!(h.item(&ids[0]).await.status, ItemStatus::NeedsReview);
    sqlx::query("UPDATE jobs SET state = 'done'")
        .execute(&h.lib.db)
        .await
        .unwrap();
    h.write_old(
        "Downloads/Completed/Some.Movie.1080p.WEB-DL.mkv",
        b"a longer, re-downloaded file",
        Duration::from_secs(60),
    );
    let report = h.lib.rescan().await.unwrap();
    assert_eq!(report.queued_for_sort, 1, "{report:?}");
}

/// Posters, fan art and screenshots that come with a video or an album are not family
/// photos. Other pictures in such a download wait for review instead of going straight
/// to Photos; a download that is only pictures still goes to Photos.
#[tokio::test]
async fn artwork_in_downloads_is_not_filed_as_family_photos() {
    let h = harness().await;
    let folder = "Downloads/Completed/Some.Movie.2019.1080p.WEB.x264-GRP";
    for (name, tag) in [
        ("Some.Movie.2019.1080p.WEB.x264-GRP.mkv", "movie"),
        ("poster.jpg", "poster"),
        ("fanart.jpg", "fanart"),
        ("Some.Movie.2019-poster.jpg", "kodi poster"),
        ("screen01.png", "screen"),
        ("www.YTS.MX.jpg", "ad"),
        ("Screens/screen02.png", "screens"),
        ("Screenshots/frame.png", "screenshots"),
        ("Artwork/back.png", "artwork"),
        ("behind the scenes photo.jpg", "unknown"),
    ] {
        h.write_old(
            &format!("{folder}/{name}"),
            &bytes(tag),
            Duration::from_secs(60),
        );
    }
    sqlx::query(
        "INSERT INTO downloads (id, kind, name, source, state, created_at, updated_at, rel_path)
         VALUES ('dl1', 'torrent', 'Some Movie', 'magnet:?xt=urn:btih:abc', 'sorting', 1, 1, ?)",
    )
    .bind(folder)
    .execute(&h.lib.db)
    .await
    .unwrap();
    let ids = h.lib.sort_path(folder, Some("dl1")).await.unwrap();
    assert_eq!(ids.len(), 2, "the movie and one picture to review");
    let mut items = Vec::new();
    for id in &ids {
        items.push(h.item(id).await);
    }
    items.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    assert_eq!(
        items[0].rel_path,
        format!("{folder}/behind the scenes photo.jpg")
    );
    assert_eq!(items[0].status, ItemStatus::NeedsReview);
    assert_eq!(items[0].category, Category::Other);
    let suggestion = items[0].suggestion().unwrap();
    assert_eq!(suggestion.confidence, crate::models::Confidence::Low);
    assert_eq!(
        h.destination(&items[0]).await,
        "Other/behind the scenes photo.jpg"
    );
    assert_eq!(
        items[1].rel_path,
        "Movies/Some Movie (2019)/Some Movie (2019).mkv"
    );
    for left in [
        "poster.jpg",
        "fanart.jpg",
        "Some.Movie.2019-poster.jpg",
        "screen01.png",
        "www.YTS.MX.jpg",
        "Screens/screen02.png",
        "Screenshots/frame.png",
        "Artwork/back.png",
    ] {
        assert!(h.exists(&format!("{folder}/{left}")), "{left} stays");
    }
    let photos_is_empty = || {
        std::fs::read_dir(h.root.join("Photos"))
            .unwrap()
            .next()
            .is_none()
    };
    assert!(photos_is_empty(), "nothing was filed as a family photo");

    // With the movie gone the folder changed and is sorted again: the artwork is still
    // the movie's, not a download of pictures.
    assert!(h.lib.sort_path(folder, None).await.unwrap().is_empty());
    assert!(photos_is_empty());
    assert!(h.exists(&format!("{folder}/poster.jpg")));

    // The leftovers do not send the folder back to the sorter on every rescan once it
    // has settled.
    for entry in walkdir::WalkDir::new(h.root.join(folder)) {
        let entry = entry.unwrap();
        if entry.file_type().is_dir() {
            std::fs::File::open(entry.path())
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(60))
                .unwrap();
        }
    }
    sqlx::query("UPDATE jobs SET state = 'done'")
        .execute(&h.lib.db)
        .await
        .unwrap();
    let report = h.lib.rescan().await.unwrap();
    assert_eq!(
        report.queued_for_sort,
        0,
        "{report:?} {:?}",
        h.jobs("sort").await
    );

    // A release folder copied into Completed by hand has no download to remember it by,
    // but its name still says it is a release: once the movie has moved, the poster
    // and screenshots stay where they are.
    let copied = "Downloads/Completed/Other.Movie.2020.720p.BluRay.x264-GRP";
    h.write(&format!("{copied}/movie.mkv"), &bytes("other movie"));
    h.write(&format!("{copied}/poster.jpg"), &bytes("other poster"));
    h.write(
        &format!("{copied}/Screens/screen01.png"),
        &bytes("other screen"),
    );
    assert_eq!(h.lib.sort_path(copied, None).await.unwrap().len(), 1);
    assert!(h.lib.sort_path(copied, None).await.unwrap().is_empty());
    assert!(h.exists(&format!("{copied}/poster.jpg")));
    assert!(photos_is_empty());

    // An album: the song goes to Other/, its cover art nowhere.
    let album = "Downloads/Completed/Artist - Album (2020) [FLAC]";
    h.write(&format!("{album}/01 - Song.flac"), &bytes("song"));
    h.write(&format!("{album}/cover.jpg"), &bytes("cover"));
    h.write(&format!("{album}/Scans/booklet 1.jpg"), &bytes("scan"));
    let ids = h.lib.sort_path(album, None).await.unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(h.item(&ids[0]).await.rel_path, "Other/01 - Song.flac");

    // A Kodi-style movie folder copied into the Inbox: the poster is not a photo either.
    // Sorted again once the movie has moved, it waits for review rather than going to
    // Photos.
    h.write("Inbox/Up (2009)/Up (2009).mkv", &bytes("up"));
    h.write("Inbox/Up (2009)/poster.jpg", &bytes("up poster"));
    let ids = h.lib.sort_path("Inbox/Up (2009)", None).await.unwrap();
    assert_eq!(ids.len(), 1);
    assert!(h.exists("Inbox/Up (2009)/poster.jpg"));
    let ids = h.lib.sort_path("Inbox/Up (2009)", None).await.unwrap();
    assert_eq!(ids.len(), 1);
    let poster = h.item(&ids[0]).await;
    assert_eq!(
        (poster.rel_path.as_str(), poster.status),
        ("Inbox/Up (2009)/poster.jpg", ItemStatus::NeedsReview)
    );
    assert_eq!(h.destination(&poster).await, "Other/poster.jpg");
    assert!(photos_is_empty());

    // The owner's own pictures next to a phone video are photos.
    h.write("Inbox/Phone/VID_20240613_101500.mp4", &bytes("clip"));
    h.write("Inbox/Phone/IMG_20240613_101700.jpg", &bytes("snap"));
    let ids = h.lib.sort_path("Inbox/Phone", None).await.unwrap();
    assert_eq!(ids.len(), 2);
    assert!(h.exists("Photos/2024/06/IMG_20240613_101700.jpg"));

    // Pictures on their own are photos: a sent image, or an album unpacked.
    h.write(
        "Downloads/Completed/IMG_20230102_030405.jpg",
        &bytes("sent"),
    );
    h.write(
        "Downloads/Completed/Grandma's album/IMG_20190704_120000.jpg",
        &bytes("album 1"),
    );
    h.write(
        "Downloads/Completed/Grandma's album/IMG_20190705_120000.jpg",
        &bytes("album 2"),
    );
    h.lib
        .sort_path("Downloads/Completed/IMG_20230102_030405.jpg", None)
        .await
        .unwrap();
    let ids = h
        .lib
        .sort_path("Downloads/Completed/Grandma's album", None)
        .await
        .unwrap();
    assert_eq!(ids.len(), 2);
    assert!(h.exists("Photos/2023/01/IMG_20230102_030405.jpg"));
    assert!(h.exists("Photos/2019/07/IMG_20190704_120000.jpg"));
    assert!(h.exists("Photos/2019/07/IMG_20190705_120000.jpg"));
}

/// An item counts as gone when a link takes its place (even one to nowhere) or when
/// its folder becomes a link out of the stash, but not when it merely cannot be read.
#[cfg(unix)]
#[tokio::test]
async fn links_in_place_of_items_count_as_gone() {
    let h = harness().await;
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(h.root.join("Movies/A (2020)")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("missing.mkv"),
        h.root.join("Movies/A (2020)/A (2020).mkv"),
    )
    .unwrap();
    assert!(super::scanner::is_gone(&h.lib, "Movies/A (2020)/A (2020).mkv").await);

    std::fs::write(outside.path().join("b.mkv"), b"x").unwrap();
    std::os::unix::fs::symlink(outside.path(), h.root.join("Movies/B")).unwrap();
    assert!(super::scanner::is_gone(&h.lib, "Movies/B/b.mkv").await);

    std::fs::create_dir_all(h.root.join("Movies/C")).unwrap();
    std::fs::write(h.root.join("Movies/C/c.mkv"), b"x").unwrap();
    assert!(!super::scanner::is_gone(&h.lib, "Movies/C/c.mkv").await);
}

// ---------------------------------------------------------------------------
// Libraries the owner defines (sections 3.1 and 4)

mod libraries {
    use std::collections::BTreeMap;

    use super::*;
    use crate::CoreError;
    use crate::library::defs::{
        self, CreateLibrary, EditLibrary, FolderRequest, STARTING_HOME_VIDEOS, STARTING_MOVIES,
        STARTING_SHOWS,
    };
    use crate::models::{Place, Waiting};

    async fn add_owner(h: &Harness) {
        sqlx::query("INSERT OR IGNORE INTO users (id, display_name, role, webauthn_id, created_at) VALUES ('u', 'Ana', 'owner', 'w-u', 0)")
            .execute(&h.lib.db)
            .await
            .unwrap();
    }

    fn new_library(name: &str, structure: Option<&str>, claims: &[&str]) -> CreateLibrary {
        CreateLibrary {
            name: Some(name.into()),
            icon: Some("baby".into()),
            content_type: Some(
                if structure.is_some() {
                    "video"
                } else {
                    "photos"
                }
                .into(),
            ),
            structure: structure.map(str::to_string),
            claims: Some(claims.iter().map(|c| c.to_string()).collect()),
            folder: Some(FolderRequest {
                mode: Some("new".into()),
                name: Some(name.into()),
            }),
            ..Default::default()
        }
    }

    fn claims(list: &[&str]) -> EditLibrary {
        EditLibrary {
            claims: Some(list.iter().map(|c| c.to_string()).collect()),
            ..Default::default()
        }
    }

    /// Every file under the stash (outside `.popcornstash`) with its bytes.
    fn files(h: &Harness) -> BTreeMap<String, Vec<u8>> {
        walkdir::WalkDir::new(&h.root)
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| {
                let rel = h.lib.layout.rel(e.path())?;
                (!rel.starts_with(".popcornstash")).then(|| (rel, std::fs::read(e.path()).unwrap()))
            })
            .collect()
    }

    fn field(err: &CoreError) -> Option<&str> {
        match err {
            CoreError::Rejected { field, .. } => field.as_deref(),
            _ => None,
        }
    }

    /// A new library that claims camera videos takes them; Home Videos keeps its files.
    #[tokio::test]
    async fn sorting_follows_the_claims() {
        let h = harness().await;
        add_owner(&h).await;
        h.write("Home Videos/2023/01/old.mp4", &bytes("old clip"));
        h.lib.rescan().await.unwrap();
        let kids = h
            .lib
            .create_library(new_library("Kids", Some("one_offs"), &["home_video"]), "u")
            .await
            .unwrap();
        let all = defs::load_all(&h.lib.db).await.unwrap();
        let home_videos = all.iter().find(|d| d.id() == STARTING_HOME_VIDEOS).unwrap();
        assert!(home_videos.claims.is_empty(), "the claim moved to Kids");
        assert!(h.exists("Kids"), "a new library's folder is made");

        h.write("Inbox/VID_20240613_101500.mp4", &bytes("clip"));
        let ids = h
            .lib
            .sort_path("Inbox/VID_20240613_101500.mp4", None)
            .await
            .unwrap();
        let clip = h.item(&ids[0]).await;
        assert_eq!(clip.rel_path, "Kids/2024/06/VID_20240613_101500.mp4");
        assert_eq!(
            (clip.place, clip.library_id.as_deref()),
            (Place::Library, Some(kids.id.as_str()))
        );
        assert!(
            h.exists("Home Videos/2023/01/old.mp4"),
            "claims never move files"
        );
        let old = repo::get_by_path(&h.lib.db, "Home Videos/2023/01/old.mp4")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(old.library_id.as_deref(), Some(STARTING_HOME_VIDEOS));

        // Movies still go where they went.
        h.write("Inbox/Heat.1995.1080p.BluRay.x264-GRP.mkv", &bytes("heat"));
        let ids = h
            .lib
            .sort_path("Inbox/Heat.1995.1080p.BluRay.x264-GRP.mkv", None)
            .await
            .unwrap();
        let heat = h.item(&ids[0]).await;
        assert_eq!(heat.rel_path, "Movies/Heat (1995)/Heat (1995).mkv");
        assert_eq!(heat.library_id.as_deref(), Some(STARTING_MOVIES));
    }

    /// A kind no library claims waits in Inbox whatever its confidence, and a person
    /// picks a library that fits.
    #[tokio::test]
    async fn unclaimed_kinds_wait_in_inbox() {
        let h = harness().await;
        add_owner(&h).await;
        h.lib
            .edit_library(STARTING_MOVIES, claims(&[]))
            .await
            .unwrap();
        h.write("Inbox/Heat.1995.1080p.BluRay.x264-GRP.mkv", &bytes("heat"));
        let ids = h
            .lib
            .sort_path("Inbox/Heat.1995.1080p.BluRay.x264-GRP.mkv", None)
            .await
            .unwrap();
        let heat = h.item(&ids[0]).await;
        assert_eq!(heat.status, ItemStatus::NeedsReview);
        assert_eq!(heat.rel_path, "Inbox/Heat.1995.1080p.BluRay.x264-GRP.mkv");
        assert_eq!(heat.place, Place::Incoming);
        let suggestion = h.suggestion(&heat).await;
        assert_eq!(
            suggestion.classification.confidence,
            crate::models::Confidence::High
        );
        assert_eq!(suggestion.place, None);
        assert_eq!(suggestion.library_id, None);
        assert_eq!(suggestion.destination, None);
        assert_eq!(suggestion.waiting, Some(Waiting::ChooseLibrary));

        // Accepting without a library is refused; naming one that fits moves the file.
        let err = h
            .lib
            .review(&heat.id, ReviewAction::Accept(OrganizeRequest::default()))
            .await
            .unwrap_err();
        assert_eq!(field(&err), Some("library_id"), "{err:?}");
        let accepted = h
            .lib
            .review(
                &heat.id,
                ReviewAction::Accept(OrganizeRequest {
                    place: Some(Place::Library),
                    library_id: Some(STARTING_MOVIES.into()),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        assert_eq!(accepted.rel_path, "Movies/Heat (1995)/Heat (1995).mkv");
        assert_eq!(accepted.library_id.as_deref(), Some(STARTING_MOVIES));
        assert_eq!(accepted.status, ItemStatus::Ready);

        // Deleting the library that claims camera videos leaves them unclaimed too.
        h.lib.delete_library(STARTING_HOME_VIDEOS).await.unwrap();
        h.write("Inbox/VID_20240613_101500.mp4", &bytes("clip"));
        let ids = h
            .lib
            .sort_path("Inbox/VID_20240613_101500.mp4", None)
            .await
            .unwrap();
        let clip = h.item(&ids[0]).await;
        assert_eq!(clip.status, ItemStatus::NeedsReview);
        assert!(h.exists("Inbox/VID_20240613_101500.mp4"));
    }

    /// One sort job, one set of claims: when the claims change while a job runs, the
    /// rest of its files wait in Inbox with their suggestion.
    #[tokio::test]
    async fn claims_that_change_during_a_sort_leave_the_rest_waiting() {
        let h = harness().await;
        add_owner(&h).await;
        h.write(
            "Downloads/Completed/pack/Heat.1995.1080p.mkv",
            &bytes("heat"),
        );
        h.write("Downloads/Completed/pack/Up.2009.1080p.mkv", &bytes("up"));
        let started = h.lib.claims_generation();
        h.lib
            .edit_library(STARTING_SHOWS, claims(&[]))
            .await
            .unwrap();
        assert_ne!(h.lib.claims_generation(), started);
        let ids = super::super::organizer::sort_path_as_of(
            &h.lib,
            "Downloads/Completed/pack",
            None,
            started,
        )
        .await
        .unwrap();
        assert_eq!(ids.len(), 2);
        for id in &ids {
            let item = h.item(id).await;
            assert_eq!(item.status, ItemStatus::NeedsReview, "{}", item.rel_path);
            assert!(item.rel_path.starts_with("Downloads/Completed/pack/"));
            let suggestion = h.suggestion(&item).await;
            assert_eq!(suggestion.library_id.as_deref(), Some(STARTING_MOVIES));
            assert_eq!(suggestion.waiting, Some(Waiting::Confirm));
        }
    }

    /// Creating, renaming, reordering, changing and deleting libraries never moves,
    /// renames or deletes a file (section 3, Movement rules).
    #[tokio::test]
    async fn library_writes_never_move_files() {
        let h = harness().await;
        add_owner(&h).await;
        h.write("Family 2019/Heat.1995.1080p.mkv", &bytes("heat"));
        h.write("Family 2019/beach.jpg", &bytes("beach"));
        h.write("Family 2019/notes.txt", b"notes");
        h.write("Movies/Up (2009)/Up (2009).mkv", &bytes("up"));
        h.write("Photos/2020/01/IMG_0001.jpg", &bytes("photo"));
        h.lib.rescan().await.unwrap();
        let before = files(&h);

        let mut family = new_library("Family", Some("one_offs"), &[]);
        family.folder = Some(FolderRequest {
            mode: Some("existing".into()),
            name: Some("Family 2019".into()),
        });
        family.visibility = Some("selected".into());
        let family = h.lib.create_library(family, "u").await.unwrap();
        h.lib
            .edit_library(
                &family.id,
                EditLibrary {
                    name: Some("Holidays".into()),
                    icon: Some("plane".into()),
                    claims: Some(vec!["movie".into()]),
                    visibility: Some("everyone".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let mut order: Vec<String> = defs::load_all(&h.lib.db)
            .await
            .unwrap()
            .iter()
            .map(|d| d.id().to_string())
            .collect();
        order.reverse();
        h.lib.reorder_libraries(&order).await.unwrap();
        h.lib.delete_library(&family.id).await.unwrap();
        h.lib.delete_library(STARTING_MOVIES).await.unwrap();
        h.lib.rescan().await.unwrap();
        assert_eq!(files(&h), before, "every file is where it was, unchanged");
        // Deleted libraries' items are in folders that are not libraries now.
        let up = repo::get_by_path(&h.lib.db, "Movies/Up (2009)/Up (2009).mkv")
            .await
            .unwrap()
            .unwrap();
        assert_eq!((up.place, up.library_id), (Place::Folder, None));
        assert_eq!(up.status, ItemStatus::Ready, "never marked missing");
        // A folder is fixed: an edit naming it is refused.
        let err = h
            .lib
            .edit_library(
                STARTING_SHOWS,
                EditLibrary {
                    folder: Some(Some(serde_json::json!({"name": "TV"}))),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert_eq!(field(&err), Some("folder"));
    }

    /// Making a library from an existing folder: what it indexes joins it (ids,
    /// favourites and progress kept), the rest leaves the index, and the files stay.
    #[tokio::test]
    async fn a_library_made_from_a_folder_takes_in_its_items() {
        let h = harness().await;
        add_owner(&h).await;
        h.write("Cartoons/Bluey/Season 01/Bluey - S01E01.mkv", &bytes("e1"));
        h.write("Cartoons/Bluey/cover.jpg", &bytes("cover"));
        h.lib.rescan().await.unwrap();
        let episode = repo::get_by_path(&h.lib.db, "Cartoons/Bluey/Season 01/Bluey - S01E01.mkv")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(episode.place, Place::Folder);
        sqlx::query("UPDATE items SET favorite = 1 WHERE id = ?")
            .bind(&episode.id)
            .execute(&h.lib.db)
            .await
            .unwrap();
        repo::set_progress(&h.lib.db, "u", &episode.id, 100.0, 600.0)
            .await
            .unwrap();

        let mut request = new_library("Cartoons", Some("episodes"), &[]);
        request.folder = Some(FolderRequest {
            mode: Some("existing".into()),
            name: Some("Cartoons".into()),
        });
        // An existing folder never becomes visible to everyone by default.
        let err = h
            .lib
            .create_library(request.clone(), "u")
            .await
            .unwrap_err();
        assert_eq!(field(&err), Some("visibility"));
        request.visibility = Some("selected".into());
        let library = h.lib.create_library(request, "u").await.unwrap();

        let joined = h.item(&episode.id).await;
        assert_eq!(joined.library_id.as_deref(), Some(library.id.as_str()));
        assert_eq!(joined.category, Category::Episode);
        assert_eq!(joined.show_name.as_deref(), Some("Bluey"));
        assert!(joined.favorite, "the favourite stays");
        let progress: i64 =
            sqlx::query_scalar("SELECT count(*) FROM item_progress WHERE item_id = ?")
                .bind(&episode.id)
                .fetch_one(&h.lib.db)
                .await
                .unwrap();
        assert_eq!(progress, 1, "the resume position stays");
        assert!(
            repo::get_by_path(&h.lib.db, "Cartoons/Bluey/cover.jpg")
                .await
                .unwrap()
                .is_none(),
            "an Episodes library does not show pictures"
        );
        assert!(h.exists("Cartoons/Bluey/cover.jpg"), "the file stays");
        // The same folder cannot be a library twice, in any case.
        let mut again = new_library("Again", Some("episodes"), &[]);
        again.folder = Some(FolderRequest {
            mode: Some("existing".into()),
            name: Some("cartoons".into()),
        });
        again.visibility = Some("everyone".into());
        let err = h.lib.create_library(again, "u").await.unwrap_err();
        assert!(matches!(field(&err), Some("folder.name")), "{err:?}");
    }

    /// A library whose folder is gone (an unplugged disk) keeps its items, is not
    /// recreated, and new files of its kinds wait in Inbox.
    #[tokio::test]
    async fn a_missing_library_folder_keeps_its_items_and_takes_nothing() {
        let h = harness().await;
        add_owner(&h).await;
        h.write("Movies/Up (2009)/Up (2009).mkv", &bytes("up"));
        h.write("Photos/2020/01/IMG_0001.jpg", &bytes("photo"));
        h.lib.rescan().await.unwrap();
        std::fs::remove_dir_all(h.root.join("Movies")).unwrap();
        h.lib.rescan().await.unwrap();
        let up = repo::get_by_path(&h.lib.db, "Movies/Up (2009)/Up (2009).mkv")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(up.status, ItemStatus::Ready, "nothing is marked missing");

        h.write("Inbox/Heat.1995.1080p.BluRay.x264-GRP.mkv", &bytes("heat"));
        let ids = h
            .lib
            .sort_path("Inbox/Heat.1995.1080p.BluRay.x264-GRP.mkv", None)
            .await
            .unwrap();
        let heat = h.item(&ids[0]).await;
        assert_eq!(heat.status, ItemStatus::NeedsReview);
        assert!(!h.exists("Movies"), "the folder is not made again");
        assert_eq!(
            h.suggestion(&heat).await.waiting,
            Some(Waiting::FolderUnavailable)
        );
        let err = h
            .lib
            .review(
                &heat.id,
                ReviewAction::Accept(OrganizeRequest {
                    place: Some(Place::Library),
                    library_id: Some(STARTING_MOVIES.into()),
                    ..Default::default()
                }),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                CoreError::Rejected {
                    kind: crate::Rejection::Conflict,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    /// A restore into a library that now reads the file as another kind keeps the time
    /// its EXIF or tags gave.
    #[tokio::test]
    async fn a_restore_that_changes_the_kind_keeps_the_capture_time() {
        let h = harness().await;
        add_owner(&h).await;
        h.write("Home Videos/2024/06/IMG_0001.MOV", &bytes("clip"));
        h.lib.rescan().await.unwrap();
        let clip = repo::get_by_path(&h.lib.db, "Home Videos/2024/06/IMG_0001.MOV")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(clip.category, Category::HomeVideo);
        set_taken(
            &h,
            &clip.id,
            1_719_757_800,
            "2024-06-30T23:30:00",
            Some(540),
        )
        .await;
        h.lib.trash_item(&clip.id, Some("u")).await.unwrap();

        // Home Videos is remade as a photos library on the same folder.
        h.lib.delete_library(STARTING_HOME_VIDEOS).await.unwrap();
        std::fs::create_dir_all(h.root.join("Home Videos")).unwrap();
        let mut camera = new_library("Camera roll", None, &[]);
        camera.folder = Some(FolderRequest {
            mode: Some("existing".into()),
            name: Some("Home Videos".into()),
        });
        camera.visibility = Some("selected".into());
        let camera = h.lib.create_library(camera, "u").await.unwrap();
        sqlx::query("DELETE FROM jobs")
            .execute(&h.lib.db)
            .await
            .unwrap();

        let restored = h.lib.restore_item(&clip.id).await.unwrap();
        assert_eq!(restored.rel_path, "Home Videos/2024/06/IMG_0001.MOV");
        assert_eq!(restored.library_id.as_deref(), Some(camera.id.as_str()));
        assert_eq!(restored.category, Category::Photo);
        assert_eq!(
            taken_of(&restored),
            (Some(1_719_757_800), Some("2024-06-30T23:30:00"), Some(540))
        );
        assert!(h.jobs("probe").await.is_empty(), "nothing to read again");
    }

    /// Restoring a file its library would no longer show is refused before anything moves.
    #[tokio::test]
    async fn restore_is_refused_when_the_library_would_not_show_the_file() {
        let h = harness().await;
        add_owner(&h).await;
        h.write("Home Videos/2024/06/IMG_1.jpg", &bytes("photo"));
        h.write("Home Videos/2024/06/VID_1.mp4", &bytes("clip"));
        h.lib.rescan().await.unwrap();
        let photo = repo::get_by_path(&h.lib.db, "Home Videos/2024/06/IMG_1.jpg")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(photo.category, Category::HomeVideo);
        h.lib.trash_item(&photo.id, Some("u")).await.unwrap();
        assert_eq!(h.lib.restore_refusal(&photo.id).await.unwrap(), None);
        h.lib
            .edit_library(STARTING_HOME_VIDEOS, claims(&["movie"]))
            .await
            .unwrap();
        let refusal = h.lib.restore_refusal(&photo.id).await.unwrap();
        assert_eq!(
            refusal.as_deref(),
            Some("Home Videos does not show this kind of file any more, so it stays in the trash.")
        );
        let err = h.lib.restore_item(&photo.id).await.unwrap_err();
        assert!(matches!(err, CoreError::Conflict(_)), "{err:?}");
        assert_eq!(h.item(&photo.id).await.status, ItemStatus::Trashed);
        assert!(!h.exists("Home Videos/2024/06/IMG_1.jpg"));
    }

    /// Accept and Organize: the file must fit the library it goes to.
    #[tokio::test]
    async fn targets_must_fit_the_file() {
        let h = harness().await;
        add_owner(&h).await;
        h.write("Inbox/notes.pdf", b"notes");
        h.write("Inbox/beach.jpg", &bytes("beach"));
        h.lib
            .settings
            .update(serde_json::json!({"auto_sort": false}))
            .await
            .unwrap();
        let notes = h.lib.sort_path("Inbox/notes.pdf", None).await.unwrap()[0].clone();
        let beach = h.lib.sort_path("Inbox/beach.jpg", None).await.unwrap()[0].clone();
        let into = |id: &str| OrganizeRequest {
            place: Some(Place::Library),
            library_id: Some(id.into()),
            ..Default::default()
        };
        let err = h
            .lib
            .review(&notes, ReviewAction::Accept(into(STARTING_MOVIES)))
            .await
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "Only videos and photos go in a library. This file goes in Other files."
        );
        let err = h
            .lib
            .review(&beach, ReviewAction::Accept(into(STARTING_MOVIES)))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "Photos go in a photos library.");
        assert_eq!(field(&err), Some("library_id"));
        let err = h
            .lib
            .review(&beach, ReviewAction::Accept(into("no-such-library")))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            CoreError::Rejected {
                kind: crate::Rejection::NotFound,
                ..
            }
        ));
        let ok = h
            .lib
            .review(
                &notes,
                ReviewAction::Accept(OrganizeRequest {
                    place: Some(Place::OtherFiles),
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        assert_eq!(
            (ok.rel_path.as_str(), ok.place),
            ("Other/notes.pdf", Place::OtherFiles)
        );
        // Accepting again: it is no longer waiting.
        let err = h
            .lib
            .review(&notes, ReviewAction::Ignore)
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "That file was already sorted.");
    }

    async fn chosen_download(h: &Harness, id: &str, library: &str, user: &str) {
        sqlx::query("INSERT INTO downloads (id, kind, name, source, state, created_at, updated_at, created_by, library_id) VALUES (?, 'upload', 'test', '', 'sorting', 0, 0, ?, ?)")
            .bind(id).bind(user).bind(library).execute(&h.lib.db).await.unwrap();
    }

    #[tokio::test]
    async fn download_destination_overrides_only_compatible_files() {
        let h = harness().await;
        add_owner(&h).await;
        let kids = h
            .lib
            .create_library(new_library("Kids", Some("one_offs"), &[]), "u")
            .await
            .unwrap();
        chosen_download(&h, "chosen", &kids.id, "u").await;
        h.write(
            "Downloads/Completed/batch/Heat.1995.1080p.mkv",
            &bytes("film"),
        );
        h.write(
            "Downloads/Completed/batch/Lantern.S01E02.mkv",
            &bytes("episode"),
        );
        h.lib
            .sort_path("Downloads/Completed/batch", Some("chosen"))
            .await
            .unwrap();
        assert!(h.exists("Kids/Heat (1995)/Heat (1995).mkv"));
        assert!(h.exists("Shows/Lantern/Season 01/Lantern - S01E02.mkv"));
        // A choice never transfers the household's movie claim.
        assert!(
            defs::load_one(&h.lib.db, STARTING_MOVIES)
                .await
                .unwrap()
                .unwrap()
                .claims(Category::Movie)
        );
    }

    #[tokio::test]
    async fn hidden_or_deleted_destination_falls_back_to_claims() {
        let h = harness().await;
        add_owner(&h).await;
        sqlx::query("INSERT INTO users (id, display_name, role, webauthn_id, created_at) VALUES ('m', 'Member', 'member', 'w-m', 0)")
            .execute(&h.lib.db).await.unwrap();
        let kids = h
            .lib
            .create_library(new_library("Kids", Some("one_offs"), &[]), "u")
            .await
            .unwrap();
        chosen_download(&h, "hidden", &kids.id, "m").await;
        h.lib
            .edit_library(
                &kids.id,
                EditLibrary {
                    visibility: Some("selected".into()),
                    member_ids: Some(vec![]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        h.write("Downloads/Completed/Heat.1995.1080p.mkv", &bytes("hidden"));
        h.lib
            .sort_path("Downloads/Completed/Heat.1995.1080p.mkv", Some("hidden"))
            .await
            .unwrap();
        assert!(h.exists("Movies/Heat (1995)/Heat (1995).mkv"));
        // Revoked access also prevents a choice from identifying an unknown clip.
        let rel = "Downloads/Completed/clip_final_v2.mkv";
        h.write(rel, &bytes("hidden unidentified"));
        let id = h.lib.sort_path(rel, Some("hidden")).await.unwrap()[0].clone();
        let item = h.item(&id).await;
        assert_eq!(item.status, ItemStatus::NeedsReview);
        assert_eq!(item.category, Category::Other);
        assert!(h.exists(rel));
        chosen_download(&h, "deleted", &kids.id, "u").await;
        h.lib.delete_library(&kids.id).await.unwrap();
        let value: Option<String> =
            sqlx::query_scalar("SELECT library_id FROM downloads WHERE id='deleted'")
                .fetch_one(&h.lib.db)
                .await
                .unwrap();
        assert_eq!(value, None);
        h.write("Downloads/Completed/Up.2009.1080p.mkv", &bytes("deleted"));
        h.lib
            .sort_path("Downloads/Completed/Up.2009.1080p.mkv", Some("deleted"))
            .await
            .unwrap();
        assert!(h.exists("Movies/Up (2009)/Up (2009).mkv"));
    }

    #[tokio::test]
    async fn chosen_destination_with_unavailable_folder_survives_history_removal() {
        let h = harness().await;
        add_owner(&h).await;
        let kids = h
            .lib
            .create_library(new_library("Kids", Some("one_offs"), &[]), "u")
            .await
            .unwrap();
        // An unavailable destination still waits, even after an explicit choice.
        // A file at the folder path prevents an empty library being recreated.
        std::fs::remove_dir(h.lib.layout.root().join("Kids")).unwrap();
        std::fs::write(h.lib.layout.root().join("Kids"), b"unavailable").unwrap();
        chosen_download(&h, "waiting", &kids.id, "u").await;
        h.write("Downloads/Completed/Heat.1995.1080p.mkv", &bytes("waiting"));
        let id = h
            .lib
            .sort_path("Downloads/Completed/Heat.1995.1080p.mkv", Some("waiting"))
            .await
            .unwrap()[0]
            .clone();
        sqlx::query("DELETE FROM downloads WHERE id='waiting'")
            .execute(&h.lib.db)
            .await
            .unwrap();
        let item = h.item(&id).await;
        assert_eq!(item.status, ItemStatus::NeedsReview);
        assert_eq!(
            h.suggestion(&item).await.library_id.as_deref(),
            Some(kids.id.as_str())
        );
        assert!(h.destination(&item).await.starts_with("Kids/"));
        // Someone else's incoming file is offered only to their visible default claim.
        let viewer = crate::library::access::Viewer {
            user_id: "someone-else".into(),
            role: crate::models::Role::Member,
            full: true,
        };
        let access = crate::library::access::Access::load(&h.lib.db, &viewer)
            .await
            .unwrap();
        let definitions = defs::load_all(&h.lib.db).await.unwrap();
        assert!(
            crate::library::destinations::suggested(&item, &access, &definitions, Category::Movie)
                .is_none()
        );
    }

    #[tokio::test]
    async fn chosen_destination_moves_uncertain_files_even_with_auto_sort_off() {
        for auto_sort in [true, false] {
            let h = harness().await;
            add_owner(&h).await;
            let kids = h
                .lib
                .create_library(new_library("Kids", Some("one_offs"), &[]), "u")
                .await
                .unwrap();
            h.lib
                .settings
                .update(serde_json::json!({"auto_sort": auto_sort}))
                .await
                .unwrap();
            chosen_download(&h, "chosen", &kids.id, "u").await;
            let rel = "Downloads/Completed/Heat.1080p.WEB-DL.mkv";
            h.write(rel, &bytes("uncertain chosen"));
            let id = h.lib.sort_path(rel, Some("chosen")).await.unwrap()[0].clone();
            let item = h.item(&id).await;
            assert_eq!(item.status, ItemStatus::Ready);
            assert_eq!(item.library_id.as_deref(), Some(kids.id.as_str()));
            assert!(item.rel_path.starts_with("Kids/"), "{}", item.rel_path);
            assert!(!h.exists(rel));

            let rel = "Downloads/Completed/clip_final_v2.mkv";
            h.write(rel, &bytes("unidentified chosen"));
            let id = h.lib.sort_path(rel, Some("chosen")).await.unwrap()[0].clone();
            let item = h.item(&id).await;
            assert_eq!(item.status, ItemStatus::Ready);
            assert_eq!(item.category, Category::Movie);
            assert_eq!(item.library_id.as_deref(), Some(kids.id.as_str()));
            assert!(!h.exists(rel));

            let rel = "Downloads/Completed/notes.pdf";
            h.write(rel, &bytes("document"));
            let id = h.lib.sort_path(rel, Some("chosen")).await.unwrap()[0].clone();
            let item = h.item(&id).await;
            assert_eq!(item.status, ItemStatus::NeedsReview);
            assert_eq!(item.category, Category::Other);
            assert!(h.exists(rel));

            // The same uncertain name without a choice still needs review.
            let rel = "Downloads/Completed/Up.1080p.WEB-DL.mkv";
            h.write(rel, &bytes("uncertain automatic"));
            let id = h.lib.sort_path(rel, None).await.unwrap()[0].clone();
            let item = h.item(&id).await;
            assert_eq!(item.status, ItemStatus::NeedsReview);
            assert_eq!(
                item.suggestion().unwrap().confidence,
                crate::models::Confidence::Low
            );
            assert!(h.exists(rel));
        }
    }

    #[tokio::test]
    async fn unidentified_video_chosen_for_camera_library_keeps_its_date() {
        let h = harness().await;
        add_owner(&h).await;
        chosen_download(&h, "chosen", STARTING_HOME_VIDEOS, "u").await;
        let rel = "Downloads/Completed/clip_final_v2.mkv";
        h.write(rel, &bytes("unidentified camera clip"));
        let id = h.lib.sort_path(rel, Some("chosen")).await.unwrap()[0].clone();
        let item = h.item(&id).await;
        assert_eq!(item.status, ItemStatus::Ready);
        assert_eq!(item.category, Category::HomeVideo);
        assert_eq!(item.library_id.as_deref(), Some(STARTING_HOME_VIDEOS));
        assert!(item.taken_at.is_some());
        assert!(!item.rel_path.contains("Undated"));
        assert!(!h.exists(rel));
    }
}

#[tokio::test]
async fn transcoded_video_with_a_new_container_keeps_identity_and_user_metadata() {
    let h = harness().await;
    let original = "Movies/Heat/Heat.mp4";
    h.write(original, b"original h264 file");
    let item = h.lib.index_path(original).await.unwrap().unwrap();
    sqlx::query("UPDATE items SET favorite = 1, title = 'My title', source_download_id = 'synthetic-download', media_json = '{\"codec\":\"h264\"}', mime = 'video/mp4', duration_s = 100, width = 1920, height = 1080 WHERE id = ?")
        .bind(&item.id).execute(&h.lib.db).await.unwrap();
    sqlx::query("INSERT INTO users (id, display_name, role, webauthn_id, created_at) VALUES ('watcher-owner', 'Owner', 'owner', 'watcher-key', 0)")
        .execute(&h.lib.db).await.unwrap();
    sqlx::query("INSERT INTO item_progress (user_id, item_id, position_s, duration_s, updated_at) VALUES ('watcher-owner', ?, 42, 100, 0)")
        .bind(&item.id).execute(&h.lib.db).await.unwrap();
    let thumb = h
        .lib
        .media
        .thumb_path(&item.id, crate::media::ThumbSize::Lg);
    std::fs::create_dir_all(thumb.parent().unwrap()).unwrap();
    std::fs::write(&thumb, b"old thumbnail").unwrap();
    std::fs::remove_file(h.root.join(original)).unwrap();
    h.write("Movies/Heat/Heat.mkv", b"smaller AV1");
    // The create/rename notification is allowed to arrive before the old delete.
    let replaced = h
        .lib
        .index_path("Movies/Heat/Heat.mkv")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replaced.id, item.id);
    assert_eq!(replaced.title, "My title");
    assert!(replaced.favorite);
    assert_eq!(
        replaced.source_download_id.as_deref(),
        Some("synthetic-download")
    );
    assert_eq!(replaced.size, 11);
    let progress: f64 =
        sqlx::query_scalar("SELECT position_s FROM item_progress WHERE item_id = ?")
            .bind(&item.id)
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
    assert_eq!(progress, 42.0);
    assert!(replaced.media_json.is_none());
    assert!(replaced.mime.is_none());
    assert!(!thumb.exists());
    assert_eq!(replaced.thumb_state, crate::models::ThumbState::Pending);
    let report = h.lib.rescan().await.unwrap();
    assert_eq!((report.added, report.missing), (0, 0));
}

#[tokio::test]
async fn container_variants_and_ambiguous_replacements_stay_separate() {
    let h = harness().await;
    h.write("Movies/Heat.mp4", b"original");
    let original = h.lib.index_path("Movies/Heat.mp4").await.unwrap().unwrap();
    h.write("Movies/Heat.mkv", b"alternate encoding");
    let alternate = h.lib.index_path("Movies/Heat.mkv").await.unwrap().unwrap();
    assert_ne!(original.id, alternate.id, "both editions still exist");
    std::fs::remove_file(h.root.join("Movies/Heat.mp4")).unwrap();
    std::fs::remove_file(h.root.join("Movies/Heat.mkv")).unwrap();
    h.write("Movies/Heat.webm", b"a third encoding");
    let third = h.lib.index_path("Movies/Heat.webm").await.unwrap().unwrap();
    assert_ne!(third.id, original.id);
    assert_ne!(
        third.id, alternate.id,
        "do not guess which edition was replaced"
    );
}

#[tokio::test]
async fn external_folder_moves_are_matched_before_the_delete_event() {
    let h = harness().await;
    h.write("Movies/old/Heat.mkv", b"video content");
    let original = h
        .lib
        .index_path("Movies/old/Heat.mkv")
        .await
        .unwrap()
        .unwrap();
    std::fs::rename(h.root.join("Movies/old"), h.root.join("Movies/new")).unwrap();
    let moved = h
        .lib
        .index_path("Movies/new/Heat.mkv")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(moved.id, original.id);
    assert_eq!(h.lib.rescan().await.unwrap().added, 0);
}

#[tokio::test]
async fn same_size_same_second_replacements_refresh_on_watch_event() {
    let h = harness().await;
    let path = h.write("Movies/Heat.mkv", b"old codec");
    let original = h.lib.index_path("Movies/Heat.mkv").await.unwrap().unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, b"AV1 codec").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let replaced = h.lib.index_path("Movies/Heat.mkv").await.unwrap().unwrap();
    assert_eq!(replaced.id, original.id);
    assert_ne!(replaced.quick_hash, original.quick_hash);
}

#[tokio::test]
async fn av1_replacement_is_reprobed_and_playback_uses_the_new_codec() {
    use crate::media::playback::{ClientCaps, PlaybackMode, decide};
    if !ffmpeg_available() {
        eprintln!("skipping AV1 test: ffmpeg/ffprobe unavailable");
        return;
    }
    let h = harness().await;
    let rel = "Movies/AV1 test.mp4";
    make_phone_clip(&h.root.join(rel));
    let item = h.lib.index_path(rel).await.unwrap().unwrap();
    h.lib.media.process_item(&item.id).await.unwrap();
    assert_eq!(
        h.item(&item.id).await.media().unwrap().video.unwrap().codec,
        "h264"
    );
    let encoded = h._dir.path().join("encoded.mp4");
    let encoders = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .unwrap();
    let encoders = String::from_utf8_lossy(&encoders.stdout);
    let encoder_options: &[&str] = if encoders.contains("libaom-av1") {
        &[
            "-c:v",
            "libaom-av1",
            "-cpu-used",
            "8",
            "-crf",
            "40",
            "-b:v",
            "0",
        ]
    } else if encoders.contains("libsvtav1") {
        &[
            "-c:v",
            "libsvtav1",
            "-preset",
            "12",
            "-crf",
            "40",
            "-svtav1-params",
            "lp=1",
        ]
    } else {
        panic!("AV1 verification needs libaom-av1 or libsvtav1 in ffmpeg");
    };
    let result = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=128x128:r=5:d=1",
            "-an",
            "-pix_fmt",
            "yuv420p",
        ])
        .args(encoder_options)
        .arg(&encoded)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "AV1 encoding failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::rename(&encoded, h.root.join(rel)).unwrap();
    let changed = h.lib.index_path(rel).await.unwrap().unwrap();
    assert_eq!(changed.id, item.id);
    assert!(changed.media_json.is_none());
    h.lib.media.process_item(&changed.id).await.unwrap();
    let has_webp = encoders
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some("libwebp"));
    if has_webp {
        h.lib.media.thumbnail_item(&changed.id).await.unwrap();
    } else {
        eprintln!("AV1 probe/playback verified; thumbnail check needs ffmpeg's libwebp encoder");
    }
    let updated = h.item(&changed.id).await;
    let summary = updated.media().unwrap();
    assert_eq!(summary.video.as_ref().unwrap().codec, "av1");
    if has_webp {
        assert_eq!(updated.thumb_state, crate::models::ThumbState::Ready);
    }
    assert_eq!(
        decide(
            &summary,
            "mp4",
            &ClientCaps {
                video_codecs: vec!["av1".into()],
                ..Default::default()
            }
        ),
        PlaybackMode::Direct
    );
    assert!(matches!(
        decide(&summary, "mp4", &ClientCaps::default()),
        PlaybackMode::Hls { .. }
    ));
}

#[tokio::test]
async fn cleanup_inventory_includes_unindexed_companions_and_ignored_files_only_in_safe_zones() {
    let h = harness().await;
    h.write("Inbox/pack/clip.mkv", b"video");
    h.write("Inbox/pack/clip.en.srt", b"subtitle");
    h.write("Inbox/pack/instructions.txt", b"readme");
    h.write("Inbox/pack/art.jpg", b"image");
    h.write("Other/song.flac", b"audio");
    h.write("Downloads/In Progress/live/clip.mkv", b"unfinished");
    h.write("Downloads/Other/live.mkv", b"unfinished");
    h.write("Inbox/hidden.part", b"unfinished");
    h.write("Inbox/.hidden", b"hidden");
    h.write("Inbox/manual.pdf", b"document");
    let ids = h.lib.sort_path("Inbox/manual.pdf", None).await.unwrap();
    let mut ignored = None;
    for id in ids {
        if repo::get(&h.lib.db, &id)
            .await
            .unwrap()
            .unwrap()
            .rel_path
            .ends_with("manual.pdf")
        {
            ignored = Some(id);
            break;
        }
    }
    let ignored = ignored.unwrap();
    h.lib.review(&ignored, ReviewAction::Ignore).await.unwrap();
    std::os::unix::fs::symlink(h.root.join("Other"), h.root.join("Inbox/link")).unwrap();
    let list = h.lib.cleanup_inventory(None).await.unwrap();
    assert!(!list.truncated);
    assert!(
        list.files
            .iter()
            .any(|f| f.choice.path.ends_with(".srt") && f.kind == "subtitle")
    );
    assert!(
        list.files
            .iter()
            .any(|f| f.choice.path.ends_with("manual.pdf") && f.review == "ignored")
    );
    assert!(
        !list
            .files
            .iter()
            .any(|f| f.choice.path.contains("In Progress")
                || f.choice.path.contains("Downloads/Other")
                || f.choice.path.ends_with(".part")
                || f.choice.path.contains("/link/"))
    );
}

#[tokio::test]
async fn cleanup_removes_only_selected_files_and_restores_without_replacing_a_video() {
    let h = harness().await;
    h.write("Inbox/pack/clip.mkv", b"video");
    h.write("Inbox/pack/clip.srt", b"subtitle");
    h.write("Inbox/pack/cover.jpg", b"image");
    let files = h.lib.cleanup_inventory(Some("Inbox")).await.unwrap().files;
    let choices: Vec<_> = files
        .into_iter()
        .filter(|f| f.kind != "video")
        .map(|f| f.choice)
        .collect();
    sqlx::query("INSERT INTO users VALUES ('owner','Owner','owner','cleanup-owner',0)")
        .execute(&h.lib.db)
        .await
        .unwrap();
    let out = h.lib.cleanup_files(&choices, "owner").await.unwrap();
    assert_eq!(out.removed.len(), 2);
    assert!(out.failed.is_empty());
    assert!(h.exists("Inbox/pack/clip.mkv"));
    assert!(!h.exists("Inbox/pack/clip.srt"));
    for entry in h.lib.file_trash().await.unwrap() {
        h.lib.restore_file_trash(&entry.id).await.unwrap();
    }
    assert_eq!(
        std::fs::read(h.root.join("Inbox/pack/clip.srt")).unwrap(),
        b"subtitle"
    );
    assert_eq!(
        std::fs::read(h.root.join("Inbox/pack/clip.mkv")).unwrap(),
        b"video"
    );
    assert!(h.lib.file_trash().await.unwrap().is_empty());
}

#[tokio::test]
async fn cleanup_rejects_changed_files_and_unsafe_batches_before_removing_anything() {
    let h = harness().await;
    h.write("Inbox/readme.txt", b"old");
    let choice = h
        .lib
        .cleanup_inventory(Some("Inbox"))
        .await
        .unwrap()
        .files
        .remove(0)
        .choice;
    h.write("Inbox/readme.txt", b"new content");
    let result = h
        .lib
        .cleanup_files(std::slice::from_ref(&choice), "owner")
        .await
        .unwrap();
    assert_eq!(result.failed.len(), 1);
    assert!(h.exists("Inbox/readme.txt"));
    for path in [
        "../outside",
        "/Inbox/readme.txt",
        "Inbox/../Other/file",
        "Downloads/In Progress/a",
        ".popcornstash/Trash/a",
        "Inbox/.hidden",
    ] {
        let bad = super::cleanup::FileChoice {
            path: path.into(),
            fingerprint: choice.fingerprint.clone(),
        };
        assert!(
            h.lib
                .cleanup_files(&[choice.clone(), bad], "owner")
                .await
                .is_err()
        );
        assert!(h.exists("Inbox/readme.txt"));
    }
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("keep.txt"), b"keep").unwrap();
    std::os::unix::fs::symlink(outside.path(), h.root.join("Inbox/link")).unwrap();
    let linked = super::cleanup::FileChoice {
        path: "Inbox/link/keep.txt".into(),
        fingerprint: choice.fingerprint,
    };
    assert_eq!(
        h.lib
            .cleanup_files(&[linked], "owner")
            .await
            .unwrap()
            .failed
            .len(),
        1
    );
    assert_eq!(
        std::fs::read(outside.path().join("keep.txt")).unwrap(),
        b"keep"
    );
}

#[tokio::test]
async fn deleting_a_library_optionally_trashes_the_folder_and_preserves_history_for_restore() {
    use super::defs::{self, STARTING_MOVIES};
    let h = harness().await;
    let def = defs::load_one(&h.lib.db, STARTING_MOVIES)
        .await
        .unwrap()
        .unwrap();
    let folder = def.folder().to_string();
    let path = format!("{folder}/Harbor (2020)/Harbor.mkv");
    h.write(&path, b"video");
    h.write(&format!("{folder}/Harbor (2020)/Harbor.srt"), b"subtitles");
    h.write(&format!("{folder}/.private.txt"), b"extra");
    let item = h.lib.index_path(&path).await.unwrap().unwrap();
    sqlx::query("UPDATE items SET favorite=1 WHERE id=?")
        .bind(&item.id)
        .execute(&h.lib.db)
        .await
        .unwrap();
    assert!(
        h.lib
            .delete_library_with_folder(STARTING_MOVIES, Some("wrong"), None)
            .await
            .is_err()
    );
    assert!(h.exists(&path));
    h.lib
        .delete_library_with_folder(STARTING_MOVIES, Some(&folder), None)
        .await
        .unwrap();
    assert!(!h.root.join(&folder).exists());
    assert!(
        defs::load_one(&h.lib.db, STARTING_MOVIES)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        repo::get(&h.lib.db, &item.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        ItemStatus::Trashed
    );
    let entry = h.lib.file_trash().await.unwrap().remove(0);
    assert!(entry.is_dir);
    assert_eq!(entry.file_count, 3);
    // Do not overwrite a new folder at the original name.
    h.write(&format!("{folder}/new.txt"), b"new");
    assert!(h.lib.restore_file_trash(&entry.id).await.is_err());
    std::fs::remove_dir_all(h.root.join(&folder)).unwrap();
    h.lib.restore_file_trash(&entry.id).await.unwrap();
    assert!(h.exists(&path));
    let restored = repo::get(&h.lib.db, &item.id).await.unwrap().unwrap();
    assert!(restored.favorite);
    assert_eq!(restored.rel_path, path);
    assert!(restored.library_id.is_none());
    assert!(
        defs::load_one(&h.lib.db, STARTING_MOVIES)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn default_library_deletion_keeps_the_folder_and_cleanup_empty_trash_removes_its_records() {
    use super::defs::{self, STARTING_MOVIES, STARTING_PHOTOS};
    let h = harness().await;
    let movies = defs::load_one(&h.lib.db, STARTING_MOVIES)
        .await
        .unwrap()
        .unwrap();
    h.write(&format!("{}/keep.mkv", movies.folder()), b"keep");
    h.lib.delete_library(STARTING_MOVIES).await.unwrap();
    assert!(h.root.join(movies.folder()).is_dir());
    let photos = defs::load_one(&h.lib.db, STARTING_PHOTOS)
        .await
        .unwrap()
        .unwrap();
    let path = format!("{}/photo.jpg", photos.folder());
    h.write(&path, b"image");
    let item = h.lib.index_path(&path).await.unwrap().unwrap();
    h.lib
        .delete_library_with_folder(STARTING_PHOTOS, Some(photos.folder()), None)
        .await
        .unwrap();
    assert_eq!(h.lib.empty_trash(None).await.unwrap(), 1);
    assert!(h.lib.file_trash().await.unwrap().is_empty());
    assert!(repo::get(&h.lib.db, &item.id).await.unwrap().is_none());
    assert!(h.root.join(movies.folder()).is_dir());
}

#[tokio::test]
async fn cleanup_database_failure_restores_the_original_file_and_preserves_the_item() {
    let h = harness().await;
    h.write("Other/record.jpg", b"photo");
    let item = h.lib.index_path("Other/record.jpg").await.unwrap().unwrap();
    let choice = h
        .lib
        .cleanup_inventory(Some("Other"))
        .await
        .unwrap()
        .files
        .remove(0)
        .choice;
    sqlx::query("CREATE TRIGGER refuse_cleanup BEFORE UPDATE OF status ON items WHEN NEW.status = 'trashed' BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END").execute(&h.lib.db).await.unwrap();
    let result = h.lib.cleanup_files(&[choice], "owner").await.unwrap();
    assert_eq!(result.failed.len(), 1);
    assert!(h.exists("Other/record.jpg"));
    assert!(h.lib.file_trash().await.unwrap().is_empty());
    assert_eq!(
        repo::get(&h.lib.db, &item.id)
            .await
            .unwrap()
            .unwrap()
            .rel_path,
        "Other/record.jpg"
    );
}

#[tokio::test]
async fn folders_move_selected_files_preserving_identity_and_reject_collisions() {
    use super::defs::STARTING_MOVIES;
    use super::folders::{CreateFolder, MoveItems};
    let h = harness().await;
    h.write("Movies/First.mkv", b"first");
    h.write("Movies/Second.mkv", b"second");
    let first = h.lib.index_path("Movies/First.mkv").await.unwrap().unwrap();
    let second = h
        .lib
        .index_path("Movies/Second.mkv")
        .await
        .unwrap()
        .unwrap();
    sqlx::query("INSERT INTO users VALUES ('folder-owner','Owner','owner','folder-key',0)")
        .execute(&h.lib.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_progress (user_id,item_id,position_s,duration_s,updated_at) VALUES ('folder-owner',?,42,100,0)").bind(&first.id).execute(&h.lib.db).await.unwrap();
    sqlx::query("UPDATE items SET favorite = 1 WHERE id = ?")
        .bind(&first.id)
        .execute(&h.lib.db)
        .await
        .unwrap();
    h.lib
        .create_library_folder(
            STARTING_MOVIES,
            &CreateFolder {
                parent: String::new(),
                name: "Collection".into(),
            },
        )
        .await
        .unwrap();
    h.lib
        .create_library_folder(
            STARTING_MOVIES,
            &CreateFolder {
                parent: "Collection".into(),
                name: "Classics".into(),
            },
        )
        .await
        .unwrap();
    let folders = h
        .lib
        .library_folders(STARTING_MOVIES, "Collection")
        .await
        .unwrap();
    assert_eq!(folders.folders, ["Collection/Classics"]);
    h.write("Movies/Collection/Second.mkv", b"occupied");
    let result = h
        .lib
        .move_library_items(
            STARTING_MOVIES,
            &MoveItems {
                item_ids: vec![first.id.clone(), second.id.clone()],
                folder: "Collection".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(result.completed, std::slice::from_ref(&first.id));
    assert_eq!(result.failed.len(), 1);
    let moved = h.item(&first.id).await;
    assert_eq!(moved.rel_path, "Movies/Collection/First.mkv");
    assert!(moved.favorite);
    let progress: f64 =
        sqlx::query_scalar("SELECT position_s FROM item_progress WHERE item_id = ?")
            .bind(&first.id)
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
    assert_eq!(progress, 42.0);
    let scoped = repo::list(
        &h.lib.db,
        &ItemQuery {
            library_id: Some(STARTING_MOVIES.into()),
            folder: Some("Collection".into()),
            ..ItemQuery::default()
        },
        &h.owner(),
    )
    .await
    .unwrap();
    assert!(scoped.items.iter().any(|i| i.id == first.id));
    assert!(!scoped.items.iter().any(|i| i.id == second.id));
    assert_eq!(moved.added_at, first.added_at);
    assert_eq!(moved.quick_hash, first.quick_hash);
    assert!(h.exists("Movies/Second.mkv"));
    assert_eq!(
        std::fs::read(h.root.join("Movies/Collection/Second.mkv")).unwrap(),
        b"occupied"
    );
    h.lib.rescan().await.unwrap();
    assert_eq!(h.item(&first.id).await.rel_path, moved.rel_path);
}

#[tokio::test]
async fn folders_refuse_traversal_links_foreign_items_and_rollback_failed_database_write() {
    use super::defs::{STARTING_MOVIES, STARTING_PHOTOS};
    use super::folders::{CreateFolder, MoveItems};
    let h = harness().await;
    for name in ["../escape", "a/b", ".hidden", " ", "a\\b"] {
        assert!(
            h.lib
                .create_library_folder(
                    STARTING_MOVIES,
                    &CreateFolder {
                        parent: String::new(),
                        name: name.into()
                    }
                )
                .await
                .is_err()
        );
    }
    std::os::unix::fs::symlink(h.root.join("Photos"), h.root.join("Movies/link")).unwrap();
    assert!(
        h.lib
            .library_folders(STARTING_MOVIES, "link")
            .await
            .is_err()
    );
    assert!(
        h.lib
            .library_folders(STARTING_MOVIES, "../Photos")
            .await
            .is_err()
    );
    assert!(
        !h.lib
            .library_folders(STARTING_MOVIES, "")
            .await
            .unwrap()
            .folders
            .contains(&"link".into())
    );
    h.lib
        .create_library_folder(
            STARTING_MOVIES,
            &CreateFolder {
                parent: String::new(),
                name: "New".into(),
            },
        )
        .await
        .unwrap();
    h.write("Movies/Keep.mkv", b"keep");
    let item = h.lib.index_path("Movies/Keep.mkv").await.unwrap().unwrap();
    let body = MoveItems {
        item_ids: vec![item.id.clone()],
        folder: String::new(),
    };
    assert_eq!(
        h.lib
            .move_library_items(STARTING_PHOTOS, &body)
            .await
            .unwrap()
            .failed
            .len(),
        1
    );
    sqlx::query("CREATE TRIGGER refuse_folder_move BEFORE UPDATE OF rel_path ON items BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END").execute(&h.lib.db).await.unwrap();
    assert_eq!(
        h.lib
            .move_library_items(
                STARTING_MOVIES,
                &MoveItems {
                    folder: "New".into(),
                    ..body
                }
            )
            .await
            .unwrap()
            .failed
            .len(),
        1
    );
    assert!(h.exists("Movies/Keep.mkv"));
    assert!(!h.exists("Movies/New/Keep.mkv"));
}

#[tokio::test]
async fn inbox_bulk_delete_removes_only_selected_images_and_stays_deleted_after_rescan() {
    let h = harness().await;
    h.lib
        .settings
        .update(serde_json::json!({"auto_sort": false}))
        .await
        .unwrap();
    sqlx::query("INSERT INTO users VALUES ('owner','Owner','owner','bulk-owner',0)")
        .execute(&h.lib.db)
        .await
        .unwrap();
    for name in ["cover.jpg", "keep.jpg", "cover.mkv"] {
        h.write(&format!("Inbox/{name}"), name.as_bytes());
    }
    h.lib.sort_path("Inbox/cover.jpg", None).await.unwrap();
    h.lib.sort_path("Inbox/keep.jpg", None).await.unwrap();
    h.lib.sort_path("Inbox/cover.mkv", None).await.unwrap();
    let image = repo::get_by_path(&h.lib.db, "Inbox/cover.jpg")
        .await
        .unwrap()
        .unwrap();
    let keep = repo::get_by_path(&h.lib.db, "Inbox/keep.jpg")
        .await
        .unwrap()
        .unwrap();
    if image.status == ItemStatus::NeedsReview {
        h.lib.review(&image.id, ReviewAction::Ignore).await.unwrap();
    }
    let result = h
        .lib
        .trash_inbox_items(std::slice::from_ref(&image.id), "owner")
        .await
        .unwrap();
    assert_eq!(
        result.completed,
        std::slice::from_ref(&image.id),
        "{:?}",
        result.failed
    );
    assert!(!h.exists("Inbox/cover.jpg"));
    assert!(h.exists("Inbox/cover.mkv"));
    assert!(h.exists("Inbox/keep.jpg"));
    h.lib.rescan().await.unwrap();
    assert_eq!(h.item(&image.id).await.status, ItemStatus::Trashed);
    assert_ne!(h.item(&keep.id).await.status, ItemStatus::Trashed);
    let query = ItemQuery {
        include_inbox: true,
        ..ItemQuery::default()
    };
    let page = repo::list(&h.lib.db, &query, &h.owner()).await.unwrap();
    assert!(!page.items.iter().any(|i| i.id == image.id));
    h.lib.restore_item(&image.id).await.unwrap();
    assert!(h.exists("Inbox/cover.jpg"));
}

/// Run on the production filesystem, under a disposable hidden fixture only:
/// STASH_TEST_ROOT=/stash cargo test -p stash-core shfs_cleanup_trash_restore -- --ignored --nocapture
#[tokio::test]
#[ignore = "requires STASH_TEST_ROOT on the filesystem being verified"]
async fn shfs_cleanup_trash_restore() {
    let base = std::env::var_os("STASH_TEST_ROOT").expect("set STASH_TEST_ROOT to /stash");
    let parent = tempfile::Builder::new()
        .prefix(".stash-cleanup-test-")
        .tempdir_in(base)
        .unwrap();
    let h = harness_in(tempfile::tempdir_in(parent.path()).unwrap()).await;
    sqlx::query("INSERT INTO users VALUES ('owner','Owner','owner','fixture-owner',0)")
        .execute(&h.lib.db)
        .await
        .unwrap();
    for (name, bytes) in [
        ("companion.srt", b"synthetic subtitle".as_slice()),
        ("cover.jpg", b"synthetic image".as_slice()),
        ("instructions.txt", b"synthetic instructions".as_slice()),
        ("keep.mkv", b"synthetic keeper".as_slice()),
    ] {
        h.write(&format!("Inbox/{name}"), bytes);
    }
    let choices: Vec<_> = h
        .lib
        .cleanup_inventory(Some("Inbox"))
        .await
        .unwrap()
        .files
        .into_iter()
        .filter(|f| f.choice.path != "Inbox/keep.mkv")
        .map(|f| f.choice)
        .collect();
    let out = h.lib.cleanup_files(&choices, "owner").await.unwrap();
    assert!(out.failed.is_empty(), "{:?}", out.failed);
    assert_eq!(out.removed.len(), 3);
    for entry in h.lib.file_trash().await.unwrap() {
        let payload = std::fs::read(h.root.join(&entry.trashed_rel_path)).unwrap();
        h.write(&entry.original_rel_path, b"synthetic collision");
        assert!(h.lib.restore_file_trash(&entry.id).await.is_err());
        assert_eq!(
            std::fs::read(h.root.join(&entry.original_rel_path)).unwrap(),
            b"synthetic collision"
        );
        assert_eq!(
            std::fs::read(h.root.join(&entry.trashed_rel_path)).unwrap(),
            payload
        );
        // Only our synthetic collision is removed before retrying.
        std::fs::remove_file(h.root.join(&entry.original_rel_path)).unwrap();
        h.lib.restore_file_trash(&entry.id).await.unwrap();
        assert_eq!(
            std::fs::read(h.root.join(&entry.original_rel_path)).unwrap(),
            payload
        );
    }
    h.write("Other/rollback.jpg", b"synthetic photo");
    let item = h
        .lib
        .index_path("Other/rollback.jpg")
        .await
        .unwrap()
        .unwrap();
    let choice = h
        .lib
        .cleanup_inventory(Some("Other"))
        .await
        .unwrap()
        .files
        .remove(0)
        .choice;
    sqlx::query("CREATE TRIGGER refuse_cleanup BEFORE UPDATE OF status ON items WHEN NEW.status = 'trashed' BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END")
        .execute(&h.lib.db).await.unwrap();
    let out = h.lib.cleanup_files(&[choice], "owner").await.unwrap();
    assert_eq!(out.failed.len(), 1);
    assert_eq!(h.item(&item.id).await.rel_path, "Other/rollback.jpg");
    assert_eq!(
        std::fs::read(h.root.join("Other/rollback.jpg")).unwrap(),
        b"synthetic photo"
    );
    assert!(h.lib.file_trash().await.unwrap().is_empty());
    sqlx::query("DROP TRIGGER refuse_cleanup")
        .execute(&h.lib.db)
        .await
        .unwrap();
    h.write("Inbox/trash.jpg", b"synthetic Inbox photo");
    h.lib
        .settings
        .update(serde_json::json!({"auto_sort": false}))
        .await
        .unwrap();
    h.lib.sort_path("Inbox/trash.jpg", None).await.unwrap();
    let item = repo::get_by_path(&h.lib.db, "Inbox/trash.jpg")
        .await
        .unwrap()
        .unwrap();
    let out = h
        .lib
        .trash_inbox_items(std::slice::from_ref(&item.id), "owner")
        .await
        .unwrap();
    assert!(out.failed.is_empty(), "{:?}", out.failed);
    assert_eq!(out.completed.len(), 1);
    assert!(!h.exists("Inbox/trash.jpg"));
    h.write("Inbox/trash.jpg", b"synthetic Inbox collision");
    let restored = h.lib.restore_item(&item.id).await.unwrap();
    assert_eq!(restored.rel_path, "Inbox/trash (2).jpg");
    assert_eq!(
        std::fs::read(h.root.join("Inbox/trash.jpg")).unwrap(),
        b"synthetic Inbox collision"
    );
    assert_eq!(
        std::fs::read(h.root.join(&restored.rel_path)).unwrap(),
        b"synthetic Inbox photo"
    );
    assert_eq!(
        std::fs::read(h.root.join("Inbox/keep.mkv")).unwrap(),
        b"synthetic keeper"
    );
    let out = h
        .lib
        .trash_inbox_items(std::slice::from_ref(&item.id), "owner")
        .await
        .unwrap();
    assert!(out.failed.is_empty(), "{:?}", out.failed);
    let trashed = h.item(&item.id).await.rel_path;
    let parent = h.root.join(&trashed).parent().unwrap().to_path_buf();
    assert_eq!(h.lib.empty_trash(None).await.unwrap(), 1);
    assert!(!h.exists(&trashed));
    assert!(!parent.exists());
    assert!(repo::get(&h.lib.db, &item.id).await.unwrap().is_none());
    assert!(repo::trash_list(&h.lib.db).await.unwrap().is_empty());
    h.lib.db.close().await;
    println!(
        "cleanup, file restore, DB rollback, Inbox trash and restore passed in {}",
        h.root.display()
    );
}

#[tokio::test]
async fn download_folders_flatten_nested_sources_keep_names_after_history_removal_and_page_videos()
{
    use super::defs::STARTING_MOVIES;
    let h = harness().await;
    sqlx::query("INSERT INTO users VALUES('u','Owner','owner','origin-test',0)")
        .execute(&h.lib.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO downloads(id,kind,name,source,state,created_by,created_at,updated_at) VALUES('source-a','torrent','Original collection','magnet:redacted','sorted','u',0,0)").execute(&h.lib.db).await.unwrap();
    let mut ids = Vec::new();
    for (path, data) in [
        ("Movies/One/deep/a.mkv", b"a".as_slice()),
        ("Movies/Two/b.mp4", b"bb".as_slice()),
        ("Movies/Separate/c.mkv", b"ccc".as_slice()),
        ("Movies/loose.mkv", b"loose".as_slice()),
    ] {
        h.write(path, data);
        let item = h.lib.index_path(path).await.unwrap().unwrap();
        if ids.len() < 2 {
            sqlx::query("UPDATE items SET source_download_id='source-a' WHERE id=?")
                .bind(&item.id)
                .execute(&h.lib.db)
                .await
                .unwrap();
        }
        ids.push(item.id);
    }
    let groups = h.lib.download_folders(STARTING_MOVIES).await.unwrap();
    assert_eq!(groups.len(), 3);
    let origin = groups
        .iter()
        .find(|g| g.key == "download:source-a")
        .unwrap();
    assert_eq!(
        (origin.name.as_str(), origin.item_count, origin.size),
        ("Original collection", 2, 3)
    );
    assert!(groups.iter().any(|g| g.key == "folder:Separate"));
    assert!(groups.iter().any(|g| g.key == "loose"));
    sqlx::query("UPDATE downloads SET name='Resolved collection' WHERE id='source-a'")
        .execute(&h.lib.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM downloads WHERE id='source-a'")
        .execute(&h.lib.db)
        .await
        .unwrap();
    assert_eq!(
        h.lib
            .download_folders(STARTING_MOVIES)
            .await
            .unwrap()
            .iter()
            .find(|g| g.key == "download:source-a")
            .unwrap()
            .name,
        "Resolved collection"
    );
    let mut query = ItemQuery {
        library_id: Some(STARTING_MOVIES.into()),
        download_group: Some("download:source-a".into()),
        limit: Some(1),
        ..Default::default()
    };
    let first = repo::list(&h.lib.db, &query, &h.owner()).await.unwrap();
    assert_eq!(first.items.len(), 1);
    query.cursor = first.next_cursor;
    let second = repo::list(&h.lib.db, &query, &h.owner()).await.unwrap();
    assert_eq!(second.items.len(), 1);
    assert_ne!(first.items[0].id, second.items[0].id);
    assert!(second.next_cursor.is_none());
    assert!(ids[..2].contains(&first.items[0].id) && ids[..2].contains(&second.items[0].id));
    assert!(h.exists("Movies/One/deep/a.mkv") && h.exists("Movies/Two/b.mp4"));
    query.library_id = None;
    assert!(repo::list(&h.lib.db, &query, &h.owner()).await.is_err());
}

#[tokio::test]
async fn download_folder_edits_are_atomic_preserve_files_history_and_current_library_scope() {
    use super::defs::{STARTING_HOME_VIDEOS, STARTING_MOVIES};
    use super::download_folders::{MoveToFolder, NameFolder};
    let h = harness().await;
    h.write("Movies/Source/deep/keep.mkv", b"untouched video");
    let item = h
        .lib
        .index_path("Movies/Source/deep/keep.mkv")
        .await
        .unwrap()
        .unwrap();
    sqlx::query("INSERT INTO users VALUES('u','Owner','owner','folder-origin-key',0)")
        .execute(&h.lib.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_progress(user_id,item_id,position_s,duration_s,updated_at) VALUES('u',?,42,100,0)").bind(&item.id).execute(&h.lib.db).await.unwrap();
    sqlx::query("UPDATE items SET favorite=1 WHERE id=?")
        .bind(&item.id)
        .execute(&h.lib.db)
        .await
        .unwrap();
    let target = h
        .lib
        .name_download_folder(
            STARTING_MOVIES,
            &NameFolder {
                key: None,
                name: "Favourites".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        h.lib
            .move_to_download_folder(
                STARTING_MOVIES,
                &MoveToFolder {
                    key: target.clone(),
                    item_ids: vec![item.id.clone(), "stale-id".into()]
                }
            )
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM library_download_members")
        .fetch_one(&h.lib.db)
        .await
        .unwrap();
    assert_eq!(count, 0, "one stale item must roll back the complete batch");
    h.lib
        .move_to_download_folder(
            STARTING_MOVIES,
            &MoveToFolder {
                key: target.clone(),
                item_ids: vec![item.id.clone()],
            },
        )
        .await
        .unwrap();
    h.lib
        .name_download_folder(
            STARTING_MOVIES,
            &NameFolder {
                key: Some(target.clone()),
                name: "Renamed collection".into(),
            },
        )
        .await
        .unwrap();
    let rows = repo::list(
        &h.lib.db,
        &ItemQuery {
            library_id: Some(STARTING_MOVIES.into()),
            download_group: Some(target.clone()),
            ..Default::default()
        },
        &h.owner(),
    )
    .await
    .unwrap();
    assert_eq!(rows.items.len(), 1);
    assert!(rows.items[0].favorite);
    assert_eq!(rows.items[0].progress.as_ref().unwrap().position_s, 42.0);
    assert_eq!(rows.items[0].rel_path, item.rel_path);
    assert_eq!(
        std::fs::read(h.root.join(&item.rel_path)).unwrap(),
        b"untouched video"
    );
    for name in ["../escape", ".hidden", "a/b", " a", "a\\b", ""] {
        assert!(
            h.lib
                .name_download_folder(
                    STARTING_MOVIES,
                    &NameFolder {
                        key: None,
                        name: name.into()
                    }
                )
                .await
                .is_err()
        );
    }
    // An external move cannot attach another library's private presentation label.
    h.write("Home Videos/Moved/keep.mkv", b"untouched video");
    sqlx::query("UPDATE items SET rel_path='Home Videos/Moved/keep.mkv' WHERE id=?")
        .bind(&item.id)
        .execute(&h.lib.db)
        .await
        .unwrap();
    let groups = h.lib.download_folders(STARTING_HOME_VIDEOS).await.unwrap();
    assert!(
        groups
            .iter()
            .all(|g| g.key != target && g.name != "Renamed collection")
    );
    assert!(
        h.lib
            .move_to_download_folder(
                STARTING_HOME_VIDEOS,
                &MoveToFolder {
                    key: target,
                    item_ids: vec![item.id]
                }
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn physical_download_flatten_previews_moves_sidecars_and_preserves_identity() {
    use super::{
        defs::STARTING_MOVIES, download_folders::NameFolder, flatten_downloads::FlattenRequest,
    };
    let h = harness().await;
    h.write("Movies/Source/deep/a.mkv", b"video");
    h.write("Movies/Source/deep/a.en.srt", b"subtitle");
    h.write("Movies/Source/deep/instructions.txt", b"keep untouched");
    let item = h
        .lib
        .index_path("Movies/Source/deep/a.mkv")
        .await
        .unwrap()
        .unwrap();
    h.lib
        .name_download_folder(
            STARTING_MOVIES,
            &NameFolder {
                key: Some("folder:Source".into()),
                name: "Collection".into(),
            },
        )
        .await
        .unwrap();
    let preview = h
        .lib
        .preview_download_flatten(STARTING_MOVIES, "folder:Source")
        .await
        .unwrap();
    assert!(preview.blocked.is_empty(), "{:?}", preview.blocked);
    assert_eq!(preview.files.len(), 2);
    assert!(h.exists(&item.rel_path));
    assert!(!h.exists("Movies/Collection/a.mkv"));
    let result = h
        .lib
        .flatten_download(
            STARTING_MOVIES,
            &FlattenRequest {
                key: preview.key,
                token: Some(preview.token),
            },
        )
        .await
        .unwrap();
    assert!(result.failed.is_empty(), "{:?}", result.failed);
    assert_eq!(result.completed, vec![item.id.clone()]);
    assert_eq!(h.item(&item.id).await.rel_path, "Movies/Collection/a.mkv");
    assert!(h.exists("Movies/Collection/a.en.srt"));
    assert!(h.exists("Movies/Source/deep/instructions.txt"));
    assert_eq!(
        std::fs::read(h.root.join("Movies/Collection/a.mkv")).unwrap(),
        b"video"
    );
    let groups = h.lib.download_folders(STARTING_MOVIES).await.unwrap();
    assert!(
        groups
            .iter()
            .any(|g| g.key == "folder:Source" && g.name == "Collection" && g.item_count == 1)
    );
    let next = h
        .lib
        .preview_download_flatten(STARTING_MOVIES, "folder:Source")
        .await
        .unwrap();
    assert!(next.blocked.is_empty());
    assert!(next.files.iter().all(|f| f.from == f.to));
    h.lib.rescan().await.unwrap();
    assert_eq!(h.item(&item.id).await.rel_path, "Movies/Collection/a.mkv");
}

#[tokio::test]
async fn physical_download_flatten_rejects_stale_collision_and_database_failure() {
    use super::{
        defs::STARTING_MOVIES, download_folders::NameFolder, flatten_downloads::FlattenRequest,
    };
    let h = harness().await;
    h.write("Movies/Source/deep/a.mkv", b"video");
    h.write("Movies/Source/deep/a.srt", b"subtitle");
    let item = h
        .lib
        .index_path("Movies/Source/deep/a.mkv")
        .await
        .unwrap()
        .unwrap();
    h.lib
        .name_download_folder(
            STARTING_MOVIES,
            &NameFolder {
                key: Some("folder:Source".into()),
                name: "Flat".into(),
            },
        )
        .await
        .unwrap();
    let plan = h
        .lib
        .preview_download_flatten(STARTING_MOVIES, "folder:Source")
        .await
        .unwrap();
    h.write("Movies/Source/deep/a.srt", b"changed subtitle");
    assert!(
        h.lib
            .flatten_download(
                STARTING_MOVIES,
                &FlattenRequest {
                    key: plan.key,
                    token: Some(plan.token)
                }
            )
            .await
            .is_err()
    );
    assert!(h.exists(&item.rel_path));
    h.write("Movies/Flat/a.mkv", b"never overwrite");
    let plan = h
        .lib
        .preview_download_flatten(STARTING_MOVIES, "folder:Source")
        .await
        .unwrap();
    assert!(!plan.blocked.is_empty());
    assert!(
        h.lib
            .flatten_download(
                STARTING_MOVIES,
                &FlattenRequest {
                    key: plan.key,
                    token: Some(plan.token)
                }
            )
            .await
            .is_err()
    );
    std::fs::remove_file(h.root.join("Movies/Flat/a.mkv")).unwrap();
    sqlx::query("CREATE TRIGGER refuse_flatten BEFORE UPDATE OF rel_path ON items BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").execute(&h.lib.db).await.unwrap();
    let plan = h
        .lib
        .preview_download_flatten(STARTING_MOVIES, "folder:Source")
        .await
        .unwrap();
    let result = h
        .lib
        .flatten_download(
            STARTING_MOVIES,
            &FlattenRequest {
                key: plan.key,
                token: Some(plan.token),
            },
        )
        .await
        .unwrap();
    assert_eq!(result.failed.len(), 1);
    assert!(h.exists(&item.rel_path));
    assert!(h.exists("Movies/Source/deep/a.srt"));
    assert!(!h.exists("Movies/Flat/a.mkv"));
    assert!(!h.exists("Movies/Flat/a.srt"));
}

#[tokio::test]
async fn physical_download_flatten_protects_active_work_duplicates_and_symlinks() {
    use super::{defs::STARTING_MOVIES, flatten_downloads::FlattenRequest};
    let h = harness().await;
    h.write("Movies/Source/one/a.mkv", b"first");
    h.write("Movies/Source/two/a.mkv", b"second");
    let first = h
        .lib
        .index_path("Movies/Source/one/a.mkv")
        .await
        .unwrap()
        .unwrap();
    h.lib.index_path("Movies/Source/two/a.mkv").await.unwrap();
    let plan = h
        .lib
        .preview_download_flatten(STARTING_MOVIES, "folder:Source")
        .await
        .unwrap();
    assert!(plan.blocked.iter().any(|s| s.contains("More than one")));
    assert!(
        h.lib
            .flatten_download(
                STARTING_MOVIES,
                &FlattenRequest {
                    key: plan.key,
                    token: Some(plan.token)
                }
            )
            .await
            .is_err()
    );
    sqlx::query("INSERT INTO conversion_jobs(id,item_id,library_id,revision,source_version,source_path,target_path,state,created_at,updated_at) VALUES('busy',?,?,0,'v',?,?,'encoding',0,0)").bind(&first.id).bind(STARTING_MOVIES).bind(&first.rel_path).bind(&first.rel_path).execute(&h.lib.db).await.unwrap();
    let plan = h
        .lib
        .preview_download_flatten(STARTING_MOVIES, "folder:Source")
        .await
        .unwrap();
    assert!(plan.blocked.iter().any(|s| s.contains("converting")));
    sqlx::query("DELETE FROM conversion_jobs")
        .execute(&h.lib.db)
        .await
        .unwrap();
    let outside = h.write("Other/outside.srt", b"outside");
    std::os::unix::fs::symlink(&outside, h.root.join("Movies/Source/one/a.srt")).unwrap();
    assert!(
        h.lib
            .preview_download_flatten(STARTING_MOVIES, "folder:Source")
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(outside).unwrap(), b"outside");
    assert!(h.exists(&first.rel_path));
}

#[tokio::test]
#[ignore = "requires an explicitly provided synthetic NAS test directory"]
async fn physical_download_flatten_on_real_nas_filesystem() {
    use super::{defs::STARTING_MOVIES, flatten_downloads::FlattenRequest};
    let base = std::env::var("POPCORN_FLATTEN_TEST_ROOT").expect("synthetic test root required");
    let temp = tempfile::Builder::new()
        .prefix(".popcorn-flatten-test-")
        .tempdir_in(base)
        .unwrap();
    let h = harness_in(temp).await;
    h.write("Movies/Source/deep/a.mkv", b"synthetic video");
    h.write("Movies/Source/deep/a.en.srt", b"synthetic subtitle");
    let item = h
        .lib
        .index_path("Movies/Source/deep/a.mkv")
        .await
        .unwrap()
        .unwrap();
    let plan = h
        .lib
        .preview_download_flatten(STARTING_MOVIES, "folder:Source")
        .await
        .unwrap();
    assert!(plan.blocked.is_empty(), "{:?}", plan.blocked);
    let result = h
        .lib
        .flatten_download(
            STARTING_MOVIES,
            &FlattenRequest {
                key: plan.key,
                token: Some(plan.token),
            },
        )
        .await
        .unwrap();
    assert!(result.failed.is_empty(), "{:?}", result.failed);
    assert_eq!(
        std::fs::read(h.root.join("Movies/Source/a.mkv")).unwrap(),
        b"synthetic video"
    );
    assert_eq!(
        std::fs::read(h.root.join("Movies/Source/a.en.srt")).unwrap(),
        b"synthetic subtitle"
    );
    assert_eq!(h.item(&item.id).await.rel_path, "Movies/Source/a.mkv");
    h.lib.db.close().await;
}

#[tokio::test]
async fn physical_download_flatten_recovers_interrupted_staging_and_publication() {
    use super::{cleanup::Root, defs::STARTING_MOVIES, flatten_downloads::recovery::Journal};
    let h = harness().await;
    h.write("Movies/Source/deep/a.mkv", b"video");
    h.write("Movies/Source/deep/a.srt", b"subtitle");
    let item = h
        .lib
        .index_path("Movies/Source/deep/a.mkv")
        .await
        .unwrap()
        .unwrap();
    let root = Root::open(h.lib.layout.root()).unwrap();
    for committed in [false, true] {
        let plan = h
            .lib
            .preview_download_flatten(STARTING_MOVIES, "folder:Source")
            .await
            .unwrap();
        let files: Vec<_> = plan.files.iter().collect();
        let journal =
            Journal::new(&item.id, STARTING_MOVIES, "folder:Source", &files, &root).unwrap();
        let stage = root.new_trash_dir("flatten").unwrap();
        root.write_flatten_journal(&stage, &serde_json::to_vec(&journal).unwrap())
            .unwrap();
        for (index, f) in plan.files.iter().enumerate() {
            root.rename_trash(&f.from, &format!("{stage}/file-{index}"))
                .unwrap();
        }
        // A crash can occur halfway through publication, before the catalogue commit.
        for (index, f) in plan
            .files
            .iter()
            .enumerate()
            .take(if committed { 2 } else { 1 })
        {
            root.rename_trash(&format!("{stage}/file-{index}"), &f.to)
                .unwrap();
        }
        if committed {
            sqlx::query("UPDATE items SET rel_path='Movies/Source/a.mkv' WHERE id=?")
                .bind(&item.id)
                .execute(&h.lib.db)
                .await
                .unwrap();
        }
        h.lib.recover_flattened_files().await.unwrap();
        assert!(!h.exists(&stage));
        let parent = if committed {
            "Movies/Source"
        } else {
            "Movies/Source/deep"
        };
        assert_eq!(
            std::fs::read(h.root.join(format!("{parent}/a.mkv"))).unwrap(),
            b"video"
        );
        assert_eq!(
            std::fs::read(h.root.join(format!("{parent}/a.srt"))).unwrap(),
            b"subtitle"
        );
        assert_eq!(h.item(&item.id).await.rel_path, format!("{parent}/a.mkv"));
    }
}

#[tokio::test]
async fn physical_download_flatten_failed_recovery_blocks_mutations_and_preparation_is_discarded() {
    use super::{cleanup::Root, defs::STARTING_MOVIES, flatten_downloads::recovery::Journal};
    let h = harness().await;
    let root = Root::open(&h.root).unwrap();
    let unfinished = root.new_trash_dir("flatten").unwrap();
    h.write(&format!("{unfinished}/preparing.json"), b"{truncated");
    h.lib.recover_flattened_files().await.unwrap();
    assert!(!h.exists(&unfinished));
    h.write("Movies/Source/deep/a.mkv", b"original");
    let item = h
        .lib
        .index_path("Movies/Source/deep/a.mkv")
        .await
        .unwrap()
        .unwrap();
    let plan = h
        .lib
        .preview_download_flatten(STARTING_MOVIES, "folder:Source")
        .await
        .unwrap();
    let files: Vec<_> = plan.files.iter().collect();
    let journal = Journal::new(&item.id, STARTING_MOVIES, &plan.key, &files, &root).unwrap();
    let stage = root.new_trash_dir("flatten").unwrap();
    root.write_flatten_journal(&stage, &serde_json::to_vec(&journal).unwrap())
        .unwrap();
    let private = format!("{stage}/file-0");
    root.rename_trash(&item.rel_path, &private).unwrap();
    h.write(&item.rel_path, b"external replacement");
    h.lib
        .flatten_pending
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(h.lib.rescan().await.is_err());
    assert!(h.lib.index_path(&item.rel_path).await.is_err());
    assert!(h.lib.trash_item(&item.id, None).await.is_err());
    assert_eq!(h.item(&item.id).await.rel_path, item.rel_path);
    assert_eq!(std::fs::read(h.root.join(&private)).unwrap(), b"original");
    assert_eq!(
        std::fs::read(h.root.join(&item.rel_path)).unwrap(),
        b"external replacement"
    );
    std::fs::remove_file(h.root.join(&item.rel_path)).unwrap();
    h.lib.index_path(&item.rel_path).await.unwrap();
    assert!(!h.exists(&stage));
    assert_eq!(
        std::fs::read(h.root.join(&item.rel_path)).unwrap(),
        b"original"
    );
}

#[tokio::test]
async fn download_tree_browses_nested_and_empty_folders_and_collapses_without_moves() {
    use super::{defs::STARTING_MOVIES, download_tree::CreateDownloadFolder};
    let h = harness().await;
    for path in [
        "Movies/Collection/root.mkv",
        "Movies/Collection/deep/a.mkv",
        "Movies/Collection/deep/nested/b.mkv",
    ] {
        h.write(path, b"synthetic");
        h.lib.index_path(path).await.unwrap();
    }
    let key = "folder:Collection";
    let tree = h.lib.download_tree(STARTING_MOVIES, key, "").await.unwrap();
    assert_eq!(tree.folders.len(), 1);
    assert_eq!(tree.folders[0].folder, "deep");
    assert_eq!(tree.folders[0].item_count, 2);
    let mut q = ItemQuery {
        library_id: Some(STARTING_MOVIES.into()),
        download_group: Some(key.into()),
        download_folder: Some(String::new()),
        ..Default::default()
    };
    assert_eq!(
        repo::list(&h.lib.db, &q, &h.owner())
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    q.download_folder = Some("deep".into());
    assert_eq!(
        repo::list(&h.lib.db, &q, &h.owner())
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    q.download_folder = None;
    assert_eq!(
        repo::list(&h.lib.db, &q, &h.owner())
            .await
            .unwrap()
            .items
            .len(),
        3
    );
    h.lib
        .create_download_directory(
            STARTING_MOVIES,
            &CreateDownloadFolder {
                key: Some(key.into()),
                parent: "deep".into(),
                name: "Empty".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        h.lib
            .download_tree(STARTING_MOVIES, key, "deep")
            .await
            .unwrap()
            .folders
            .iter()
            .any(|f| f.name == "Empty" && f.item_count == 0)
    );
    q.download_folder = Some("../escape".into());
    assert!(repo::list(&h.lib.db, &q, &h.owner()).await.is_err());
    assert!(h.exists("Movies/Collection/deep/a.mkv"));
}

#[tokio::test]
async fn physical_folder_rename_preserves_tree_companions_metadata_and_stable_group() {
    use super::{defs::STARTING_MOVIES, rename_folder::RenameRequest};
    let h = harness().await;
    h.write("Movies/Collection/deep/clip.mkv", b"synthetic-video");
    h.write("Movies/Collection/deep/clip.srt", b"subtitle");
    h.write("Movies/Collection/notes.txt", b"document");
    std::fs::create_dir_all(h.root.join("Movies/Collection/empty")).unwrap();
    let item = h
        .lib
        .index_path("Movies/Collection/deep/clip.mkv")
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE items SET favorite=1 WHERE id=?")
        .bind(&item.id)
        .execute(&h.lib.db)
        .await
        .unwrap();
    let mut b = RenameRequest {
        key: "folder:Collection".into(),
        folder: "".into(),
        name: "Renamed".into(),
        token: None,
    };
    let preview = h
        .lib
        .preview_folder_rename(STARTING_MOVIES, &b)
        .await
        .unwrap();
    assert_eq!(preview.file_count, 3);
    assert!(preview.blocked.is_empty());
    assert!(
        h.lib
            .rename_download_directory(STARTING_MOVIES, &b)
            .await
            .is_err()
    );
    b.token = Some(preview.token);
    h.lib
        .rename_download_directory(STARTING_MOVIES, &b)
        .await
        .unwrap();
    assert!(!h.exists("Movies/Collection"));
    for p in [
        "Movies/Renamed/deep/clip.mkv",
        "Movies/Renamed/deep/clip.srt",
        "Movies/Renamed/notes.txt",
        "Movies/Renamed/empty",
    ] {
        assert!(h.exists(p));
    }
    let current = h.item(&item.id).await;
    assert!(current.favorite);
    assert_eq!(current.rel_path, "Movies/Renamed/deep/clip.mkv");
    assert_eq!(current.quick_hash, item.quick_hash);
    assert!(
        h.lib
            .download_folders(STARTING_MOVIES)
            .await
            .unwrap()
            .iter()
            .any(|g| g.key == b.key && g.name == "Renamed")
    );
    let mut nested = RenameRequest {
        key: b.key,
        folder: "deep".into(),
        name: "Clips".into(),
        token: None,
    };
    nested.token = Some(
        h.lib
            .preview_folder_rename(STARTING_MOVIES, &nested)
            .await
            .unwrap()
            .token,
    );
    h.lib
        .rename_download_directory(STARTING_MOVIES, &nested)
        .await
        .unwrap();
    assert_eq!(
        h.item(&item.id).await.rel_path,
        "Movies/Renamed/Clips/clip.mkv"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM library_folder_edits")
            .fetch_one(&h.lib.db)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn folder_rename_rejects_stale_collisions_links_busy_work_and_rolls_back_database_failure() {
    use super::{defs::STARTING_MOVIES, rename_folder::RenameRequest};
    let h = harness().await;
    h.write("Movies/Collection/deep/clip.mkv", b"original");
    h.write("Movies/Collection/note.txt", b"note");
    let item = h
        .lib
        .index_path("Movies/Collection/deep/clip.mkv")
        .await
        .unwrap()
        .unwrap();
    let mut b = RenameRequest {
        key: "folder:Collection".into(),
        folder: "".into(),
        name: "Renamed".into(),
        token: None,
    };
    b.token = Some(
        h.lib
            .preview_folder_rename(STARTING_MOVIES, &b)
            .await
            .unwrap()
            .token,
    );
    h.write("Movies/Collection/new.txt", b"new");
    assert!(
        h.lib
            .rename_download_directory(STARTING_MOVIES, &b)
            .await
            .is_err()
    );
    h.write("Movies/Renamed/existing.txt", b"kept");
    assert!(
        !h.lib
            .preview_folder_rename(STARTING_MOVIES, &b)
            .await
            .unwrap()
            .blocked
            .is_empty()
    );
    std::fs::remove_file(h.root.join("Movies/Renamed/existing.txt")).unwrap();
    std::fs::remove_dir(h.root.join("Movies/Renamed")).unwrap();
    std::os::unix::fs::symlink("note.txt", h.root.join("Movies/Collection/link")).unwrap();
    assert!(
        h.lib
            .preview_folder_rename(STARTING_MOVIES, &b)
            .await
            .is_err()
    );
    std::fs::remove_file(h.root.join("Movies/Collection/link")).unwrap();
    sqlx::query("CREATE TRIGGER refuse_folder_rename BEFORE UPDATE OF rel_path ON items BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").execute(&h.lib.db).await.unwrap();
    b.token = Some(
        h.lib
            .preview_folder_rename(STARTING_MOVIES, &b)
            .await
            .unwrap()
            .token,
    );
    assert!(
        h.lib
            .rename_download_directory(STARTING_MOVIES, &b)
            .await
            .is_err()
    );
    assert_eq!(h.item(&item.id).await.rel_path, item.rel_path);
    assert_eq!(
        std::fs::read(h.root.join(&item.rel_path)).unwrap(),
        b"original"
    );
    assert!(h.exists("Movies/Collection/new.txt"));
    assert!(!h.exists("Movies/Renamed"));
    assert_eq!(
        std::fs::read_dir(h.root.join(".popcornstash/Trash"))
            .unwrap()
            .count(),
        0
    );
    sqlx::query("DROP TRIGGER refuse_folder_rename")
        .execute(&h.lib.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO conversion_jobs(id,item_id,library_id,revision,source_version,source_path,target_path,state,created_at,updated_at) VALUES('busy',?,?,1,'x',?,?,'encoding',0,0)").bind(&item.id).bind(STARTING_MOVIES).bind(&item.rel_path).bind(&item.rel_path).execute(&h.lib.db).await.unwrap();
    assert!(
        !h.lib
            .preview_folder_rename(STARTING_MOVIES, &b)
            .await
            .unwrap()
            .blocked
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "requires STASH_FILESYSTEM_TEST_ROOT pointing to a disposable native NAS directory"]
async fn physical_folder_rename_on_real_nas_filesystem() {
    use super::{defs::STARTING_MOVIES, rename_folder::RenameRequest};
    let base = std::env::var("STASH_FILESYSTEM_TEST_ROOT").unwrap();
    let dir = tempfile::Builder::new()
        .prefix("organization-fixture-")
        .tempdir_in(base)
        .unwrap();
    let h = harness_in(dir).await;
    h.write("Movies/Collection/deep/clip.mkv", b"synthetic");
    h.write("Movies/Collection/notes.txt", b"synthetic-notes");
    let item = h
        .lib
        .index_path("Movies/Collection/deep/clip.mkv")
        .await
        .unwrap()
        .unwrap();
    let mut b = RenameRequest {
        key: "folder:Collection".into(),
        folder: "".into(),
        name: "Renamed".into(),
        token: None,
    };
    b.token = Some(
        h.lib
            .preview_folder_rename(STARTING_MOVIES, &b)
            .await
            .unwrap()
            .token,
    );
    h.lib
        .rename_download_directory(STARTING_MOVIES, &b)
        .await
        .unwrap();
    assert!(!h.exists("Movies/Collection"));
    assert!(h.exists("Movies/Renamed/notes.txt"));
    assert_eq!(
        h.item(&item.id).await.rel_path,
        "Movies/Renamed/deep/clip.mkv"
    );
    h.lib.db.close().await;
}

#[tokio::test]
async fn selected_moves_protect_active_work_and_retain_conversion_targets() {
    use super::{defs::STARTING_MOVIES, folders::MoveItems};
    let h = harness().await;
    h.write("Movies/Source/clip.mkv", b"synthetic");
    std::fs::create_dir_all(h.root.join("Movies/Destination/nested")).unwrap();
    let item = h
        .lib
        .index_path("Movies/Source/clip.mkv")
        .await
        .unwrap()
        .unwrap();
    sqlx::query("INSERT INTO conversion_jobs(id,item_id,library_id,revision,source_version,source_path,target_path,state,created_at,updated_at) VALUES('selected-test',?,?,0,'v',?,?,'encoding',0,0)")
        .bind(&item.id).bind(STARTING_MOVIES).bind(&item.rel_path).bind(&item.rel_path).execute(&h.lib.db).await.unwrap();
    let request = MoveItems {
        item_ids: vec![item.id.clone()],
        folder: "Destination/nested".into(),
    };
    let blocked = h
        .lib
        .move_library_items(STARTING_MOVIES, &request)
        .await
        .unwrap();
    assert_eq!(blocked.failed.len(), 1);
    assert!(h.exists(&item.rel_path));
    sqlx::query("UPDATE conversion_jobs SET state='complete' WHERE id='selected-test'")
        .execute(&h.lib.db)
        .await
        .unwrap();
    let moved = h
        .lib
        .move_library_items(STARTING_MOVIES, &request)
        .await
        .unwrap();
    assert_eq!(moved.completed, std::slice::from_ref(&item.id));
    let target: String =
        sqlx::query_scalar("SELECT target_path FROM conversion_jobs WHERE id='selected-test'")
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
    assert_eq!(target, "Movies/Destination/nested/clip.mkv");
    let source: String =
        sqlx::query_scalar("SELECT source_path FROM conversion_jobs WHERE id='selected-test'")
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
    assert_eq!(source, item.rel_path);
    let groups = h.lib.download_folders(STARTING_MOVIES).await.unwrap();
    assert!(
        groups
            .iter()
            .any(|g| g.key == "folder:Destination" && g.item_count == 1)
    );
    assert!(
        std::fs::read_dir(h.root.join(".popcornstash/Trash"))
            .unwrap()
            .next()
            .is_none()
    );
}

#[tokio::test]
#[ignore = "requires STASH_FILESYSTEM_TEST_ROOT pointing to a disposable native NAS directory"]
async fn selected_moves_on_real_nas_filesystem() {
    use super::{
        defs::{STARTING_MOVIES, STARTING_PHOTOS},
        folders::MoveItems,
    };
    let base = std::env::var("STASH_FILESYSTEM_TEST_ROOT").unwrap();
    let dir = tempfile::Builder::new()
        .prefix("selected-fixture-")
        .tempdir_in(base)
        .unwrap();
    let h = harness_in(dir).await;
    for (library, source, destination) in [
        (
            STARTING_MOVIES,
            "Movies/clip.mkv",
            "Movies/Collection/clip.mkv",
        ),
        (
            STARTING_PHOTOS,
            "Photos/photo.jpg",
            "Photos/Collection/photo.jpg",
        ),
    ] {
        h.write(source, b"synthetic-preserved");
        std::fs::create_dir_all(h.root.join(destination).parent().unwrap()).unwrap();
        let item = h.lib.index_path(source).await.unwrap().unwrap();
        let request = MoveItems {
            item_ids: vec![item.id.clone()],
            folder: "Collection".into(),
        };
        let result = h.lib.move_library_items(library, &request).await.unwrap();
        assert_eq!(result.completed, std::slice::from_ref(&item.id));
        assert!(!h.exists(source));
        assert_eq!(
            std::fs::read(h.root.join(destination)).unwrap(),
            b"synthetic-preserved"
        );
        assert_eq!(h.item(&item.id).await.rel_path, destination);
        h.write(source, b"different");
        let other = h.lib.index_path(source).await.unwrap().unwrap();
        let failed = h
            .lib
            .move_library_items(
                library,
                &MoveItems {
                    item_ids: vec![other.id],
                    ..request
                },
            )
            .await
            .unwrap();
        assert_eq!(failed.failed.len(), 1);
        assert_eq!(
            std::fs::read(h.root.join(destination)).unwrap(),
            b"synthetic-preserved"
        );
        assert_eq!(std::fs::read(h.root.join(source)).unwrap(), b"different");
    }
    h.lib.db.close().await;
}
