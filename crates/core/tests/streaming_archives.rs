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

#[test]
fn linked_copies_preserve_originals_and_remap_relationships_and_history() {
    use base64::Engine;
    use command_center_core::{attachments::Attachment, workspace::Record};
    use std::io::Cursor;
    let mut source = Repository::open(":memory:").unwrap();
    let owner = note(&mut source, "Owner", "Source body");
    let mut task = owner.clone();
    task.id = uuid::Uuid::new_v4().to_string();
    task.kind = "task".into();
    task.title = "Prerequisite".into();
    let task = source.save(task).unwrap();
    let mut dependent = task.clone();
    dependent.id = uuid::Uuid::new_v4().to_string();
    dependent.title = "Dependent".into();
    dependent.recurrence = format!("after:{}", task.id);
    let dependent = source.save(dependent).unwrap();
    let mut project = Record::new("projects", "Project", &source.device_id);
    project.links.push(owner.id.clone());
    let project = source.save_record(project).unwrap();
    let mut child = Record::new("ideas", "Owned idea", &source.device_id);
    child
        .fields
        .insert("workspace_owner".into(), project.id.clone());
    source.save_record(child).unwrap();
    let bytes = b"Copy fixture attachment";
    let attachment = Attachment {
        id: uuid::Uuid::new_v4().to_string(),
        owner_id: owner.id.clone(),
        filename: "copy.txt".into(),
        mime: "text/plain".into(),
        content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        created_at: now(),
        previous_id: None,
        deleted_at: None,
        extracted_text: "Copy fixture".into(),
    };
    source.add_attachment(&attachment).unwrap();
    let mut newer = attachment.clone();
    newer.id = uuid::Uuid::new_v4().to_string();
    newer.previous_id = Some(attachment.id.clone());
    source.add_attachment(&newer).unwrap();
    source
        .set_setting("preferences", r#"{"quiet_start":22}"#)
        .unwrap();
    let mut archive = Vec::new();
    source.export_stream(&mut archive, PASSWORD, None).unwrap();
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let mut target = Repository::open(":memory:").unwrap();
    target.import_stream(&archive[..], PASSWORD, None).unwrap();
    let mut local = owner.clone();
    local.body = "Keep this local edit".into();
    let local = target.save(local).unwrap();
    target
        .set_setting("preferences", r#"{"quiet_start":21}"#)
        .unwrap();
    let history_before = target.history().unwrap();
    let result = target
        .import_stream_copies(Cursor::new(&archive), PASSWORD, None, Some(&checksum))
        .unwrap();
    assert_eq!(result.imported, 7);
    let items = target.list("", false).unwrap();
    assert_eq!(items.len(), 6);
    assert!(items.iter().find(|i| i.id == owner.id).unwrap() == &local);
    let copy = items
        .iter()
        .find(|i| i.title == "Owner (imported copy)")
        .unwrap();
    assert_ne!(copy.id, owner.id);
    assert_eq!(copy.body, "Source body");
    assert_eq!(copy.revision, 1);
    assert_eq!(copy.device_id, target.device_id);
    let parent = items
        .iter()
        .find(|i| i.title == "Prerequisite (imported copy)")
        .unwrap();
    let child_task = items
        .iter()
        .find(|i| i.title == "Dependent (imported copy)")
        .unwrap();
    assert_eq!(child_task.recurrence, format!("after:{}", parent.id));
    assert_eq!(
        items
            .iter()
            .find(|i| i.id == dependent.id)
            .unwrap()
            .recurrence,
        dependent.recurrence
    );
    let records = target.records("", false).unwrap();
    let project_copy = records
        .iter()
        .find(|r| r.title == "Project (imported copy)")
        .unwrap();
    let idea_copy = records
        .iter()
        .find(|r| r.title == "Owned idea (imported copy)")
        .unwrap();
    assert_eq!(project_copy.links, vec![copy.id.clone()]);
    assert_eq!(idea_copy.fields["workspace_owner"], project_copy.id);
    assert_eq!(idea_copy.links, vec![project_copy.id.clone()]);
    let attachments = target.attachments(Some(&copy.id), "", false).unwrap();
    assert_eq!(attachments.len(), 2);
    let first = attachments
        .iter()
        .find(|a| a.previous_id.is_none())
        .unwrap();
    let second = attachments
        .iter()
        .find(|a| a.previous_id.is_some())
        .unwrap();
    assert_eq!(second.previous_id.as_deref(), Some(first.id.as_str()));
    assert_eq!(first.content_base64, attachment.content_base64);
    assert!(target.attachment(&attachment.id).unwrap() == attachment);
    for event in target
        .history()
        .unwrap()
        .iter()
        .filter(|e| !history_before.iter().any(|old| old.event_id == e.event_id))
    {
        assert!(![&owner.id, &task.id, &dependent.id, &project.id].contains(&&event.entity_id));
        let payload: serde_json::Value = serde_json::from_str(&event.payload).unwrap();
        assert_eq!(payload["id"].as_str(), Some(event.entity_id.as_str()));
        if payload["title"] == "Dependent" {
            assert_eq!(payload["recurrence"], format!("after:{}", parent.id));
        }
    }
    assert_eq!(
        target.setting("preferences").unwrap().unwrap(),
        r#"{"quiet_start":21}"#
    );
}

#[test]
fn linked_copy_private_records_use_fresh_ids_and_receiver_key() {
    let mut source = Repository::open(":memory:").unwrap();
    let a = new_key();
    let b = new_key();
    let entry = VaultEntry {
        id: uuid::Uuid::new_v4().to_string(),
        title: "私".repeat(166),
        category: "document".into(),
        body: "Private copy fixture".into(),
        updated_at: now(),
        ..Default::default()
    };
    source
        .save_vault_if_unchanged(&entry.id, &seal(&*a, &entry).unwrap(), None)
        .unwrap();
    let mut archive = Vec::new();
    source
        .export_stream(&mut archive, PASSWORD, Some(&*a))
        .unwrap();
    let mut target = Repository::open(":memory:").unwrap();
    target
        .import_stream_copies(std::io::Cursor::new(&archive), PASSWORD, Some(&*b), None)
        .unwrap();
    let rows = target.vault_rows().unwrap();
    assert_eq!(rows.len(), 1);
    assert_ne!(rows[0].0, entry.id);
    let copy = reveal(&*b, &rows[0].0, &rows[0].1).unwrap();
    assert_eq!(copy.body, entry.body);
    assert!(copy.title.ends_with(" (imported copy)"));
    assert!(copy.title.len() <= 500);
    assert!(reveal(&*a, &rows[0].0, &rows[0].1).is_err());
}

#[test]
fn linked_copy_rejects_bad_review_and_changed_second_pass_atomically() {
    use std::io::{Cursor, Read, Seek, SeekFrom};
    struct ChangesAfterRewind {
        cursor: Cursor<Vec<u8>>,
        rewinds: u8,
    }
    impl Read for ChangesAfterRewind {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.cursor.read(b)
        }
    }
    impl Seek for ChangesAfterRewind {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            if matches!(pos, SeekFrom::Start(0)) {
                self.rewinds += 1;
                if self.rewinds == 2 {
                    let n = self.cursor.get_ref().len();
                    self.cursor.get_mut()[n - 1] ^= 1;
                }
            }
            self.cursor.seek(pos)
        }
    }
    let mut source = Repository::open(":memory:").unwrap();
    note(&mut source, "Original", "Fixture");
    let mut archive = Vec::new();
    source.export_stream(&mut archive, PASSWORD, None).unwrap();
    let mut target = Repository::open(":memory:").unwrap();
    assert!(target
        .import_stream_copies(Cursor::new(&archive), PASSWORD, None, Some("wrong-review"))
        .is_err());
    assert!(target
        .import_stream_copies(
            ChangesAfterRewind {
                cursor: Cursor::new(archive),
                rewinds: 0
            },
            PASSWORD,
            None,
            None
        )
        .is_err());
    assert!(target.list("", true).unwrap().is_empty());
    assert!(target.history().unwrap().is_empty());
}
