/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 用户级限制热路径读取：IP 白名单（支持 CIDR）。
//! 列存在 users 表上，60s TTL 内存缓存；管理员改完由 update_user 调 `invalidate` 立即生效。

use crate::db::Database;
use dashmap::DashMap;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Default)]
pub struct UserLimits {
    /// 逗号分隔的单 IP 或 CIDR 网段；空 = 不限
    pub allowed_ips: String,
}

pub struct UserLimitsCache {
    entries: DashMap<String, (UserLimits, Instant)>,
}

impl UserLimitsCache {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    pub async fn get(&self, db: &Database, user_id: &str) -> UserLimits {
        if let Some(entry) = self.entries.get(user_id) {
            let (limits, at) = entry.value();
            if Instant::now().duration_since(*at) < CACHE_TTL {
                return limits.clone();
            }
        }
        let limits = load_user_limits(db, user_id).await;
        self.entries
            .insert(user_id.to_string(), (limits.clone(), Instant::now()));
        limits
    }

    pub fn invalidate(&self, user_id: &str) {
        self.entries.remove(user_id);
    }
}

async fn load_user_limits(db: &Database, user_id: &str) -> UserLimits {
    let row: Option<Option<String>> = sqlx::query_scalar(
        &db.format_query("SELECT allowed_ips FROM users WHERE id = ?"),
    )
    .bind(user_id)
    .fetch_optional(&db.pool)
    .await
    .unwrap_or(None);

    UserLimits {
        allowed_ips: row.flatten().unwrap_or_default(),
    }
}
