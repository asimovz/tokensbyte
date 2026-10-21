/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 用户级限制热路径读取：IP 白名单（支持 CIDR）与「用户×模型」RPM。
//! 两列都存在 users 表上，60s TTL 内存缓存；管理员改完由 update_user 调 `invalidate` 立即生效。

use crate::db::Database;
use dashmap::DashMap;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Default)]
pub struct UserLimits {
    /// 逗号分隔的单 IP 或 CIDR 网段；空 = 不限
    pub allowed_ips: String,
    /// 模型名 → RPM，`*` 为兜底；0 = 不限
    pub model_rpm: Vec<(String, i64)>,
}

impl UserLimits {
    /// 命中顺序：精确模型名 → `*` 兜底 → 未配置视为不限（0）
    pub fn rpm_for_model(&self, model: &str) -> i64 {
        let m = model.trim();
        let exact = self.model_rpm.iter().find(|(k, _)| k == m).map(|(_, v)| *v);
        match exact.or_else(|| self.model_rpm.iter().find(|(k, _)| k == "*").map(|(_, v)| *v)) {
            Some(rpm) => rpm,
            None => 0,
        }
    }
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

    pub fn clear(&self) {
        self.entries.clear();
    }
}

async fn load_user_limits(db: &Database, user_id: &str) -> UserLimits {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        &db.format_query("SELECT allowed_ips, model_rpm_limits FROM users WHERE id = ?"),
    )
    .bind(user_id)
    .fetch_optional(&db.pool)
    .await
    .unwrap_or(None);

    let (allowed_ips, rpm_json) = row.unwrap_or_default();
    UserLimits {
        allowed_ips: allowed_ips.unwrap_or_default(),
        model_rpm: rpm_json
            .as_deref()
            .and_then(|raw| parse_model_rpm_json(raw).ok())
            .unwrap_or_default(),
    }
}

/// 解析 `{"模型名": rpm}`。形状不合法时返回错误说明——写库前也用它校验，
/// 免得脏数据到了热路径只能静默丢弃。
pub fn parse_model_rpm_json(raw: &str) -> Result<Vec<(String, i64)>, String> {
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(raw)
        .map_err(|e| format!("模型 RPM 配置需为 JSON 对象（如 {{\"gpt-4o\": 60}}）：{}", e))?;

    let mut out: Vec<(String, i64)> = Vec::with_capacity(map.len());
    for (model, value) in map {
        let rpm = match value {
            serde_json::Value::Number(n) => n
                .as_i64()
                .ok_or_else(|| format!("模型 {} 的 RPM 需为整数", model))?,
            serde_json::Value::String(s) => s
                .trim()
                .parse::<i64>()
                .map_err(|_| format!("模型 {} 的 RPM 需为整数", model))?,
            serde_json::Value::Null => continue,
            _ => return Err(format!("模型 {} 的 RPM 需为整数", model)),
        };
        let model = model.trim().to_string();
        if model.is_empty() {
            return Err("模型名不能为空".to_string());
        }
        out.push((model, rpm.max(0)));
    }
    Ok(out)
}
