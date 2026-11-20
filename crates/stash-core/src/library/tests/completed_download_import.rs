use super::*;
use crate::internal_api::{FileWitness, ReleaseWitness};
use crate::library::defs::{STARTING_MOVIES, STARTING_SHOWS};

fn release_witness(root: &Path, files: &[(usize, &Path)]) -> ReleaseWitness {
    use std::os::unix::fs::MetadataExt;

    let meta = root.symlink_metadata().unwrap();
    ReleaseWitness {
        root_dev: meta.dev(),
        root_ino: meta.ino(),
        files: files
            .iter()
            .map(|(index, path)| (*index, FileWitness::from(&path.symlink_metadata().unwrap())))
            .collect(),
    }
}

async fn download(h: &Harness, source: &str, library: Option<&str>) {
    sqlx::query(
        "INSERT OR IGNORE INTO users VALUES('u','Owner','owner','completed-import-owner',0)",
    )
    .execute(&h.lib.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO downloads(id,kind,name,source,state,created_at,updated_at,rel_path,created_by,library_id) VALUES('selected','torrent','Collection','magnet:redacted','sorting',0,0,?,'u',?)")
        .bind(source).bind(library).execute(&h.lib.db).await.unwrap();
}

/// Return the public completion root and the physical payload. Force the
/// private rename on one pass even on filesystems with exclusive rename.
async fn publish_manifest(
    h: &Harness,
    source: &str,
    manifest: serde_json::Value,
    wrapped: bool,
) -> (String, String) {
    sqlx::query("INSERT INTO downloads(id,kind,name,source,state,rel_path,torrent_files_json,created_at,updated_at)
        VALUES('frozen','torrent','fixture','fixture','completed',?,?,0,0)")
        .bind(source).bind(manifest.to_string()).execute(&h.lib.db).await.unwrap();
    let meta = std::fs::metadata(h.root.join(source)).unwrap();
    if wrapped {
        h.lib
            .handoff_torrent_with("frozen", source, Some(&meta), |_, _, _| {
                Err(std::io::Error::from_raw_os_error(libc::EOPNOTSUPP).into())
            })
            .await
            .unwrap();
    } else {
        h.lib
            .handoff_torrent("frozen", source, Some(&meta))
            .await
            .unwrap();
    }
    let (dest, leaf): (String, Option<String>) = sqlx::query_as(
        "SELECT destination,payload_leaf FROM torrent_handoffs WHERE download_id='frozen' AND published")
        .fetch_one(&h.lib.db).await.unwrap();
    if wrapped {
        assert!(leaf.is_some());
    }
    let physical = leaf.map_or_else(|| dest.clone(), |leaf| format!("{dest}/{leaf}"));
    (dest, physical)
}

#[tokio::test]
async fn frozen_torrent_import_rejects_nested_additions_and_recovers_journaled_members() {
    for wrapped in [false, true] {
        let h = harness().await;
        h.lib
            .settings
            .update(serde_json::json!({"auto_sort":false}))
            .await
            .unwrap();
        let source = "Downloads/In Progress/fixture";
        for sub in ["a", "b"] {
            h.write(&format!("{source}/{sub}/leaf.pdf"), sub.as_bytes());
        }
        std::fs::create_dir(h.root.join(source).join("empty")).unwrap();
        let (dest, physical) = publish_manifest(
            &h,
            source,
            serde_json::json!({"ready":true,"files":[
                {"index":0,"path":"a/leaf.pdf","selected":true,"size":1,"kind":"other"},
                {"index":1,"path":"b/leaf.pdf","selected":true,"size":1,"kind":"other"},
                {"index":2,"path":"a/skipped.pdf","selected":false,"size":1,"kind":"other"},
                {"index":3,"path":"empty/absent/skipped.pdf","selected":false,"size":1,"kind":"other"}
            ]}),
            wrapped,
        )
        .await;
        // Neither addition changes the published root's mtime. Exercise the
        // watcher entry point as well as an explicitly owned sort.
        for name in ["late.pdf", "skipped.pdf"] {
            let foreign = format!("{physical}/a/{name}");
            h.write(&foreign, b"x");
            assert!(h.lib.sort_path(&foreign, None).await.is_err());
            assert_eq!(std::fs::read(h.root.join(&foreign)).unwrap(), b"x");
            assert_eq!(
                std::fs::read(h.root.join(format!("{physical}/b/leaf.pdf"))).unwrap(),
                b"b"
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT count(*) FROM items WHERE source_download_id='frozen'"
                )
                .fetch_one(&h.lib.db)
                .await
                .unwrap(),
                0
            );
            assert!(!h.exists(&format!("{dest}/leaf.pdf")));
            std::fs::remove_file(h.root.join(foreign)).unwrap();
        }
        let original = h.root.join(format!("{physical}/a/leaf.pdf"));
        let swapped = h.root.join(format!("{physical}/a/swapped.pdf"));
        std::fs::rename(&original, &swapped).unwrap();
        assert!(h.lib.sort_path(&dest, Some("frozen")).await.is_err());
        assert_eq!(std::fs::read(&swapped).unwrap(), b"a");
        std::fs::rename(swapped, &original).unwrap();
        std::fs::write(&original, b"larger").unwrap();
        assert!(h.lib.sort_path(&dest, Some("frozen")).await.is_err());
        assert_eq!(std::fs::read(&original).unwrap(), b"larger");
        std::fs::write(original, b"a").unwrap();
        // Interrupt the same transaction that advances paths. Journal recovery
        // must restore both originals; the next sort must recognize them.
        sqlx::query(
            "CREATE TRIGGER interrupt_flatten BEFORE UPDATE OF current_path ON torrent_import_files
            BEGIN SELECT RAISE(ABORT,'interrupted'); END",
        )
        .execute(&h.lib.db)
        .await
        .unwrap();
        assert!(h.lib.sort_path(&dest, Some("frozen")).await.is_err());
        assert!(h.exists(&format!("{physical}/a/leaf.pdf")));
        assert!(h.exists(&format!("{physical}/b/leaf.pdf")));
        assert!(!h.exists(&format!("{dest}/leaf.pdf")));
        sqlx::query("DROP TRIGGER interrupt_flatten")
            .execute(&h.lib.db)
            .await
            .unwrap();
        let items = h.lib.sort_path(&dest, Some("frozen")).await.unwrap();
        assert_eq!(items.len(), 2);
        let flat = format!("{dest}/leaf (2).pdf");
        assert_eq!(std::fs::read(h.root.join(&flat)).unwrap(), b"b");
        // A same-size replacement at a valid flattened name must not inherit
        // the recorded file's identity on retry.
        let kept = h.root.join("held-fixture");
        std::fs::rename(h.root.join(&flat), &kept).unwrap();
        h.write(&flat, b"z");
        assert!(h.lib.sort_path(&dest, None).await.is_err());
        assert_eq!(std::fs::read(h.root.join(&flat)).unwrap(), b"z");
        std::fs::remove_file(h.root.join(&flat)).unwrap();
        std::fs::rename(kept, h.root.join(&flat)).unwrap();
        // A late empty directory survives explicit review. Once it is removed
        // the remaining selected item can be planned on its own.
        let late = h.root.join(&dest).join("a");
        std::fs::create_dir(&late).unwrap();
        h.lib
            .organize(
                &items[0],
                OrganizeRequest {
                    place: Some(crate::models::Place::OtherFiles),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(late.is_dir());
        assert!(h.lib.sort_path(&dest, None).await.is_err());
        std::fs::remove_dir(late).unwrap();
        h.lib.sort_path(&dest, None).await.unwrap();
        assert!(h.exists(&flat));
    }
}

#[tokio::test]
async fn frozen_torrent_import_checks_selected_release_identity_before_flattening() {
    for wrapped in [false, true] {
        let h = harness().await;
        h.lib
            .settings
            .update(serde_json::json!({"auto_sort":false}))
            .await
            .unwrap();
        let source = "Downloads/In Progress/fixture";
        let selected = h.write(&format!("{source}/nested/leaf.pdf"), b"selected");
        let omitted = h.write(&format!("{source}/nested/omitted.pdf"), b"omitted");
        let modified = std::time::UNIX_EPOCH + Duration::new(1_700_000_000, 200_000_000);
        let original = std::fs::File::open(&selected).unwrap();
        original.set_modified(modified).unwrap();
        let mut witness = release_witness(&h.root.join(source), &[(41, &selected), (3, &omitted)]);
        // Model the engine's earlier ownership without a privileged chown in the test.
        let engine = witness.files.get_mut(&41).unwrap();
        engine.uid = engine.uid.wrapping_add(1);
        engine.ctime_sec -= 1;
        std::fs::remove_file(omitted).unwrap();
        let manifest = serde_json::json!({"ready":true,"review_required":false,"rules":null,"files":[
            {"index":3,"path":"nested/omitted.pdf","selected":false,"size":7,"kind":"other"},
            {"index":41,"path":"nested/leaf.pdf","selected":true,"size":8,"complete":true,"verified_bytes":8,"kind":"other"}
        ],"release_witness":witness});
        let (dest, physical) = publish_manifest(&h, source, manifest.clone(), wrapped).await;
        let leaf = h.root.join(physical).join("nested/leaf.pdf");
        let held = h.root.join("held-original");
        std::fs::rename(&leaf, &held).unwrap();
        std::fs::write(&leaf, b"replaced").unwrap();
        std::fs::File::open(&leaf)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert!(h.lib.sort_path(&dest, None).await.is_err());
        assert_eq!(std::fs::read(&leaf).unwrap(), b"replaced");
        std::fs::remove_file(&leaf).unwrap();
        std::fs::rename(held, &leaf).unwrap();
        original
            .set_modified(modified + Duration::from_millis(100))
            .unwrap();
        assert!(h.lib.sort_path(&dest, Some("frozen")).await.is_err());
        original.set_modified(modified).unwrap();
        drop(original);

        let mut missing = manifest.clone();
        missing["release_witness"]["files"]
            .as_object_mut()
            .unwrap()
            .remove("41");
        let mut wrong_root = manifest.clone();
        wrong_root["release_witness"]["root_ino"] = witness.root_ino.wrapping_add(1).into();
        let mut wrong_device = manifest.clone();
        wrong_device["release_witness"]["files"]["41"]["dev"] =
            witness.files[&41].dev.wrapping_add(1).into();
        let mut wrong_size = manifest.clone();
        wrong_size["files"][1]["size"] = 9.into();
        for invalid in [missing, wrong_root, wrong_device, wrong_size] {
            sqlx::query("UPDATE downloads SET torrent_files_json=? WHERE id='frozen'")
                .bind(invalid.to_string())
                .execute(&h.lib.db)
                .await
                .unwrap();
            assert!(h.lib.sort_path(&dest, None).await.is_err());
            assert_eq!(std::fs::read(&leaf).unwrap(), b"selected");
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM torrent_import_files WHERE download_id='frozen'"
            )
            .fetch_one(&h.lib.db)
            .await
            .unwrap(),
            0
        );
        sqlx::query("UPDATE downloads SET torrent_files_json=? WHERE id='frozen'")
            .bind(manifest.to_string())
            .execute(&h.lib.db)
            .await
            .unwrap();
        let ids = h.lib.sort_path(&dest, None).await.unwrap();
        assert_eq!(ids.len(), 1);
        let item = h.item(&ids[0]).await;
        assert_eq!(item.source_download_id.as_deref(), Some("frozen"));
        assert_eq!(
            std::fs::read(h.root.join(item.rel_path)).unwrap(),
            b"selected"
        );
        // The original nested path is gone. Retry uses the committed import identities.
        assert!(!leaf.exists());
        h.lib.sort_path(&dest, None).await.unwrap();
    }
}

#[tokio::test]
async fn frozen_torrent_import_recovers_after_an_open_reader_closes() {
    let h = harness().await;
    h.lib
        .settings
        .update(serde_json::json!({"auto_sort":false}))
        .await
        .unwrap();
    let source = "Downloads/In Progress/fixture";
    let selected = h.write(&format!("{source}/nested/leaf.pdf"), b"selected");
    let reader = std::fs::File::open(&selected).unwrap();
    let witness = release_witness(&h.root.join(source), &[(0, &selected)]);
    let (dest, _) = publish_manifest(&h, source, serde_json::json!({"ready":true,"files":[
        {"index":0,"path":"nested/leaf.pdf","selected":true,"size":8,"complete":true,"verified_bytes":8,"kind":"other"}
    ], "release_witness":witness}), true).await;
    let first = h.lib.sort_path(&dest, Some("frozen")).await;
    drop(reader);
    let flat = format!("{dest}/leaf.pdf");
    // Native rename can finish with an open reader. SHFS can retain the
    // unlinked private name until close, after the move has already committed.
    let ids = match first {
        Ok(ids) => ids,
        Err(crate::CoreError::Io(e)) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
            assert_eq!(std::fs::read(h.root.join(&flat)).unwrap(), b"selected");
            let current: String = sqlx::query_scalar(
                "SELECT f.current_path FROM torrent_import_files f JOIN download_import_edits e
                 ON e.download_id=f.download_id WHERE f.download_id='frozen'",
            )
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
            assert_eq!(current, flat);
            // FUSE may finish releasing the handle asynchronously. Retry the
            // normal sort entry point, and accept only the original cleanup error.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                match h.lib.sort_path(&dest, Some("frozen")).await {
                    Err(crate::CoreError::Io(e))
                        if e.kind() == std::io::ErrorKind::DirectoryNotEmpty
                            && tokio::time::Instant::now() < deadline =>
                    {
                        tokio::time::sleep(Duration::from_millis(50)).await
                    }
                    result => break result.unwrap(),
                }
            }
        }
        Err(e) => panic!("unexpected held-reader sort error: {e}"),
    };
    assert_eq!(ids.len(), 1);
    let item = h.item(&ids[0]).await;
    assert_eq!(item.source_download_id.as_deref(), Some("frozen"));
    assert_eq!(item.rel_path, flat);
    assert_eq!(std::fs::read(h.root.join(&flat)).unwrap(), b"selected");
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM items WHERE source_download_id='frozen'),
        (SELECT count(*) FROM download_import_edits WHERE download_id='frozen')",
    )
    .fetch_one(&h.lib.db)
    .await
    .unwrap();
    assert_eq!(counts, (1, 0));
    assert!(h.lib.layout.trash().read_dir().unwrap().next().is_none());
}

#[tokio::test]
async fn frozen_torrent_import_single_file_uses_original_name_at_reserved_destination() {
    for wrapped in [false, true] {
        let h = harness().await;
        let source = "Downloads/In Progress/fixture.pdf";
        let occupied = "Downloads/Completed/fixture.pdf";
        h.write(occupied, b"collision");
        let file = h.write(source, b"selected");
        let witness = release_witness(&file, &[(0, &file)]);
        let (dest, _) = publish_manifest(
            &h,
            source,
            serde_json::json!({"ready":true,"files":[
                {"index":0,"path":"fixture.pdf","selected":true,"size":8,"complete":true,"verified_bytes":8,"kind":"other"}
            ], "release_witness":witness}),
            wrapped,
        )
        .await;
        assert_ne!(dest, occupied);
        let ids = h.lib.sort_path(&dest, None).await.unwrap();
        assert_eq!(ids.len(), 1);
        let item = h.item(&ids[0]).await;
        assert_eq!(item.source_download_id.as_deref(), Some("frozen"));
        assert_eq!(
            std::fs::read(h.root.join(item.rel_path)).unwrap(),
            b"selected"
        );
        assert_eq!(std::fs::read(h.root.join(occupied)).unwrap(), b"collision");
    }
}

#[tokio::test]
async fn completed_selected_files_publish_every_companion_per_destination() {
    let h = harness().await;
    let source = "Downloads/Completed/Collection";
    for (name, data) in [
        ("season/Fixture.S01E01.mkv", "episode"),
        ("subtitles/Fixture.S01E01.en.srt", "subtitle"),
        ("titles/Fixture.2020.mkv", "movie"),
        ("notes/deep/instructions.txt", "document"),
        ("audio/track.mp3", "music"),
        ("artwork/cover.jpg", "artwork"),
        ("archives/source.zip", "archive"),
    ] {
        h.write(&format!("{source}/{name}"), data.as_bytes());
    }
    download(&h, source, None).await;
    let ids = h.lib.sort_path(source, None).await.unwrap();
    assert_eq!(ids.len(), 2);
    assert!(h.exists("Shows/Collection/Fixture.S01E01.mkv"));
    assert!(h.exists("Shows/Collection/Fixture.S01E01.en.srt"));
    assert!(h.exists("Movies/Collection/Fixture.2020.mkv"));
    for name in ["instructions.txt", "cover.jpg", "source.zip", "track.mp3"] {
        let path = format!("Movies/Collection/{name}");
        assert!(h.exists(&path));
        assert!(repo::get_by_path(&h.lib.db, &path).await.unwrap().is_none());
    }
    assert!(!h.exists(source));
    sqlx::query("DELETE FROM downloads WHERE id='selected'")
        .execute(&h.lib.db)
        .await
        .unwrap();
    assert_eq!(
        h.lib.download_folders(STARTING_SHOWS).await.unwrap()[0].key,
        "download:selected"
    );
    assert!(
        h.root
            .join(".popcornstash/Trash")
            .read_dir()
            .unwrap()
            .next()
            .is_none()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM download_import_edits")
            .fetch_one(&h.lib.db)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn completed_selected_files_respect_immediate_target_with_auto_sort_disabled() {
    let h = harness().await;
    let source = "Downloads/Completed/Collection";
    h.lib
        .settings
        .update(serde_json::json!({"auto_sort":false}))
        .await
        .unwrap();
    h.write(
        &format!("{source}/deep/clip_final.mkv"),
        b"unidentified video",
    );
    h.write(&format!("{source}/deep/Fixture.S01E01.mkv"), b"episode");
    h.write(
        &format!("{source}/documents/notes.pdf"),
        b"companion document",
    );
    download(&h, source, Some(STARTING_MOVIES)).await;
    let ids = h.lib.sort_path(source, Some("selected")).await.unwrap();
    assert_eq!(ids.len(), 2);
    let movie = repo::get_by_path(&h.lib.db, "Movies/Collection/clip_final.mkv")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(movie.status, ItemStatus::Ready);
    assert_eq!(
        repo::get_by_path(&h.lib.db, &format!("{source}/Fixture.S01E01.mkv"))
            .await
            .unwrap()
            .unwrap()
            .status,
        ItemStatus::NeedsReview
    );
    assert!(h.exists("Movies/Collection/notes.pdf"));
    assert!(
        repo::get_by_path(&h.lib.db, "Movies/Collection/notes.pdf")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn completed_selected_files_number_case_insensitive_collisions_with_sidecar_pairs() {
    let h = harness().await;
    let source = "Downloads/Completed/Collection";
    download(&h, source, Some(STARTING_MOVIES)).await;
    for (name, data) in [
        ("first/clip.mkv", "first video"),
        ("first/clip.en.srt", "first subtitles"),
        ("second/CLIP.mkv", "second video"),
        ("second/CLIP.en.srt", "second subtitles"),
        ("first/notes.txt", "first notes"),
        ("second/NOTES.txt", "second notes"),
    ] {
        h.write(&format!("{source}/{name}"), data.as_bytes());
    }
    h.lib.sort_path(source, None).await.unwrap();
    for (name, data) in [
        ("clip.mkv", "first video"),
        ("clip.en.srt", "first subtitles"),
        ("CLIP (2).mkv", "second video"),
        ("CLIP (2).en.srt", "second subtitles"),
        ("notes.txt", "first notes"),
        ("NOTES (2).txt", "second notes"),
    ] {
        assert_eq!(
            std::fs::read(h.root.join(format!("Movies/Collection/{name}"))).unwrap(),
            data.as_bytes()
        );
    }
    assert!(!h.exists(source));
}

#[tokio::test]
async fn completed_selected_files_allocate_new_source_folders_and_preserve_saved_names() {
    let h = harness().await;
    let source = "Downloads/Completed/Collection";
    download(&h, source, None).await;
    h.write(&format!("{source}/nested/Fixture.2020.mkv"), b"first video");
    h.lib.sort_path(source, None).await.unwrap();
    assert!(!h.exists(source));
    h.write(
        &format!("{source}/nested/Fixture.2020.mkv"),
        b"second video",
    );
    sqlx::query("INSERT INTO downloads(id,kind,name,source,state,created_at,updated_at,rel_path,created_by) VALUES('next','torrent','Collection','magnet:redacted','sorting',0,1,?,'u')").bind(source).execute(&h.lib.db).await.unwrap();
    h.lib.sort_path(source, Some("next")).await.unwrap();
    assert_eq!(
        std::fs::read(h.root.join("Movies/Collection/Fixture.2020.mkv")).unwrap(),
        b"first video"
    );
    assert_eq!(
        std::fs::read(h.root.join("Movies/Collection (2)/Fixture.2020.mkv")).unwrap(),
        b"second video"
    );
    let name:String=sqlx::query_scalar("SELECT name FROM library_download_folders WHERE library_id=? AND group_key='download:next'").bind(STARTING_MOVIES).fetch_one(&h.lib.db).await.unwrap();
    assert_eq!(name, "Collection (2)");
    h.write(
        &format!("{source}/more/Another.2021.mkv"),
        b"additional video",
    );
    sqlx::query("UPDATE library_download_folders SET name='Edited' WHERE library_id=? AND group_key='download:next'").bind(STARTING_MOVIES).execute(&h.lib.db).await.unwrap();
    assert!(h.lib.sort_path(source, Some("next")).await.is_err());
    assert!(h.exists(&format!("{source}/Another.2021.mkv")));
    assert!(!h.exists("Movies/Edited/Another.2021.mkv"));
}

#[tokio::test]
async fn completed_selected_files_keep_photo_policy_and_pending_companions_after_history_removal() {
    let h = harness().await;
    let source = "Downloads/Completed/Collection";
    h.lib
        .settings
        .update(serde_json::json!({"auto_sort":false}))
        .await
        .unwrap();
    h.write(
        &format!("{source}/nested/Fixture.2020.mkv"),
        b"waiting video",
    );
    h.write(&format!("{source}/art/cover.jpg"), b"selected artwork");
    h.write(&format!("{source}/docs/notes.txt"), b"selected notes");
    download(&h, source, None).await;
    let id = h.lib.sort_path(source, None).await.unwrap()[0].clone();
    sqlx::query("DELETE FROM downloads WHERE id='selected'")
        .execute(&h.lib.db)
        .await
        .unwrap();
    h.lib
        .review(&id, ReviewAction::Accept(Default::default()))
        .await
        .unwrap();
    assert_eq!(
        h.item(&id).await.rel_path,
        "Movies/Collection/Fixture.2020.mkv"
    );
    assert!(h.exists("Movies/Collection/cover.jpg"));
    assert!(h.exists("Movies/Collection/notes.txt"));
    assert!(!h.exists(source));
    let h = harness().await;
    h.write(&format!("{source}/nested/20240101.jpg"), b"photo only");
    download(&h, source, None).await;
    h.lib.sort_path(source, None).await.unwrap();
    assert!(h.exists("Photos/Collection/20240101.jpg"));
}

#[tokio::test]
async fn completed_selected_files_review_allocates_peer_folders_after_history_removal() {
    for clear_history in [false, true] {
        let h = harness().await;
        let source = "Downloads/Completed/Collection";
        h.lib
            .settings
            .update(serde_json::json!({"auto_sort":false}))
            .await
            .unwrap();
        h.write(&format!("{source}/nested/Fixture.2020.mkv"), b"first video");
        download(&h, source, None).await;
        let first = h.lib.sort_path(source, Some("selected")).await.unwrap()[0].clone();
        if clear_history {
            sqlx::query("DELETE FROM downloads WHERE id='selected'")
                .execute(&h.lib.db)
                .await
                .unwrap();
        }
        h.lib
            .review(&first, ReviewAction::Accept(Default::default()))
            .await
            .unwrap();
        assert!(!h.exists(source));
        h.write(
            &format!("{source}/nested/Fixture.2020.mkv"),
            b"second video",
        );
        h.write(
            &format!("{source}/nested/Another.2021.mkv"),
            b"same second source",
        );
        h.write(&format!("{source}/art/cover.jpg"), b"second companion");
        sqlx::query("INSERT INTO downloads(id,kind,name,source,state,created_at,updated_at,rel_path,created_by) VALUES('peer','torrent','Collection','magnet:redacted','sorting',0,1,?,'u')")
            .bind(source).execute(&h.lib.db).await.unwrap();
        let pending = h.lib.sort_path(source, Some("peer")).await.unwrap();
        assert_eq!(pending.len(), 2);
        if clear_history {
            sqlx::query("DELETE FROM downloads WHERE id='peer'")
                .execute(&h.lib.db)
                .await
                .unwrap();
        }
        for id in &pending {
            let item = h
                .lib
                .review(id, ReviewAction::Accept(Default::default()))
                .await
                .unwrap();
            assert!(item.rel_path.starts_with("Movies/Collection (2)/"));
            assert_eq!(item.source_download_id.as_deref(), Some("peer"));
        }
        assert_eq!(
            std::fs::read(h.root.join("Movies/Collection/Fixture.2020.mkv")).unwrap(),
            b"first video"
        );
        assert_eq!(
            std::fs::read(h.root.join("Movies/Collection (2)/Fixture.2020.mkv")).unwrap(),
            b"second video"
        );
        assert!(h.exists("Movies/Collection (2)/Another.2021.mkv"));
        assert!(h.exists("Movies/Collection (2)/cover.jpg"));
        let saved: String = sqlx::query_scalar("SELECT name FROM library_download_folders WHERE library_id=? AND group_key='download:peer'")
            .bind(STARTING_MOVIES).fetch_one(&h.lib.db).await.unwrap();
        assert_eq!(saved, "Collection (2)");
        assert!(!h.exists(source));
    }
}

#[tokio::test]
async fn completed_selected_files_review_metadata_is_atomic_before_and_after_commit() {
    for committed in [false, true] {
        let h = harness().await;
        let source = "Downloads/Completed/Collection";
        h.lib
            .settings
            .update(serde_json::json!({"auto_sort":false}))
            .await
            .unwrap();
        h.write(
            &format!("{source}/nested/clip_final.mkv"),
            b"unidentified video",
        );
        h.write(&format!("{source}/docs/notes.txt"), b"selected notes");
        download(&h, source, None).await;
        let id = h.lib.sort_path(source, Some("selected")).await.unwrap()[0].clone();
        sqlx::query("UPDATE items SET favorite=1 WHERE id=?")
            .bind(&id)
            .execute(&h.lib.db)
            .await
            .unwrap();
        repo::set_progress(&h.lib.db, "u", &id, 42.0, 100.0)
            .await
            .unwrap();
        let before = h.item(&id).await;
        assert_eq!(before.category, Category::Other);
        let request = OrganizeRequest {
            library_id: Some(STARTING_SHOWS.into()),
            category: Some(Category::Episode),
            show_name: Some("Reviewed Series".into()),
            season: Some(2),
            episode: Some(3),
            year: Some(2024),
            ..Default::default()
        };
        let trigger = if committed {
            // Interrupt after the publication transaction commits, before review returns.
            "CREATE TRIGGER interrupt_review BEFORE DELETE ON download_import_edits BEGIN SELECT RAISE(ABORT,'synthetic post-commit interruption'); END"
        } else {
            // The path update has executed in the transaction when metadata is rejected.
            "CREATE TRIGGER interrupt_review BEFORE UPDATE OF category ON items WHEN NEW.category='episode' BEGIN SELECT RAISE(ABORT,'synthetic metadata interruption'); END"
        };
        sqlx::query(trigger).execute(&h.lib.db).await.unwrap();
        assert!(
            h.lib
                .review(&id, ReviewAction::Accept(request.clone()))
                .await
                .is_err()
        );
        let after = h.item(&id).await;
        assert!(after.favorite);
        if committed {
            assert_eq!(after.rel_path, "Shows/Collection/clip_final.mkv");
            assert_eq!(after.status, ItemStatus::Ready);
            assert_eq!(after.category, Category::Episode);
            assert_eq!(after.title, "Reviewed Series S02E03");
            assert_eq!(after.show_name.as_deref(), Some("Reviewed Series"));
            assert_eq!(
                (after.season, after.episode, after.year),
                (Some(2), Some(3), Some(2024))
            );
            assert!(after.suggestion_json.is_none());
            assert!(h.exists("Shows/Collection/notes.txt"));
            assert!(!h.exists(&before.rel_path));
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM download_import_edits")
                    .fetch_one(&h.lib.db)
                    .await
                    .unwrap(),
                1
            );
        } else {
            assert_eq!(after.rel_path, before.rel_path);
            assert_eq!(after.status, before.status);
            assert_eq!(after.category, before.category);
            assert_eq!(after.title, before.title);
            assert_eq!(after.suggestion_json, before.suggestion_json);
            assert!(h.exists(&before.rel_path));
            assert!(h.exists(&format!("{source}/notes.txt")));
            assert!(!h.exists("Shows/Collection/clip_final.mkv"));
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM download_import_edits")
                    .fetch_one(&h.lib.db)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM library_download_folders WHERE group_key='download:selected'").fetch_one(&h.lib.db).await.unwrap(), 0);
        }
        sqlx::query("DROP TRIGGER interrupt_review")
            .execute(&h.lib.db)
            .await
            .unwrap();
        h.lib.recover_flattened_files().await.unwrap();
        if !committed {
            let accepted = h
                .lib
                .review(&id, ReviewAction::Accept(request))
                .await
                .unwrap();
            assert_eq!(accepted.category, Category::Episode);
            assert_eq!(accepted.title, "Reviewed Series S02E03");
            assert_eq!(accepted.status, ItemStatus::Ready);
        } else {
            let recovered = h.item(&id).await;
            assert_eq!(recovered.category, Category::Episode);
            assert_eq!(recovered.title, "Reviewed Series S02E03");
        }
        assert_eq!(
            sqlx::query_scalar::<_, f64>("SELECT position_s FROM item_progress WHERE item_id=?")
                .bind(&id)
                .fetch_one(&h.lib.db)
                .await
                .unwrap(),
            42.0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM download_import_edits")
                .fetch_one(&h.lib.db)
                .await
                .unwrap(),
            0
        );
        assert!(
            h.root
                .join(".popcornstash/Trash")
                .read_dir()
                .unwrap()
                .next()
                .is_none()
        );
    }
}

#[tokio::test]
async fn completed_selected_files_do_not_touch_active_or_unrelated_manual_folders() {
    for state in ["queued", "downloading", "paused", "seeding"] {
        let h = harness().await;
        let source = "Downloads/Completed/Collection";
        h.write(&format!("{source}/deep/Fixture.2020.mkv"), b"active bytes");
        download(&h, source, None).await;
        sqlx::query("UPDATE downloads SET state=? WHERE id='selected'")
            .bind(state)
            .execute(&h.lib.db)
            .await
            .unwrap();
        assert!(h.lib.sort_path(source, None).await.is_err());
        assert!(h.exists(&format!("{source}/deep/Fixture.2020.mkv")));
        assert!(!h.exists(&format!("{source}/Fixture.2020.mkv")));
    }
    let h = harness().await;
    h.write(
        "Downloads/In Progress/Collection/deep/Fixture.2020.mkv",
        b"engine bytes",
    );
    h.write("Movies/Manual/deep/keep.mkv", b"manual bytes");
    download(&h, "Downloads/In Progress/Collection", None).await;
    assert!(
        h.lib
            .sort_path("Downloads/In Progress/Collection", Some("selected"))
            .await
            .is_err()
    );
    assert!(h.lib.sort_path("Movies/Manual", None).await.is_err());
    assert!(h.exists("Downloads/In Progress/Collection/deep/Fixture.2020.mkv"));
    assert!(h.exists("Movies/Manual/deep/keep.mkv"));
}

#[tokio::test]
async fn completed_selected_files_restore_failed_moves_and_preserve_identity_and_ledger() {
    let h = harness().await;
    let source = "Downloads/Completed/Collection";
    let original = format!("{source}/deep/Fixture.2020.mkv");
    h.write(&original, b"video with history");
    h.lib
        .settings
        .update(serde_json::json!({"auto_sort":false}))
        .await
        .unwrap();
    let id = h.lib.sort_path(source, None).await.unwrap()[0].clone();
    download(&h, source, None).await;
    sqlx::query("UPDATE items SET source_download_id='selected' WHERE id=?")
        .bind(&id)
        .execute(&h.lib.db)
        .await
        .unwrap();
    sqlx::query("UPDATE items SET favorite=1 WHERE id=?")
        .bind(&id)
        .execute(&h.lib.db)
        .await
        .unwrap();
    repo::set_progress(&h.lib.db, "u", &id, 42.0, 100.0)
        .await
        .unwrap();
    sqlx::query("INSERT INTO conversion_jobs(id,item_id,library_id,revision,source_version,source_path,target_path,state,created_at,updated_at) VALUES('import-ledger',?,?,0,'v',?,?,'complete',0,0)")
        .bind(&id).bind(STARTING_MOVIES).bind(&original).bind(&original).execute(&h.lib.db).await.unwrap();
    sqlx::query("CREATE TRIGGER refuse_import BEFORE UPDATE OF rel_path ON items BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").execute(&h.lib.db).await.unwrap();
    assert!(h.lib.sort_path(source, None).await.is_err());
    assert_eq!(h.item(&id).await.rel_path, original);
    assert!(h.exists(&original));
    assert!(!h.exists(&format!("{source}/Fixture.2020.mkv")));
    sqlx::query("DROP TRIGGER refuse_import")
        .execute(&h.lib.db)
        .await
        .unwrap();
    h.lib
        .settings
        .update(serde_json::json!({"auto_sort":true}))
        .await
        .unwrap();
    let result = h.lib.sort_path(source, None).await.unwrap();
    assert_eq!(result, vec![id.clone()]);
    let item = h.item(&id).await;
    assert!(item.favorite);
    assert_eq!(
        sqlx::query_scalar::<_, f64>("SELECT position_s FROM item_progress WHERE item_id=?")
            .bind(&id)
            .fetch_one(&h.lib.db)
            .await
            .unwrap(),
        42.0
    );
    assert_eq!(item.rel_path, "Movies/Collection/Fixture.2020.mkv");
    let ledger: (String, String) = sqlx::query_as(
        "SELECT source_path,target_path FROM conversion_jobs WHERE id='import-ledger'",
    )
    .fetch_one(&h.lib.db)
    .await
    .unwrap();
    assert_eq!(ledger, (original, item.rel_path));
    assert!(
        h.root
            .join(".popcornstash/Trash")
            .read_dir()
            .unwrap()
            .next()
            .is_none()
    );
}

#[tokio::test]
async fn completed_selected_files_recover_interrupted_staging_and_publication() {
    use crate::library::cleanup::{Root, fingerprint};
    use std::os::unix::fs::MetadataExt;
    for (published, committed) in [(false, false), (true, false), (true, true)] {
        let h = harness().await;
        let source = "Downloads/Completed/Collection";
        let from = format!("{source}/deep/notes.txt");
        let to = format!("{source}/notes.txt");
        h.write(&from, b"selected companion");
        download(&h, source, None).await;
        let root = Root::open(&h.root).unwrap();
        let meta = root.metadata(&from).unwrap();
        let stage = root.new_trash_dir("flatten-import").unwrap();
        let journal = serde_json::json!({"import_version":1,"id":"interrupted","download_id":"selected","source":source,"files":[{"file":{"item_id":"","from":from,"to":to,"kind":"item","fingerprint":fingerprint(&meta)},"identity":[meta.dev(),meta.ino(),meta.len(),meta.mtime() as u64,meta.mtime_nsec() as u64]}]});
        root.write_flatten_journal(&stage, &serde_json::to_vec(&journal).unwrap())
            .unwrap();
        let private = format!("{stage}/file-0");
        root.rename_trash(&from, &private).unwrap();
        if published {
            root.rename_trash(&private, &to).unwrap();
        }
        if committed {
            sqlx::query("INSERT INTO download_import_edits VALUES('interrupted','selected')")
                .execute(&h.lib.db)
                .await
                .unwrap();
        }
        h.lib.recover_flattened_files().await.unwrap();
        assert_eq!(h.exists(&from), !committed);
        assert_eq!(h.exists(&to), committed);
        assert!(!h.exists(&stage));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM download_import_edits")
                .fetch_one(&h.lib.db)
                .await
                .unwrap(),
            0
        );
    }
}
