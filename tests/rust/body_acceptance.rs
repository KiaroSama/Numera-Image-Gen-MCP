use numera_image_gen_mcp::{error::NumeraError, http::bounded::BoundedBytes};

#[test]
fn exact_source_chunks_advertised_actual_limits_absent_body_and_cancel_reason_are_preserved() {
    let mut body = BoundedBytes::new(None, true, 3).unwrap();
    body.push(&[1, 2], None).unwrap();
    body.push(&[3], None).unwrap();
    assert_eq!(body.finish(), [1, 2, 3]);
    let error = BoundedBytes::new(Some(4), true, 3).err().unwrap();
    assert_eq!(error.code, "invalid_response");
    assert_eq!(error.stage, "response");
    let mut body = BoundedBytes::new(None, true, 2).unwrap();
    body.push(&[1, 2], None).unwrap();
    assert_eq!(body.push(&[3], None).unwrap_err().code, "invalid_response");
    let error = BoundedBytes::new(None, false, 3).err().unwrap();
    assert_eq!(error.code, "invalid_response");
    assert_eq!(error.message, "Response body is empty.");
    let mut body = BoundedBytes::new(None, true, 3).unwrap();
    let reason = NumeraError::new("cancelled", "fixture cancellation");
    let error = body.push(&[1, 2], Some(reason)).unwrap_err();
    assert_eq!(error.message, "fixture cancellation");
    assert_eq!(error.code, "cancelled");
    assert!(body.finish().is_empty());
    assert!(
        BoundedBytes::new(None, true, 3)
            .unwrap()
            .finish()
            .is_empty()
    );
}
