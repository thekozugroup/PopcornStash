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
    clip_with_crf(path, "0").await;
}

async fn clip_with_crf(path: &Path, crf: &str) {
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
        crf,
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
        // H.264 lossless uses a profile the Arc cannot decode in hardware.
        clip_with_crf(&path, if compute == "hardware" { "1" } else { "0" }).await;
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
        assert_eq!(view.pending, 0);
        assert!(!run_one(&lib, &CancellationToken::new()).await.unwrap());
        let cleared = clear_activity(&lib, defs::STARTING_MOVIES).await.unwrap();
        assert!(cleared.jobs.is_empty());
        assert_eq!(cleared.savings, view.savings);
        assert_eq!(cleared.pending, 0);
        assert!(!run_one(&lib, &CancellationToken::new()).await.unwrap());
        assert_eq!(view.savings.saved_bytes, view.jobs[0].saved_bytes);
        assert_eq!(view.savings.original_bytes, view.jobs[0].original_bytes);
        assert_eq!(
            view.savings.current_bytes,
            view.jobs[0].result_bytes.unwrap()
        );
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

#[test]
fn undetermined_language_is_equivalent_but_named_languages_and_titles_are_preserved() {
    use serde_json::json;
    for kind in ["video", "audio", "subtitle"] {
        let mut before = process::Facts {
            streams: vec![
                json!({"codec_type":kind,"codec_name":"av1","tags":{"language":"und","title":"Kept"}}),
            ],
            chapters: vec![],
            format: json!({"duration":"1"}),
        };
        let mut after = process::Facts {
            streams: before.streams.clone(),
            chapters: vec![],
            format: before.format.clone(),
        };
        after.streams[0]["tags"]
            .as_object_mut()
            .unwrap()
            .remove("language");
        assert!(process::validate(&before, &after, "av1").is_ok());
        assert!(process::validate(&after, &before, "av1").is_ok());
        for language in ["eng", "fra", "zxx"] {
            before.streams[0]["tags"]["language"] = json!(language);
            assert!(
                process::validate(&before, &after, "av1")
                    .unwrap_err()
                    .to_string()
                    .contains("language changed")
            );
            assert!(process::validate(&after, &before, "av1").is_err());
            after.streams[0]["tags"]["language"] = json!(language);
            assert!(process::validate(&before, &after, "av1").is_ok());
            after.streams[0]["tags"]["language"] = json!("und");
        }
        before.streams[0]["tags"]["language"] = json!("und");
        after.streams[0]["tags"]["title"] = json!("Changed");
        assert!(
            process::validate(&before, &after, "av1")
                .unwrap_err()
                .to_string()
                .contains("title changed")
        );
    }
}

async fn mp4_roundtrip(compute: &str) {
    for codec in ["av1", "hevc"] {
        let (_dir, lib) = harness().await;
        let path = lib.layout.root().join("Movies/Undetermined.mp4");
        let mut cmd = tools::command(tools::FFMPEG);
        cmd.args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=24",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "2",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "1:a",
            "-c:v",
            "libx264",
            "-crf",
            "1",
            "-threads",
            "2",
            "-c:a",
            "aac",
            "-metadata:s:a:1",
            "language=eng",
        ])
        .arg(&path);
        tools::run_ok(cmd, tools::FFMPEG, Duration::from_secs(20))
            .await
            .unwrap();
        let facts = process::probe(&path).await.unwrap();
        assert_eq!(facts.streams[0]["tags"]["language"], "und");
        assert_eq!(facts.streams[1]["tags"]["language"], "und");
        let item = lib
            .index_path("Movies/Undetermined.mp4")
            .await
            .unwrap()
            .unwrap();
        save(
            &lib,
            defs::STARTING_MOVIES,
            Policy {
                enabled: true,
                codec: codec.into(),
                compute: compute.into(),
                ..Policy::default()
            },
        )
        .await
        .unwrap();
        assert!(run_one(&lib, &CancellationToken::new()).await.unwrap());
        let view = view(&lib, defs::STARTING_MOVIES).await.unwrap();
        assert_eq!(view.jobs[0].state, "complete", "{view:?}");
        assert!(view.jobs[0].saved_bytes > 0);
        let row = repo::get(&lib.db, &item.id).await.unwrap().unwrap();
        assert_eq!(row.rel_path, "Movies/Undetermined.mkv");
        let facts = process::probe(&lib.layout.root().join(&row.rel_path))
            .await
            .unwrap();
        assert_eq!(facts.streams[0]["codec_name"], codec);
        assert_eq!(facts.streams[1]["codec_name"], "aac");
        assert_eq!(facts.streams[2]["tags"]["language"], "eng");
        assert!(!path.exists());
    }
}

#[tokio::test]
async fn mp4_undetermined_language_passes_complete_conversion_checks() {
    mp4_roundtrip("cpu").await;
}

#[tokio::test]
#[ignore = "Requires a working Intel Arc VAAPI device; run explicitly with an isolated synthetic stash."]
async fn arc_mp4_undetermined_language_replacement() {
    mp4_roundtrip("hardware").await;
}

#[tokio::test]
#[ignore = "Requires a working Intel Arc VAAPI device; run explicitly in the production runtime with an isolated synthetic stash."]
async fn arc_av1_and_hevc_replacement_on_real_filesystem() {
    roundtrip("hardware").await;
}

#[tokio::test]
#[ignore = "Requires a working Intel Arc VAAPI device; run explicitly in the production runtime with an isolated synthetic stash."]
async fn arc_av1_quality_presets_affect_verified_output_size() {
    let mut sizes = Vec::new();
    for quality in ["high", "balanced", "small"] {
        let (_dir, lib) = harness().await;
        let path = lib.layout.root().join("Movies/Quality.mkv");
        clip_with_crf(&path, "1").await;
        lib.index_path("Movies/Quality.mkv").await.unwrap().unwrap();
        save(
            &lib,
            defs::STARTING_MOVIES,
            Policy {
                enabled: true,
                codec: "av1".into(),
                quality: quality.into(),
                compute: "hardware".into(),
            },
        )
        .await
        .unwrap();
        assert!(run_one(&lib, &CancellationToken::new()).await.unwrap());
        let view = view(&lib, defs::STARTING_MOVIES).await.unwrap();
        assert_eq!(view.jobs[0].state, "complete", "{quality}: {view:?}");
        assert!(view.jobs[0].saved_bytes > 0);
        sizes.push(view.jobs[0].result_bytes.unwrap());
    }
    // The old -qp bug ignored every AV1 preset and produced identical sizes.
    assert!(sizes[0] > sizes[1] && sizes[1] > sizes[2], "{sizes:?}");
}

#[tokio::test]
async fn savings_follow_verified_chains_and_current_library_membership() {
    let (_dir, lib) = harness().await;
    let rel = "Movies/Savings.mkv";
    std::fs::write(lib.layout.root().join(rel), b"fixture").unwrap();
    let item = lib.index_path(rel).await.unwrap().unwrap();
    sqlx::query("UPDATE items SET size=50,mtime=30,quick_hash='final' WHERE id=?")
        .bind(&item.id)
        .execute(&lib.db)
        .await
        .unwrap();
    for (source, output, saved, time) in [
        ("100:10:first", "70:20:middle", 30, 20),
        ("70:20:middle", "50:30:final", 20, 30),
    ] {
        sqlx::query("INSERT INTO conversion_jobs(id,item_id,library_id,revision,source_version,output_version,source_path,target_path,state,saved_bytes,created_at,updated_at) VALUES(?,?,?,1,?,?,?,?,'complete',?,?,?)")
            .bind(new_id()).bind(&item.id).bind(defs::STARTING_MOVIES)
            .bind(source).bind(output).bind(rel).bind(rel).bind(saved).bind(time).bind(time)
            .execute(&lib.db).await.unwrap();
    }
    let total = savings(&lib.db, defs::STARTING_MOVIES).await.unwrap();
    assert_eq!(
        (total.original_bytes, total.current_bytes, total.saved_bytes),
        (100, 50, 50)
    );
    assert_eq!(total.percent, 50.0);
    // Attribution follows current membership rather than the historical job library.
    sqlx::query("UPDATE items SET library_id=? WHERE id=?")
        .bind(defs::STARTING_SHOWS)
        .bind(&item.id)
        .execute(&lib.db)
        .await
        .unwrap();
    assert_eq!(
        savings(&lib.db, defs::STARTING_MOVIES).await.unwrap(),
        Savings::default()
    );
    assert_eq!(savings(&lib.db, defs::STARTING_SHOWS).await.unwrap(), total);
    // An external edit invalidates old savings even if the byte size is unchanged.
    sqlx::query("UPDATE items SET quick_hash='edited' WHERE id=?")
        .bind(&item.id)
        .execute(&lib.db)
        .await
        .unwrap();
    let edited = savings(&lib.db, defs::STARTING_SHOWS).await.unwrap();
    assert_eq!(
        (
            edited.original_bytes,
            edited.current_bytes,
            edited.saved_bytes
        ),
        (50, 50, 0)
    );
    sqlx::query("DELETE FROM items WHERE id=?")
        .bind(&item.id)
        .execute(&lib.db)
        .await
        .unwrap();
    assert_eq!(
        savings(&lib.db, defs::STARTING_SHOWS).await.unwrap(),
        Savings::default()
    );
}

#[tokio::test]
async fn migration_recovers_matching_legacy_results_without_counting_edited_files() {
    let (_dir, lib) = harness().await;
    for (name, mtime, expected) in [("Matching", 15, Some("50:15:legacy")), ("Edited", 25, None)] {
        let rel = format!("Movies/{name}.mkv");
        std::fs::write(lib.layout.root().join(&rel), b"fixture").unwrap();
        let item = lib.index_path(&rel).await.unwrap().unwrap();
        sqlx::query("UPDATE items SET size=50,mtime=?,quick_hash='legacy' WHERE id=?")
            .bind(mtime)
            .bind(&item.id)
            .execute(&lib.db)
            .await
            .unwrap();
        let job = new_id();
        sqlx::query("INSERT INTO conversion_jobs(id,item_id,library_id,revision,source_version,source_path,target_path,state,saved_bytes,created_at,updated_at) VALUES(?,?,?,1,'100:5:source',?,?,'complete',50,10,20)")
            .bind(&job).bind(&item.id).bind(defs::STARTING_MOVIES).bind(&rel).bind(&rel)
            .execute(&lib.db).await.unwrap();
        let migration = include_str!("../../../migrations/0009_conversion_savings.sql");
        let update = &migration[migration.find("UPDATE conversion_jobs").unwrap()..];
        sqlx::raw_sql(update).execute(&lib.db).await.unwrap();
        let version: Option<String> =
            sqlx::query_scalar("SELECT output_version FROM conversion_jobs WHERE id=?")
                .bind(&job)
                .fetch_one(&lib.db)
                .await
                .unwrap();
        assert_eq!(version.as_deref(), expected);
    }
    let total = savings(&lib.db, defs::STARTING_MOVIES).await.unwrap();
    assert_eq!(
        (total.original_bytes, total.current_bytes, total.saved_bytes),
        (150, 100, 50)
    );
}

#[tokio::test]
async fn policy_changes_and_progress_publish_conversion_events() {
    let (_dir, lib) = harness().await;
    let mut events = lib.events.subscribe();
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
    assert!(
        matches!(events.try_recv().unwrap(), crate::events::Event::Job {kind, state, item_id: None} if kind == "library_conversion" && state == "settings")
    );
    changed(&lib, "encoding");
    assert!(
        matches!(events.try_recv().unwrap(), crate::events::Event::Job {kind, state, item_id: None} if kind == "library_conversion" && state == "encoding")
    );
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
    let item = repo::get(&lib.db, &item.id).await.unwrap().unwrap();
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
            &Encoder {
                codec: "h264".into(),
                name: "libx264".into(),
                label: "CPU".into(),
                device: None,
                hardware: false
            },
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

#[tokio::test]
async fn completing_one_worker_keeps_another_workers_live_journal_and_output() {
    let (_dir, lib) = harness().await;
    let root = Root::open(lib.layout.root()).unwrap();
    let stage = root.new_trash_dir("conversion").unwrap();
    std::fs::write(
        lib.layout.root().join(&stage).join("output.mkv"),
        b"live output",
    )
    .unwrap();
    sqlx::query("INSERT INTO conversion_jobs(id,revision,source_version,source_path,target_path,state,stage,created_at,updated_at) VALUES('live',1,'v','Movies/Live.mkv','Movies/Live.mkv','encoding',?,0,0)")
        .bind(&stage).execute(&lib.db).await.unwrap();
    sqlx::query("INSERT INTO conversion_jobs(id,revision,source_version,source_path,target_path,state,created_at,updated_at) VALUES('reserved',1,'v','Movies/Reserved.mkv','Movies/Reserved.mkv','encoding',0,0),('finished',1,'v','Movies/Finished.mkv','Movies/Finished.mkv','skipped',0,0)")
        .execute(&lib.db).await.unwrap();
    publication::recover_job(&lib, "finished").await.unwrap();
    assert_eq!(
        std::fs::read(lib.layout.root().join(stage).join("output.mkv")).unwrap(),
        b"live output"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM conversion_jobs WHERE state='encoding'")
            .fetch_one(&lib.db)
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn parallel_claims_and_execution_never_share_an_item_or_recover_a_live_worker() {
    parallel_roundtrip("cpu").await;
}

#[tokio::test]
#[ignore = "Requires a working Intel Arc VAAPI device and full-feature driver; isolated synthetic stash only."]
async fn arc_parallel_replacements_and_gpu_verification() {
    parallel_roundtrip("hardware").await;
}

async fn parallel_roundtrip(compute: &str) {
    let (_dir, lib) = harness().await;
    clip_with_crf(
        &lib.layout.root().join("Movies/ParallelOne.mkv"),
        if compute == "hardware" { "1" } else { "0" },
    )
    .await;
    std::fs::copy(
        lib.layout.root().join("Movies/ParallelOne.mkv"),
        lib.layout.root().join("Movies/ParallelTwo.mkv"),
    )
    .unwrap();
    lib.index_path("Movies/ParallelOne.mkv").await.unwrap();
    lib.index_path("Movies/ParallelTwo.mkv").await.unwrap();
    save(
        &lib,
        defs::STARTING_MOVIES,
        Policy {
            enabled: true,
            compute: compute.into(),
            ..Policy::default()
        },
    )
    .await
    .unwrap();
    // The extra production slot must not claim a CPU policy, even on a GPU host.
    if compute == "cpu" {
        assert!(reserve(&lib, true).await.unwrap().is_none());
    }
    let a = reserve(&lib, compute == "hardware").await.unwrap().unwrap();
    let b = reserve(&lib, false).await.unwrap().unwrap();
    assert_ne!(a.0.id, b.0.id);
    assert!(reserve(&lib, false).await.unwrap().is_none());
    let cancel = CancellationToken::new();
    let (first, second) = tokio::join!(
        execute(&lib, a.0, a.1, a.2, &cancel),
        execute(&lib, b.0, b.1, b.2, &cancel)
    );
    first.unwrap();
    second.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM conversion_jobs WHERE state='complete' AND stage IS NULL"
        )
        .fetch_one(&lib.db)
        .await
        .unwrap(),
        2
    );
    assert!(
        std::fs::read_dir(lib.layout.root().join(".popcornstash/Trash"))
            .unwrap()
            .next()
            .is_none()
    );
}

#[tokio::test]
async fn unavailable_gpu_verification_falls_back_to_a_full_software_check() {
    let (_dir, lib) = harness().await;
    let source = lib.layout.root().join("Movies/Fallback.mkv");
    clip(&source).await;
    let item = lib
        .index_path("Movies/Fallback.mkv")
        .await
        .unwrap()
        .unwrap();
    let item = repo::get(&lib.db, &item.id).await.unwrap().unwrap();
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
    let job = Journal {
        id: "fallback".into(),
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
    let encoder = Encoder {
        codec: "h264".into(),
        name: "h264_vaapi".into(),
        label: "Unavailable GPU".into(),
        device: Some("/dev/dri/does-not-exist".into()),
        hardware: true,
    };
    assert!(current(&lib, &job).await.unwrap());
    let facts = process::probe(&source).await.unwrap();
    process::verify(
        &lib,
        &job,
        &source,
        &source,
        &facts,
        &encoder,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let bytes = std::fs::read(&source).unwrap();
    let truncated = _dir.path().join("truncated.mkv");
    std::fs::write(&truncated, &bytes[..bytes.len() * 2 / 3]).unwrap();
    assert!(
        process::verify(
            &lib,
            &job,
            &source,
            &truncated,
            &facts,
            &encoder,
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

async fn legacy_roundtrip(compute: &str) {
    for (extension, video, audio) in [("rm", "rv20", "ac3"), ("wmv", "wmv2", "wmav2")] {
        for codec in ["av1", "hevc"] {
            let (_dir, lib) = harness().await;
            let relative = format!("Movies/Legacy.{extension}");
            let path = lib.layout.root().join(&relative);
            let mut cmd = tools::command(tools::FFMPEG);
            cmd.args([
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=25",
                "-f",
                "lavfi",
                "-i",
                "sine=sample_rate=44100",
                "-t",
                "3",
                "-c:v",
                video,
                "-b:v",
                "1800k",
                "-c:a",
                audio,
                "-b:a",
                "128k",
                "-threads",
                "2",
            ])
            .arg(&path);
            tools::run_ok(cmd, tools::FFMPEG, Duration::from_secs(30))
                .await
                .unwrap();
            let item = lib.index_path(&relative).await.unwrap().unwrap();
            assert_eq!(item.kind, MediaKind::Video);
            save(
                &lib,
                defs::STARTING_MOVIES,
                Policy {
                    enabled: true,
                    codec: codec.into(),
                    compute: compute.into(),
                    ..Policy::default()
                },
            )
            .await
            .unwrap();
            assert!(run_one(&lib, &CancellationToken::new()).await.unwrap());
            let view = view(&lib, defs::STARTING_MOVIES).await.unwrap();
            assert_eq!(
                view.jobs[0].state, "complete",
                "{extension}/{codec}: {view:?}"
            );
            assert!(view.jobs[0].saved_bytes > 0);
            assert!(!path.exists());
            let after = repo::get(&lib.db, &item.id).await.unwrap().unwrap();
            assert_eq!(after.rel_path, "Movies/Legacy.mkv");
            let facts = process::probe(&lib.layout.root().join(after.rel_path))
                .await
                .unwrap();
            assert_eq!(facts.streams[0]["codec_name"], codec);
            assert_eq!(facts.streams[1]["codec_name"], audio);
            assert_eq!(facts.streams[0]["width"], 320);
            assert_eq!(facts.streams[0]["height"], 240);
            assert_eq!(view.pending, 0);
        }
    }
}

#[tokio::test]
async fn legacy_realmedia_and_windows_media_become_smaller_verified_mkv() {
    legacy_roundtrip("cpu").await;
}

#[tokio::test]
#[ignore = "Requires a real Intel Arc GPU and isolated writable test filesystem"]
async fn arc_legacy_realmedia_and_windows_media_replacement() {
    legacy_roundtrip("hardware").await;
}

#[tokio::test]
async fn clearing_activity_preserves_active_jobs_and_recovery_journals() {
    let (_dir, lib) = harness().await;
    for (id, state, stage) in [
        ("active", "encoding", None),
        ("journal", "complete", Some("private")),
        ("failed", "failed", None),
        ("skipped", "skipped", None),
    ] {
        sqlx::query("INSERT INTO conversion_jobs(id,library_id,revision,source_version,source_path,target_path,state,stage,created_at,updated_at) VALUES(?,?,1,'10:1:hash','Movies/a.rm','Movies/a.mkv',?,?,1,1)")
            .bind(id).bind(defs::STARTING_MOVIES).bind(state).bind(stage).execute(&lib.db).await.unwrap();
    }
    let cleared = clear_activity(&lib, defs::STARTING_MOVIES).await.unwrap();
    assert_eq!(cleared.jobs.len(), 2);
    assert_eq!(cleared.retryable, 1);
    assert!(cleared.jobs.iter().any(|job| job.id == "active"));
    assert!(cleared.jobs.iter().any(|job| job.id == "journal"));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conversion_jobs")
        .fetch_one(&lib.db)
        .await
        .unwrap();
    assert_eq!(count, 4);
}

#[test]
fn legacy_video_extensions_are_video_in_scanning_and_torrent_rules() {
    for file in ["movie.rm", "movie.RMVB", "movie.wmv", "movie.ASF"] {
        assert_eq!(
            super::super::classify::media_kind(Path::new(file)),
            MediaKind::Video
        );
        assert_eq!(
            crate::internal_api::TorrentFileKind::of(file),
            crate::internal_api::TorrentFileKind::Video
        );
    }
}

#[test]
fn unspecified_legacy_aspect_matches_square_pixels_but_not_non_square_pixels() {
    use serde_json::json;
    let before = process::Facts {
        streams: vec![json!({"codec_type":"video","codec_name":"rv20","width":320,"height":240})],
        chapters: vec![],
        format: json!({"duration":"3"}),
    };
    let mut after = process::Facts {
        streams: before.streams.clone(),
        chapters: vec![],
        format: before.format.clone(),
    };
    after.streams[0]["codec_name"] = json!("av1");
    after.streams[0]["sample_aspect_ratio"] = json!("1:1");
    assert!(process::validate(&before, &after, "av1").is_ok());
    after.streams[0]["sample_aspect_ratio"] = json!("6:5");
    assert!(process::validate(&before, &after, "av1").is_err());
}

#[tokio::test]
async fn completed_output_does_not_block_a_different_library_policy() {
    let (_dir, lib) = harness().await;
    let path = lib.layout.root().join("Movies/Move.mkv");
    std::fs::write(&path, b"synthetic video input").unwrap();
    let item = lib.index_path("Movies/Move.mkv").await.unwrap().unwrap();
    sqlx::query("INSERT INTO conversion_jobs(id,item_id,library_id,revision,source_version,output_version,source_path,target_path,state,created_at,updated_at) VALUES('prior',?,?,1,'old',?,'Movies/Move.rm','Movies/Move.mkv','complete',1,1)")
        .bind(&item.id).bind(defs::STARTING_SHOWS).bind(version(&item)).execute(&lib.db).await.unwrap();
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
    assert_eq!(view(&lib, defs::STARTING_MOVIES).await.unwrap().pending, 1);
    assert!(reserve(&lib, false).await.unwrap().is_some());
}

#[tokio::test]
async fn migration_hides_only_redundant_finished_checks_and_keeps_savings() {
    let (_dir, lib) = harness().await;
    std::fs::write(lib.layout.root().join("Movies/Finished.mkv"), b"fixture").unwrap();
    let item = lib
        .index_path("Movies/Finished.mkv")
        .await
        .unwrap()
        .unwrap();
    sqlx::query("INSERT INTO conversion_jobs(id,item_id,library_id,revision,source_version,output_version,source_path,target_path,state,saved_bytes,created_at,updated_at) VALUES('completed',?,?,1,'100:5:source',?,'Movies/Finished.rm','Movies/Finished.mkv','complete',93,1,1)")
        .bind(&item.id).bind(defs::STARTING_MOVIES).bind(version(&item)).execute(&lib.db).await.unwrap();
    for (id, revision, source, state, stage) in [
        ("redundant", 1, version(&item), "skipped", None),
        ("other_revision", 2, version(&item), "skipped", None),
        ("other_version", 1, "new".into(), "skipped", None),
        ("active", 3, version(&item), "encoding", None),
        ("journal", 4, version(&item), "skipped", Some("private")),
    ] {
        sqlx::query("INSERT INTO conversion_jobs(id,item_id,library_id,revision,source_version,source_path,target_path,state,stage,message,created_at,updated_at) VALUES(?,?,?,?,?,'Movies/Finished.mkv','Movies/Finished.mkv',?,?,'Already in the selected format.',2,2)")
            .bind(id).bind(&item.id).bind(defs::STARTING_MOVIES).bind(revision).bind(source).bind(state).bind(stage).execute(&lib.db).await.unwrap();
    }
    let before = savings(&lib.db, defs::STARTING_MOVIES).await.unwrap();
    assert_eq!(before.saved_bytes, 93);
    let migration = include_str!("../../../migrations/0011_conversion_activity.sql");
    sqlx::raw_sql(&migration[migration.find("UPDATE conversion_jobs").unwrap()..])
        .execute(&lib.db)
        .await
        .unwrap();
    let hidden: Vec<String> =
        sqlx::query_scalar("SELECT id FROM conversion_jobs WHERE activity_hidden=1")
            .fetch_all(&lib.db)
            .await
            .unwrap();
    assert_eq!(hidden, vec!["redundant"]);
    assert_eq!(
        savings(&lib.db, defs::STARTING_MOVIES).await.unwrap(),
        before
    );
}

#[tokio::test]
async fn incompatible_legacy_audio_converts_without_exempting_copied_tracks() {
    for (legacy, rate) in [("cook", "44100"), ("sipr", "16000")] {
        let (_dir, lib) = harness().await;
        let source = lib.layout.root().join("Movies/Audio.mkv");
        let mut cmd = tools::command(tools::FFMPEG);
        cmd.args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=25",
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:sample_rate={rate}"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=880:sample_rate={rate}"),
            "-t",
            "2",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-c:v",
            "mpeg4",
            "-c:a",
            "pcm_f32le",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:a:1",
            "language=fra",
            "-disposition:a:0",
            "0",
            "-disposition:a:1",
            "0",
        ])
        .arg(&source);
        tools::run_ok(cmd, tools::FFMPEG, Duration::from_secs(30))
            .await
            .unwrap();
        lib.index_path("Movies/Audio.mkv").await.unwrap().unwrap();
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
        let (_, p, job) = reserve(&lib, false).await.unwrap().unwrap();
        let encoder = process::choose_encoder(&p).await.unwrap();
        let mut facts = process::probe(&source).await.unwrap();
        // PCM stands in for the legacy decoder output; Cook/SIPR have no encoder.
        // Separate ignored real-source tests exercise their actual demuxers/decoders.
        facts.streams[1]["codec_name"] = serde_json::json!(legacy);
        let output = _dir.path().join("converted.mkv");
        let cancel = CancellationToken::new();
        process::encode(&lib, &job, &p, &encoder, &source, &output, &facts, &cancel)
            .await
            .unwrap();
        let after = process::probe(&output).await.unwrap();
        assert_eq!(after.streams[1]["codec_name"], "aac");
        assert_eq!(after.streams[2]["codec_name"], "pcm_f32le");
        process::verify(&lib, &job, &source, &output, &facts, &encoder, &cancel)
            .await
            .unwrap();
        let tampered = _dir.path().join("tampered.mkv");
        let mut cmd = tools::command(tools::FFMPEG);
        cmd.args(["-v", "error", "-nostdin", "-i"])
            .arg(&output)
            .args([
                "-map",
                "0",
                "-c",
                "copy",
                "-c:a:1",
                "pcm_f32le",
                "-filter:a:1",
                "volume=0.5",
                "-disposition:a:0",
                "0",
                "-disposition:a:1",
                "0",
            ])
            .arg(&tampered);
        tools::run_ok(cmd, tools::FFMPEG, Duration::from_secs(30))
            .await
            .unwrap();
        let error = process::verify(&lib, &job, &source, &tampered, &facts, &encoder, &cancel)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("Audio or subtitle data changed"), "{error}");
    }
}

#[tokio::test]
#[ignore = "Requires real legacy fixtures mounted read-only and Intel Arc; all writes remain in an isolated RAM filesystem"]
async fn arc_incompatible_realaudio_becomes_verified_modern_mkv() {
    let root = std::env::var_os("POPCORNSTASH_LEGACY_FIXTURES")
        .expect("Provide read-only legacy fixtures");
    let root = Path::new(&root);
    let mut completed = 0;
    for name in ["cook.rm", "sipr.rm", "rv40.rm"] {
        let original = root.join(name);
        let hash = process::hash(&original).await.unwrap();
        for codec in ["av1", "hevc"] {
            let (_dir, lib) = harness().await;
            let source = lib.layout.root().join("Movies/Legacy.rm");
            std::fs::copy(&original, &source).unwrap();
            let item = lib.index_path("Movies/Legacy.rm").await.unwrap().unwrap();
            save(
                &lib,
                defs::STARTING_MOVIES,
                Policy {
                    enabled: true,
                    codec: codec.into(),
                    compute: "hardware".into(),
                    ..Policy::default()
                },
            )
            .await
            .unwrap();
            assert!(run_one(&lib, &CancellationToken::new()).await.unwrap());
            let result = view(&lib, defs::STARTING_MOVIES).await.unwrap();
            let job = &result.jobs[0];
            match job.state.as_str() {
                "complete" => {
                    completed += 1;
                    assert!(job.saved_bytes > 0);
                    assert!(!source.exists());
                    let current = repo::get(&lib.db, &item.id).await.unwrap().unwrap();
                    let after = process::probe(&lib.layout.root().join(current.rel_path))
                        .await
                        .unwrap();
                    assert_eq!(
                        after
                            .streams
                            .iter()
                            .find(|s| s["codec_type"] == "video")
                            .unwrap()["codec_name"],
                        codec
                    );
                    assert_eq!(
                        after
                            .streams
                            .iter()
                            .find(|s| s["codec_type"] == "audio")
                            .unwrap()["codec_name"],
                        "aac"
                    );
                }
                "skipped" => {
                    assert!(
                        job.message.as_deref().unwrap_or("").contains("smaller"),
                        "{result:?}"
                    );
                    assert_eq!(process::hash(&source).await.unwrap(), hash);
                }
                _ => panic!("{name}/{codec}: {result:?}"),
            }
            assert_eq!(result.pending, 0);
        }
        assert_eq!(process::hash(&original).await.unwrap(), hash);
    }
    assert!(
        completed >= 3,
        "Expected verified smaller legacy replacements: {completed}"
    );
}
