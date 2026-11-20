use super::*;
use crate::library::{defs::STARTING_MOVIES, download_folders::NameFolder, download_tree, watcher};
use std::collections::HashMap;

async fn indexed(h: &Harness, path: &str, bytes: &[u8]) -> String {
    h.write(path, bytes);
    h.lib.index_path(path).await.unwrap().unwrap().id
}
async fn source(h: &Harness, item: &str, id: &str, label: &str) {
    sqlx::query("INSERT OR IGNORE INTO download_origins(id,name) VALUES(?,?)")
        .bind(id)
        .bind(label)
        .execute(&h.lib.db)
        .await
        .unwrap();
    sqlx::query("UPDATE items SET source_download_id=? WHERE id=?")
        .bind(id)
        .bind(item)
        .execute(&h.lib.db)
        .await
        .unwrap();
}
async fn saved(h: &Harness, key: &str, name: &str) {
    sqlx::query("INSERT INTO library_download_folders(library_id,group_key,name) VALUES(?,?,?)")
        .bind(STARTING_MOVIES)
        .bind(key)
        .bind(name)
        .execute(&h.lib.db)
        .await
        .unwrap();
}
async fn label(h: &Harness, key: &str) -> String {
    h.lib
        .download_folders(STARTING_MOVIES)
        .await
        .unwrap()
        .into_iter()
        .find(|g| g.key == key)
        .unwrap()
        .name
}

#[tokio::test]
async fn already_indexed_paths_repair_source_titles_and_saved_destinations_without_item_changes() {
    let mut h = harness().await;
    let a = indexed(&h, "Movies/Current/deep/a.mkv", b"fixture a").await;
    source(&h, &a, "src", "Original").await;
    let b = indexed(&h, "Movies/Current/deep/b.mkv", b"fixture b").await;
    source(&h, &b, "src", "Original").await;
    let key = "download:src";
    assert_eq!(label(&h, key).await, "Current");
    saved(&h, key, "Original").await;
    h.drain_events();
    assert_eq!(
        h.lib.rescan().await.unwrap(),
        crate::library::ScanReport::default()
    );
    assert_eq!(label(&h, key).await, "Current");
    let saved: String = sqlx::query_scalar(
        "SELECT name FROM library_download_folders WHERE library_id=? AND group_key=?",
    )
    .bind(STARTING_MOVIES)
    .bind(key)
    .fetch_one(&h.lib.db)
    .await
    .unwrap();
    assert_eq!(saved, "Current");
    repo::tests::add_user(&h.lib.db, "u").await;
    let viewer = crate::library::access::Access::load(&h.lib.db, &h.owner())
        .await
        .unwrap();
    let feed = repo::video_feed(&h.lib, &viewer, STARTING_MOVIES, 0)
        .await
        .unwrap();
    let shelf = &feed.collections[0];
    assert_eq!(
        (shelf.folder.key.as_str(), shelf.folder.name.as_str()),
        (key, "Current")
    );
    assert_eq!(shelf.recent.len(), 2);
    assert_eq!(
        download_tree::base(&h.lib.db, STARTING_MOVIES, key)
            .await
            .unwrap(),
        "Current"
    );
    assert!(h.drain_events().iter().any(|e|matches!(e,Event::Library{r#type:LibraryChange::Updated,item_id,..} if item_id==&a || item_id==&b)));
    h.lib.rescan().await.unwrap();
    assert!(
        !h.drain_events()
            .iter()
            .any(|e| matches!(e, Event::Library { .. } | Event::Libraries))
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM download_origins WHERE id='src'")
            .fetch_one(&h.lib.db)
            .await
            .unwrap(),
        "Original"
    );
    // An explicit alias remains intentional even if it happens to match the origin.
    h.lib
        .name_download_folder(
            STARTING_MOVIES,
            &NameFolder {
                key: Some(key.into()),
                name: "Original".into(),
            },
        )
        .await
        .unwrap();
    h.lib.rescan().await.unwrap();
    assert_eq!(label(&h, key).await, "Original");
    assert_eq!(
        crate::library::download_folders::physical_group(&h.lib, STARTING_MOVIES, "Current")
            .await
            .unwrap()
            .as_deref(),
        Some(key)
    );
}

#[tokio::test]
async fn scattered_and_shared_sources_keep_their_provenance_labels() {
    let h = harness().await;
    for (path, id, name) in [
        ("Movies/A/1.mkv", "one", "First"),
        ("Movies/B/2.mkv", "one", "First"),
        ("Movies/B/3.mkv", "two", "Second"),
    ] {
        let item = indexed(&h, path, path.as_bytes()).await;
        source(&h, &item, id, name).await;
    }
    assert_eq!(label(&h, "download:one").await, "First");
    assert_eq!(label(&h, "download:two").await, "Second");
    assert!(
        crate::library::download_folders::physical_group(&h.lib, STARTING_MOVIES, "B")
            .await
            .is_err()
    );
    assert!(
        download_tree::base(&h.lib.db, STARTING_MOVIES, "download:one")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn share_poll_observes_external_collection_rename_preserving_history_and_bytes() {
    let mut h = harness().await;
    let item = indexed(&h, "Movies/Before/nested/video.mkv", b"fixture video").await;
    source(&h, &item, "src", "Source label").await;
    saved(&h, "download:src", "Before").await;
    h.write("Movies/Before/nested/video.srt", b"fixture companion");
    h.add_owner().await;
    sqlx::query("UPDATE items SET favorite=1 WHERE id=?")
        .bind(&item)
        .execute(&h.lib.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO item_progress VALUES('owner',?,42,100,0,0)")
        .bind(&item)
        .execute(&h.lib.db)
        .await
        .unwrap();
    let (mut snapshot, mut pending) = (HashMap::new(), HashMap::new());
    watcher::poll_directories(&h.lib, &mut snapshot, &mut pending).await;
    h.drain_events();
    std::fs::rename(h.root.join("Movies/Before"), h.root.join("Movies/After")).unwrap();
    watcher::poll_directories(&h.lib, &mut snapshot, &mut pending).await;
    for _ in 0..2 {
        watcher::process_pending(&h.lib, &mut pending, Duration::ZERO).await;
    }
    let row = h.item(&item).await;
    assert_eq!(row.rel_path, "Movies/After/nested/video.mkv");
    assert!(row.favorite);
    assert_eq!(row.source_download_id.as_deref(), Some("src"));
    assert_eq!(label(&h, "download:src").await, "After");
    assert_eq!(
        std::fs::read(h.root.join("Movies/After/nested/video.srt")).unwrap(),
        b"fixture companion"
    );
    assert_eq!(
        sqlx::query_scalar::<_, f64>("SELECT position_s FROM item_progress WHERE item_id=?")
            .bind(&item)
            .fetch_one(&h.lib.db)
            .await
            .unwrap(),
        42.
    );
    assert!(
        h.drain_events()
            .iter()
            .any(|e| matches!(e,Event::Library{item_id,..} if item_id==&item))
    );
}

#[tokio::test]
async fn startup_scan_follows_only_a_witnessed_library_root_and_keeps_its_access() {
    let h = harness().await;
    let item = indexed(&h, "Movies/Collection/video.mkv", b"root witness").await;
    sqlx::query("UPDATE libraries SET name='Curated',visibility='selected' WHERE id=?")
        .bind(STARTING_MOVIES)
        .execute(&h.lib.db)
        .await
        .unwrap();
    h.lib.rescan().await.unwrap();
    std::fs::rename(h.root.join("Movies"), h.root.join("Renamed root")).unwrap();
    h.lib.rescan().await.unwrap();
    let row = h.item(&item).await;
    assert_eq!(row.library_id.as_deref(), Some(STARTING_MOVIES));
    assert_eq!(row.rel_path, "Renamed root/Collection/video.mkv");
    let def = crate::library::defs::load_one(&h.lib.db, STARTING_MOVIES)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(def.folder(), "Renamed root");
    assert_eq!(def.row.name, "Curated");
    assert_eq!(def.row.visibility, crate::models::Visibility::Selected);
    assert!(!h.exists("Movies"));
}

#[tokio::test]
async fn offline_root_and_identical_unrelated_copy_never_inherit_library_or_item_identity() {
    let h = harness().await;
    let item = indexed(&h, "Movies/Collection/video.mkv", b"offline witness").await;
    h.lib.rescan().await.unwrap();
    std::fs::rename(h.root.join("Movies"), h._dir.path().join("detached")).unwrap();
    h.write("Unassigned/Collection/video.mkv", b"offline witness");
    h.lib.rescan().await.unwrap();
    let other = repo::get_by_path(&h.lib.db, "Unassigned/Collection/video.mkv")
        .await
        .unwrap()
        .unwrap();
    assert_ne!(item, other.id);
    assert!(other.library_id.is_none());
    let original = h.item(&item).await;
    assert_eq!(original.status, ItemStatus::Ready);
    assert_eq!(original.rel_path, "Movies/Collection/video.mkv");
    assert_eq!(original.library_id.as_deref(), Some(STARTING_MOVIES));
}

#[tokio::test]
async fn legacy_content_matches_reject_ambiguous_sources_and_destinations() {
    for (old_count, new_count) in [(2, 1), (1, 2)] {
        let h = harness().await;
        let mut old = Vec::new();
        for n in 0..old_count {
            old.push(indexed(&h, &format!("Movies/Before/{n}.mkv"), b"identical").await);
        }
        sqlx::query("UPDATE items SET file_dev=NULL,file_ino=NULL")
            .execute(&h.lib.db)
            .await
            .unwrap();
        for n in 0..old_count {
            std::fs::remove_file(h.root.join(format!("Movies/Before/{n}.mkv"))).unwrap();
        }
        for n in 0..new_count {
            h.write(&format!("Movies/After/{n}.mkv"), b"identical");
        }
        let report = h.lib.rescan().await.unwrap();
        assert_eq!(report.moved, 0);
        assert_eq!(report.added, new_count);
        for id in old {
            assert_eq!(h.item(&id).await.status, ItemStatus::Missing);
        }
    }
}

#[tokio::test]
async fn identical_bytes_with_distinct_inodes_follow_the_correct_renamed_folders() {
    let h = harness().await;
    let left = indexed(&h, "Movies/Left/video.mkv", b"duplicate fixture").await;
    let right = indexed(&h, "Movies/Right/video.mkv", b"duplicate fixture").await;
    std::fs::rename(h.root.join("Movies/Left"), h.root.join("Movies/Z")).unwrap();
    std::fs::rename(h.root.join("Movies/Right"), h.root.join("Movies/A")).unwrap();
    let report = h.lib.rescan().await.unwrap();
    assert_eq!((report.moved, report.added), (2, 0));
    assert_eq!(h.item(&left).await.rel_path, "Movies/Z/video.mkv");
    assert_eq!(h.item(&right).await.rel_path, "Movies/A/video.mkv");
}

#[tokio::test]
async fn occupied_old_root_blocks_rebinding_even_with_a_matching_directory_witness() {
    let h = harness().await;
    let item = indexed(&h, "Movies/video.mkv", b"witness fixture").await;
    h.lib.rescan().await.unwrap();
    std::fs::rename(h.root.join("Movies"), h.root.join("Candidate")).unwrap();
    std::fs::create_dir(h.root.join("Movies")).unwrap();
    h.lib.rescan().await.unwrap();
    assert_eq!(
        crate::library::defs::load_one(&h.lib.db, STARTING_MOVIES)
            .await
            .unwrap()
            .unwrap()
            .folder(),
        "Movies"
    );
    assert_eq!(h.item(&item).await.rel_path, "Movies/video.mkv");
    let copy = repo::get_by_path(&h.lib.db, "Candidate/video.mkv")
        .await
        .unwrap()
        .unwrap();
    assert_ne!(copy.id, item);
    assert!(copy.library_id.is_none());
}

#[tokio::test]
async fn later_imports_use_the_confirmed_new_directory_and_reject_split_members() {
    let h = harness().await;
    let key = "download:observed-import";
    h.add_owner().await;
    let source_path = "Downloads/Completed/Fixture";
    sqlx::query("INSERT INTO downloads(id,kind,name,source,state,created_by,created_at,updated_at,rel_path) VALUES('observed-import','torrent','Fixture','synthetic','sorting','owner',0,0,?)")
        .bind(source_path).execute(&h.lib.db).await.unwrap();
    h.write(
        &format!("{source_path}/Fixture.2020.mkv"),
        b"first synthetic clip",
    );
    h.lib
        .sort_path(source_path, Some("observed-import"))
        .await
        .unwrap();
    std::fs::rename(h.root.join("Movies/Fixture"), h.root.join("Movies/Current")).unwrap();
    h.lib.rescan().await.unwrap();
    assert_eq!(label(&h, key).await, "Current");
    h.lib
        .name_download_folder(
            STARTING_MOVIES,
            &NameFolder {
                key: Some(key.into()),
                name: "Virtual shelf".into(),
            },
        )
        .await
        .unwrap();
    h.write(
        &format!("{source_path}/Second.2021.mkv"),
        b"second synthetic clip",
    );
    h.lib
        .sort_path(source_path, Some("observed-import"))
        .await
        .unwrap();
    assert_eq!(label(&h, key).await, "Virtual shelf");
    assert!(h.exists("Movies/Current/Second.2021.mkv"));
    assert!(!h.exists("Movies/Virtual shelf"));
    assert!(!h.exists("Movies/Fixture"));
    std::fs::create_dir(h.root.join("Movies/Split")).unwrap();
    std::fs::rename(
        h.root.join("Movies/Current/Second.2021.mkv"),
        h.root.join("Movies/Split/Second.2021.mkv"),
    )
    .unwrap();
    h.lib.rescan().await.unwrap();
    h.write(
        &format!("{source_path}/Third.2022.mkv"),
        b"third synthetic clip",
    );
    assert!(
        h.lib
            .sort_path(source_path, Some("observed-import"))
            .await
            .is_err()
    );
    assert!(h.exists(&format!("{source_path}/Third.2022.mkv")));
}
