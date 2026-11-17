//! Data-driven native-core workflow acceptance using public caches from the DMG.
//! This is not an interactive file-picker or UI test. All data lives in tempdir.
use base64::{engine::general_purpose::STANDARD, Engine};
use command_center_core::{archive, attachments::Attachment, workspace::Record, Item, Repository};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fixture: Value = serde_json::from_slice(&std::fs::read(&args[1]).unwrap()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut repo = Repository::open(temp.path().join("source.db")).unwrap();
    let mut saved_ids = vec![];
    for row in fixture["saved"].as_array().unwrap() {
        let mut item: Item = serde_json::from_value(row.clone()).unwrap();
        item.device_id = repo.device_id.clone();
        saved_ids.push(repo.save(item).unwrap().id);
    }
    let mut document_ids = vec![];
    for row in fixture["documents"].as_array().unwrap() {
        let mut item: Item = serde_json::from_value(row.clone()).unwrap();
        item.device_id = repo.device_id.clone();
        document_ids.push(repo.save(item).unwrap().id);
    }
    assert_eq!(repo.list("Public acceptance", false).unwrap().iter().filter(|i| i.kind == "note").count(), document_ids.len());
    let mut project = Record::new("projects", "Public discovery acceptance workspace", &repo.device_id);
    project.links = saved_ids.iter().take(14).chain(document_ids.iter()).cloned().collect();
    let project = repo.save_record(project).unwrap();
    let now = command_center_core::now();
    let task = Item {
        id: uuid::Uuid::new_v4().to_string(), kind: "task".into(),
        title: "Review public discovery workspace".into(), body: project.id.clone(),
        pinned: true, priority: "normal".into(), status: "active".into(),
        due: Some("2070-01-31".into()), recurrence: "monthly".into(), url: None,
        created_at: now.clone(), updated_at: now.clone(), device_id: repo.device_id.clone(),
        revision: 1, deleted_at: None,
    };
    let task = repo.save(task).unwrap();
    let next = repo.complete(&task.id).unwrap();
    assert_eq!(next.due.as_deref(), Some("2070-02-28"));
    let bytes = b"Selected synthetic public-workflow attachment.\n";
    let attachment = Attachment {
        id: uuid::Uuid::new_v4().to_string(), owner_id: project.id.clone(),
        filename: "public-acceptance.txt".into(), mime: "text/plain".into(),
        content_base64: STANDARD.encode(bytes), sha256: format!("{:x}", Sha256::digest(bytes)),
        created_at: now, previous_id: None, deleted_at: None, extracted_text: String::new(),
    };
    repo.add_attachment(&attachment).unwrap();
    repo.index_attachment_text(&attachment.id, &attachment.sha256, "Synthetic public acceptance attachment").unwrap();
    assert_eq!(repo.attachment(&attachment.id).unwrap().validate().unwrap(), bytes);
    repo.trash_attachment(&attachment.id, true).unwrap();
    assert!(repo.attachment(&attachment.id).unwrap().deleted_at.is_some());
    repo.trash_attachment(&attachment.id, false).unwrap();
    let data = archive::ExportData {
        items: repo.list("", true).unwrap(), workspace: repo.portable_workspace().unwrap(), vault: vec![],
    };
    let plain = archive::export_all(&data).unwrap();
    let encrypted = archive::encrypt(&plain, "Synthetic acceptance password 2026!").unwrap();
    assert!(archive::decrypt(&encrypted, "Incorrect synthetic password").is_err());
    let restored_bytes = archive::decrypt(&encrypted, "Synthetic acceptance password 2026!").unwrap();
    let portable = archive::inspect_all(&restored_bytes).unwrap();
    let mut destination = Repository::open(temp.path().join("destination.db")).unwrap();
    destination.import_snapshot(&portable.items, &portable.workspace, &[]).unwrap();
    assert_eq!(destination.list("", false).unwrap().len(), data.items.len());
    assert_eq!(destination.import_snapshot(&portable.items, &portable.workspace, &[]).unwrap(), 0);
    assert_eq!(destination.attachment(&attachment.id).unwrap().validate().unwrap(), bytes);
    assert_eq!(destination.records("", false).unwrap()[0].links, project.links);
    drop(destination);
    let reopened = Repository::open(temp.path().join("destination.db")).unwrap();
    assert_eq!(reopened.list("Public acceptance", false).unwrap().iter().filter(|i| i.kind == "note").count(), document_ids.len());
    let result = json!({
        "checked_at":command_center_core::now(), "source_revision":fixture["source_revision"],
        "dmg_sha256":fixture["dmg_sha256"], "normal_profile_accessed":false,
        "app_login_used":false, "api_keys_used":false, "native_core_not_ui":true,
        "public_items_saved":saved_ids.len(), "provider_documents_saved":document_ids.len(),
        "workspace_links":project.links.len(), "task_month_end_recurrence_passed":true,
        "attachment_bytes_index_trash_restore_passed":true, "plain_encrypted_backup_roundtrip_passed":true,
        "wrong_password_rejected":true, "duplicate_restore_added":0, "reopen_and_fts_passed":true
    });
    std::fs::write(&args[2], serde_json::to_string_pretty(&result).unwrap()).unwrap();
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
}
