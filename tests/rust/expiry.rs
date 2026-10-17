mod common;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::jobs::{Receipt, Store};

fn receipt(id: &str) -> Receipt {
    Receipt::new(
        json!({"request_id":id,"connection":"test","status":"prepared","generation_outcome":"not_submitted","outputs":[],"errors":[],"warnings":[],"deviations":[]}),
    )
}

#[test]
fn expired_owner_is_fenced_and_only_unsubmitted_or_empty_unknown_capacity_is_released() {
    let workspace = Workspace::new();
    for mode in [
        "prepared", "unknown", "upstream", "retained", "outputs", "terminal",
    ] {
        let root = workspace.0.join(mode);
        let mut owner = Store::open(&root).unwrap();
        let (_, mut stale) = owner
            .prepare(receipt("old"), "effective", "account")
            .unwrap();
        if mode != "prepared" {
            assert!(owner.admit("old", "test", 4, 4).unwrap());
            stale.value["status"] = json!(if mode == "terminal" {
                "failed"
            } else {
                "submitting"
            });
            stale.value["generation_outcome"] = json!(if mode == "terminal" {
                "failed"
            } else {
                "unknown"
            });
            if mode == "upstream" {
                stale.value["upstream_job"] = json!({"id":"original","kind":"responses"});
            }
            if mode == "outputs" {
                stale.value["outputs"] = json!([{"output_id":"existing"}]);
                stale.value["generation_outcome"] = json!("completed");
            }
            if mode == "terminal" {
                stale.value["upstream_terminal"] = json!("failed");
            }
            owner.update(&mut stale).unwrap();
            owner.reserve("old", 4096, 8192).unwrap();
            if mode == "retained" {
                owner
                    .retain("old", 0, &image_fixture::original(), Some(&stale))
                    .unwrap();
            }
        }
        let fault = rusqlite::Connection::open(root.join("receipts.sqlite")).unwrap();
        fault
            .execute("UPDATE requests SET lease=0 WHERE id='old'", [])
            .unwrap();
        let mut peer = Store::open(&root).unwrap();
        assert_eq!(peer.queued().unwrap(), 0);
        let current = peer.get("old").unwrap();
        assert!(!owner.owns("old", Some(&stale)).unwrap());
        assert!(owner.update(&mut stale).is_err());
        match mode {
            "prepared" => {
                assert_eq!(current.status(), "failed");
                assert_eq!(current.value["generation_outcome"], "not_submitted");
            }
            "unknown" => {
                assert_eq!(current.status(), "outcome_unknown");
                assert_eq!(current.value["generation_outcome"], "unknown");
            }
            "upstream" => {
                assert_eq!(current.status(), "running");
                assert_eq!(current.value["upstream_job"]["id"], "original");
            }
            "retained" => assert!(peer.has_pending("old").unwrap()),
            "outputs" => assert_eq!(current.value["outputs"][0]["output_id"], "existing"),
            "terminal" => assert_eq!(current.value["upstream_terminal"], "failed"),
            _ => unreachable!(),
        }
        peer.prepare(receipt("fresh"), "new", "account").unwrap();
        assert_eq!(
            peer.admit("fresh", "test", 1, 1).unwrap(),
            mode != "upstream"
        );
        let reservation: i64 = fault
            .query_row(
                "SELECT reservation FROM requests WHERE id='old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            reservation,
            if matches!(mode, "retained" | "upstream") {
                4096
            } else {
                0
            }
        );
        if mode == "unknown" {
            assert!(peer.claim("old", None, false).unwrap());
            assert!(!peer.needs_local_recovery("old").unwrap());
        }
        drop(fault);
        drop(peer);
        drop(owner);
        let mut reopened = Store::open(&root).unwrap();
        assert!(
            !reopened
                .prepare(receipt("old"), "effective", "account")
                .unwrap()
                .0
        );
    }
}

#[test]
fn terminal_confirmation_validates_state_and_keeps_completed_bytes_authoritative() {
    let workspace = Workspace::new();
    let mut store = Store::open(&workspace.0.join("state")).unwrap();
    let (_, mut original) = store
        .prepare(receipt("complete"), "effective", "account")
        .unwrap();
    assert!(store.admit("complete", "test", 2, 2).unwrap());
    original.value["generation_outcome"] = json!("completed");
    store.update(&mut original).unwrap();
    assert_eq!(
        store
            .terminate("complete", "completed", &json!({}), None, None)
            .unwrap_err()
            .code,
        "invalid_response"
    );
    let closed = store
        .terminate(
            "complete",
            "cancelled",
            &json!({"code":"provider_rejection"}),
            Some(&original),
            None,
        )
        .unwrap();
    assert_eq!(closed.value["generation_outcome"], "completed");
    assert!(closed.value["upstream_terminal"].is_null());
    assert!(closed.value["errors"].as_array().unwrap().is_empty());
}
