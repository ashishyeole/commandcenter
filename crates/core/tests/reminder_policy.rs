use chrono::{TimeZone, Utc};
use command_center_core::{
    now,
    reminders::{delivery_key, Cadence},
    Item, Repository,
};
use uuid::Uuid;
fn task(repo: &mut Repository) -> Item {
    repo.save(Item {
        id: Uuid::new_v4().to_string(),
        kind: "task".into(),
        title: "Reminder fixture".into(),
        body: String::new(),
        pinned: false,
        priority: "critical".into(),
        status: "active".into(),
        due: Some("2027-01-31".into()),
        recurrence: "monthly".into(),
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
fn acknowledgement_survives_reopen_but_does_not_suppress_next_occurrence() {
    let path = std::env::temp_dir().join(format!("pcc-reminder-{}.db", Uuid::new_v4()));
    let mut repo = Repository::open(&path).unwrap();
    let mut item = task(&mut repo);
    repo.update_task_reminder(&item.id, item.revision, Cadence::Daily, true)
        .unwrap();
    drop(repo);
    let mut repo = Repository::open(&path).unwrap();
    let policy = repo.task_reminder_policy(&item.id).unwrap();
    let clock = Utc.with_ymd_and_hms(2027, 3, 1, 12, 0, 0).unwrap();
    assert!(delivery_key(&item, &policy, "2027-03-01", clock, "", true).is_none());
    item.due = Some("2027-02-28".into());
    let item = repo.save(item).unwrap();
    assert!(delivery_key(&item, &policy, "2027-03-01", clock, "", true).is_some());
    assert!(repo
        .update_task_reminder(&item.id, item.revision - 1, Cadence::Muted, false)
        .is_err());
    drop(repo);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn policy_cadence_snooze_and_eligibility() {
    let mut repo = Repository::open(":memory:").unwrap();
    let mut item = task(&mut repo);
    let clock = Utc.with_ymd_and_hms(2027, 2, 1, 12, 0, 0).unwrap();
    let policy = repo.task_reminder_policy(&item.id).unwrap();
    assert!(delivery_key(&item, &policy, "2027-02-01", clock, "", true)
        .unwrap()
        .contains(":hour:"));
    let daily = repo
        .update_task_reminder(&item.id, item.revision, Cadence::Daily, false)
        .unwrap();
    assert!(delivery_key(&item, &daily, "2027-02-01", clock, "", true)
        .unwrap()
        .contains(":2027-02-01:"));
    assert!(delivery_key(
        &item,
        &daily,
        "2027-02-01",
        clock,
        "2027-02-01T13:00:00Z",
        true
    )
    .is_none());
    assert!(delivery_key(
        &item,
        &daily,
        "2027-02-01",
        clock,
        "2027-02-01T11:00:00Z",
        true
    )
    .is_some());
    let once = repo
        .update_task_reminder(&item.id, item.revision, Cadence::Once, false)
        .unwrap();
    assert_eq!(
        delivery_key(&item, &once, "2027-02-01", clock, "", true),
        delivery_key(&item, &once, "2027-02-02", clock, "", true)
    );
    let mute = repo
        .update_task_reminder(&item.id, item.revision, Cadence::Muted, false)
        .unwrap();
    assert!(delivery_key(&item, &mute, "2027-02-01", clock, "", true).is_none());
    item.status = "done".into();
    assert!(delivery_key(&item, &daily, "2027-02-01", clock, "", true).is_none());
    item.status = "active".into();
    assert!(delivery_key(&item, &daily, "2027-01-30", clock, "", true).is_none());
}
#[test]
fn policy_rejects_non_tasks_and_unknown_modes() {
    let mut repo = Repository::open(":memory:").unwrap();
    let mut item = task(&mut repo);
    item.id = Uuid::new_v4().to_string();
    item.revision = 1;
    item.kind = "note".into();
    item.recurrence = "none".into();
    item.due = None;
    let item = repo.save(item).unwrap();
    assert!(repo
        .update_task_reminder(&item.id, item.revision, Cadence::Daily, false)
        .is_err());
    assert!(serde_json::from_str::<Cadence>("\"every_second\"").is_err());
}
