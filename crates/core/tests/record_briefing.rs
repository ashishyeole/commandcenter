use command_center_core::{workspace::Record, Repository};
#[test]
fn briefing_only_uses_selected_current_ordinary_records() {
    let mut repo = Repository::open(":memory:").unwrap();
    let mut r = Record::new("robotics", "Public demo", &repo.device_id);
    r.status = "researching".into();
    r.fields
        .insert("next_action".into(), "Check official source".into());
    let r = repo.save_record(r).unwrap();
    let hidden = repo
        .save_record(Record::new(
            "robotics",
            "Unselected material",
            &repo.device_id,
        ))
        .unwrap();
    let mission = repo
        .save_record(Record::new("missions", "Excluded mission", &repo.device_id))
        .unwrap();
    let output = repo
        .record_briefing(&[r.id.clone(), r.id.clone(), mission.id], "2026-11-14")
        .unwrap();
    assert!(output.contains("Public demo"));
    assert!(output.contains(&r.id));
    assert!(output.contains("Check official source"));
    assert!(!output.contains(&hidden.title));
    assert!(!output.contains("Excluded mission"));
    assert!(output.contains("excluded selections: 1"));
}
#[test]
fn briefing_rejects_unbounded_or_invalid_requests() {
    let repo = Repository::open(":memory:").unwrap();
    assert!(repo
        .record_briefing(&vec!["id".into(); 31], "2026-11-14")
        .is_err());
    assert!(repo.record_briefing(&[], "not a date").is_err());
    assert!(repo
        .record_briefing(&[], "2026-11-14")
        .unwrap()
        .contains("No matching selected records"));
}
