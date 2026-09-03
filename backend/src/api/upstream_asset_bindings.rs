/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 上游素材绑定管理（upstream_asset_bindings CRUD + 连通性测试）
//!
//! 绑定 = 指定哪个上游渠道承担素材接口透传（凭证读 channel_configs，绑定只存指针与素材路径后缀）。
//! 供 /api?Action= 透传分支（ns=uar:N / 分组默认路由）与视频生成素材转换共用。

use crate::error::AppError;
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const BINDING_SELECT: &str = "SELECT b.id, b.name, b.channel_config_id, b.asset_base_path, \
     b.asset_api_profile, b.group_id, b.is_active, b.remark, b.created_at::text AS created_at, \
     b.updated_at::text AS updated_at, c.name AS channel_name, c.base_url AS channel_base_url, \
     c.status AS channel_status \
     FROM upstream_asset_bindings b LEFT JOIN channel_configs c ON c.id = b.channel_config_id";

#[derive(sqlx::FromRow, Serialize)]
pub struct BindingRow {
    pub id: i64,
    pub name: String,
    pub channel_config_id: i64,
    pub asset_base_path: String,
    /// API 协议描述符 JSON（空 = 原火山透传行为），供前端编辑回显
    pub asset_api_profile: Option<String>,
    pub group_id: Option<String>,
    pub is_active: i64,
    pub remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub channel_name: Option<String>,
    pub channel_base_url: Option<String>,
    pub channel_status: Option<i32>,
}

#[derive(Deserialize)]
pub struct CreateBindingRequest {
    pub name: String,
    pub channel_config_id: i64,
    #[serde(default)]
    pub asset_base_path: String,
    #[serde(default)]
    pub remark: Option<String>,
    #[serde(default)]
    pub asset_api_profile: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateBindingRequest {
    pub name: Option<String>,
    pub channel_config_id: Option<i64>,
    pub asset_base_path: Option<String>,
    pub is_active: Option<i64>,
    pub remark: Option<String>,
    pub asset_api_profile: Option<String>,
}

/// 校验协议描述符：未传（None）= 不改动；空串 = 清除描述符（回落原火山透传）；
/// 非空必须能解析为合法 AssetApiProfile——描述符加载端对非法 JSON 是静默回落，
/// 若不在入口拦住，存坏后透传行为会悄然变成火山直连、极难排查
fn validate_profile(raw: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(s) = raw else { return Ok(None) };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(Some(String::new()));
    }
    if crate::relay::asset_api_profile::AssetApiProfile::parse(trimmed).is_none() {
        return Err(AppError::BadRequest(
            "API 协议描述符不是合法 JSON，或不符合描述符结构（顶层需为对象，actions 为动作映射）".into(),
        ));
    }
    Ok(Some(trimmed.to_string()))
}

async fn ensure_channel_config_exists(state: &AppState, id: i64) -> Result<(), AppError> {
    let exists: Option<i64> = sqlx::query_scalar(
        &state
            .db
            .format_query("SELECT id FROM channel_configs WHERE id = ?"),
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| AppError::Internal(format!("查询渠道配置失败: {e}")))?;
    if exists.is_none() {
        return Err(AppError::BadRequest("上游渠道不存在".into()));
    }
    Ok(())
}

pub async fn list_bindings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rows: Vec<BindingRow> = sqlx::query_as(&state.db.format_query(&format!(
        "{} ORDER BY b.id DESC",
        BINDING_SELECT
    )))
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| AppError::Internal(format!("查询素材绑定失败: {e}")))?;
    Ok(Json(serde_json::json!({ "data": rows })))
}

pub async fn create_binding(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateBindingRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::BadRequest("名称不能为空".into()));
    }
    if req.channel_config_id <= 0 {
        return Err(AppError::BadRequest("请选择上游渠道".into()));
    }
    ensure_channel_config_exists(&state, req.channel_config_id).await?;
    let profile = validate_profile(req.asset_api_profile.as_deref())?;
    let id: i64 = sqlx::query_scalar(&state.db.format_query(
        "INSERT INTO upstream_asset_bindings (name, channel_config_id, asset_base_path, remark, asset_api_profile) \
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    ))
    .bind(&name)
    .bind(req.channel_config_id)
    .bind(req.asset_base_path.trim())
    .bind(req.remark.as_deref().unwrap_or_default())
    .bind(profile.as_deref().unwrap_or_default())
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| AppError::Internal(format!("创建素材绑定失败: {e}")))?;
    Ok(Json(serde_json::json!({ "success": true, "id": id })))
}

pub async fn update_binding(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateBindingRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if let Some(name) = &req.name {
        if name.trim().is_empty() {
            return Err(AppError::BadRequest("名称不能为空".into()));
        }
    }
    if let Some(cid) = req.channel_config_id {
        if cid <= 0 {
            return Err(AppError::BadRequest("请选择上游渠道".into()));
        }
        ensure_channel_config_exists(&state, cid).await?;
    }
    let profile = validate_profile(req.asset_api_profile.as_deref())?;
    let res = sqlx::query(&state.db.format_query(
        "UPDATE upstream_asset_bindings SET \
         name = COALESCE(?, name), \
         channel_config_id = COALESCE(?, channel_config_id), \
         asset_base_path = COALESCE(?, asset_base_path), \
         is_active = COALESCE(?, is_active), \
         remark = COALESCE(?, remark), \
         asset_api_profile = COALESCE(?, asset_api_profile), \
         updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    ))
    .bind(req.name.as_deref().map(str::trim))
    .bind(req.channel_config_id)
    .bind(req.asset_base_path.as_deref().map(str::trim))
    .bind(req.is_active)
    .bind(req.remark.as_deref())
    .bind(profile)
    .bind(id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| AppError::Internal(format!("更新素材绑定失败: {e}")))?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("素材绑定不存在".into()));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn delete_binding(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let res = sqlx::query(
        &state
            .db
            .format_query("DELETE FROM upstream_asset_bindings WHERE id = ?"),
    )
    .bind(id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| AppError::Internal(format!("删除素材绑定失败: {e}")))?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound("素材绑定不存在".into()));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

/// 连通性测试：以绑定的渠道凭证对上游发起轻量 ListAssetGroups
pub async fn test_binding(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    use crate::services::upstream_asset_client as uac;
    let creds = uac::load_binding_endpoints(&state.db, &[id]).await;
    let Some((endpoint, api_key)) = creds.get(&id).cloned() else {
        return Err(AppError::BadRequest(
            "绑定不存在，或渠道已停用/凭证缺失".into(),
        ));
    };
    let ctx = uac::UpstreamCallCtx {
        http: &state.http_client,
        db: &state.db,
        user_id: "admin",
        plugin_name: uac::PLUGIN_NAME,
        endpoint_base: &endpoint,
        api_key: &api_key,
    };
    let start = std::time::Instant::now();
    match uac::call_action_logged(&ctx, "ListAssetGroups", &serde_json::json!({})).await {
        Ok(res) => Ok(Json(serde_json::json!({
            "ok": true,
            "latency_ms": start.elapsed().as_millis() as u64,
            "response": res,
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "ok": false,
            "latency_ms": start.elapsed().as_millis() as u64,
            "error": e.to_string(),
        }))),
    }
}
