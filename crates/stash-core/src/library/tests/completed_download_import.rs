use super::*;
use crate::library::defs::{STARTING_MOVIES, STARTING_SHOWS};

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
