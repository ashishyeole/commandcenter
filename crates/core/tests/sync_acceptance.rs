use base64::{engine::general_purpose::STANDARD, Engine};
use command_center_core::{attachments::Attachment, now, Item, Repository};
use sha2::{Digest, Sha256};
use uuid::Uuid;
fn note(repo: &mut Repository) -> Item {
    repo.save(Item {
        id: Uuid::new_v4().to_string(),
        kind: "note".into(),
        title: "Fixture note".into(),
        body: "Original body".into(),
        pinned: true,
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
fn document(owner: &str, n: usize) -> Attachment {
    let bytes = vec![42; n];
    Attachment {
        id: Uuid::new_v4().to_string(),
        owner_id: owner.into(),
        filename: "fixture.bin".into(),
        mime: "application/octet-stream".into(),
        content_base64: STANDARD.encode(&bytes),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        created_at: now(),
        previous_id: None,
        deleted_at: None,
        extracted_text: "Test document evidence".into(),
    }
}
fn scopes() -> Vec<String> {
    vec!["note".into(), "attachment".into()]
}
fn transfer(a: &mut Repository, b: &mut Repository, s: &[String]) -> usize {
    let peer = a.device_id.clone();
    let mut changed = 0;
    for _ in 0..100 {
        let after = b.sync_progress(&peer).unwrap().1;
        let batch = a.sync_batch(after, s).unwrap();
        changed += b.apply_sync_batch(&peer, &batch, s).unwrap();
        a.sync_ack(&b.device_id, batch.through).unwrap();
        if !batch.more {
            return changed;
        }
    }
    panic!("Sync did not converge")
}
#[test]
fn attachment_roundtrip_reopen_and_idempotence() {
    let d = tempfile::tempdir().unwrap();
    let mut a = Repository::open(d.path().join("a.db")).unwrap();
    let mut b = Repository::open(d.path().join("b.db")).unwrap();
    let n = note(&mut a);
    let f = document(&n.id, 4096);
    a.add_attachment(&f).unwrap();
    assert_eq!(transfer(&mut a, &mut b, &scopes()), 2);
    assert!(b.attachment(&f.id).unwrap() == f);
    assert_eq!(transfer(&mut a, &mut b, &scopes()), 0);
    drop(b);
    let b = Repository::open(d.path().join("b.db")).unwrap();
    assert!(b.attachment(&f.id).unwrap() == f);
}
#[test]
fn file_requires_both_attachment_and_parent_scope() {
    let mut a = Repository::open(":memory:").unwrap();
    let mut b = Repository::open(":memory:").unwrap();
    let n = note(&mut a);
    let f = document(&n.id, 10);
    a.add_attachment(&f).unwrap();
    transfer(&mut a, &mut b, &["note".into()]);
    assert!(b.attachment(&f.id).is_err());
    let mut c = Repository::open(":memory:").unwrap();
    transfer(&mut a, &mut c, &["attachment".into()]);
    assert!(c.attachment(&f.id).is_err());
}
#[test]
fn attachment_checksum_failure_does_not_advance_cursor() {
    let mut a = Repository::open(":memory:").unwrap();
    let mut b = Repository::open(":memory:").unwrap();
    let n = note(&mut a);
    let f = document(&n.id, 10);
    a.add_attachment(&f).unwrap();
    let first = a.sync_batch(0, &scopes()).unwrap();
    b.apply_sync_batch(&a.device_id, &first, &scopes()).unwrap();
    let cursor = b.sync_progress(&a.device_id).unwrap().1;
    let good = a.sync_batch(cursor, &scopes()).unwrap();
    let mut bad = good.clone();
    bad.events[0].payload["sha256"] = serde_json::json!("0".repeat(64));
    assert!(b.apply_sync_batch(&a.device_id, &bad, &scopes()).is_err());
    assert_eq!(b.sync_progress(&a.device_id).unwrap().1, cursor);
    assert!(b.attachment(&f.id).is_err());
    b.apply_sync_batch(&a.device_id, &good, &scopes()).unwrap();
    assert!(b.attachment(&f.id).unwrap() == f);
}
#[test]
fn attachment_delete_vs_index_edit_is_reviewable() {
    let mut a = Repository::open(":memory:").unwrap();
    let mut b = Repository::open(":memory:").unwrap();
    let n = note(&mut a);
    let f = document(&n.id, 30);
    a.add_attachment(&f).unwrap();
    transfer(&mut a, &mut b, &scopes());
    a.trash_attachment(&f.id, true).unwrap();
    b.index_attachment_text(&f.id, &f.sha256, "Edited extraction")
        .unwrap();
    transfer(&mut a, &mut b, &scopes());
    let conflicts = b.sync_conflicts().unwrap();
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].local.get("content_base64").is_none());
    let rev = conflicts[0].local["revision"].as_i64().unwrap();
    b.resolve_sync_conflict(&conflicts[0].id, "local", rev, None)
        .unwrap();
    assert!(b.attachment(&f.id).unwrap().deleted_at.is_none());
    transfer(&mut b, &mut a, &scopes());
    assert_eq!(
        a.attachment(&f.id).unwrap().extracted_text,
        "Edited extraction"
    );
    assert!(a.attachment(&f.id).unwrap().deleted_at.is_none());
}
#[test]
fn concurrent_note_edits_never_silently_overwrite() {
    let mut a = Repository::open(":memory:").unwrap();
    let mut b = Repository::open(":memory:").unwrap();
    let mut n = note(&mut a);
    transfer(&mut a, &mut b, &scopes());
    let mut other = b.list("", false).unwrap().remove(0);
    n.body = "Mac edit".into();
    a.save(n).unwrap();
    other.body = "Windows edit".into();
    b.save(other).unwrap();
    transfer(&mut a, &mut b, &scopes());
    assert_eq!(b.list("", false).unwrap()[0].body, "Windows edit");
    assert_eq!(b.sync_conflicts().unwrap().len(), 1);
}
#[test]
fn maximum_file_fits_wire_and_retries_after_lost_ack() {
    let mut a = Repository::open(":memory:").unwrap();
    let mut b = Repository::open(":memory:").unwrap();
    let n = note(&mut a);
    let f = document(&n.id, 10 * 1024 * 1024);
    a.add_attachment(&f).unwrap();
    transfer(&mut a, &mut b, &scopes());
    a.trash_attachment(&f.id, true).unwrap();
    let batch = a
        .sync_batch(b.sync_progress(&a.device_id).unwrap().1, &scopes())
        .unwrap();
    assert!(serde_json::to_vec(&batch).unwrap().len() < 32 * 1024 * 1024);
    b.apply_sync_batch(&a.device_id, &batch, &scopes()).unwrap();
    assert_eq!(transfer(&mut a, &mut b, &scopes()), 0);
    assert!(b.attachment(&f.id).unwrap().deleted_at.is_some());
}

#[test]
fn receiver_rejects_attachment_when_parent_category_is_not_granted() {
    let mut a = Repository::open(":memory:").unwrap();
    let mut b = Repository::open(":memory:").unwrap();
    let n = note(&mut a);
    let f = document(&n.id, 20);
    a.add_attachment(&f).unwrap();
    let parent = a.sync_batch(0, &scopes()).unwrap();
    b.apply_sync_batch(&a.device_id, &parent, &scopes())
        .unwrap();
    let batch = a.sync_batch(parent.through, &scopes()).unwrap();
    assert!(b
        .apply_sync_batch(&a.device_id, &batch, &["attachment".into()])
        .is_err());
    assert!(b.attachment(&f.id).is_err());
    assert_eq!(b.sync_progress(&a.device_id).unwrap().1, parent.through);
}
#[test]
fn existing_file_content_cannot_change_under_same_identity() {
    let mut a = Repository::open(":memory:").unwrap();
    let mut b = Repository::open(":memory:").unwrap();
    let n = note(&mut a);
    let f = document(&n.id, 20);
    a.add_attachment(&f).unwrap();
    transfer(&mut a, &mut b, &scopes());
    a.trash_attachment(&f.id, true).unwrap();
    let cursor = b.sync_progress(&a.device_id).unwrap().1;
    let mut batch = a.sync_batch(cursor, &scopes()).unwrap();
    let bytes = b"replacement content";
    batch.events[0].payload["content_base64"] = serde_json::json!(STANDARD.encode(bytes));
    batch.events[0].payload["sha256"] = serde_json::json!(format!("{:x}", Sha256::digest(bytes)));
    assert!(b.apply_sync_batch(&a.device_id, &batch, &scopes()).is_err());
    assert!(b.attachment(&f.id).unwrap() == f);
    assert_eq!(b.sync_progress(&a.device_id).unwrap().1, cursor);
}
