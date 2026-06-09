use axum::http::{HeaderMap, HeaderValue, header};

use crate::config::OpenPlatformConfig;

use super::session::{build_clear_cookie_value, build_set_cookie_value, read_cookie_value};

fn test_cfg() -> OpenPlatformConfig {
    let mut cfg = OpenPlatformConfig::default();
    cfg.session.cookie_name = "op_session".into();
    cfg.session.cookie_secure = false;
    cfg
}

#[test]
fn cookie_builder_sets_expected_flags() {
    let cfg = test_cfg();
    let set_cookie = build_set_cookie_value(&cfg, "abc");
    assert!(set_cookie.contains("op_session=abc"));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));

    let clear_cookie = build_clear_cookie_value(&cfg);
    assert!(clear_cookie.contains("Max-Age=0"));
}

#[test]
fn cookie_reader_extracts_value() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_static("foo=1; op_session=token-123; bar=2"),
    );
    let got = read_cookie_value(&headers, "op_session");
    assert_eq!(got.as_deref(), Some("token-123"));
}
