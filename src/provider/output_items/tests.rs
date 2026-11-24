use super::*;

#[test]
fn historical_responses_summary_retains_exact_validation() {
    let items = json!({"format":FORMAT,"terminal_reasoning":null,
        "terminal_output":[{"type":"web_search_call"}],"completed_item_events":{}});
    let summary = metadata(&items).unwrap();
    assert!(metadata_matches(&items, &summary));
    let mut historical = summary.clone();
    for section in ["terminal_output", "completed_item_events"] {
        historical[section].as_object_mut().unwrap().remove("web_search_items");
    }
    assert!(metadata_matches(&items, &historical));
    for (field, value) in [("sha256",json!("wrong")),("serialized_utf8_bytes",json!(0)),
        ("scope",json!("different"))] {
        let mut altered = historical.clone();
        altered[field] = value;
        assert!(!metadata_matches(&items, &altered));
    }
    let mut altered = historical.clone();
    altered["terminal_output"]["items"] = json!(0);
    assert!(!metadata_matches(&items, &altered));
    let mut incomplete = summary;
    incomplete["terminal_output"].as_object_mut().unwrap().remove("web_search_items");
    assert!(!metadata_matches(&items, &incomplete));
    historical["completed_item_events"].as_object_mut().unwrap().remove("reasoning_items");
    assert!(!metadata_matches(&items, &historical));
}
