use command_center_core::{workspace::Record, Repository};
#[test]
fn cached_threshold_triggers_once_and_preserves_source_evidence() {
    let mut repo = Repository::open(":memory:").unwrap();
    let mut r = Record::new("watchlists", "Repo threshold", &repo.device_id);
    r.status = "watching".into();
    for (k, v) in [
        ("source", "https://github.com/example/repo"),
        ("metric", "stars"),
        ("operator", "at_least"),
        ("target", "100"),
        ("monitor", "enabled"),
    ] {
        r.fields.insert(k.into(), v.into());
    }
    repo.save_record(r).unwrap();
    repo.cache_research_query("github","robotics",&serde_json::json!([{"source":"github","url":"https://github.com/example/repo","metric":"120 stars"}])).unwrap();
    assert_eq!(repo.evaluate_watchlists().unwrap(), 1);
    assert_eq!(repo.evaluate_watchlists().unwrap(), 0);
    let rows = repo.records("", false).unwrap();
    assert!(rows
        .iter()
        .any(|r| r.module == "watchlists" && r.status == "triggered"));
    assert_eq!(
        rows.iter().filter(|r| r.module == "notifications").count(),
        1
    );
}
#[test]
fn arbitrary_prices_and_disabled_monitors_are_not_evaluated() {
    let mut repo = Repository::open(":memory:").unwrap();
    let mut r = Record::new("watchlists", "Disabled", &repo.device_id);
    r.status = "watching".into();
    for (k, v) in [
        ("source", "https://example.com"),
        ("metric", "stars"),
        ("operator", "at_least"),
        ("target", "1"),
        ("monitor", "disabled"),
    ] {
        r.fields.insert(k.into(), v.into());
    }
    repo.save_record(r).unwrap();
    repo.cache_research_query(
        "github",
        "",
        &serde_json::json!([{"source":"github","url":"https://example.com","metric":"120 stars"}]),
    )
    .unwrap();
    assert_eq!(repo.evaluate_watchlists().unwrap(), 0);
}
#[test]
fn enabled_monitor_rejects_invalid_threshold_or_mismatched_host() {
    let mut repo = Repository::open(":memory:").unwrap();
    let mut r = Record::new("watchlists", "Invalid", &repo.device_id);
    for (k, v) in [
        ("monitor", "enabled"),
        ("metric", "stars"),
        ("operator", "at_least"),
        ("target", "NaN"),
        ("source", "https://github.com/example/repo"),
    ] {
        r.fields.insert(k.into(), v.into());
    }
    assert!(repo.save_record(r.clone()).is_err());
    r.fields.insert("target".into(), "100".into());
    r.fields
        .insert("source".into(), "https://example.com".into());
    assert!(repo.save_record(r).is_err());
}

#[test]
fn topic_feed_observations_override_stale_queries_and_trigger_once() {
    let mut repo = Repository::open(":memory:").unwrap();
    let mut watch = Record::new("watchlists", &"🤖".repeat(125), &repo.device_id);
    watch.status = "watching".into();
    for (k, v) in [
        ("source", "https://github.com/example/repo"),
        ("metric", "stars"),
        ("operator", "at_least"),
        ("target", "100"),
        ("monitor", "enabled"),
    ] {
        watch.fields.insert(k.into(), v.into());
    }
    let id = watch.id.clone();
    repo.save_record(watch).unwrap();
    repo.set_setting("research_query:stale",&serde_json::json!({"fetched_at":"2000-01-01T00:00:00Z","items":[{"source":"github","url":"https://github.com/example/repo","metric":"999 stars"}]}).to_string()).unwrap();
    repo.cache_feed("hub:robotics-repos",&serde_json::json!([{"source":"github","url":"https://github.com/example/repo","metric":"20 stars"}])).unwrap();
    assert_eq!(repo.evaluate_watchlists().unwrap(), 1);
    assert_eq!(
        repo.records("", false)
            .unwrap()
            .iter()
            .find(|r| r.id == id)
            .unwrap()
            .status,
        "watching"
    );
    repo.cache_feed("hub:robotics-repos",&serde_json::json!([{"source":"github","url":"https://github.com/example/repo","metric":"120 stars"}])).unwrap();
    assert_eq!(repo.evaluate_watchlists().unwrap(), 1);
    assert_eq!(repo.evaluate_watchlists().unwrap(), 0);
    let records = repo.records("", false).unwrap();
    assert_eq!(
        records.iter().find(|r| r.id == id).unwrap().status,
        "triggered"
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r.module == "notifications")
            .count(),
        1
    );
}
