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

#[derive(serde::Deserialize)]
struct TextOracle {
    text: String,
    stored: String,
    #[serde(rename = "bytes")]
    byteshex: String,
    utf8_bytes: usize,
}

#[test]
fn sqlite_text_url_binding_matches_hosted_utf16_replacement_oracle() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".ci-work/parity/sqlite-utf16-text.json");
    let cases: Vec<TextOracle> =
        serde_json::from_slice(&std::fs::read(path).expect("hosted SQLite text oracle required"))
            .expect("outer oracle envelope");
    assert_eq!(cases.len(), 2, "both UTF16 text boundary cases required");
    let workspace = Workspace::new();
    let mut store = Store::open(&workspace.0.join("state")).unwrap();
    for (index, case) in cases.iter().enumerate() {
        let id = format!("text-{index}");
        let (_, owner) = store.prepare(receipt(&id), &id, "account").unwrap();
        assert!(store.admit(&id, "test", 4, 4).unwrap());
        store.reserve(&id, case.utf8_bytes as i64, 1048576).unwrap();
        let value =
            numera_image_gen_mcp::compat::value::Value::parse(&case.text).expect("raw UTF16 text");
        store
            .retain_url(&id, 0, value.as_js_string().expect("string"), Some(&owner))
            .unwrap();
        let stored = store.pending(&id).unwrap()[0].url.clone().unwrap();
        assert_eq!(stored.checked_str().unwrap(), case.stored);
        assert_eq!(
            case.stored
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<String>(),
            case.byteshex
        );
        assert_eq!(case.stored.len(), case.utf8_bytes);
        store.release(&id, Some(&owner)).unwrap();
    }
}

#[test]
fn journal_reservations_urls_and_stable_publication_fail_closed_under_writer_fences() {
    let workspace = Workspace::new();
    let mut store = Store::open(&workspace.0.join("state")).unwrap();
    store
        .prepare(receipt("first"), "effective", "account")
        .unwrap();
    assert!(store.admit("first", "test", 4, 4).unwrap());
    let original = store.get("first").unwrap();
    for (amount, budget) in [
        (0, 100),
        (-1, 100),
        (10, 0),
        (9007199254740992, 9007199254740992),
    ] {
        assert_eq!(
            store.reserve("first", amount, budget).unwrap_err().code,
            "invalid_configuration"
        );
    }
    store.reserve("first", 1048576, 2097152).unwrap();
    store
        .retain_url(
            "first",
            0,
            &"https://fixture.example/original".into(),
            Some(&original),
        )
        .unwrap();
    assert_eq!(store.pending("first").unwrap()[0].phase, "waiting");
    assert_eq!(
        store
            .publication("first", 0, |_| Ok("unused".into()), Some(&original))
            .unwrap_err()
            .code,
        "output_file_error"
    );
    let bytes = image_fixture::original();
    store.retain("first", 0, &bytes, Some(&original)).unwrap();
    let stable = store.pending("first").unwrap()[0].output_id.clone();
    store
        .retain_url(
            "first",
            0,
            &"https://fixture.example/other".into(),
            Some(&original),
        )
        .unwrap();
    assert_eq!(store.pending("first").unwrap()[0].output_id, stable);
    assert!(store.pending("first").unwrap()[0].url.is_none());
    assert_eq!(
        store
            .retain("first", 0, b"different", Some(&original))
            .unwrap_err()
            .code,
        "invalid_response"
    );
    assert_eq!(
        store
            .retain("first", 10, &bytes, Some(&original))
            .unwrap_err()
            .code,
        "invalid_response"
    );
    let item = store
        .publication(
            "first",
            0,
            |_| {
                Ok(workspace
                    .0
                    .join("original.png")
                    .to_string_lossy()
                    .into_owned())
            },
            Some(&original),
        )
        .unwrap();
    let same = store
        .publication(
            "first",
            0,
            |_| panic!("target cannot change"),
            Some(&original),
        )
        .unwrap();
    assert_eq!(item.target, same.target);
    assert_eq!(
        store
            .commit(
                "first",
                0,
                &json!({"output_id":"wrong","path":"wrong","sha256":"wrong"}),
                &[],
                Some(&original)
            )
            .unwrap_err()
            .code,
        "output_file_error"
    );
    store
        .save_continuation("first", "scope", &json!({"signature":"synthetic"}))
        .unwrap();
    assert_eq!(
        store.continuation("first", "scope").unwrap(),
        json!({"signature":"synthetic"})
    );
    assert_eq!(
        store.continuation("first", "another").unwrap_err().code,
        "permission_denied"
    );
    assert!(store.continuation("absent", "scope").unwrap().is_null());
    let (_, second) = store
        .prepare(receipt("second"), "second", "account")
        .unwrap();
    assert!(store.admit("second", "test", 4, 4).unwrap());
    assert_eq!(
        store.reserve("second", 1048576, 1048576).unwrap_err().code,
        "rate_limited"
    );
    store.release("second", Some(&second)).unwrap();
    store.release("first", Some(&original)).unwrap();
    assert_eq!(
        store
            .retain("first", 1, &bytes, Some(&original))
            .unwrap_err()
            .code,
        "outcome_unknown"
    );
}
