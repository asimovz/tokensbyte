/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 上游素材绑定管理（upstream_asset_bindings CRUD + 连通性测试）
//!
//! 绑定 = 指定哪个上游渠道承担素材接口透传（凭证读 channel_configs，绑定只存指针与素材路径后缀）。
//! 供 /api?Action= 透传分支（ns=uar:N / 等级映射 / 默认绑定）与视频生成素材转换共用。
//!
//! 适用等级（asset_binding_levels）在本页配置：一个绑定可挂多个等级，一个等级只能属于一个绑定。
//! 唯一性除 DB 唯一索引外，入口再做一次占用校验，以便返回「被哪条绑定占用」的可读提示。

use crate::error::AppError;
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const BINDING_SELECT: &str = "SELECT b.id, b.name, b.channel_config_id, b.asset_base_path, \
     b.asset_api_profile, b.group_id, b.is_active, b.is_default, b.remark, \
     b.created_at::text AS created_at, b.updated_at::text AS updated_at, \
     c.name AS channel_name, c.base_url AS channel_base_url, c.status AS channel_status \
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
    /// 1 = 默认素材上游（等级未命中映射时兜底），全表至多一条
    pub is_default: i64,
    pub remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub channel_name: Option<String>,
    pub channel_base_url: Option<String>,
    pub channel_status: Option<i32>,
}

/// 等级映射行：list 时一次取全表在内存归组，避免每条绑定各查一次
#[derive(sqlx::FromRow)]
struct LevelMapRow {
    binding_id: i64,
    level_id: i64,
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
    /// 适用用户等级 ID 列表；缺省/空 = 不挂等级（仅靠默认标记或显式 ns 命中）
    #[serde(default)]
    pub level_ids: Option<Vec<i64>>,
    /// 1 = 设为默认素材上游，其他绑定的默认标记自动取消
    #[serde(default)]
    pub is_default: Option<i64>,
}

#[derive(Deserialize)]
pub struct UpdateBindingRequest {
    pub name: Option<String>,
    pub channel_config_id: Option<i64>,
    pub asset_base_path: Option<String>,
    pub is_active: Option<i64>,
    pub remark: Option<String>,
    pub asset_api_profile: Option<String>,
    /// None = 不改动等级映射；Some(vec) = 整体替换（空数组 = 清空）
    #[serde(default)]
    pub level_ids: Option<Vec<i64>>,
    /// None = 不改动；Some(1) = 设为默认并取消其他；Some(0) = 取消默认
    #[serde(default)]
    pub is_default: Option<i64>,
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

/// 等级 ID 归一：剔除非正数 + 去重排序。多选控件可能传入重复值，
/// 不归一会让后续「COUNT 是否等于入参个数」的存在性校验误判
fn normalize_level_ids(raw: Option<&[i64]>) -> Vec<i64> {
    let mut v: Vec<i64> = raw
        .unwrap_or_default()
        .iter()
        .copied()
        .filter(|id| *id > 0)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// 校验等级 ID 均真实存在：存入野 ID 不会报错，但路由时 JOIN 不到等级，
/// 表现为「配了却不生效」，比直接拒绝更难排查
async fn ensure_levels_exist(state: &AppState, ids: &[i64]) -> Result<(), AppError> {
    if ids.is_empty() {
        return Ok(());
    }
    let n: i64 = sqlx::query_scalar(
        &state
            .db
            .format_query("SELECT COUNT(*) FROM user_levels WHERE id = ANY(?)"),
    )
    .bind(ids.to_vec())
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| AppError::Internal(format!("查询用户等级失败: {e}")))?;
    if n != ids.len() as i64 {
        return Err(AppError::BadRequest(
            "部分用户等级不存在或已被删除，请刷新后重新选择".into(),
        ));
    }
    Ok(())
}

/// 校验等级未被其他绑定占用。`self_id` = 正在编辑的绑定，其自身已占用的等级需放行，
/// 否则编辑时不改等级也会撞自己的旧映射
async fn ensure_levels_free(
    state: &AppState,
    ids: &[i64],
    self_id: Option<i64>,
) -> Result<(), AppError> {
    if ids.is_empty() {
        return Ok(());
    }
    #[derive(sqlx::FromRow)]
    struct ConflictRow {
        level_id: i64,
        level_name: Option<String>,
        binding_id: i64,
        binding_name: Option<String>,
    }
    let conflicts: Vec<ConflictRow> = sqlx::query_as(&state.db.format_query(
        "SELECT l.level_id, ul.name AS level_name, l.binding_id, b.name AS binding_name \
         FROM asset_binding_levels l \
         LEFT JOIN user_levels ul ON ul.id = l.level_id \
         LEFT JOIN upstream_asset_bindings b ON b.id = l.binding_id \
         WHERE l.level_id = ANY(?) AND l.binding_id <> ? ORDER BY l.level_id",
    ))
    .bind(ids.to_vec())
    .bind(self_id.unwrap_or(0))
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| AppError::Internal(format!("查询等级占用情况失败: {e}")))?;
    if let Some(c) = conflicts.first() {
        // 借用后再兜底，避免从 &ConflictRow 里 move 出 Option<String>（E0507）
        let level_label = match &c.level_name {
            Some(n) => n.clone(),
            None => format!("#{}", c.level_id),
        };
        let binding_label = match &c.binding_name {
            Some(n) => n.clone(),
            None => format!("#{}", c.binding_id),
        };
        return Err(AppError::BadRequest(format!(
            "用户等级「{}」已绑定到「{}」，一个等级只能对应一个素材上游；如需改绑请先在原绑定上取消该等级",
            level_label, binding_label,
        )));
    }
    Ok(())
}

/// 整体重写绑定的等级映射（先删后插）。校验须在调用前完成，本函数只做落库
async fn rewrite_level_map(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    state: &AppState,
    binding_id: i64,
    level_ids: &[i64],
) -> Result<(), AppError> {
    sqlx::query(
        &state
            .db
            .format_query("DELETE FROM asset_binding_levels WHERE binding_id = ?"),
    )
    .bind(binding_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::Internal(format!("清理等级映射失败: {e}")))?;
    for lid in level_ids {
        sqlx::query(&state.db.format_query(
            "INSERT INTO asset_binding_levels (binding_id, level_id) VALUES (?, ?)",
        ))
        .bind(binding_id)
        .bind(lid)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::Internal(format!("写入等级映射失败: {e}")))?;
    }
    Ok(())
}

/// 写默认标记：置 1 时先清掉其他绑定的默认，保证全表至多一条默认上游
async fn apply_default_flag(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    state: &AppState,
    binding_id: i64,
    is_default: i64,
) -> Result<(), AppError> {
    if is_default == 1 {
        sqlx::query(&state.db.format_query(
            "UPDATE upstream_asset_bindings SET is_default = 0 WHERE is_default = 1 AND id <> ?",
        ))
        .bind(binding_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| AppError::Internal(format!("取消原默认绑定失败: {e}")))?;
    }
    sqlx::query(&state.db.format_query(
        "UPDATE upstream_asset_bindings SET is_default = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    ))
    .bind(is_default)
    .bind(binding_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| AppError::Internal(format!("更新默认标记失败: {e}")))?;
    Ok(())
}

pub async fn list_bindings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let rows: Vec<BindingRow> = sqlx::query_as(&state.db.format_query(&format!(
        "{} ORDER BY b.is_default DESC, b.id DESC",
        BINDING_SELECT
    )))
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| AppError::Internal(format!("查询素材绑定失败: {e}")))?;
    // 一次取全部等级映射后在内存归组；前端靠它算出「已被占用的等级」以从下拉里排除
    let maps: Vec<LevelMapRow> = sqlx::query_as(&state.db.format_query(
        "SELECT binding_id, level_id FROM asset_binding_levels ORDER BY level_id",
    ))
    .fetch_all(&state.db.pool)
    .await
    .unwrap_or_default();
    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let mut v = serde_json::to_value(r).unwrap_or_else(|_| serde_json::json!({}));
            let ids: Vec<i64> = maps
                .iter()
                .filter(|m| m.binding_id == r.id)
                .map(|m| m.level_id)
                .collect();
            v["level_ids"] = serde_json::json!(ids);
            v
        })
        .collect();
    Ok(Json(serde_json::json!({ "data": data })))
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
    let level_ids = normalize_level_ids(req.level_ids.as_deref());
    ensure_levels_exist(&state, &level_ids).await?;
    ensure_levels_free(&state, &level_ids, None).await?;
    let is_default = if req.is_default.unwrap_or(0) == 1 { 1 } else { 0 };

    // 绑定主记录 + 等级映射 + 默认标记必须同事务：否则可能落下「占了等级却没绑定」的孤儿映射
    let mut tx = state
        .db
        .pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("开启事务失败: {e}")))?;
    let id: i64 = sqlx::query_scalar(&state.db.format_query(
        "INSERT INTO upstream_asset_bindings (name, channel_config_id, asset_base_path, remark, asset_api_profile) \
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    ))
    .bind(&name)
    .bind(req.channel_config_id)
    .bind(req.asset_base_path.trim())
    .bind(req.remark.as_deref().unwrap_or_default())
    .bind(profile.as_deref().unwrap_or_default())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(format!("创建素材绑定失败: {e}")))?;
    apply_default_flag(&mut tx, &state, id, is_default).await?;
    rewrite_level_map(&mut tx, &state, id, &level_ids).await?;
    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("提交事务失败: {e}")))?;
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
    // None = 本次不动等级映射（如列表页只切换启用开关）；Some = 整体替换
    let level_ids = req
        .level_ids
        .as_deref()
        .map(|raw| normalize_level_ids(Some(raw)));
    if let Some(ids) = &level_ids {
        ensure_levels_exist(&state, ids).await?;
        ensure_levels_free(&state, ids, Some(id)).await?;
    }

    let mut tx = state
        .db
        .pool
        .begin()
        .await
        .map_err(|e| AppError::Internal(format!("开启事务失败: {e}")))?;
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
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::Internal(format!("更新素材绑定失败: {e}")))?;
    if res.rows_affected() == 0 {
        // tx 未 commit，drop 时自动回滚
        return Err(AppError::NotFound("素材绑定不存在".into()));
    }
    if let Some(ids) = &level_ids {
        rewrite_level_map(&mut tx, &state, id, ids).await?;
    }
    if let Some(d) = req.is_default {
        apply_default_flag(&mut tx, &state, id, if d == 1 { 1 } else { 0 }).await?;
    }
    tx.commit()
        .await
        .map_err(|e| AppError::Internal(format!("提交事务失败: {e}")))?;
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
