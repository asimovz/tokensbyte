/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 客户模型折扣「预约生效」：把 `users.model_discounts` 的修改排队到站点时区自然日 0 点，
//! 让某一整天内客户只用一个折扣，日统计与成本口径不再出现「一天两段」。
//!
//! 口径 v2（11-19 定稿，同日二次收窄）：首次配置某模型折扣走原有即时保存；已有键的修改
//! 只能提交预约，**预约只有「指定日期 0 点生效」一种**（无立即生效分支，故不存在当天两段口径）。
//! 服务端拦截在 `api::users::update_user`。当天要纠错就把模型从「已选」里删掉再重设。

use crate::error::{AppError, AppResult};
use crate::AppState;
use axum::extract::{Extension, Path, State};
use axum::Json;
use chrono::{Datelike, NaiveDate, Timelike, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

const TASK_NAME: &str = "DiscountScheduleApply";

/// 折扣倍率允许区间，与前端「模型折扣」输入框（`max=2`）一致；0 视作无效值。
const DISCOUNT_MIN: f64 = f64::EPSILON;
const DISCOUNT_MAX: f64 = 2.0;

static LAST_APPLY_DATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[inline]
fn ymd_num(day: NaiveDate) -> u32 {
    (day.year() as u32) * 10000 + (day.month() as u32) * 100 + day.day() as u32
}

/// 解析 `users.model_discounts`（JSON，键 = `models.mid`）。
/// 解析失败按空集处理，与 `relay::proxy::parse_user_model_discount` 的静默降级保持一致。
pub fn parse_model_discounts(raw: &Option<String>) -> BTreeMap<String, f64> {
    raw.as_deref()
        .and_then(|s| serde_json::from_str::<BTreeMap<String, f64>>(s).ok())
        .unwrap_or_default()
}

/// 把 `updates` 覆盖进原 JSON，**未涉及的 mid 原样保留** —— 预约只改自己那几条键，
/// 绝不整表重写，否则会把别的模型（含 0 点前刚即时设置过的）冲掉。
pub fn merge_model_discounts(raw: &Option<String>, updates: &[(String, f64)]) -> String {
    let mut map = parse_model_discounts(raw);
    for (mid, discount) in updates {
        map.insert(mid.clone(), *discount);
    }
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
}

/// 站点时区的「今天」。预约的生效判定、0 点睡眠都以此为准，用 UTC 会和日统计的日界错开。
async fn site_today(state: &Arc<AppState>) -> (NaiveDate, chrono_tz::Tz) {
    let tz_name = crate::relay::relay_settings::get_cached_site_timezone(&state.db).await;
    let tz = crate::time_system::parse_timedisplay(&tz_name);
    (Utc::now().with_timezone(&tz).date_naive(), tz)
}

/// 在已开启的事务内把 `updates` 写入某个用户的 `model_discounts`。
/// 先对 `users` 行加锁再重读 JSON：同一用户被两个节点分别处理时也会串行，且不会丢更新。
/// 返回写入后的 JSON 文本；用户已不存在时返回 `None`（调用方跳过其预约）。
async fn write_discounts_in_tx(
    state: &Arc<AppState>,
    tx: &mut sqlx::PgConnection,
    user_id: &str,
    updates: &[(String, f64)],
) -> AppResult<Option<String>> {
    let lock_sql = state
        .db
        .format_query("SELECT model_discounts FROM users WHERE id = ? FOR UPDATE");
    let current: Option<Option<String>> = sqlx::query_scalar(&lock_sql)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
    if current.is_none() {
        return Ok(None);
    }
    let merged = merge_model_discounts(&current.flatten(), updates);
    let update_sql = state
        .db
        .format_query("UPDATE users SET model_discounts = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?");
    sqlx::query(&update_sql)
        .bind(&merged)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    Ok(Some(merged))
}

/// 把到期（`effective_date <= 站点时区今天`）的 pending 预约落到 `users.model_discounts`。
///
/// 三台生产节点都会跑这个函数：`FOR UPDATE SKIP LOCKED` 让各台抢到互不相交的预约行，
/// 同一用户的行改写又在 `users` 行锁上串行，因此不需要咨询锁。重复执行不会二次改写
/// （置 `applied` 时再带 `status = 'pending'` 判据）。
pub async fn apply_due_schedules(state: &Arc<AppState>) -> AppResult<usize> {
    let (today, tz) = site_today(state).await;
    let select_sql = state.db.format_query(
        "SELECT id, user_id, model_mid, discount FROM user_model_discount_schedules \
         WHERE status = 'pending' AND effective_date <= ? ORDER BY id FOR UPDATE SKIP LOCKED",
    );

    let mut tx = state.db.pool.begin().await?;
    let rows = sqlx::query(&select_sql)
        .bind(today)
        .fetch_all(&mut *tx)
        .await?;
    if rows.is_empty() {
        return Ok(0);
    }

    // 按用户分组：一个用户的多条到期预约合并成一次 JSON 改写，行锁只加一次
    let mut by_user: BTreeMap<String, (Vec<i64>, Vec<(String, f64)>)> = BTreeMap::new();
    for row in &rows {
        let id: i64 = row.get("id");
        let user_id: String = row.get("user_id");
        let model_mid: String = row.get("model_mid");
        let discount: f64 = row.get("discount");
        let entry = by_user
            .entry(user_id)
            .or_insert_with(|| (Vec::new(), Vec::new()));
        entry.0.push(id);
        entry.1.push((model_mid, discount));
    }

    let stamp = Utc::now().with_timezone(&tz).format("%Y-%m-%d %H:%M:%S").to_string();
    let mut applied = 0usize;
    for (user_id, (ids, updates)) in by_user {
        let merged = write_discounts_in_tx(state, &mut tx, &user_id, &updates).await?;
        if merged.is_none() {
            tracing::warn!(
                "[{}] 用户 {} 已不存在，跳过其 {} 条到期预约",
                TASK_NAME,
                user_id,
                ids.len()
            );
            continue;
        }
        for id in &ids {
            let mark_sql = state.db.format_query(
                "UPDATE user_model_discount_schedules SET status = 'applied', applied_at = ? \
                 WHERE id = ? AND status = 'pending'",
            );
            let done = sqlx::query(&mark_sql)
                .bind(&stamp)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            if done.rows_affected() > 0 {
                applied += 1;
            }
        }
    }

    tx.commit().await?;
    Ok(applied)
}

/// 睡到站点时区次日 00:00 再应用到期预约；首轮即启动补跑（覆盖 0 点发布/重启的窗口）。
pub async fn run_discount_schedule_loop(
    state: Arc<AppState>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        let (today, tz) = site_today(&state).await;
        match apply_due_schedules(&state).await {
            Ok(n) => {
                LAST_APPLY_DATE.store(ymd_num(today), Ordering::Relaxed);
                if n > 0 {
                    tracing::info!("[{}] 本次应用了 {} 条到期折扣预约（{}）", TASK_NAME, n, today);
                }
            }
            Err(e) => tracing::error!("[{}] 应用到期折扣预约失败: {:?}", TASK_NAME, e),
        }

        let local_now = Utc::now().with_timezone(&tz);
        // 0 点这一整点内还没成功跑过 → 5 分钟后重试；否则睡到下一个 0 点
        let need_retry =
            local_now.hour() == 0 && LAST_APPLY_DATE.load(Ordering::Relaxed) != ymd_num(today);
        let wait = if need_retry {
            std::time::Duration::from_secs(300)
        } else {
            let next_local = (local_now.date_naive() + chrono::Duration::days(1))
                .and_hms_opt(0, 0, 0)
                .map(|naive| naive.and_local_timezone(tz).single())
                .unwrap_or(None);
            match next_local {
                Some(next) => (next.with_timezone(&Utc) - Utc::now())
                    .to_std()
                    .unwrap_or(std::time::Duration::from_secs(60))
                    .max(std::time::Duration::from_secs(1)),
                None => std::time::Duration::from_secs(3600),
            }
        };

        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = shutdown_rx.changed() => {
                tracing::info!("[CronTask] {} 定时任务已优雅关闭退出", TASK_NAME);
                return;
            }
        }
    }
}

// ── 管理端接口 ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ScheduleItem {
    pub id: i64,
    pub model_mid: String,
    pub model_name: Option<String>,
    pub discount: f64,
    pub effective_date: String,
    pub status: String,
    pub created_by: Option<String>,
    pub created_at: String,
    pub applied_at: Option<String>,
}

#[derive(Serialize)]
pub struct ScheduleListResponse {
    pub data: Vec<ScheduleItem>,
    pub total: i64,
}

const LIST_SELECT: &str = "SELECT s.id, s.model_mid, s.discount, \
     to_char(s.effective_date, 'YYYY-MM-DD') AS effective_date, s.status, \
     s.created_by, s.created_at, s.applied_at, \
     (SELECT m.name FROM models m WHERE m.mid = s.model_mid LIMIT 1) AS model_name \
     FROM user_model_discount_schedules s WHERE s.user_id = ?";

pub async fn list_user_discount_schedules(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> AppResult<Json<ScheduleListResponse>> {
    let status = query
        .get("status")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // 过滤条件必须拼在 ORDER BY 之前，否则会生成非法 SQL
    let sql = state.db.format_query(&match status {
        Some(_) => format!("{} AND s.status = ? ORDER BY s.id DESC", LIST_SELECT),
        None => format!("{} ORDER BY s.id DESC", LIST_SELECT),
    });
    let mut q = sqlx::query(&sql);
    if let Some(ref st) = status {
        q = q.bind(&id).bind(st);
    } else {
        q = q.bind(&id);
    }
    let rows = q.fetch_all(&state.db.pool).await?;

    let data: Vec<ScheduleItem> = rows
        .iter()
        .map(|row| ScheduleItem {
            id: row.get("id"),
            model_mid: row.get("model_mid"),
            model_name: row.get("model_name"),
            discount: row.get("discount"),
            effective_date: row.get("effective_date"),
            status: row.get("status"),
            created_by: row.get("created_by"),
            created_at: row.get("created_at"),
            applied_at: row.get("applied_at"),
        })
        .collect();
    let total = data.len() as i64;
    Ok(Json(ScheduleListResponse { data, total }))
}

#[derive(Deserialize)]
pub struct CreateScheduleRequest {
    pub model_mid: String,
    pub discount: f64,
    /// `YYYY-MM-DD`，站点时区该自然日 0 点生效，必须晚于今天
    pub effective_date: Option<String>,
}

#[derive(Serialize)]
pub struct CreateScheduleResponse {
    pub id: i64,
    pub effective_date: String,
}

pub async fn create_discount_schedule(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(id): Path<String>,
    Json(request): Json<CreateScheduleRequest>,
) -> AppResult<Json<CreateScheduleResponse>> {
    let mid = request.model_mid.trim().to_string();
    if mid.is_empty() {
        return Err(AppError::BadRequest("请先选择模型".to_string()));
    }
    if !(request.discount > DISCOUNT_MIN && request.discount <= DISCOUNT_MAX) {
        return Err(AppError::BadRequest(
            "折扣倍率必须大于 0 且不超过 2".to_string(),
        ));
    }
    let (today, tz) = site_today(&state).await;
    // 台账时间一律用站点时区的本地串：列默认值 `now()::text` 跟着 PG 会话时区走
    // （122 是 UTC、生产是 Asia/Shanghai），显式写入才能保证三台与测试机口径一致。
    let now_stamp = Utc::now().with_timezone(&tz).format("%Y-%m-%d %H:%M:%S").to_string();
    let effective_date = {
        let raw = request
            .effective_date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::BadRequest("请选择预约生效日期".to_string()))?;
        let parsed = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .map_err(|_| AppError::BadRequest("生效日期格式应为 YYYY-MM-DD".to_string()))?;
        if parsed <= today {
            return Err(AppError::BadRequest(format!(
                "预约生效日必须晚于今天（{}）；今天要改折扣或改错了，请把该模型从「已选」里删掉后重新设置",
                today.format("%Y-%m-%d")
            )));
        }
        parsed
    };

    let exists: bool = sqlx::query_scalar(&state.db.format_query("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?)"))
        .bind(&id)
        .fetch_one(&state.db.pool)
        .await?;
    if !exists {
        return Err(AppError::NotFound("用户不存在".to_string()));
    }

    let insert_sql = state.db.format_query(
        "INSERT INTO user_model_discount_schedules \
         (user_id, model_mid, discount, effective_date, status, created_by, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    );
    let mut tx = state.db.pool.begin().await?;
    let inserted = sqlx::query(&insert_sql)
        .bind(&id)
        .bind(&mid)
        .bind(request.discount)
        .bind(effective_date)
        .bind("pending")
        .bind(&claims.username)
        .bind(&now_stamp)
        .fetch_optional(&mut *tx)
        .await;
    let schedule_id = match inserted {
        Ok(Some(row)) => row.get::<i64, _>("id"),
        Ok(None) => {
            return Err(AppError::BadRequest(
                "预约创建失败，请重试".to_string(),
            ))
        }
        Err(e) => {
            // 部分唯一索引 uq_umds_pending 命中 = 同用户同模型同日期已有待生效预约
            let duplicated = matches!(
                &e,
                sqlx::Error::Database(db) if db.code().as_deref() == Some("23505")
            );
            if duplicated {
                return Err(AppError::Conflict(format!(
                    "模型 {} 在 {} 已有一条待生效预约，请先取消它",
                    mid,
                    effective_date.format("%Y-%m-%d")
                )));
            }
            return Err(e.into());
        }
    };

    tx.commit().await?;
    tracing::info!(
        "[{}] {} 提交折扣预约: 用户 {} 模型 {} -> {}（{} 起生效）",
        TASK_NAME,
        claims.username,
        id,
        mid,
        request.discount,
        effective_date
    );
    Ok(Json(CreateScheduleResponse {
        id: schedule_id,
        effective_date: effective_date.format("%Y-%m-%d").to_string(),
    }))
}

#[derive(Serialize)]
pub struct CancelScheduleResponse {
    cancelled: bool,
}

/// 取消预约：置 `cancelled` 不物理删，台账要留痕。
pub async fn cancel_discount_schedule(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(schedule_id): Path<i64>,
) -> AppResult<Json<CancelScheduleResponse>> {
    let sql = state.db.format_query(
        "UPDATE user_model_discount_schedules SET status = 'cancelled' \
         WHERE id = ? AND status = 'pending'",
    );
    let done = sqlx::query(&sql)
        .bind(schedule_id)
        .execute(&state.db.pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::BadRequest(
            "只有待生效的预约可以取消".to_string(),
        ));
    }
    tracing::info!(
        "[{}] {} 取消折扣预约 #{}",
        TASK_NAME,
        claims.username,
        schedule_id
    );
    Ok(Json(CancelScheduleResponse { cancelled: true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(json: &str) -> Option<String> {
        Some(json.to_string())
    }

    #[test]
    fn merge_keeps_other_mids_untouched() {
        let old = raw(r#"{"101":0.8,"202":0.6}"#);
        let merged = merge_model_discounts(&old, &[("101".to_string(), 0.55)]);
        let map = parse_model_discounts(&Some(merged));
        assert_eq!(map.get("101"), Some(&0.55));
        assert_eq!(map.get("202"), Some(&0.6));
    }

    #[test]
    fn merge_creates_key_when_absent() {
        let merged = merge_model_discounts(&None, &[("777".to_string(), 0.9)]);
        assert_eq!(parse_model_discounts(&Some(merged)).get("777"), Some(&0.9));
    }

    #[test]
    fn dirty_json_degrades_to_empty() {
        assert!(parse_model_discounts(&raw("not json")).is_empty());
        assert!(parse_model_discounts(&None).is_empty());
    }

    /// 防护本体：合并写入不能把已有键冲掉。去掉 `merge_model_discounts` 里的
    /// 「先读旧表」那一步，本用例必须变红。
    #[test]
    fn merge_never_drops_existing_keys() {
        let old = raw(r#"{"101":0.8,"202":0.6,"303":0.7}"#);
        let merged = merge_model_discounts(&old, &[("101".to_string(), 0.5)]);
        let map = parse_model_discounts(&Some(merged));
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("303"), Some(&0.7));
    }

    #[test]
    fn ymd_num_orders_by_calendar_day() {
        let d1 = NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
        assert!(ymd_num(d1) < ymd_num(d2));
    }
}
