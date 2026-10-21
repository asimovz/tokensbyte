/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

use axum::{
    extract::{Path, State},
    Json,
};
use rand::Rng;
use sqlx::Row;
use std::sync::Arc;

use crate::error::AppResult;
use crate::models::{CreateModelRequest, Model, ModelListResponse, UpdateModelRequest};
use crate::AppState;

#[derive(Debug, serde::Deserialize)]
pub struct ModelQuery {
    pub provider_id: Option<i64>,
    pub api_provider_id: Option<i64>,
    pub type_id: Option<i64>,
    pub page_size: Option<i64>,
    /// `system` | `custom`；缺省或 `all` 为全部
    pub source: Option<String>,
}

fn model_list_where(query: &ModelQuery) -> String {
    let mut sql = String::from(" WHERE 1=1");
    if query.provider_id.is_some() {
        sql.push_str(" AND provider_id = ?");
    }
    if query.api_provider_id.is_some() {
        sql.push_str(" AND api_provider_id = ?");
    }
    if query.type_id.is_some() {
        sql.push_str(" AND type_id = ?");
    }
    sql.push_str(crate::db::preset_models::source_filter_sql(
        query.source.as_deref(),
    ));
    sql
}

/// 与 [`model_list_where`] 占位符顺序一致
fn model_filter_ids(query: &ModelQuery) -> impl Iterator<Item = i64> + '_ {
    [query.provider_id, query.api_provider_id, query.type_id]
        .into_iter()
        .flatten()
}

pub async fn list_models(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<ModelQuery>,
) -> AppResult<Json<ModelListResponse>> {
    let mut where_sql = model_list_where(&query);

    #[cfg(feature = "plugin_volcengine_enhance")]
    {
        let volc_active =
            crate::api::plugins::is_plugin_enabled(&state, "volcengine_enhance").await;
        if !volc_active {
            where_sql.push_str(" AND (mid IS NULL OR mid NOT IN ('vve-sd', 'vve-pf', 'vve-ft', 'vve-gt', 'vvs-er', 'vvs-ep', 'dbs-sr', 'dbs-fs'))");
        }
    }
    #[cfg(not(feature = "plugin_volcengine_enhance"))]
    {
        where_sql.push_str(" AND (mid IS NULL OR mid NOT IN ('vve-sd', 'vve-pf', 'vve-ft', 'vve-gt', 'vvs-er', 'vvs-ep', 'dbs-sr', 'dbs-fs'))");
    }

    // 忽略 ≤0；上限防止异常大 LIMIT
    let limit = query.page_size.filter(|&p| p > 0).map(|p| p.min(10_000));
    let mut list_sql = format!("SELECT * FROM models{where_sql} ORDER BY id DESC");
    if let Some(ps) = limit {
        list_sql.push_str(&format!(" LIMIT {ps}"));
    }

    let formatted_sql = state.db.format_query(&list_sql);
    let mut q = sqlx::query_as::<_, Model>(&formatted_sql);
    for id in model_filter_ids(&query) {
        q = q.bind(id);
    }
    let models = q.fetch_all(&state.db.pool).await?;

    // 无 LIMIT 时列表即全集，不必再跑 COUNT
    let total = if limit.is_some() {
        let count_sql = format!("SELECT COUNT(*) FROM models{where_sql}");
        let formatted_count_sql = state.db.format_query(&count_sql);
        let mut cq = sqlx::query_scalar::<_, i64>(&formatted_count_sql);
        for id in model_filter_ids(&query) {
            cq = cq.bind(id);
        }
        cq.fetch_one(&state.db.pool).await?
    } else {
        models.len() as i64
    };

    Ok(Json(ModelListResponse {
        data: models,
        total,
    }))
}

pub async fn create_model(
    State(state): State<Arc<AppState>>,
    Json(mut req): Json<CreateModelRequest>,
) -> AppResult<Json<Model>> {
    req.name = req.name.trim().to_string();
    req.model_id = req.model_id.trim().to_string();

    if req.name.is_empty() || req.model_id.is_empty() {
        return Err(crate::error::AppError::BadRequest(
            "名称和模型 ID 不能为空".to_string(),
        ));
    }

    // mid 是唯一标识，允许 name 和 model_id 重复（不同 mid 可对应相同 model_id，用于差异化计费）

    // 自动生成唯一 6 位 mid，固定以 "30" 开头
    let mid = loop {
        let candidate: String = {
            let n: u32 = rand::thread_rng().gen_range(300000..=309999);
            n.to_string()
        };
        let taken: Option<i64> =
            sqlx::query_scalar(&state.db.format_query("SELECT id FROM models WHERE mid = ?"))
                .bind(&candidate)
                .fetch_optional(&state.db.pool)
                .await?;
        if taken.is_none() {
            break candidate;
        }
    };

    let group_ratios = serde_json::to_string(&req.group_ratios.unwrap_or_default())
        .unwrap_or_else(|_| "{}".to_string());
    // 过滤掉不存在或已禁用的规则 ID，防止脏数据入库
    let forward_rule_ids: Option<String> = if let Some(ids) = req.forward_rule_ids {
        let mut valid: Vec<i64> = Vec::new();
        for rid in &ids {
            let exists: bool = sqlx::query_scalar(&state.db.format_query(
                "SELECT EXISTS(SELECT 1 FROM forward_rules WHERE id = ? AND is_active = 1)",
            ))
            .bind(rid)
            .fetch_one(&state.db.pool)
            .await
            .unwrap_or(false);
            if exists {
                valid.push(*rid);
            }
        }
        Some(serde_json::to_string(&valid).unwrap_or_else(|_| "[]".to_string()))
    } else {
        None
    };

    let pre_deduction = req.pre_deduction.unwrap_or(0.0);
    let site_discount = req.site_discount.unwrap_or(1.0);
    let site_discount_enabled = req.site_discount_enabled.unwrap_or(0);
    if site_discount_enabled == 1 && site_discount <= 0.0 {
        return Err(crate::error::AppError::BadRequest(
            "开启折扣限价时，折扣限价倍率必须大于 0".to_string(),
        ));
    }
    let global_discount = req.global_discount.unwrap_or(1.0);
    let global_discount_enabled = req.global_discount_enabled.unwrap_or(0);

    let original_id = req.original_id.unwrap_or_default();
    let model_id_alias = req.model_id_alias.unwrap_or_default();

    let is_active = req.is_active.unwrap_or(1);
    let enable_log_content = req.enable_log_content.unwrap_or(1);

    let new_id = sqlx::query(
        &state.db.format_query(r#"INSERT INTO models (mid, name, model_id, original_id, model_id_alias, provider_id, api_provider_id, type_id, group_ratios, forward_rule_ids, billing_rule_id, pre_deduction, site_discount, site_discount_enabled, global_discount, global_discount_enabled, is_active, enable_log_content, is_system, logo, remark, description, feature_attributes)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?)
           RETURNING id"#)
    )
    .bind(&mid)
    .bind(&req.name)
    .bind(&req.model_id)
    .bind(&original_id)
    .bind(&model_id_alias)
    .bind(req.provider_id)
    .bind(req.api_provider_id)
    .bind(req.type_id)
    .bind(&group_ratios)
    .bind(forward_rule_ids)
    .bind(req.billing_rule_id)
    .bind(pre_deduction)
    .bind(site_discount)
    .bind(site_discount_enabled)
    .bind(global_discount)
    .bind(global_discount_enabled)
    .bind(is_active)
    .bind(enable_log_content)
    .bind(&req.logo)
    .bind(&req.remark)
    .bind(&req.description)
    .bind(&req.feature_attributes)
    .fetch_one(&state.db.pool)
    .await?
    .get::<i64, _>("id");

    let model = sqlx::query_as(&state.db.format_query("SELECT * FROM models WHERE id = ?"))
        .bind(new_id)
        .fetch_one(&state.db.pool)
        .await?;

    crate::api::plugins::notify_marketplace_data_changed(&state).await;

    Ok(Json(model))
}

pub async fn update_model(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(mut req): Json<UpdateModelRequest>,
) -> AppResult<Json<Model>> {
    // Basic trimming if fields are provided
    if let Some(name) = &mut req.name {
        *name = name.trim().to_string();
        if name.is_empty() {
            return Err(crate::error::AppError::BadRequest(
                "名称不能为空".to_string(),
            ));
        }
    }
    if let Some(model_id) = &mut req.model_id {
        *model_id = model_id.trim().to_string();
        if model_id.is_empty() {
            return Err(crate::error::AppError::BadRequest(
                "模型 ID 不能为空".to_string(),
            ));
        }
    }

    // mid 是唯一标识，允许 name 和 model_id 重复（不同 mid 可对应相同 model_id，用于差异化计费）

    // 若当前模型 mid 为空，自动生成一个 30 开头的唯一 6 位 mid
    let current_mid: Option<String> =
        sqlx::query_scalar(&state.db.format_query("SELECT mid FROM models WHERE id = ?"))
            .bind(id)
            .fetch_optional(&state.db.pool)
            .await?;
    if current_mid.as_deref().unwrap_or("").is_empty() {
        let new_mid = loop {
            let candidate: String = {
                let n: u32 = rand::thread_rng().gen_range(300000..=309999);
                n.to_string()
            };
            let taken: Option<i64> = sqlx::query_scalar(
                &state
                    .db
                    .format_query("SELECT id FROM models WHERE mid = ? AND id != ?"),
            )
            .bind(&candidate)
            .bind(id)
            .fetch_optional(&state.db.pool)
            .await?;
            if taken.is_none() {
                break candidate;
            }
        };
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET mid = ? WHERE id = ?"),
        )
        .bind(&new_mid)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }

    if let Some(name) = &req.name {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET name = ? WHERE id = ?"),
        )
        .bind(name)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(model_id) = &req.model_id {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET model_id = ? WHERE id = ?"),
        )
        .bind(model_id)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(original_id) = &req.original_id {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET original_id = ? WHERE id = ?"),
        )
        .bind(original_id)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(model_id_alias) = &req.model_id_alias {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET model_id_alias = ? WHERE id = ?"),
        )
        .bind(model_id_alias)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(pid) = req.provider_id {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET provider_id = ? WHERE id = ?"),
        )
        .bind(pid)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(apid) = req.api_provider_id {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET api_provider_id = ? WHERE id = ?"),
        )
        .bind(apid)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(tid) = req.type_id {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET type_id = ? WHERE id = ?"),
        )
        .bind(tid)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(ref gr) = req.group_ratios {
        let gr_str = serde_json::to_string(gr).unwrap_or_else(|_| "{}".to_string());
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET group_ratios = ? WHERE id = ?"),
        )
        .bind(&gr_str)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(rule_id) = req.billing_rule_id {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET billing_rule_id = ? WHERE id = ?"),
        )
        .bind(rule_id)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(pd) = req.pre_deduction {
        // Here we could enforce billing_type == "tokens" logic, but since billing_type is now in billing_rules,
        // we'll rely on the frontend to send pre_deduction = 0.0, or we could fetch the rule and check.
        // For strictness, if the frontend implements it, this is fine. If not, we can check here.
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET pre_deduction = ? WHERE id = ?"),
        )
        .bind(pd)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(rules) = &req.forward_rule_ids {
        // 过滤掉不存在或已禁用的规则 ID，防止脏数据入库
        let mut valid_ids: Vec<i64> = Vec::new();
        for rid in rules {
            let exists: bool = sqlx::query_scalar(&state.db.format_query(
                "SELECT EXISTS(SELECT 1 FROM forward_rules WHERE id = ? AND is_active = 1)",
            ))
            .bind(rid)
            .fetch_one(&state.db.pool)
            .await
            .unwrap_or(false);
            if exists {
                valid_ids.push(*rid);
            }
        }
        let rules_str = serde_json::to_string(&valid_ids).unwrap_or_else(|_| "[]".to_string());
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET forward_rule_ids = ? WHERE id = ?"),
        )
        .bind(&rules_str)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(active) = req.is_active {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET is_active = ? WHERE id = ?"),
        )
        .bind(active)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(elc) = req.enable_log_content {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET enable_log_content = ? WHERE id = ?"),
        )
        .bind(elc)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(sd) = req.site_discount {
        if sd <= 0.0 {
            return Err(crate::error::AppError::BadRequest(
                "折扣限价倍率必须大于 0".to_string(),
            ));
        }
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET site_discount = ? WHERE id = ?"),
        )
        .bind(sd)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(sde) = req.site_discount_enabled {
        if sde == 1 {
            let current_sd: f64 = match req.site_discount {
                Some(v) => v,
                None => sqlx::query_scalar(
                    &state
                        .db
                        .format_query("SELECT site_discount FROM models WHERE id = ?"),
                )
                .bind(id)
                .fetch_optional(&state.db.pool)
                .await?
                .unwrap_or(1.0),
            };
            if current_sd <= 0.0 {
                return Err(crate::error::AppError::BadRequest(
                    "开启折扣限价时，折扣限价倍率必须大于 0".to_string(),
                ));
            }
        }
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET site_discount_enabled = ? WHERE id = ?"),
        )
        .bind(sde)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(gd) = req.global_discount {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET global_discount = ? WHERE id = ?"),
        )
        .bind(gd)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(gde) = req.global_discount_enabled {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET global_discount_enabled = ? WHERE id = ?"),
        )
        .bind(gde)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(ref logo) = req.logo {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET logo = ? WHERE id = ?"),
        )
        .bind(logo)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(ref remark) = req.remark {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET remark = ? WHERE id = ?"),
        )
        .bind(remark)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(ref description) = req.description {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET description = ? WHERE id = ?"),
        )
        .bind(description)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }
    if let Some(ref feature_attributes) = req.feature_attributes {
        sqlx::query(
            &state
                .db
                .format_query("UPDATE models SET feature_attributes = ? WHERE id = ?"),
        )
        .bind(feature_attributes)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    }

    sqlx::query(
        &state
            .db
            .format_query("UPDATE models SET updated_at = CURRENT_TIMESTAMP WHERE id = ?"),
    )
    .bind(id)
    .execute(&state.db.pool)
    .await?;

    let model = sqlx::query_as(&state.db.format_query("SELECT * FROM models WHERE id = ?"))
        .bind(id)
        .fetch_one(&state.db.pool)
        .await?;

    crate::api::plugins::notify_marketplace_data_changed(&state).await;

    Ok(Json(model))
}

pub async fn delete_model(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let is_system: Option<i32> = sqlx::query_scalar(
        &state
            .db
            .format_query("SELECT is_system FROM models WHERE id = ?"),
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?;
    if is_system.unwrap_or(0) == 1 {
        return Err(crate::error::AppError::BadRequest(
            "系统预设模型不可删除，如需停用请禁用该模型。".to_string(),
        ));
    }

    sqlx::query(&state.db.format_query("DELETE FROM models WHERE id = ?"))
        .bind(id)
        .execute(&state.db.pool)
        .await?;

    crate::api::plugins::notify_marketplace_data_changed(&state).await;

    Ok(Json(serde_json::json!({ "success": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(source: Option<&str>) -> ModelQuery {
        ModelQuery {
            provider_id: None,
            api_provider_id: None,
            type_id: None,
            page_size: None,
            source: source.map(str::to_string),
        }
    }

    #[test]
    fn list_where_filters_system_and_custom() {
        let system = model_list_where(&query(Some("system")));
        assert!(system.contains("is_system = 1"));
        let custom = model_list_where(&query(Some("custom")));
        assert!(custom.contains("is_system = 0"));
        let all = model_list_where(&query(Some("all")));
        assert!(!all.contains("is_system"));
        let empty = model_list_where(&query(None));
        assert!(!empty.contains("is_system"));
        let injected = model_list_where(&query(Some("1; drop table models")));
        assert!(!injected.contains("drop"));
        assert!(!injected.contains("is_system"));
    }

    #[test]
    fn list_where_keeps_id_filters() {
        let q = ModelQuery {
            provider_id: Some(3),
            api_provider_id: Some(4),
            type_id: Some(5),
            page_size: None,
            source: Some("system".into()),
        };
        let sql = model_list_where(&q);
        assert!(sql.contains("provider_id = ?"));
        assert!(sql.contains("api_provider_id = ?"));
        assert!(sql.contains("type_id = ?"));
        assert!(sql.contains("is_system = 1"));
    }
}

