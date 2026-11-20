use super::*;

#[tokio::test]
async fn interrupted_torrent_handoff_forced_private_rename_keeps_only_owned_payload() {
    for directory in [false, true] {
        let h = harness().await;
        let source = "Downloads/In Progress/fixture.pdf";
        let dest = "Downloads/Completed/fixture (2).pdf";
        let occupied = "Downloads/Completed/fixture.pdf";
        h.write(occupied, b"foreign");
        let input = if directory {
            format!("{source}/nested/fixture.pdf")
        } else {
            source.to_owned()
        };
        use crate::internal_api::{FileWitness, ReleaseWitness};
        use std::os::unix::fs::MetadataExt;
        let selected = h.write(&input, b"original");
        let before = std::fs::metadata(h.root.join(source)).unwrap();
        let index = usize::from(directory);
        let mut witness = ReleaseWitness {
            root_dev: before.dev(),
            root_ino: before.ino(),
            files: [(index, FileWitness::from(&selected.metadata().unwrap()))].into(),
        };
        let mut files = Vec::new();
        if directory {
            // The release snapshot includes the skipped file. Only its shared
            // boundary byte was on disk, and the server pruned it before import.
            let skipped = h.write(&format!("{source}/skipped.bin"), b"x");
            witness
                .files
                .insert(0, FileWitness::from(&skipped.metadata().unwrap()));
            files.push(
                serde_json::json!({"index":0, "path":"skipped.bin", "size":16, "kind":"other",
                "selected":false, "verified_bytes":1, "complete":false}),
            );
            std::fs::remove_file(skipped).unwrap();
        }
        files.push(serde_json::json!({"index":index, "path":if directory {"nested/fixture.pdf"} else {"fixture.pdf"},
            "size":8, "kind":"other", "selected":true,"verified_bytes":8,"complete":true}));
        sqlx::query(
            "INSERT INTO downloads(id,kind,name,source,state,rel_path,created_at,updated_at,torrent_files_json)
            VALUES ('t','torrent','fixture','fixture','completed',?,0,0,?)",
        )
        .bind(source)
        .bind(serde_json::json!({"ready":true,"review_required":false,"rules":null,
            "files":files,"release_witness":witness}).to_string())
        .execute(&h.lib.db).await.unwrap();
        // Adoption/pruning may change directory timestamps. The handoff
        // receives the prepared root AFTER pruning.
        let before = std::fs::metadata(h.root.join(source)).unwrap();
        // Real directory operations and ordinary rename. Only the rejected
        // public syscall is injected on filesystems that otherwise support it.
        sqlx::query(
            "CREATE TRIGGER stop_sort BEFORE INSERT ON jobs WHEN NEW.kind='sort'
            BEGIN SELECT RAISE(ABORT,'interrupted'); END",
        )
        .execute(&h.lib.db)
        .await
        .unwrap();
        assert!(
            h.lib
                .handoff_torrent_with("t", source, Some(&before), |_, _, _| Err(
                    std::io::Error::from_raw_os_error(libc::EINVAL).into()
                ))
                .await
                .is_err()
        );
        let (logical, leaf): (String, String) = sqlx::query_as(
            "SELECT destination,payload_leaf
            FROM torrent_handoffs WHERE download_id='t' AND wrapper_identity IS NOT NULL",
        )
        .fetch_one(&h.lib.db)
        .await
        .unwrap();
        assert_eq!(logical, dest);
        let physical = h.root.join(dest).join(leaf);
        let moved = std::fs::metadata(&physical).unwrap();
        assert_eq!((before.dev(), before.ino()), (moved.dev(), moved.ino()));
        assert_eq!(
            (before.mtime(), before.mtime_nsec()),
            (moved.mtime(), moved.mtime_nsec())
        );
        assert!(!h.exists(source));
        sqlx::query("DROP TRIGGER stop_sort")
            .execute(&h.lib.db)
            .await
            .unwrap();
        h.write(source, b"reused");
        h.write(&format!("{dest}/unexpected"), b"foreign");
        assert!(h.lib.handoff_torrent("t", source, None).await.is_err());
        assert!(h.lib.sort_path(dest, None).await.is_err());
        std::fs::remove_file(h.root.join(dest).join("unexpected")).unwrap();
        assert!(h.lib.handoff_torrent("t", source, None).await.unwrap());
        let held = h.root.join("held");
        std::fs::rename(&physical, &held).unwrap();
        std::fs::write(&physical, b"original").unwrap();
        assert!(h.lib.sort_path(dest, Some("t")).await.is_err());
        std::fs::remove_file(&physical).unwrap();
        std::fs::rename(&held, &physical).unwrap();
        if directory {
            // Keep the payload directory, size, bytes and nanosecond mtime
            // intact, but substitute the selected leaf after the DB publication.
            let member = physical.join("nested/fixture.pdf");
            let mtime = member.metadata().unwrap().modified().unwrap();
            std::fs::rename(&member, &held).unwrap();
            std::fs::write(&member, b"original").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&member)
                .unwrap()
                .set_modified(mtime)
                .unwrap();
            assert!(h.lib.sort_path(dest, None).await.is_err());
            let started: bool = sqlx::query_scalar(
                "SELECT import_started FROM torrent_handoffs WHERE download_id='t'",
            )
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
            assert!(!started);
            let recorded: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM torrent_import_files WHERE download_id='t'",
            )
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
            assert_eq!(recorded, 0);
            std::fs::remove_file(&member).unwrap();
            std::fs::rename(&held, &member).unwrap();
        }
        let sorted = h.lib.sort_path(dest, None).await.unwrap();
        assert_eq!(sorted.len(), 1);
        let item = h.item(&sorted[0]).await;
        assert_eq!(item.source_download_id.as_deref(), Some("t"));
        assert_eq!(
            std::fs::read(h.root.join(&item.rel_path)).unwrap(),
            b"original"
        );
        // Review can legitimately retain the item in Completed. Explicitly
        // organize it to verify that the last move also retires the wrapper.
        if h.exists(dest) {
            h.lib
                .organize(
                    &item.id,
                    OrganizeRequest {
                        place: Some(crate::models::Place::OtherFiles),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
        }
        assert!(!h.exists(dest));
        assert_eq!(std::fs::read(h.root.join(source)).unwrap(), b"reused");
        assert_eq!(std::fs::read(h.root.join(occupied)).unwrap(), b"foreign");
    }
}

async fn completed_fixture(h: &Harness, id: &str, source: &str, bytes: &[u8], force_private: bool) {
    h.write(source, bytes);
    sqlx::query(
        "INSERT INTO downloads(id,kind,name,source,state,rel_path,created_at,updated_at)
        VALUES (?,'torrent','fixture','fixture','completed',?,0,0)",
    )
    .bind(id)
    .bind(source)
    .execute(&h.lib.db)
    .await
    .unwrap();
    let meta = std::fs::metadata(h.root.join(source)).unwrap();
    let published = if force_private {
        h.lib
            .handoff_torrent_with(id, source, Some(&meta), |_, _, _| {
                Err(std::io::Error::from_raw_os_error(libc::EINVAL).into())
            })
            .await
    } else {
        h.lib.handoff_torrent(id, source, Some(&meta)).await
    };
    assert!(published.unwrap());
}

async fn move_reviewed_fixture(h: &Harness, id: &str) -> crate::Result<()> {
    let item: Option<String> = sqlx::query_scalar(
        "SELECT id FROM items WHERE source_download_id=?
        AND rel_path LIKE 'Downloads/Completed/%'",
    )
    .bind(id)
    .fetch_optional(&h.lib.db)
    .await?;
    if let Some(item) = item {
        h.lib
            .organize(
                &item,
                OrganizeRequest {
                    place: Some(crate::models::Place::OtherFiles),
                    ..Default::default()
                },
            )
            .await?;
    }
    Ok(())
}

#[tokio::test]
async fn interrupted_torrent_handoff_retires_names_for_http_and_torrent_successors() {
    for kind in ["http", "torrent"] {
        let h = harness().await;
        let source = "Downloads/In Progress/reusable.pdf";
        let logical = "Downloads/Completed/reusable.pdf";
        let sibling = h.root.join("Downloads/Completed/unrelated-empty");
        std::fs::create_dir(&sibling).unwrap();
        completed_fixture(&h, "first", source, b"first", kind == "torrent").await;
        h.lib.sort_path(logical, None).await.unwrap();
        move_reviewed_fixture(&h, "first").await.unwrap();
        assert!(!h.exists(logical));
        let retired: String =
            sqlx::query_scalar("SELECT retirement FROM torrent_handoffs WHERE download_id='first'")
                .fetch_one(&h.lib.db)
                .await
                .unwrap();
        assert_eq!(retired, "retired");
        assert!(sibling.is_dir());
        if kind == "http" {
            h.write(logical, b"second");
            sqlx::query(
                "INSERT INTO downloads(id,kind,name,source,state,rel_path,created_at,updated_at)
                VALUES ('next','http','fixture','fixture','completed',?,1,1)",
            )
            .bind(logical)
            .execute(&h.lib.db)
            .await
            .unwrap();
            h.lib.enqueue_sort(logical, Some("next")).await.unwrap();
        } else {
            completed_fixture(&h, "next", source, b"second", true).await;
        }
        let owned: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE kind='sort'
            AND json_extract(payload,'$.rel_path')=? AND json_extract(payload,'$.download_id')='next'")
            .bind(logical).fetch_one(&h.lib.db).await.unwrap();
        assert_eq!(owned, 1);
        assert!(h.lib.sort_path(logical, Some("first")).await.is_err());
        let ids = h.lib.sort_path(logical, None).await.unwrap();
        assert_eq!(ids.len(), 1);
        let item = h.item(&ids[0]).await;
        assert_eq!(item.source_download_id.as_deref(), Some("next"));
        assert_eq!(
            std::fs::read(h.root.join(item.rel_path)).unwrap(),
            b"second"
        );
    }
}

#[tokio::test]
async fn interrupted_torrent_handoff_restart_after_witness_unlink_preserves_reused_public_name() {
    let h = harness().await;
    let source = "Downloads/In Progress/restart.pdf";
    let logical = "Downloads/Completed/restart.pdf";
    completed_fixture(&h, "old", source, b"original", true).await;
    // Interrupt the first retirement attempt. Linking and the 'sorted' state
    // must already have committed, even if this was the last review action.
    sqlx::query(
        "CREATE TRIGGER stop_retirement BEFORE UPDATE OF retirement ON torrent_handoffs
        WHEN NEW.retirement='retiring' BEGIN SELECT RAISE(ABORT,'interruption'); END",
    )
    .execute(&h.lib.db)
    .await
    .unwrap();
    let sort = h.lib.sort_path(logical, None).await;
    let review = move_reviewed_fixture(&h, "old").await;
    assert!(sort.is_err() || review.is_err());
    let state: String = sqlx::query_scalar("SELECT state FROM downloads WHERE id='old'")
        .fetch_one(&h.lib.db)
        .await
        .unwrap();
    assert_eq!(state, "sorted");
    sqlx::query("DROP TRIGGER stop_retirement")
        .execute(&h.lib.db)
        .await
        .unwrap();

    // Reproduce the exact durable DB and disk state between the two
    // syscalls. The markerless directory is under the private parent.
    let key: String = sqlx::query_scalar(
        "UPDATE torrent_handoffs SET retirement='retiring'
        WHERE download_id='old' RETURNING private_key",
    )
    .fetch_one(&h.lib.db)
    .await
    .unwrap();
    let root = crate::library::cleanup::Root::open(&h.root).unwrap();
    let parent = ".popcornstash/TorrentHandoffs";
    let dir = root.handoff_retirement_directory(parent).unwrap();
    root.rename_into_handoff(logical, &dir, &key).unwrap();
    let staged = h.root.join(parent).join(&key);
    sqlx::query("UPDATE torrent_handoffs SET retirement='staged' WHERE download_id='old'")
        .execute(&h.lib.db)
        .await
        .unwrap();
    std::fs::remove_file(staged.join(format!(".popcornstash-handoff-{key}"))).unwrap();
    h.write(logical, b"new occupant");
    let restarted = Library::new(
        h.lib.db.clone(),
        h.lib.layout.clone(),
        h.lib.events.clone(),
        h.lib.jobs.clone(),
        h.lib.settings.clone(),
        h.lib.media.clone(),
    );
    drop(restarted.lock_moves().await.unwrap());
    assert!(!staged.exists());
    assert_eq!(
        std::fs::read(h.root.join(logical)).unwrap(),
        b"new occupant"
    );
    let retired: String =
        sqlx::query_scalar("SELECT retirement FROM torrent_handoffs WHERE download_id='old'")
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
    assert_eq!(retired, "retired");
    assert!(restarted.sort_path(logical, Some("old")).await.is_err());
    let links: i64 =
        sqlx::query_scalar("SELECT count(*) FROM download_items WHERE download_id='old'")
            .fetch_one(&h.lib.db)
            .await
            .unwrap();
    assert_eq!(links, 1);
}
