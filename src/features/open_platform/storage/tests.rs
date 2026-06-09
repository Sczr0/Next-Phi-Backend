use std::path::PathBuf;

use uuid::Uuid;

use super::*;

fn temp_db_path() -> PathBuf {
    std::env::temp_dir().join(format!("phi_open_platform_{}.db", Uuid::new_v4()))
}

async fn setup_storage() -> OpenPlatformStorage {
    let path = temp_db_path();
    let storage = OpenPlatformStorage::connect_sqlite(path.to_string_lossy().as_ref(), true)
        .await
        .expect("connect sqlite for open platform");
    storage.init_schema().await.expect("init schema");
    storage
}

#[tokio::test]
async fn upsert_developer_by_github_is_idempotent() {
    let storage = setup_storage().await;
    let now1 = 1_700_000_000_i64;
    let now2 = now1 + 10;

    let dev1 = storage
        .upsert_developer_by_github("1001", "alice", Some("alice@x.test"), now1)
        .await
        .expect("first upsert developer");
    let dev2 = storage
        .upsert_developer_by_github("1001", "alice-renamed", None, now2)
        .await
        .expect("second upsert developer");

    assert_eq!(dev1.id, dev2.id);
    assert_eq!(dev2.github_login, "alice-renamed");
    assert_eq!(dev2.email, None);
    assert_eq!(dev2.updated_at, now2);
}

#[test]
fn open_platform_select_queries_use_explicit_columns() {
    let queries = [
        SELECT_DEVELOPER_BY_GITHUB_USER_ID,
        SELECT_DEVELOPER_BY_ID,
        SELECT_API_KEY_BY_ID,
        SELECT_API_KEY_BY_HASH,
        SELECT_API_KEYS_BY_DEVELOPER,
        SELECT_ACTIVE_API_KEYS_BY_DEVELOPER,
        SELECT_API_KEY_EVENTS_BY_KEY,
    ];

    for query in queries {
        assert!(
            !query.contains("SELECT *"),
            "open platform storage query must keep an explicit column list: {query}"
        );
    }
    assert!(SELECT_API_KEY_BY_ID.contains("usage_count"));
    assert!(SELECT_API_KEY_EVENTS_BY_KEY.contains("metadata"));
}

#[test]
fn cleanup_expired_active_keys_query_matches_expiry_index_shape() {
    assert!(CLEANUP_EXPIRED_ACTIVE_API_KEYS_SQL.contains("UPDATE api_keys"));
    assert!(CLEANUP_EXPIRED_ACTIVE_API_KEYS_SQL.contains("status = ?"));
    assert!(CLEANUP_EXPIRED_ACTIVE_API_KEYS_SQL.contains("expires_at IS NOT NULL"));
    assert!(CLEANUP_EXPIRED_ACTIVE_API_KEYS_SQL.contains("expires_at > 0"));
    assert!(CLEANUP_EXPIRED_ACTIVE_API_KEYS_SQL.contains("expires_at <= ?"));
}

#[tokio::test]
async fn api_key_lifecycle_issue_rotate_revoke_and_events() {
    let storage = setup_storage().await;
    let now = 1_700_000_100_i64;

    let developer = storage
        .upsert_developer_by_github("2002", "bob", Some("bob@x.test"), now)
        .await
        .expect("upsert developer");

    let key1 = storage
        .create_api_key(CreateApiKeyParams {
            developer_id: developer.id.clone(),
            name: "prod-key".to_string(),
            key_prefix: "pgr_live_".to_string(),
            key_last4: "a1b2".to_string(),
            key_hash: "hash_key_1".to_string(),
            scopes: vec![String::from("public.read"), String::from("profile.read")],
            expires_at: None,
            now_ts: now,
        })
        .await
        .expect("create key1");
    assert_eq!(key1.status, API_KEY_STATUS_ACTIVE);
    assert_eq!(key1.scopes.len(), 2);

    let listed = storage
        .list_api_keys_by_developer(&developer.id, false)
        .await
        .expect("list keys");
    assert_eq!(listed.len(), 1);

    let rotate_grace = now + 3600;
    let key2 = storage
        .rotate_api_key(RotateApiKeyParams {
            key_id: key1.id.clone(),
            new_name: "prod-key-v2".to_string(),
            new_key_prefix: "pgr_live_".to_string(),
            new_key_last4: "c3d4".to_string(),
            new_key_hash: "hash_key_2".to_string(),
            new_scopes: vec![String::from("public.read")],
            grace_expires_at: Some(rotate_grace),
            now_ts: now + 5,
            operator_id: Some(developer.id.clone()),
            request_id: Some("req_rotate_001".to_string()),
        })
        .await
        .expect("rotate key");
    assert_eq!(key2.status, API_KEY_STATUS_ACTIVE);
    assert_eq!(key2.name, "prod-key-v2");
    assert_eq!(key2.expires_at, None);

    let old_after_rotate = storage
        .get_api_key_by_id(&key1.id)
        .await
        .expect("query old key")
        .expect("old key should exist");
    assert_eq!(old_after_rotate.status, API_KEY_STATUS_ACTIVE);
    assert_eq!(
        old_after_rotate.replaced_by_key_id.as_deref(),
        Some(key2.id.as_str())
    );
    assert_eq!(old_after_rotate.expires_at, Some(rotate_grace));

    storage
        .revoke_api_key(
            &key2.id,
            Some("manual revoke"),
            Some(&developer.id),
            Some("req_revoke_001"),
            now + 20,
        )
        .await
        .expect("revoke key2");

    let key2_after_revoke = storage
        .get_api_key_by_id(&key2.id)
        .await
        .expect("query key2")
        .expect("key2 exists");
    assert_eq!(key2_after_revoke.status, API_KEY_STATUS_REVOKED);
    assert_eq!(key2_after_revoke.revoked_at, Some(now + 20));
    let active_after_revoke = storage
        .list_api_keys_by_developer(&developer.id, false)
        .await
        .expect("list active keys after revoke");
    assert_eq!(active_after_revoke.len(), 1);

    let events_key1 = storage
        .list_api_key_events(&key1.id, 20)
        .await
        .expect("list events key1");
    assert!(
        events_key1
            .iter()
            .any(|e| e.event_type == API_KEY_EVENT_ROTATED),
        "old key should have rotated event"
    );

    let events_key2 = storage
        .list_api_key_events(&key2.id, 20)
        .await
        .expect("list events key2");
    assert!(
        events_key2
            .iter()
            .any(|e| e.event_type == API_KEY_EVENT_ISSUED),
        "new key should have issued event"
    );
    assert!(
        events_key2
            .iter()
            .any(|e| e.event_type == API_KEY_EVENT_REVOKED),
        "new key should have revoked event"
    );

    let expired_rows = storage
        .cleanup_expired_active_keys(rotate_grace + 1)
        .await
        .expect("cleanup expired keys");
    assert!(expired_rows >= 1);

    let old_after_cleanup = storage
        .get_api_key_by_id(&key1.id)
        .await
        .expect("query old key after cleanup")
        .expect("old key exists");
    assert_eq!(old_after_cleanup.status, API_KEY_STATUS_EXPIRED);

    let active_after_cleanup = storage
        .list_api_keys_by_developer(&developer.id, false)
        .await
        .expect("list active keys after cleanup");
    assert_eq!(active_after_cleanup.len(), 0);
    let all_after_cleanup = storage
        .list_api_keys_by_developer(&developer.id, true)
        .await
        .expect("list all keys after cleanup");
    assert_eq!(all_after_cleanup.len(), 2);

    storage
        .soft_delete_api_key(
            &key2.id,
            Some("hide from default list"),
            Some(&developer.id),
            Some("req_delete_001"),
            now + 30,
        )
        .await
        .expect("soft delete key2");
    let key2_after_delete = storage
        .get_api_key_by_id(&key2.id)
        .await
        .expect("query key2 after delete")
        .expect("key2 exists after soft delete");
    assert_eq!(key2_after_delete.status, API_KEY_STATUS_DELETED);

    let events_key2_after_delete = storage
        .list_api_key_events(&key2.id, 20)
        .await
        .expect("list events key2 after delete");
    assert!(
        events_key2_after_delete
            .iter()
            .any(|e| e.event_type == API_KEY_EVENT_DELETED),
        "key2 should have deleted event"
    );
}
