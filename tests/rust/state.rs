use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::jobs::{Receipt, Store};
mod common;

#[test]
fn existing_request_identity_survives_reopen_and_never_replays() {
    let workspace = common::Workspace::new();
    let root = workspace.0.join("state");
    {
        let mut store = Store::open(&root).expect("isolated state");
        let receipt = Receipt::new(
            json!({"request_id":"same-id","status":"prepared","connection":"default", "outputs":[],"generation_outcome":"not_submitted"}),
        );
        let (fresh, receipt) = store
            .prepare(receipt, "effective", "account")
            .expect("intent");
        assert!(fresh);
        assert!(store.owns("same-id", Some(&receipt)).expect("ownership"));
        assert!(
            store
                .prepare(receipt.clone(), "different", "account")
                .is_err()
        );
        store.release("same-id", Some(&receipt)).expect("release");
        drop(store);
        let mut reopened = Store::open(&root).expect("reopen");
        let (fresh, old) = reopened
            .prepare(receipt, "effective", "account")
            .expect("existing intent");
        assert!(!fresh);
        assert_eq!(old.value["request_id"], "same-id");
        assert_eq!(reopened.identity("same-id").expect("identity"), "account");
    }
}
