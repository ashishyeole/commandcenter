use command_center_core::{now, Item, Repository};
fn note(repo: &mut Repository) -> Item {
    repo.save(Item {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "note".into(),
        title: "Synthetic QA".into(),
        body: "Original".into(),
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
#[test]
fn compaction_waits_for_all_peers_and_retains_current_state() {
    let mut repo = Repository::open(":memory:").unwrap();
    let mut item = note(&mut repo);
    repo.prepare_sync().unwrap();
    item.body = "Updated".into();
    repo.save(item).unwrap();
    repo.prepare_sync().unwrap();
    let peer = uuid::Uuid::new_v4().to_string();
    repo.set_setting("paired_peers", &serde_json::json!([peer]).to_string())
        .unwrap();
    let blocked = repo.sync_compaction_preview().unwrap();
    assert_eq!(blocked["removable_events"], 0);
    let batch = repo.sync_batch(0, &["note".into()]).unwrap();
    repo.sync_ack(&peer, batch.through).unwrap();
    let preview = repo.sync_compaction_preview().unwrap();
    assert_eq!(preview["removable_events"], 1);
    assert_eq!(repo.compact_acknowledged_sync(&preview).unwrap(), 1);
    let mut receiver = Repository::open(":memory:").unwrap();
    let batch = repo.sync_batch(0, &["note".into()]).unwrap();
    receiver
        .apply_sync_batch(&repo.device_id, &batch, &["note".into()])
        .unwrap();
    assert_eq!(receiver.list("", false).unwrap()[0].body, "Updated");
    assert!(repo.compact_acknowledged_sync(&preview).is_err());
}
#[test]
fn checked_private_batch_is_atomic_on_stale_input() {
    let mut repo = Repository::open(":memory:").unwrap();
    let a = uuid::Uuid::new_v4().to_string();
    let b = uuid::Uuid::new_v4().to_string();
    repo.save_vault(&a, b"original").unwrap();
    let result = repo.save_vault_batch_checked(&[
        (b.clone(), b"new".to_vec(), None),
        (a.clone(), b"replacement".to_vec(), Some(b"stale".to_vec())),
    ]);
    assert!(result.is_err());
    assert!(repo.vault_ciphertext(&b).unwrap().is_none());
    assert_eq!(repo.vault_ciphertext(&a).unwrap().unwrap(), b"original");
}
#[test]
fn integration_lifecycle_rejects_unknown_flags_and_is_bounded() {
    let repo = Repository::open(":memory:").unwrap();
    assert!(repo.set_integration_flag("user-secret", true).is_err());
    for n in 0..210 {
        repo.set_integration_flag("google_enabled", n % 2 == 0)
            .unwrap();
    }
    let value = repo.integration_inventory().unwrap();
    assert_eq!(value["events"].as_array().unwrap().len(), 200);
    assert!(!value.to_string().contains("token"));
}
#[test]
fn per_record_ai_policy_is_enforced_without_mutating_source() {
    let mut repo = Repository::open(":memory:").unwrap();
    let item = note(&mut repo);
    repo.set_ai_exclusion(&item.id, true).unwrap();
    assert!(repo.ensure_ai_context_allowed(&[item.id.clone()]).is_err());
    assert_eq!(repo.list("", false).unwrap()[0].body, "Original");
    repo.set_ai_exclusion(&item.id, false).unwrap();
    repo.ensure_ai_context_allowed(&[item.id]).unwrap();
    assert!(repo
        .set_ai_exclusion(&uuid::Uuid::new_v4().to_string(), true)
        .is_err());
}

#[test]
fn price_watches_require_currency_and_stock_then_trigger_once() {
    use command_center_core::workspace::Record;
    use serde_json::json;
    let mut repo = Repository::open(":memory:").unwrap();
    let mut watch = Record::new("watchlists", "Synthetic offer", &repo.device_id);
    watch.status = "watching".into();
    watch.fields.extend([
        ("source".into(), "https://example.com/offer.json".into()),
        ("metric".into(), "price".into()),
        ("currency".into(), "USD".into()),
        ("target".into(), "20".into()),
        ("operator".into(), "at_most".into()),
        ("monitor".into(), "enabled".into()),
    ]);
    repo.save_record(watch).unwrap();
    let sample = |currency: &str, available: bool| json!([{"source":"price","url":"https://example.com/offer.json","price":12.5,"currency":currency,"available":available}]);
    repo.cache_research_query(
        "price",
        "https://example.com/offer.json",
        &sample("INR", true),
    )
    .unwrap();
    assert_eq!(repo.evaluate_watchlists().unwrap(), 0);
    repo.cache_research_query(
        "price",
        "https://example.com/offer.json",
        &sample("USD", false),
    )
    .unwrap();
    assert_eq!(repo.evaluate_watchlists().unwrap(), 0);
    repo.cache_research_query(
        "price",
        "https://example.com/offer.json",
        &sample("USD", true),
    )
    .unwrap();
    assert_eq!(repo.evaluate_watchlists().unwrap(), 1);
    assert_eq!(repo.evaluate_watchlists().unwrap(), 0);
    let rows = repo.records("", false).unwrap();
    assert_eq!(
        rows.iter().filter(|r| r.module == "notifications").count(),
        1
    );
    assert_eq!(
        rows.iter()
            .find(|r| r.module == "watchlists")
            .unwrap()
            .status,
        "triggered"
    );
}
