use command_center_core::{
    archive::VaultEntry,
    collections::{Collection, Search},
    now,
    vault_codec::{new_key, reveal, seal},
    Item, Repository,
};
use sha2::{Digest, Sha256};
fn note(repo: &mut Repository, title: &str, body: &str) -> Item {
    repo.save(Item {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "note".into(),
        title: title.into(),
        body: body.into(),
        pinned: false,
        priority: "normal".into(),
        status: "active".into(),
        due: None,
        recurrence: "none".into(),
        url: None,
        created_at: now(),
        updated_at: now(),
        device_id: repo.device_id.clone(),
        revision: 1,
        deleted_at: None,
    })
    .unwrap()
}
const PASSWORD: &str = "synthetic-stream-backup-password";
#[test]
fn streaming_roundtrip_keeps_conflicts_and_excludes_permissions() {
    let mut source = Repository::open(":memory:").unwrap();
    let item = note(&mut source, "Fixture", "Source version");
    source.set_setting("background_reminders", "true").unwrap();
    source
        .set_setting(
            "preferences",
            r#"{"notifications":true,"quiet_start":22,"quiet_end":8,"enabled_modules":[]}"#,
        )
        .unwrap();
    source
        .set_setting("google_accounts", "private permission fixture")
        .unwrap();
    source
        .save_research_collections(&[Collection {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Research".into(),
            searches: vec![Search {
                source: "ollama".into(),
                query: "".into(),
            }],
        }])
        .unwrap();
    let mut bytes = Vec::new();
    source.export_stream(&mut bytes, PASSWORD, None).unwrap();
    let checksum = format!("{:x}", Sha256::digest(&bytes));
    let mut target = Repository::open(":memory:").unwrap();
    let summary = target.inspect_stream(&bytes[..], PASSWORD).unwrap();
    assert_eq!(summary.new_records, 1);
    let imported = target
        .import_stream_checked(&bytes[..], PASSWORD, None, Some(&checksum))
        .unwrap();
    assert_eq!(imported.imported, 1);
    let mut local = item;
    local.body = "Local version".into();
    target.save(local).unwrap();
    assert_eq!(
        target
            .import_stream(&bytes[..], PASSWORD, None)
            .unwrap()
            .imported,
        0
    );
    assert_eq!(target.list("", false).unwrap()[0].body, "Local version");
    assert_eq!(target.research_collections().unwrap().len(), 1);
    assert!(target.setting("background_reminders").unwrap().is_none());
    assert!(target.setting("google_accounts").unwrap().is_none());
    let prefs: serde_json::Value =
        serde_json::from_str(&target.setting("preferences").unwrap().unwrap()).unwrap();
    assert!(prefs.get("notifications").is_none());
}
#[test]
fn corrupt_truncated_wrong_password_and_review_mismatch_are_atomic() {
    let mut source = Repository::open(":memory:").unwrap();
    note(&mut source, "Fixture", "Body");
    let mut bytes = Vec::new();
    source.export_stream(&mut bytes, PASSWORD, None).unwrap();
    for mode in 0..5 {
        let mut target = Repository::open(":memory:").unwrap();
        let mut damaged = bytes.clone();
        let result = match mode {
            0 => target.import_stream(&damaged[..], "wrong-password", None),
            1 => {
                damaged.truncate(damaged.len() - 8);
                target.import_stream(&damaged[..], PASSWORD, None)
            }
            2 => {
                let last = damaged.len() - 1;
                damaged[last] ^= 1;
                target.import_stream(&damaged[..], PASSWORD, None)
            }
            3 => target.import_stream_checked(
                &damaged[..],
                PASSWORD,
                None,
                Some("not-the-reviewed-sha256"),
            ),
            _ => {
                damaged.extend_from_slice(b"extra");
                target.import_stream(&damaged[..], PASSWORD, None)
            }
        };
        assert!(result.is_err());
        assert!(target.list("", true).unwrap().is_empty());
        assert_eq!(target.snapshot("").unwrap().changes, 0);
    }
}
#[test]
fn private_records_are_reencrypted_with_receiver_key() {
    let mut source = Repository::open(":memory:").unwrap();
    let a = new_key();
    let b = new_key();
    let entry = VaultEntry {
        id: uuid::Uuid::new_v4().to_string(),
        title: "Private fixture".into(),
        category: "document".into(),
        body: "Synthetic secret".into(),
        updated_at: now(),
        ..Default::default()
    };
    let cipher = seal(&*a, &entry).unwrap();
    source
        .save_vault_if_unchanged(&entry.id, &cipher, None)
        .unwrap();
    let mut bytes = Vec::new();
    source
        .export_stream(&mut bytes, PASSWORD, Some(&*a))
        .unwrap();
    let mut target = Repository::open(":memory:").unwrap();
    assert!(target.import_stream(&bytes[..], PASSWORD, None).is_err());
    assert!(target.vault_rows().unwrap().is_empty());
    target
        .import_stream(&bytes[..], PASSWORD, Some(&*b))
        .unwrap();
    let rows = target.vault_rows().unwrap();
    assert_eq!(
        reveal(&*b, &rows[0].0, &rows[0].1).unwrap().body,
        "Synthetic secret"
    );
    assert!(reveal(&*a, &rows[0].0, &rows[0].1).is_err());
}
#[test]
fn streams_beyond_standard_archive_size_without_collecting_the_output() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = Repository::open(dir.path().join("source.db")).unwrap();
    let body = "x".repeat(80_000);
    for i in 0..430 {
        note(&mut source, &format!("Large fixture {i}"), &body);
    }
    let file = std::fs::File::create(dir.path().join("large.paccs")).unwrap();
    source
        .export_stream(std::io::BufWriter::new(file), PASSWORD, None)
        .unwrap();
    assert!(
        std::fs::metadata(dir.path().join("large.paccs"))
            .unwrap()
            .len()
            > 64 * 1024 * 1024
    );
    let mut target = Repository::open(dir.path().join("target.db")).unwrap();
    let result = target
        .import_stream(
            std::io::BufReader::new(std::fs::File::open(dir.path().join("large.paccs")).unwrap()),
            PASSWORD,
            None,
        )
        .unwrap();
    assert_eq!(result.imported, 430);
    assert_eq!(target.list("Large fixture", false).unwrap().len(), 430);
}
#[test]
fn standard_archives_accept_all_nine_current_feed_sources() {
    let mut data = command_center_core::archive::ExportData::default();
    for source in [
        "github", "models", "ollama", "news", "mcp", "apps", "youtube", "arxiv", "rss",
    ] {
        data.workspace
            .feeds
            .push(command_center_core::workspace::CachedFeed {
                source: source.into(),
                fetched_at: now(),
                items: serde_json::json!([]),
            });
    }
    let bytes = command_center_core::archive::export_all(&data).unwrap();
    assert_eq!(
        command_center_core::archive::inspect_all(&bytes)
            .unwrap()
            .workspace
            .feeds
            .len(),
        9
    );
}
#[test]
fn streaming_preserves_workspace_relationships_and_original_attachments() {
    use base64::Engine;
    use command_center_core::{attachments::Attachment, workspace::Record};
    let mut source = Repository::open(":memory:").unwrap();
    let item = note(&mut source, "Owner", "Body");
    let mut record = Record::new("projects", "Linked workspace", &source.device_id);
    record.links.push(item.id.clone());
    record.fields.insert("next_action".into(), "Review".into());
    source.save_record(record).unwrap();
    let bytes = b"Synthetic attachment bytes";
    let a = Attachment {
        id: uuid::Uuid::new_v4().to_string(),
        owner_id: item.id.clone(),
        filename: "fixture.txt".into(),
        mime: "text/plain".into(),
        content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        created_at: now(),
        previous_id: None,
        deleted_at: None,
        extracted_text: "Synthetic".into(),
    };
    source.add_attachment(&a).unwrap();
    let mut archive = vec![];
    source.export_stream(&mut archive, PASSWORD, None).unwrap();
    let mut target = Repository::open(":memory:").unwrap();
    target.import_stream(&archive[..], PASSWORD, None).unwrap();
    assert!(target.attachment(&a.id).unwrap() == a);
    assert_eq!(target.records("", false).unwrap()[0].links[0], item.id);
    assert_eq!(
        target
            .import_stream(&archive[..], PASSWORD, None)
            .unwrap()
            .imported,
        0
    );
}
