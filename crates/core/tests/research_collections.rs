use command_center_core::{
    collections::{Collection, Search},
    Repository,
};
#[test]
fn collections_preserve_queries_and_cache_independently() {
    let path = tempfile::NamedTempFile::new().unwrap();
    let repo = Repository::open(path.path()).unwrap();
    let c = Collection {
        id: uuid::Uuid::new_v4().to_string(),
        name: "Robotics".into(),
        searches: vec![Search {
            source: "arxiv".into(),
            query: "robotics".into(),
        }],
    };
    repo.save_research_collections(&[c]).unwrap();
    repo.cache_research_query("arxiv", "robotics", &serde_json::json!([{ "id":"a" }]))
        .unwrap();
    repo.cache_research_query("arxiv", "agriculture", &serde_json::json!([{ "id":"b" }]))
        .unwrap();
    drop(repo);
    let repo = Repository::open(path.path()).unwrap();
    assert_eq!(repo.research_collections().unwrap()[0].name, "Robotics");
    assert_eq!(repo.research_query_snapshots().unwrap().len(), 2);
    repo.cache_research_query("arxiv", "robotics", &serde_json::json!([]))
        .unwrap();
    assert_eq!(repo.research_query_snapshots().unwrap().len(), 2);
}
#[test]
fn collection_limits_and_cache_bound() {
    let repo = Repository::open(":memory:").unwrap();
    assert!(repo
        .save_research_collections(&[Collection {
            id: "invalid".into(),
            name: "test".into(),
            searches: vec![]
        }])
        .is_err());
    for i in 0..105 {
        repo.cache_research_query("news", &format!("query {i}"), &serde_json::json!([]))
            .unwrap();
    }
    assert_eq!(repo.research_query_snapshots().unwrap().len(), 100);
}
