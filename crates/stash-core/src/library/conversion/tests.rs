use super::*;
use crate::{
    config::{Config, HwAccel},
    events::EventBus,
    jobs::JobQueue,
    media::{MediaService, tools},
    paths::StashLayout,
    settings::SettingsStore,
};
use std::{path::Path, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

async fn harness() -> (tempfile::TempDir, Arc<Library>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("stash");
    std::fs::create_dir_all(&root).unwrap();
    let config = Arc::new(Config::for_tests(&root, &dir.path().join("config")));
    let db = crate::db::open_memory().await.unwrap();
    let settings = SettingsStore::load(db.clone(), HwAccel::None)
        .await
        .unwrap();
    let layout = Arc::new(StashLayout::new(root));
    layout.ensure().await.unwrap();
    defs::ensure_folders(&db, &layout).await.unwrap();
    let events = EventBus::new();
    let media = Arc::new(MediaService::new(
        config,
        db.clone(),
        settings.clone(),
        layout.clone(),
        events.clone(),
    ));
    let jobs = JobQueue::new(db.clone(), events.clone());
    (
        dir,
        Arc::new(Library::new(db, layout, events, jobs, settings, media)),
    )
}
async fn clip(path: &Path) {
    let metadata = tempfile::tempdir().unwrap();
    let subtitle = metadata.path().join("captions.srt");
    let chapters = metadata.path().join("chapters.txt");
    std::fs::write(
        &subtitle,
        "1\n00:00:00,000 --> 00:00:00,900\nA synthetic caption.\n",
    )
    .unwrap();
    std::fs::write(&chapters, ";FFMETADATA1\ntitle=Fixture\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=900\ntitle=Chapter one\n").unwrap();
    let mut cmd = tools::command(tools::FFMPEG);
    cmd.args([
        "-v",
        "error",
        "-nostdin",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=320x180:rate=24",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000",
        "-i",
    ])
    .arg(&subtitle)
    .args(["-f", "ffmetadata", "-i"])
    .arg(&chapters)
    .args([
        "-t",
        "1",
        "-map",
        "0:v",
        "-map",
        "1:a",
        "-map",
        "1:a",
        "-map",
        "2:s",
        "-map_metadata",
        "3",
        "-map_chapters",
        "3",
        "-c:v",
        "libx264",
        "-crf",
        "0",
        "-threads",
        "2",
        "-c:a",
        "pcm_s16le",
        "-c:s",
        "srt",
        "-metadata:s:a:0",
        "language=eng",
        "-metadata:s:a:1",
        "title=Commentary",
        "-metadata:s:s:0",
        "language=eng",
    ])
    .arg(path);
    tools::run_ok(cmd, tools::FFMPEG, Duration::from_secs(20))
        .await
        .unwrap();
}

#[tokio::test]
async fn policies_are_off_by_default_and_photos_cannot_enable_conversion() {
    let (_dir, lib) = harness().await;
    assert!(
        !policy(&lib.db, defs::STARTING_MOVIES)
            .await
            .unwrap()
            .0
            .enabled
    );
    let invalid = Policy {
        codec: "h264".into(),
        ..Policy::default()
    };
    assert!(save(&lib, defs::STARTING_MOVIES, invalid).await.is_err());
    assert!(
        save(
            &lib,
            defs::STARTING_PHOTOS,
            Policy {
                enabled: true,
                ..Policy::default()
            }
        )
        .await
        .is_err()
    );
    assert!(!run_one(&lib, &CancellationToken::new()).await.unwrap());
}

async fn roundtrip(compute: &str) {
    for codec in ["av1", "hevc"] {
        let (_dir, lib) = harness().await;
        let path = lib.layout.root().join("Movies/Fixture.mkv");
        clip(&path).await;
        let item = lib.index_path("Movies/Fixture.mkv").await.unwrap().unwrap();
        sqlx::query("UPDATE items SET favorite=1,title='Kept title' WHERE id=?")
            .bind(&item.id)
            .execute(&lib.db)
            .await
            .unwrap();
        let p = Policy {
            enabled: true,
            codec: codec.into(),
            compute: compute.into(),
            ..Policy::default()
        };
        save(&lib, defs::STARTING_MOVIES, p).await.unwrap();
        assert!(run_one(&lib, &CancellationToken::new()).await.unwrap());
        let view = view(&lib, defs::STARTING_MOVIES).await.unwrap();
        assert_eq!(view.jobs[0].state, "complete", "{view:?}");
        assert!(view.jobs[0].saved_bytes > 0);
        let row = repo::get(&lib.db, &item.id).await.unwrap().unwrap();
        assert!(row.favorite);
        assert_eq!(row.title, "Kept title");
        let facts = process::probe(&path).await.unwrap();
        assert_eq!(facts.streams[0]["codec_name"], codec);
        assert_eq!(facts.streams[1]["codec_name"], "pcm_s16le");
        assert!(
            std::fs::read_dir(lib.layout.root().join(".popcornstash/Trash"))
                .unwrap()
                .next()
                .is_none()
        );
    }
}

#[tokio::test]
async fn real_av1_and_hevc_conversion_verify_and_preserve_item_identity() {
    roundtrip("cpu").await;
}

#[tokio::test]
#[ignore = "Requires a working Intel Arc VAAPI device; run explicitly in the production runtime with an isolated synthetic stash."]
async fn arc_av1_and_hevc_replacement_on_real_filesystem() {
    roundtrip("hardware").await;
}

#[tokio::test]
async fn name_collision_and_invalid_video_keep_original_bytes() {
    let (_dir, lib) = harness().await;
    let source = lib.layout.root().join("Movies/Collision.mp4");
    clip(&source.with_extension("mkv")).await;
    std::fs::copy(source.with_extension("mkv"), &source).unwrap();
    lib.index_path("Movies/Collision.mp4").await.unwrap();
    let before = process::hash(&source).await.unwrap();
    save(
        &lib,
        defs::STARTING_MOVIES,
        Policy {
            enabled: true,
            compute: "cpu".into(),
            ..Policy::default()
        },
    )
    .await
    .unwrap();
    run_one(&lib, &CancellationToken::new()).await.unwrap();
    assert_eq!(process::hash(&source).await.unwrap(), before);
    assert_eq!(
        view(&lib, defs::STARTING_MOVIES).await.unwrap().jobs[0].state,
        "failed"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM conversion_jobs WHERE stage IS NOT NULL"
        )
        .fetch_one(&lib.db)
        .await
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn interrupted_publish_restores_original_and_refuses_an_unrelated_destination() {
    for occupied in [false, true] {
        let (_dir, lib) = harness().await;
        let rel = "Movies/Recover.mkv";
        std::fs::write(lib.layout.root().join(rel), b"original").unwrap();
        let item = lib.index_path(rel).await.unwrap().unwrap();
        let root = Root::open(lib.layout.root()).unwrap();
        let stage = root.new_trash_dir("conversion").unwrap();
        let source_hash = process::hash(&lib.layout.root().join(rel)).await.unwrap();
        std::fs::write(
            lib.layout.root().join(&stage).join("output.mkv"),
            b"converted",
        )
        .unwrap();
        let output_hash = process::hash(&lib.layout.root().join(&stage).join("output.mkv"))
            .await
            .unwrap();
        root.rename_trash(rel, &format!("{stage}/original"))
            .unwrap();
        root.rename_trash(&format!("{stage}/output.mkv"), rel)
            .unwrap();
        if occupied {
            std::fs::write(lib.layout.root().join(rel), b"owner's new file").unwrap();
        }
        sqlx::query("INSERT INTO conversion_jobs(id,item_id,library_id,revision,source_version,source_path,target_path,state,stage,source_hash,output_hash,created_at,updated_at) VALUES('test',?,?,1,'test',?,?,'publishing',?,?,?,0,0)")
            .bind(&item.id).bind(defs::STARTING_MOVIES).bind(rel).bind(rel).bind(&stage).bind(source_hash).bind(output_hash).execute(&lib.db).await.unwrap();
        if occupied {
            assert!(recover(&lib).await.is_err());
            assert_eq!(
                std::fs::read(lib.layout.root().join(rel)).unwrap(),
                b"owner's new file"
            );
            assert_eq!(
                std::fs::read(lib.layout.root().join(&stage).join("original")).unwrap(),
                b"original"
            );
        } else {
            recover(&lib).await.unwrap();
            recover(&lib).await.unwrap();
            assert_eq!(
                std::fs::read(lib.layout.root().join(rel)).unwrap(),
                b"original"
            );
            assert!(!lib.layout.root().join(stage).exists());
        }
    }
}

#[tokio::test]
async fn disabling_cancels_work_before_publication() {
    let (_dir, lib) = harness().await;
    let source = lib.layout.root().join("Movies/Stopped.mkv");
    clip(&source).await;
    lib.index_path("Movies/Stopped.mkv").await.unwrap();
    let before = process::hash(&source).await.unwrap();
    save(
        &lib,
        defs::STARTING_MOVIES,
        Policy {
            enabled: true,
            compute: "cpu".into(),
            ..Policy::default()
        },
    )
    .await
    .unwrap();
    let token = CancellationToken::new();
    token.cancel();
    run_one(&lib, &token).await.unwrap();
    assert_eq!(process::hash(&source).await.unwrap(), before);
    assert_eq!(
        view(&lib, defs::STARTING_MOVIES).await.unwrap().jobs[0].state,
        "cancelled"
    );
}

#[tokio::test]
async fn full_video_validation_rejects_a_truncated_playable_prefix() {
    let (_dir, lib) = harness().await;
    let source = lib.layout.root().join("Movies/Integrity.mkv");
    clip(&source).await;
    let item = lib
        .index_path("Movies/Integrity.mkv")
        .await
        .unwrap()
        .unwrap();
    save(
        &lib,
        defs::STARTING_MOVIES,
        Policy {
            enabled: true,
            ..Policy::default()
        },
    )
    .await
    .unwrap();
    let (_, revision) = policy(&lib.db, defs::STARTING_MOVIES).await.unwrap();
    let bytes = std::fs::read(&source).unwrap();
    let output = _dir.path().join("truncated.mkv");
    std::fs::write(&output, &bytes[..bytes.len() * 2 / 3]).unwrap();
    let job = Journal {
        id: "integrity".into(),
        item_id: Some(item.id.clone()),
        library_id: item.library_id.clone(),
        revision,
        source_version: version(&item),
        source_path: item.rel_path.clone(),
        target_path: item.rel_path,
        state: "verifying".into(),
        stage: None,
        source_hash: None,
        output_hash: None,
    };
    let facts = process::probe(&source).await.unwrap();
    assert!(
        process::verify(
            &lib,
            &job,
            &source,
            &output,
            &facts,
            "h264",
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
}

#[tokio::test]
async fn symlink_substitution_cannot_convert_or_remove_the_target() {
    let (_dir, lib) = harness().await;
    let source = lib.layout.root().join("Movies/Link.mkv");
    std::fs::write(&source, b"indexed source").unwrap();
    lib.index_path("Movies/Link.mkv").await.unwrap();
    let outside = _dir.path().join("outside.mkv");
    std::fs::write(&outside, b"must be kept").unwrap();
    std::fs::remove_file(&source).unwrap();
    std::os::unix::fs::symlink(&outside, &source).unwrap();
    save(
        &lib,
        defs::STARTING_MOVIES,
        Policy {
            enabled: true,
            ..Policy::default()
        },
    )
    .await
    .unwrap();
    run_one(&lib, &CancellationToken::new()).await.unwrap();
    assert_eq!(std::fs::read(&outside).unwrap(), b"must be kept");
    assert!(std::fs::symlink_metadata(&source).unwrap().is_symlink());
}

#[tokio::test]
async fn completed_cleanup_can_resume_after_the_private_folder_is_already_removed() {
    let (_dir, lib) = harness().await;
    let rel = "Movies/Complete.mkv";
    std::fs::write(lib.layout.root().join(rel), b"verified converted bytes").unwrap();
    let item = lib.index_path(rel).await.unwrap().unwrap();
    let hash = process::hash(&lib.layout.root().join(rel)).await.unwrap();
    sqlx::query("INSERT INTO conversion_jobs(id,item_id,library_id,revision,source_version,source_path,target_path,state,stage,output_hash,created_at,updated_at) VALUES('cleanup',?,?,1,'old',?,?,'complete','.popcornstash/Trash/conversion-missing',?,0,0)")
        .bind(item.id).bind(defs::STARTING_MOVIES).bind(rel).bind(rel).bind(hash).execute(&lib.db).await.unwrap();
    recover(&lib).await.unwrap();
    recover(&lib).await.unwrap();
    assert_eq!(
        std::fs::read(lib.layout.root().join(rel)).unwrap(),
        b"verified converted bytes"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM conversion_jobs WHERE stage IS NOT NULL"
        )
        .fetch_one(&lib.db)
        .await
        .unwrap(),
        0
    );
}

#[tokio::test]
async fn seeding_and_paused_downloads_are_not_conversion_candidates() {
    let (_dir, lib) = harness().await;
    let source = lib.layout.root().join("Movies/Seeding.mkv");
    std::fs::write(&source, b"active torrent bytes").unwrap();
    let item = lib.index_path("Movies/Seeding.mkv").await.unwrap().unwrap();
    sqlx::query("INSERT INTO downloads(id,kind,name,source,state,created_at,updated_at) VALUES('torrent','torrent','Synthetic','synthetic','seeding',0,0)").execute(&lib.db).await.unwrap();
    sqlx::query("INSERT INTO download_items(download_id,item_id) VALUES('torrent',?)")
        .bind(item.id)
        .execute(&lib.db)
        .await
        .unwrap();
    save(
        &lib,
        defs::STARTING_MOVIES,
        Policy {
            enabled: true,
            ..Policy::default()
        },
    )
    .await
    .unwrap();
    assert!(!run_one(&lib, &CancellationToken::new()).await.unwrap());
    sqlx::query("UPDATE downloads SET state='paused'")
        .execute(&lib.db)
        .await
        .unwrap();
    assert!(!run_one(&lib, &CancellationToken::new()).await.unwrap());
    assert_eq!(std::fs::read(source).unwrap(), b"active torrent bytes");
}
