mod common;
use common::Workspace;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    compat::canonical_json::fingerprint,
    jobs::{Receipt, Store},
};

fn receipt(id: &str, connection: &str) -> Receipt {
    Receipt::new(json!({
        "request_id":id,"connection":connection,"adapter":"openai-images","operation":"generate",
        "status":"prepared","requested_model":"image-model","upstream_model":null,
        "outputs":[],"warnings":[],"deviations":[],"errors":[],
        "generation_outcome":"not_submitted","storage_outcome":"not_started",
        "another_submission_may_charge":false,
        "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z"
    }))
}

fn output(id: &str, request: &str) -> Value {
    json!({
        "output_id":id,"request_id":request,"path":format!("/fixture/{id}.png"),
        "mime_type":"image/png","width":8,"height":6,"has_alpha":true,
        "bytes":100,"sha256":"a".repeat(64)
    })
}

#[test]
fn exact_source_known_hash_ignores_object_order_and_omitted_undefined_field() {
    assert_eq!(
        fingerprint(&json!({})).unwrap(),
        "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    );
    // The source's undefined property is omitted at the JSON input boundary.
    let first =
        Value::parse_with_limit(r#"{"prompt":"café","options":{"b":2,"a":1}}"#, 1024).unwrap();
    let second = json!({"options":{"a":1,"b":2},"prompt":"café"});
    assert_eq!(fingerprint(&first).unwrap(), fingerprint(&second).unwrap());
}

#[test]
fn exact_source_fingerprint_distinguishes_reference_order_bytes_model_endpoint_and_scalar_types() {
    let initial = json!({
        "references":["hash-a","hash-b"],"model":"model-a",
        "endpoint":"https://a.example/v1","count":1
    });
    let expected = fingerprint(&initial).unwrap();
    for change in [
        json!({"references":["hash-b","hash-a"]}),
        json!({"references":["hash-a","hash-c"]}),
        json!({"model":"model-b"}),
        json!({"endpoint":"https://b.example/v1"}),
        json!({"count":"1"}),
    ] {
        let mut value = initial.clone();
        value
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert_ne!(fingerprint(&value).unwrap(), expected);
    }
    assert_ne!(
        fingerprint(&json!({"value":null})).unwrap(),
        fingerprint(&json!({})).unwrap()
    );
}

#[test]
fn exact_source_independent_stores_reuse_detached_receipts_and_restart_without_resubmission() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    let hash = fingerprint(&json!({"prompt":"draw","model":"image-model"})).unwrap();
    {
        let mut first = Store::open(&directory).unwrap();
        let mut second = Store::open(&directory).unwrap();
        let initial = receipt("request-1", "local");
        let (fresh, prepared) = first
            .prepare(initial.clone(), &hash, "connection-identity")
            .unwrap();
        assert!(fresh);
        assert_eq!(prepared.value, initial.value);
        let (fresh, duplicate) = second
            .prepare(receipt("request-1", "local"), &hash, "connection-identity")
            .unwrap();
        assert!(!fresh);
        assert_eq!(duplicate.value, initial.value);
        let mut finished = first.get("request-1").unwrap();
        finished.value["status"] = json!("completed");
        finished.value["generation_outcome"] = json!("completed");
        finished.value["storage_outcome"] = json!("completed");
        first.update(&mut finished).unwrap();
        let mut detached = second.get("request-1").unwrap();
        assert_eq!(detached.value["status"], "completed");
        assert_eq!(detached.value["generation_outcome"], "completed");
        detached.value["warnings"]
            .as_array_mut()
            .unwrap()
            .push(json!("not persisted"));
        assert_eq!(first.get("request-1").unwrap().value["warnings"], json!([]));
    }
    let mut restarted = Store::open(&directory).unwrap();
    let (fresh, original) = restarted
        .prepare(receipt("request-1", "local"), &hash, "connection-identity")
        .unwrap();
    assert!(!fresh);
    assert_eq!(original.value["status"], "completed");
}

#[test]
fn exact_source_identity_conflicts_roll_back_without_corrupting_original_receipt() {
    let workspace = Workspace::new();
    let mut store = Store::open(&workspace.0.join("state")).unwrap();
    store
        .prepare(receipt("request-1", "local"), "hash-1", "identity-1")
        .unwrap();
    for (hash, identity) in [("hash-2", "identity-1"), ("hash-1", "identity-2")] {
        assert_eq!(
            store
                .prepare(receipt("request-1", "local"), hash, identity)
                .unwrap_err()
                .code,
            "request_id_conflict"
        );
    }
    assert!(
        !store
            .prepare(receipt("request-1", "local"), "hash-1", "identity-1")
            .unwrap()
            .0
    );
    assert_eq!(store.get("request-1").unwrap().status(), "prepared");
    assert_eq!(store.get("unknown").unwrap_err().code, "invalid_input");
    assert_eq!(store.cancel("unknown").unwrap_err().code, "invalid_input");
}

#[test]
fn exact_source_cross_store_admission_limits_release_only_completed_work() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    let mut first = Store::open(&directory).unwrap();
    let mut second = Store::open(&directory).unwrap();
    for (id, connection) in [("a1", "a"), ("a2", "a"), ("b1", "b"), ("b2", "b")] {
        let store = if matches!(id, "a2" | "b1") {
            &mut second
        } else {
            &mut first
        };
        store
            .prepare(receipt(id, connection), id, connection)
            .unwrap();
    }
    assert_eq!(second.queued().unwrap(), 4);
    assert!(first.admit("a1", "a", 2, 1).unwrap());
    assert!(!second.admit("a2", "a", 2, 1).unwrap());
    assert!(second.admit("b1", "b", 2, 1).unwrap());
    assert!(!first.admit("b2", "b", 2, 2).unwrap());
    assert_eq!(first.get("a2").unwrap().status(), "prepared");
    assert_eq!(first.queued().unwrap(), 2);
    let mut done = first.get("a1").unwrap();
    done.value["status"] = json!("completed");
    first.update(&mut done).unwrap();
    assert!(second.admit("a2", "a", 2, 1).unwrap());
    assert_eq!(first.get("a2").unwrap().status(), "submitting");
}

#[test]
fn exact_source_double_cancellation_persists_independently_of_billing_and_outcome() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    {
        let mut first = Store::open(&directory).unwrap();
        let second = Store::open(&directory).unwrap();
        first
            .prepare(receipt("request-1", "local"), "hash", "identity")
            .unwrap();
        assert!(!second.cancelled("request-1").unwrap());
        first.cancel("request-1").unwrap();
        first.cancel("request-1").unwrap();
        assert!(second.cancelled("request-1").unwrap());
        let current = second.get("request-1").unwrap();
        assert_eq!(current.status(), "prepared");
        assert_eq!(current.value["another_submission_may_charge"], false);
        assert!(!second.cancelled("absent").unwrap());
    }
    let restarted = Store::open(&directory).unwrap();
    assert!(restarted.cancelled("request-1").unwrap());
}

#[test]
fn exact_source_empty_orphan_reservations_preserve_uncertainty_across_two_reopens() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    {
        let mut store = Store::open(&directory).unwrap();
        for id in ["old-a", "old-b"] {
            let mut current = store
                .prepare(receipt(id, "local"), id, "identity")
                .unwrap()
                .1;
            assert!(store.admit(id, "local", 10, 10).unwrap());
            current.value["status"] = json!("submitting");
            current.value["generation_outcome"] = json!("unknown");
            store.update(&mut current).unwrap();
            store.reserve(id, 1024, 2048).unwrap();
        }
        let database = rusqlite::Connection::open(directory.join("receipts.sqlite")).unwrap();
        database
            .execute_batch("UPDATE requests SET lease=0")
            .unwrap();
        drop(database);
        store.queued().unwrap();
    }
    for round in 0..2 {
        let mut store = Store::open(&directory).unwrap();
        for id in ["old-a", "old-b"] {
            let original = store.get(id).unwrap();
            assert_eq!(original.status(), "outcome_unknown");
            assert_eq!(original.value["generation_outcome"], "unknown");
            assert!(
                !store
                    .prepare(receipt(id, "local"), id, "identity")
                    .unwrap()
                    .0
            );
        }
        let id = format!("fresh-{round}");
        let mut current = store
            .prepare(receipt(&id, "local"), "fresh", "identity")
            .unwrap()
            .1;
        assert!(store.admit(&id, "local", 1, 1).unwrap());
        store.reserve(&id, 2048, 2048).unwrap();
        current.value["status"] = json!("failed");
        store.update(&mut current).unwrap();
        store.release(&id, Some(&current)).unwrap();
    }
}

#[test]
fn exact_source_bytes_urls_live_and_detached_writers_keep_charges_and_fence_expiry() {
    let workspace = Workspace::new();
    for mode in ["bytes", "url", "live", "detached"] {
        let directory = workspace.0.join(mode);
        let mut owner = Store::open(&directory).unwrap();
        let mut peer = Store::open(&directory).unwrap();
        let mut old = owner
            .prepare(receipt("old", "local"), "hash", "identity")
            .unwrap()
            .1;
        assert!(owner.admit("old", "local", 10, 10).unwrap());
        old.value["status"] = json!("submitting");
        owner.update(&mut old).unwrap();
        owner.reserve("old", 1024, 2048).unwrap();
        match mode {
            "bytes" => owner.retain("old", 0, &[0u8; 512], Some(&old)).unwrap(),
            "url" => owner
                .retain_url("old", 0, &"https://example.com/final".into(), Some(&old))
                .unwrap(),
            "detached" => {
                old.value["status"] = json!("running");
                old.value["upstream_job"] = json!({"id":"job","kind":"responses"});
                owner.update(&mut old).unwrap();
                owner.release("old", Some(&old)).unwrap();
            }
            _ => {}
        }
        if matches!(mode, "bytes" | "url") {
            let database = rusqlite::Connection::open(directory.join("receipts.sqlite")).unwrap();
            database
                .execute_batch("UPDATE requests SET lease=0")
                .unwrap();
            drop(database);
            peer.queued().unwrap();
            assert!(!owner.owns("old", Some(&old)).unwrap());
            assert!(owner.retain("old", 1, &[0u8; 8], Some(&old)).is_err());
        }
        peer.prepare(receipt("fresh", "local"), "fresh", "identity")
            .unwrap();
        peer.admit("fresh", "local", 10, 10).unwrap();
        assert_eq!(
            peer.reserve("fresh", 2048, 2048).unwrap_err().code,
            "rate_limited",
            "{mode}"
        );
        if matches!(mode, "bytes" | "url") {
            assert_eq!(peer.pending("old").unwrap().len(), 1);
        }
    }
}

#[test]
fn exact_source_received_completion_wins_late_cancel_and_keeps_both_retained_byte_items() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    let mut owner = Store::open(&directory).unwrap();
    let mut control = Store::open(&directory).unwrap();
    let mut current = owner
        .prepare(receipt("race", "local"), "hash", "identity")
        .unwrap()
        .1;
    assert!(owner.admit("race", "local", 1, 1).unwrap());
    current.value["status"] = json!("running");
    current.value["upstream_job"] = json!({"id":"job","kind":"comfyui"});
    owner.update(&mut current).unwrap();
    owner.reserve("race", 1024, 2048).unwrap();
    owner.retain("race", 0, &[0u8; 16], Some(&current)).unwrap();
    control
        .terminate(
            "race",
            "cancelled",
            &json!({"code":"provider_rejection"}),
            None,
            Some(&current.value["upstream_job"]),
        )
        .unwrap();
    assert!(owner.owns("race", Some(&current)).unwrap());
    owner.retain("race", 1, &[0u8; 8], Some(&current)).unwrap();
    let items = control.pending("race").unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].bytes.as_deref(), Some([0u8; 16].as_slice()));
    assert_eq!(items[1].bytes.as_deref(), Some([0u8; 8].as_slice()));
    let result = control.get("race").unwrap();
    assert_eq!(result.value["generation_outcome"], "completed");
    assert!(result.value.get("upstream_terminal").is_none());
    owner.release("race", Some(&current)).unwrap();
    control
        .prepare(receipt("fresh", "local"), "fresh", "identity")
        .unwrap();
    assert!(control.admit("fresh", "local", 1, 1).unwrap());
}

#[test]
fn exact_source_completed_output_wins_later_cancel_without_resurrecting_state() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    let mut owner = Store::open(&directory).unwrap();
    let mut control = Store::open(&directory).unwrap();
    let mut current = owner
        .prepare(receipt("done", "local"), "hash", "identity")
        .unwrap()
        .1;
    assert!(owner.admit("done", "local", 1, 1).unwrap());
    current.value["status"] = json!("completed");
    current.value["generation_outcome"] = json!("completed");
    current.value["upstream_job"] = json!({"id":"job","kind":"comfyui"});
    owner.update(&mut current).unwrap();
    owner.release("done", Some(&current)).unwrap();
    let result = control
        .terminate(
            "done",
            "cancelled",
            &json!({"code":"provider_rejection"}),
            None,
            Some(&current.value["upstream_job"]),
        )
        .unwrap();
    assert_eq!(result.status(), "completed");
    assert!(result.value.get("upstream_terminal").is_none());
    assert_eq!(result.value["errors"], json!([]));
}

#[test]
fn exact_source_known_completion_wins_cancel_before_first_retention() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    let mut owner = Store::open(&directory).unwrap();
    let mut control = Store::open(&directory).unwrap();
    let mut current = owner
        .prepare(receipt("finishing", "local"), "hash", "identity")
        .unwrap()
        .1;
    assert!(owner.admit("finishing", "local", 1, 1).unwrap());
    current.value["status"] = json!("finalizing");
    current.value["generation_outcome"] = json!("completed");
    current.value["upstream_job"] = json!({"id":"job","kind":"comfyui"});
    owner.update(&mut current).unwrap();
    owner.reserve("finishing", 1024, 2048).unwrap();
    let result = control
        .terminate(
            "finishing",
            "cancelled",
            &json!({"code":"provider_rejection"}),
            None,
            Some(&current.value["upstream_job"]),
        )
        .unwrap();
    assert_eq!(result.value["generation_outcome"], "completed");
    assert!(owner.owns("finishing", Some(&current)).unwrap());
    owner
        .retain("finishing", 0, &[0u8; 16], Some(&current))
        .unwrap();
    assert_eq!(control.pending("finishing").unwrap().len(), 1);
}

#[test]
fn exact_source_empty_terminal_release_frees_full_budget_without_erasing_receipt() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    let mut owner = Store::open(&directory).unwrap();
    let mut peer = Store::open(&directory).unwrap();
    let current = owner
        .prepare(receipt("empty", "local"), "hash", "identity")
        .unwrap()
        .1;
    assert!(owner.admit("empty", "local", 1, 1).unwrap());
    owner.reserve("empty", 1024, 1024).unwrap();
    let done = owner
        .terminate(
            "empty",
            "failed",
            &json!({"code":"provider_rejection"}),
            Some(&current),
            None,
        )
        .unwrap();
    owner.release("empty", Some(&done)).unwrap();
    peer.prepare(receipt("fresh", "local"), "fresh", "identity")
        .unwrap();
    peer.reserve("fresh", 1024, 1024).unwrap();
    assert_eq!(peer.get("empty").unwrap().status(), "failed");
}

#[test]
fn exact_source_four_output_records_preserve_pagination_lookahead_and_duplicate_rollback() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    let ids = (0..4)
        .map(|_| uuid::Uuid::new_v4().to_string())
        .collect::<Vec<_>>();
    {
        let store = Store::open(&directory).unwrap();
        assert!(store.list_outputs(0, 100).unwrap().is_empty());
        for id in &ids {
            store.add_output(&output(id, "request-1")).unwrap();
        }
        assert_eq!(store.output(&ids[0]).unwrap(), output(&ids[0], "request-1"));
        let ids_for = |offset| {
            store
                .list_outputs(offset, 2)
                .unwrap()
                .into_iter()
                .map(|value| value["output_id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids_for(0),
            vec![ids[3].clone(), ids[2].clone(), ids[1].clone()]
        );
        assert_eq!(ids_for(2), vec![ids[1].clone(), ids[0].clone()]);
        assert!(store.list_outputs(4, 2).unwrap().is_empty());
        assert_eq!(store.output("absent").unwrap_err().code, "invalid_input");
        assert!(
            store
                .add_output(&output(&ids[0], "different-request"))
                .is_err()
        );
        assert_eq!(store.output(&ids[0]).unwrap()["request_id"], "request-1");
    }
    let restarted = Store::open(&directory).unwrap();
    assert_eq!(
        restarted.output(&ids[3]).unwrap(),
        output(&ids[3], "request-1")
    );
}
