//! db_split：D1（ADR-0002）一次性迁移工具——把单文件 `usage_stats.db` 拆分为
//! 统计库（events/daily_*/stats_meta）与领域库（8 张业务表）。
//!
//! 用法（维护窗内执行，**必须先停止服务**）：
//! ```bash
//! cargo run -p impl-storage --bin db_split -- ./resources/usage_stats.db ./resources/state.db
//! ```
//! 步骤：备份旧库（`<usage_stats.db>.bak-<ts>`）→ 拷贝为领域库 → 领域库删统计表 →
//! 统计库删业务表 → 两边 VACUUM + integrity_check + **表集互斥断言**。
//! 完成后把 `stats.state_db_path` 配置为 `<state_db>` 并重启（双库模式生效）。

use std::path::{Path, PathBuf};

use sqlx::{AssertSqlSafe, ConnectOptions, Row, SqlitePool, sqlite::SqliteConnectOptions};

/// 统计库表集（10）
const STATS_TABLES: &[&str] = &[
    "events",
    "daily_status",
    "daily_instance",
    "daily_action",
    "daily_user",
    "daily_ip",
    "daily_agg",
    "daily_dau",
    "daily_latency",
    "stats_meta",
];

/// 领域库表集（8）
const STATE_TABLES: &[&str] = &[
    "leaderboard_rks",
    "leaderboard_details",
    "user_profile",
    "save_submissions",
    "session_token_blacklist",
    "session_logout_gate",
    "user_moderation_state",
    "moderation_flags",
];

async fn open_raw(path: &Path) -> Result<SqlitePool, String> {
    let opt = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .log_statements(tracing::log::LevelFilter::Off);
    SqlitePool::connect_with(opt)
        .await
        .map_err(|e| format!("open {path:?}: {e}"))
}

async fn drop_tables(pool: &SqlitePool, tables: &[&str]) -> Result<(), String> {
    for t in tables {
        // 表名来自本文件常量（非用户输入），AssertSqlSafe 标注后构造动态 DDL。
        sqlx::query(AssertSqlSafe(format!("DROP TABLE IF EXISTS \"{t}\"")))
            .execute(pool)
            .await
            .map_err(|e| format!("drop {t}: {e}"))?;
    }
    Ok(())
}

async fn table_set(path: &Path) -> Result<Vec<String>, String> {
    let pool = open_raw(path).await?;
    let rows = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("list tables: {e}"))?;
    let mut out: Vec<String> = rows
        .iter()
        .filter_map(|r| r.try_get::<String, _>("name").ok())
        .collect();
    out.sort();
    pool.close().await;
    Ok(out)
}

/// 表集互斥断言（安全关键）：各库恰好包含预期表集。
async fn assert_table_set(path: &Path, expected: &[&str]) -> Result<(), String> {
    let mut exp: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    exp.sort();
    let actual = table_set(path).await?;
    if actual != exp {
        return Err(format!(
            "表集断言失败 {path:?}：期望 {exp:?}，实际 {actual:?}（中止，勿启用配置）"
        ));
    }
    Ok(())
}

async fn run_split(old_db: &Path, state_db: &Path) -> Result<(), String> {
    if state_db.exists() {
        return Err(format!(
            "{state_db:?} 已存在（拒绝覆盖）；请先移除或指定其它路径"
        ));
    }
    // 1. 旧库快照（在旧库被破坏前完成）
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let bak = PathBuf::from(format!("{}.bak-{ts}", old_db.display()));
    std::fs::copy(old_db, &bak).map_err(|e| format!("backup: {e}"))?;
    tracing::warn!(backup = %bak.display(), "已写入旧库快照（回滚用的最后防线）");

    // 2. 拷贝为领域库
    std::fs::copy(old_db, state_db).map_err(|e| format!("copy to state: {e}"))?;

    // 3. 领域库删统计表；统计库删业务表
    let state_pool = open_raw(state_db).await?;
    drop_tables(&state_pool, STATS_TABLES).await?;
    state_pool.close().await;
    let stats_pool = open_raw(old_db).await?;
    drop_tables(&stats_pool, STATE_TABLES).await?;
    stats_pool.close().await;

    // 4. 表集互斥断言（防漏删/多删）
    assert_table_set(state_db, STATE_TABLES).await?;
    assert_table_set(old_db, STATS_TABLES).await?;

    // 5. VACUUM + integrity_check
    for path in [old_db, state_db] {
        let pool = open_raw(path).await?;
        sqlx::query("VACUUM;")
            .execute(&pool)
            .await
            .map_err(|e| format!("vacuum: {e}"))?;
        let ok: i64 =
            sqlx::query_scalar("SELECT integrity_check FROM pragma_integrity_check LIMIT 1")
                .fetch_one(&pool)
                .await
                .map_err(|e| format!("integrity: {e}"))?;
        if ok != 1 {
            return Err(format!("integrity_check 未通过：{path:?}"));
        }
        pool.close().await;
    }

    println!(
        "[OK] 拆分完成。统计库 {old_db:?}（{} 表）| 领域库 {state_db:?}（{} 表）",
        STATS_TABLES.len(),
        STATE_TABLES.len()
    );
    println!("下一步：config.toml [stats] state_db_path = {state_db:?} 后重启（双库模式生效）");
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("用法: db_split <usage_stats.db> <state.db>");
        std::process::exit(2);
    }
    let (_bin, old, state) = (&args[0], &args[1], &args[2]);
    if let Err(e) = run_split(Path::new(old), Path::new(state)).await {
        eprintln!("[FAIL] {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    /// 全链路（不含 VACUUM/integrity 的温路径）：建单库（全部表）→ 按迁移逻辑拆分 →
    /// 断言两边表集互斥。
    #[tokio::test]
    async fn split_logic_produces_disjoint_sets() {
        let tmp = std::env::temp_dir();
        let old_db = tmp.join(format!("phi_split_old_{}.db", Uuid::new_v4()));
        let state_db = tmp.join(format!("phi_split_state_{}.db", Uuid::new_v4()));

        // 单文件模式建表（两池同文件 = 全部 18 表）
        let storage = impl_storage::storage::StatsStorage::connect_sqlite(
            old_db.to_string_lossy().as_ref(),
            false,
        )
        .await
        .expect("connect");
        storage.init_schema().await.expect("schema");

        // 迁移核心步骤（与 run_split 相同的删除逻辑）
        std::fs::copy(&old_db, &state_db).expect("copy");
        let state_pool = open_raw(&state_db).await.unwrap();
        drop_tables(&state_pool, STATS_TABLES).await.unwrap();
        state_pool.close().await;
        let stats_pool = open_raw(&old_db).await.unwrap();
        drop_tables(&stats_pool, STATE_TABLES).await.unwrap();
        stats_pool.close().await;

        assert_table_set(&state_db, STATE_TABLES)
            .await
            .expect("state 表集");
        assert_table_set(&old_db, STATS_TABLES)
            .await
            .expect("stats 表集");

        // 清理
        for p in [&old_db, &state_db] {
            let _ = std::fs::remove_file(p);
        }
    }
}
