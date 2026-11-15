use command_center_core::{workspace::Record, Repository};
#[test]
fn ownership_is_linked_and_cycles_are_rejected() {
    let mut repo = Repository::open(":memory:").unwrap();
    let parent = repo
        .save_record(Record::new("projects", "Parent", &repo.device_id))
        .unwrap();
    let mut child = Record::new("projects", "Child", &repo.device_id);
    child
        .fields
        .insert("workspace_owner".into(), parent.id.clone());
    let child = repo.save_record(child).unwrap();
    assert!(child.links.contains(&parent.id));
    let mut cycle = parent;
    cycle.fields.insert("workspace_owner".into(), child.id);
    assert!(repo.save_record(cycle).is_err());
}
#[test]
fn arbitrary_or_missing_owners_are_rejected() {
    let mut repo = Repository::open(":memory:").unwrap();
    let note = repo
        .save_record(Record::new("ideas", "Idea", &repo.device_id))
        .unwrap();
    let mut child = Record::new("robotics", "Research", &repo.device_id);
    child.fields.insert("workspace_owner".into(), note.id);
    assert!(repo.save_record(child.clone()).is_err());
    child
        .fields
        .insert("workspace_owner".into(), uuid::Uuid::new_v4().to_string());
    assert!(repo.save_record(child).is_err());
}
