mod common;
// auth parsing + key fingerprint — extracted from src (CHORE-004)

use byline::ingest::auth::parse;
use byline::ingest::fingerprint;
use byline::store::AuthMethod;

#[test]
fn basic_whole_payload_is_key_no_colon() {
    // "3f9f...key" base64-encoded, no username: separator
    let enc = "a2V5LXV1aWQ="; // base64("key-uuid")
    let r = parse(Some(&format!("Basic {enc}")), None);
    assert_eq!(r.key().unwrap(), "key-uuid");
    assert_eq!(r.method(), Some(AuthMethod::Basic));
}

#[test]
fn query_fallback() {
    let r = parse(None, Some("api_key=q-key&other=1"));
    assert_eq!(r.key().unwrap(), "q-key");
    assert_eq!(r.method(), Some(AuthMethod::Query));
}

#[test]
fn missing_auth_is_error() {
    let r = parse(None, None);
    assert!(r.key().is_none());
    let r = parse(Some("Bearer x"), None);
    assert!(r.key().is_none());
    let r = parse(Some("Basic !!!not-b64!!!"), None);
    assert!(r.key().is_none());
}

#[test]
fn fingerprint_caps_short_keys() {
    assert_eq!(fingerprint("abcdefgh"), "efgh");
    assert_eq!(fingerprint("abc"), "");  // short: nothing, never partial plaintext
    assert_eq!(fingerprint(""), "");     // empty auth present in code today
    assert_eq!(fingerprint("héllo"), ""); // multibyte boundary: no panic, no split char
}
