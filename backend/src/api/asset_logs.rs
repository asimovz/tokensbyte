/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 素材请求日志查询（admin only）
//!
//! 展示 plugin_api_logs 中 source='upstream_relay_convert' 或 'api_proxy' 的记录，
//! 通过 JOIN 关联出用户昵称、用户等级、渠道名称（方案 A：不关联 channels，避免多行）。

use crate::auth;
use crate::error::{AppError, AppResult};
use crate::time_system::DbTs;
use crate::AppState;
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 列表行（轻量：不传 payload，减少传输量）
#[derive(Serialize, sqlx::FromRow)]
pub struct AssetLogRow {
    pub id: i64,
    pub user_id: String,
    pub user_display: Option<String>,
    pub level_name: Option<String>,
    pub channel_name: Option<String>,
    pub binding_name: Option<String>,
    pub api_endpoint: String,
    pub status_code: Option<i32>,
    pub source: String,
    pub created_at: DbTs,
}

/// 详情（含 payload）
#[derive(Serialize, sqlx::FromRow)]
pub struct AssetLogDetail {
    pub id: i64,
    pub user_id: String,
    pub user_display: Option<String>,
    pub level_name: Option<String>,
    pub channel_name: Option<String>,
    pub binding_name: Option<String>,
    pub api_endpoint: String,
    pub status_code: Option<i32>,
    pub source: String,
    pub request_payload: Option<String>,
    pub response_payload: Option<String>,
    pub created_at: DbTs,
}

/// 列表查询参数
#[derive(Deserialize)]
pub struct AssetLogQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    /// 用户 ID 或昵称关键词
    pub keyword: Option<String>,
    /// 用户等级名称
    pub level: Option<String>,
    /// 绑定 ID（对应 plugin_name = 'uar:N'）
    pub binding_id: Option<i64>,
    /// Action 类型
    pub action: Option<String>,
    /// 起始日期 "2026-10-01"
    pub date_from: Option<String>,
    /// 结束日期 "2026-10-31"
    pub date_to: Option<String>,
    /// 来源筛选: upstream_relay_convert / api_proxy / relay_convert / all（默认 all）
    pub source: Option<String>,
}

/// GET /api/v1/asset-logs —— 分页列表
pub async fn list_asset_logs(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<auth::Claims>,
    Query(q): Query<AssetLogQuery>,
) -> AppResult<Json<serde_json::Value>> {
    // 管理员鉴权
    let role: String = sqlx::query_scalar(&state.db.format_query("SELECT role FROM users WHERE id = ?"))
        .bind(&claims.sub)
        .fetch_optional(&state.db.pool)
        .await?
        .unwrap_or_default();
    if role != "admin" {
        return Err(AppError::Forbidden("需要管理员权限".to_string()));
    }

    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    // 动态拼接 WHERE
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();
    let mut idx = 1u32;

    // source 过滤
    let source_filter = q.source.as_deref().unwrap_or("all").to_string();
    if source_filter != "all" {
        conditions.push(format!("l.source = ${}", idx));
        params.push(source_filter.clone());
        idx += 1;
    } else {
        // 默认只查透传和插件（排除无意义的 source）
        conditions.push(format!("l.source IN (${}, ${}, ${})", idx, idx + 1, idx + 2));
        params.push("upstream_relay_convert".to_string());
        params.push("api_proxy".to_string());
        params.push("relay_convert".to_string());
        idx += 3;
    }

    // keyword（用户 ID / 昵称模糊）
    if let Some(ref kw) = q.keyword {
        if !kw.is_empty() {
            conditions.push(format!(
                "(l.user_id ILIKE ${p} OR EXISTS (SELECT 1 FROM users u2 WHERE u2.id = l.user_id AND (u2.nickname ILIKE ${p} OR u2.uid ILIKE ${p})))",
                p = idx
            ));
            params.push(format!("%{}%", kw));
            idx += 1;
        }
    }

    // level
    if let Some(ref lv) = q.level {
        if !lv.is_empty() {
            conditions.push(format!("ul.name = ${}", idx));
            params.push(lv.clone());
            idx += 1;
        }
    }

    // binding_id
    if let Some(bid) = q.binding_id {
        if bid > 0 {
            conditions.push(format!("b.id = ${}::BIGINT", idx));
            params.push(bid.to_string());
            idx += 1;
        }
    }

    // action
    if let Some(ref act) = q.action {
        if !act.is_empty() {
            conditions.push(format!("l.api_endpoint = ${}", idx));
            params.push(act.clone());
            idx += 1;
        }
    }

    // date_from / date_to（created_at 是 TEXT，格式 "YYYY-MM-DD HH:MM:SS+00"）
    if let Some(ref df) = q.date_from {
        if !df.is_empty() {
            conditions.push(format!("l.created_at >= ${}", idx));
            params.push(format!("{} 00:00:00", df));
            idx += 1;
        }
    }
    if let Some(ref dt) = q.date_to {
        if !dt.is_empty() {
            conditions.push(format!("l.created_at <= ${}", idx));
            params.push(format!("{} 23:59:59+999", dt));
            idx += 1;
        }
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // 基础 JOIN
    let join_clause = "LEFT JOIN users u ON u.id = l.user_id \
                       LEFT JOIN user_levels ul ON ul.group_key = u.user_group \
                       LEFT JOIN upstream_asset_bindings b \
                         ON b.id = CASE WHEN l.plugin_name LIKE 'uar:%' \
                              THEN SUBSTRING(l.plugin_name FROM 5)::BIGINT ELSE NULL END \
                       LEFT JOIN channel_configs cc ON cc.id = b.channel_config_id";

    // COUNT
    let count_sql = format!(
        "SELECT COUNT(*) FROM plugin_api_logs l {} {}",
        join_clause, where_clause
    );
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for p in &params {
        count_q = count_q.bind(p);
    }
    let total: i64 = count_q.fetch_one(&state.db.pool).await?;

    // DATA
    let data_sql = format!(
        "SELECT l.id, l.user_id, \
         CASE WHEN COALESCE(u.admin_remark,'') != '' THEN u.username || '(' || u.admin_remark || ')' ELSE u.username END AS user_display, \
         ul.name AS level_name, \
         cc.name AS channel_name, \
         b.name AS binding_name, \
         l.api_endpoint, l.status_code, l.source, \
         l.created_at \
         FROM plugin_api_logs l {} {} \
         ORDER BY l.id DESC LIMIT ${} OFFSET ${}",
        join_clause, where_clause, idx, idx + 1
    );
    let mut data_q = sqlx::query_as::<_, AssetLogRow>(&data_sql);
    for p in &params {
        data_q = data_q.bind(p);
    }
    data_q = data_q.bind(page_size).bind(offset);
    let rows: Vec<AssetLogRow> = data_q.fetch_all(&state.db.pool).await?;

    Ok(Json(serde_json::json!({
        "list": rows,
        "total": total,
        "page": page,
        "page_size": page_size,
    })))
}

/// GET /api/v1/asset-logs/{id} —— 单条详情（含 payload）
pub async fn get_asset_log_detail(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<auth::Claims>,
    Path(id): Path<i64>,
) -> AppResult<Json<AssetLogDetail>> {
    let role: String = sqlx::query_scalar(&state.db.format_query("SELECT role FROM users WHERE id = ?"))
        .bind(&claims.sub)
        .fetch_optional(&state.db.pool)
        .await?
        .unwrap_or_default();
    if role != "admin" {
        return Err(AppError::Forbidden("需要管理员权限".to_string()));
    }

    let sql = state.db.format_query(
        "SELECT l.id, l.user_id, \
         CASE WHEN COALESCE(u.admin_remark,'') != '' THEN u.username || '(' || u.admin_remark || ')' ELSE u.username END AS user_display, \
         ul.name AS level_name, \
         cc.name AS channel_name, \
         b.name AS binding_name, \
         l.api_endpoint, l.status_code, l.source, \
         l.request_payload, l.response_payload, \
         l.created_at \
         FROM plugin_api_logs l \
         LEFT JOIN users u ON u.id = l.user_id \
         LEFT JOIN user_levels ul ON ul.group_key = u.user_group \
         LEFT JOIN upstream_asset_bindings b \
           ON b.id = CASE WHEN l.plugin_name LIKE 'uar:%' \
                THEN SUBSTRING(l.plugin_name FROM 5)::BIGINT ELSE NULL END \
         LEFT JOIN channel_configs cc ON cc.id = b.channel_config_id \
         WHERE l.id = ?",
    );
    let row: Option<AssetLogDetail> = sqlx::query_as::<_, AssetLogDetail>(&sql)
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await?;

    row.map(Json).ok_or_else(|| AppError::NotFound("日志记录不存在".to_string()))
}
