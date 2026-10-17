#[path = "common/image.rs"]
mod image_fixture;
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::{
    error::Result,
    jobs::{Receipt, Store},
};
mod common;

// Store-level source contracts: integration/{terminal-lifecycle,reservation-lifecycle}.test.ts
// and unit/store.test.ts. Two SQLite handles/reopen are not two actual stdio hosts.
fn receipt(id: &str, connection: &str) -> Receipt {
    Receipt::new(
        json!({"request_id":id,"connection":connection,"status":"prepared","adapter":"openai-responses",
        "operation":"generate","requested_model":"image-model","outputs":[],"warnings":[],"errors":[],"deviations":[],
        "generation_outcome":"not_submitted","storage_outcome":"not_started","another_submission_may_charge":true,
        "created_at":"2026-10-11T00:00:00.000Z","updated_at":"2026-10-11T00:00:00.000Z"}),
    )
}
fn code<T>(result: Result<T>) -> String {
    result.err().expect("invalid fixture must reject").code
}
fn prepare(store: &mut Store, id: &str, connection: &str) -> Receipt {
    let (fresh, receipt) = store
        .prepare(receipt(id, connection), id, "identity")
        .unwrap();
    assert!(fresh, "fixture intent must be new");
    receipt
}
fn running(store: &mut Store, id: &str, connection: &str, kind: &str) -> Receipt {
    let mut receipt = prepare(store, id, connection);
    assert!(store.admit(id, connection, 16, 16).unwrap());
    receipt.value["status"] = json!("running");
    receipt.value["generation_outcome"] = json!("running");
    receipt.value["upstream_job"] = json!({"id":"owned-job","kind":kind});
    store.update(&mut receipt).unwrap();
    receipt
}
fn finish_probe(store: &mut Store, id: &str) {
    let mut end = store.get(id).unwrap();
    end.value["status"] = json!("failed");
    store.update(&mut end).unwrap();
    store.release(id, Some(&end)).unwrap();
}

#[test]
fn authoritative_terminal_states_release_admission_and_survive_reopen_without_replay() {
    let workspace = common::Workspace::new();
    for (adapter, kind, state) in [
        ("openai-responses", "responses", "failed"),
        ("openai-responses", "responses", "cancelled"),
        ("gemini-interactions", "interactions", "failed"),
        ("openai-responses", "responses", "incomplete"),
    ] {
        let directory = workspace.0.join(format!("{adapter}-{state}"));
        let mut owner = Store::open(&directory).unwrap();
        let mut peer = Store::open(&directory).unwrap();
        let old = running(&mut owner, "old", "local", kind);
        owner.release("old", Some(&old)).unwrap();
        prepare(&mut peer, "blocked", "local");
        assert!(
            !peer.admit("blocked", "local", 1, 1).unwrap(),
            "detached running job retains capacity"
        );
        let done = peer
            .terminate(
                "old",
                state,
                &json!({"code":"provider_rejection"}),
                None,
                Some(&old.value["upstream_job"]),
            )
            .unwrap();
        assert_eq!(done.value["generation_outcome"], state);
        assert_eq!(
            done.value["status"],
            if state == "incomplete" {
                "failed"
            } else {
                state
            }
        );
        assert!(peer.admit("blocked", "local", 1, 1).unwrap());
        finish_probe(&mut peer, "blocked");
        let repeated = peer
            .terminate(
                "old",
                state,
                &json!({"code":"must-not-append"}),
                None,
                Some(&old.value["upstream_job"]),
            )
            .unwrap();
        assert_eq!(
            repeated.value["errors"].as_array().unwrap().len(),
            1,
            "terminal reporting is idempotent"
        );
        drop(owner);
        drop(peer);
        for round in 0..2 {
            let mut reopened = Store::open(&directory).unwrap();
            assert_eq!(
                reopened.get("old").unwrap().value["generation_outcome"],
                state
            );
            assert_eq!(
                reopened.get("old").unwrap().value["upstream_job"],
                json!({"id":"owned-job","kind":kind})
            );
            let (fresh, original) = reopened
                .prepare(receipt("old", "local"), "old", "identity")
                .unwrap();
            assert!(
                !fresh,
                "known logical request can never become new paid intent"
            );
            assert_eq!(original.value["generation_outcome"], state);
            assert_eq!(
                code(reopened.prepare(receipt("old", "local"), "changed-inputs", "identity")),
                "request_id_conflict"
            );
            let id = format!("fresh-{round}");
            prepare(&mut reopened, &id, "local");
            assert!(reopened.admit(&id, "local", 1, 1).unwrap());
            finish_probe(&mut reopened, &id);
        }
    }
}

#[test]
fn wrong_job_and_local_cancellation_do_not_claim_authoritative_terminal_state() {
    let workspace = common::Workspace::new();
    let directory = workspace.0.join("state");
    let mut owner = Store::open(&directory).unwrap();
    let mut control = Store::open(&directory).unwrap();
    let old = running(&mut owner, "uncertain", "local", "responses");
    owner.release("uncertain", Some(&old)).unwrap();
    control.cancel("uncertain").unwrap();
    assert!(owner.cancelled("uncertain").unwrap());
    assert_eq!(
        code(control.terminate(
            "uncertain",
            "failed",
            &json!({"code":"provider_rejection"}),
            None,
            Some(&json!({"id":"another-job","kind":"responses"}))
        )),
        "permission_denied"
    );
    assert_eq!(control.get("uncertain").unwrap().value["status"], "running");
    assert!(control.get("uncertain").unwrap().value["upstream_terminal"].is_null());
    prepare(&mut control, "fresh", "local");
    assert!(!control.admit("fresh", "local", 1, 1).unwrap());
    drop(owner);
    drop(control);
    let mut reopened = Store::open(&directory).unwrap();
    assert!(reopened.cancelled("uncertain").unwrap());
    assert!(!reopened.admit("fresh", "local", 1, 1).unwrap());
}

#[test]
fn shared_terminal_control_fences_old_writer_and_preserves_retained_originals() {
    let workspace = common::Workspace::new();
    let bytes = image_fixture::original();
    let directory = workspace.0.join("state");
    let mut owner = Store::open(&directory).unwrap();
    let mut control = Store::open(&directory).unwrap();
    let mut stale = running(&mut owner, "race", "local", "responses");
    owner.reserve("race", 4096, 8192).unwrap();
    let terminal = control
        .terminate(
            "race",
            "failed",
            &json!({"code":"provider_rejection"}),
            None,
            Some(&stale.value["upstream_job"]),
        )
        .unwrap();
    assert!(!owner.owns("race", Some(&stale)).unwrap());
    stale.value["status"] = json!("completed");
    assert_eq!(code(owner.update(&mut stale)), "outcome_unknown");
    assert_eq!(
        code(owner.retain("race", 0, &bytes, Some(&stale))),
        "outcome_unknown"
    );
    assert!(!owner.renew("race", Some(&stale)).unwrap());
    assert_eq!(terminal.value["status"], "failed");
    assert!(
        control.claim("race", None, true).unwrap(),
        "received original can reopen storage recovery, not generation"
    );
    let recovered = control.get("race").unwrap();
    control.reserve("race", 4096, 8192).unwrap();
    control.retain("race", 0, &bytes, Some(&recovered)).unwrap();
    let item = control.pending("race").unwrap().remove(0);
    assert_eq!(item.bytes.as_deref(), Some(bytes.as_slice()));
    control.release("race", Some(&recovered)).unwrap();
    assert!(
        control.claim("race", None, false).unwrap(),
        "retained original is locally recoverable"
    );
    let current = control.get("race").unwrap();
    assert_ne!(
        current.generation, recovered.generation,
        "recovery increments the writer generation"
    );
    let mut obsolete = recovered.clone();
    assert_eq!(code(control.update(&mut obsolete)), "outcome_unknown");
    assert_eq!(
        code(control.retain("race", 1, &bytes, Some(&obsolete))),
        "outcome_unknown"
    );
    control.release("race", Some(&current)).unwrap();
    drop(owner);
    drop(control);
    let restarted = Store::open(&directory).unwrap();
    let retained = restarted.pending("race").unwrap().remove(0);
    assert_eq!(
        retained.output_id, item.output_id,
        "reopen must preserve result identity"
    );
    assert_eq!(retained.bytes.as_deref(), Some(bytes.as_slice()));
    assert_eq!(
        restarted.get("race").unwrap().value["generation_outcome"],
        "failed"
    );
}

#[test]
fn received_completion_wins_late_cancellation_and_preserves_accounting() {
    let workspace = common::Workspace::new();
    let bytes = image_fixture::original();
    let directory = workspace.0.join("state");
    let mut owner = Store::open(&directory).unwrap();
    let mut control = Store::open(&directory).unwrap();
    let old = running(&mut owner, "completed", "local", "comfyui");
    owner.reserve("completed", 4096, 8192).unwrap();
    owner.retain("completed", 0, &bytes, Some(&old)).unwrap();
    let first = control
        .terminate(
            "completed",
            "cancelled",
            &json!({"code":"provider_rejection"}),
            None,
            Some(&old.value["upstream_job"]),
        )
        .unwrap();
    assert_eq!(first.value["generation_outcome"], "completed");
    assert!(first.value["upstream_terminal"].is_null());
    assert!(first.value["errors"].as_array().unwrap().is_empty());
    assert!(owner.owns("completed", Some(&old)).unwrap());
    owner.retain("completed", 1, &bytes, Some(&old)).unwrap();
    assert_eq!(control.pending("completed").unwrap().len(), 2);
    owner.release("completed", Some(&old)).unwrap();
    prepare(&mut control, "fresh", "local");
    assert!(control.admit("fresh", "local", 1, 1).unwrap());
    drop(owner);
    drop(control);
    let reopened = Store::open(&directory).unwrap();
    assert_eq!(
        reopened.get("completed").unwrap().value["generation_outcome"],
        "completed"
    );
    assert!(reopened.get("completed").unwrap().value["upstream_terminal"].is_null());
    assert_eq!(reopened.pending("completed").unwrap().len(), 2);
}

#[test]
fn terminal_writer_keeps_reservation_until_retention_then_releases_unused_capacity() {
    let workspace = common::Workspace::new();
    let bytes = image_fixture::original();
    assert!(bytes.len() < 4096, "tiny source-generated image fixture");
    let directory = workspace.0.join("state");
    let mut owner = Store::open(&directory).unwrap();
    let mut peer = Store::open(&directory).unwrap();
    let old = running(&mut owner, "terminal", "local", "responses");
    owner.reserve("terminal", 4096, 4096).unwrap();
    let done = owner
        .terminate(
            "terminal",
            "failed",
            &json!({"code":"provider_rejection"}),
            Some(&old),
            Some(&old.value["upstream_job"]),
        )
        .unwrap();
    assert!(owner.owns("terminal", Some(&done)).unwrap());
    prepare(&mut peer, "fresh", "local");
    assert!(peer.admit("fresh", "local", 1, 1).unwrap());
    assert_eq!(code(peer.reserve("fresh", 4096, 4096)), "rate_limited");
    owner.retain("terminal", 0, &bytes, Some(&done)).unwrap();
    owner.release("terminal", Some(&done)).unwrap();
    peer.reserve("fresh", 4096 - bytes.len() as i64, 4096)
        .unwrap();
    assert_eq!(
        owner.pending("terminal").unwrap()[0].bytes.as_deref(),
        Some(bytes.as_slice())
    );
    assert_eq!(
        owner.get("terminal").unwrap().value["generation_outcome"],
        "failed"
    );
}

#[test]
fn exact_source_terminal_retention_keeps_full_budget_until_writer_release() {
    let workspace = common::Workspace::new();
    let directory = workspace.0.join("state");
    let mut owner = Store::open(&directory).unwrap();
    let mut peer = Store::open(&directory).unwrap();
    let initial = |id: &str| {
        Receipt::new(json!({
            "request_id":id,"connection":"local","adapter":"openai-images",
            "operation":"generate","status":"prepared","requested_model":"image-model",
            "upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
            "generation_outcome":"not_submitted","storage_outcome":"not_started",
            "another_submission_may_charge":false,
            "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z"
        }))
    };
    let mut receipt = owner
        .prepare(initial("terminal"), "hash", "identity")
        .unwrap()
        .1;
    assert!(owner.admit("terminal", "local", 1, 1).unwrap());
    receipt.value["status"] = json!("running");
    receipt.value["upstream_job"] = json!({"id":"job","kind":"responses"});
    owner.update(&mut receipt).unwrap();
    owner.reserve("terminal", 1024, 1024).unwrap();
    let done = owner
        .terminate(
            "terminal",
            "failed",
            &json!({"code":"provider_rejection"}),
            Some(&receipt),
            Some(&receipt.value["upstream_job"]),
        )
        .unwrap();
    assert!(owner.owns("terminal", Some(&done)).unwrap());
    peer.prepare(initial("fresh"), "fresh", "identity").unwrap();
    assert!(peer.admit("fresh", "local", 1, 1).unwrap());
    assert_eq!(code(peer.reserve("fresh", 1024, 1024)), "rate_limited");
    let bytes = [0u8; 32];
    owner.retain("terminal", 0, &bytes, Some(&done)).unwrap();
    assert!(owner.owns("terminal", Some(&done)).unwrap());
    assert_eq!(code(peer.reserve("fresh", 992, 1024)), "rate_limited");
    assert_eq!(
        owner.pending("terminal").unwrap()[0].bytes.as_deref(),
        Some(bytes.as_slice())
    );
    owner.release("terminal", Some(&done)).unwrap();
    peer.reserve("fresh", 992, 1024).unwrap();
    assert!(!owner.owns("terminal", Some(&done)).unwrap());
    assert_eq!(owner.get("terminal").unwrap().value["status"], "failed");
    assert_eq!(
        owner.get("terminal").unwrap().value["generation_outcome"],
        "failed"
    );
}

#[test]
fn releasing_empty_terminal_reservation_keeps_receipt_but_frees_full_budget() {
    let workspace = common::Workspace::new();
    let directory = workspace.0.join("state");
    let mut owner = Store::open(&directory).unwrap();
    let mut peer = Store::open(&directory).unwrap();
    let old = running(&mut owner, "empty", "local", "responses");
    owner.reserve("empty", 4096, 4096).unwrap();
    let done = owner
        .terminate(
            "empty",
            "failed",
            &json!({"code":"provider_rejection"}),
            Some(&old),
            None,
        )
        .unwrap();
    owner.release("empty", Some(&done)).unwrap();
    prepare(&mut peer, "fresh", "local");
    peer.reserve("fresh", 4096, 4096).unwrap();
    assert_eq!(peer.get("empty").unwrap().value["status"], "failed");
    assert!(
        !peer
            .prepare(receipt("empty", "local"), "empty", "identity")
            .unwrap()
            .0
    );
    drop(owner);
    drop(peer);
    let reopened = Store::open(&directory).unwrap();
    assert_eq!(
        reopened.get("empty").unwrap().value["generation_outcome"],
        "failed"
    );
    assert!(reopened.pending("empty").unwrap().is_empty());
}

#[test]
fn two_real_store_instances_enforce_global_and_connection_admission_limits() {
    let workspace = common::Workspace::new();
    let directory = workspace.0.join("state");
    let mut first = Store::open(&directory).unwrap();
    let mut second = Store::open(&directory).unwrap();
    prepare(&mut first, "a1", "a");
    prepare(&mut second, "a2", "a");
    prepare(&mut second, "b1", "b");
    prepare(&mut first, "b2", "b");
    assert_eq!(second.queued().unwrap(), 4);
    assert!(first.admit("a1", "a", 2, 1).unwrap());
    assert!(!second.admit("a2", "a", 2, 1).unwrap());
    assert!(second.admit("b1", "b", 2, 1).unwrap());
    assert!(!first.admit("b2", "b", 2, 2).unwrap());
    assert_eq!(first.queued().unwrap(), 2);
    let mut done = first.get("a1").unwrap();
    done.value["status"] = json!("completed");
    first.update(&mut done).unwrap();
    assert!(second.admit("a2", "a", 2, 1).unwrap());
    assert_eq!(first.get("a2").unwrap().value["status"], "submitting");
}
