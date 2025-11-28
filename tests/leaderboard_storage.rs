use phi_backend::features::stats::storage::StatsStorage;
use sqlx::Row;

#[tokio::test]
async fn upsert_improves_only() {
    let path = "./resources/test_lb.db";
    if std::fs::metadata(path).is_ok() {
        let _ = std::fs::remove_file(path);
    }
    let storage = StatsStorage::connect_sqlite(path, false).await.unwrap();
    storage.init_schema().await.unwrap();

    let now = chrono::Utc::now().to_rfc3339();
    storage
        .upsert_leaderboard_rks("u1", 10.0, Some("k"), 0.0, false, &now)
        .await
        .unwrap();
    // worse score should not overwrite
    storage
        .upsert_leaderboard_rks("u1", 9.0, Some("k"), 0.0, false, &now)
        .await
        .unwrap();
    // read back
    let row = sqlx::query("SELECT total_rks FROM leaderboard_rks WHERE user_hash='u1'")
        .fetch_one(&storage.pool)
        .await
        .unwrap();
    let v: f64 = row.get::<f64, _>(0);
    assert!((v - 10.0).abs() < 1e-6);
    // better score overwrites
    storage
        .upsert_leaderboard_rks("u1", 11.0, Some("k"), 0.0, false, &now)
        .await
        .unwrap();
    let row = sqlx::query("SELECT total_rks FROM leaderboard_rks WHERE user_hash='u1'")
        .fetch_one(&storage.pool)
        .await
        .unwrap();
    let v: f64 = row.get::<f64, _>(0);
    assert!((v - 11.0).abs() < 1e-6);
}
