use super::helpers::{derive_key_last4, mask_key, sanitize_name};

#[test]
fn sanitize_name_rejects_empty() {
    assert!(sanitize_name("   ").is_err());
    assert!(sanitize_name("valid-name").is_ok());
}

#[test]
fn key_masking_works() {
    let last4 = derive_key_last4("pgr_live_xxxxxxxxxxABCD");
    assert_eq!(last4, "ABCD");
    let masked = mask_key("pgr_live_", &last4);
    assert_eq!(masked, "pgr_live_****ABCD");
}
