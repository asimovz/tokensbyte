/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

use crate::error::{AppError, AppResult};
use crate::models::{
    Channel, ChannelListResponse, ChannelSafe, CreateChannelRequest, UpdateChannelRequest,
};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use std::sync::Arc;

pub async fn list_channels(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::auth::Claims>>,
) -> AppResult<Json<ChannelListResponse>> {
    let is_admin = claims.as_ref().map_or(false, |c| c.0.role == "admin");

    let channels: Vec<Channel> = sqlx::query_as(&state.db.format_query(
        "SELECT * FROM channels ORDER BY status DESC, sort_order DESC, priority DESC, id DESC",
    ))
    .fetch_all(&state.db.pool)
    .await?;

    let safe_channels: Vec<ChannelSafe> = channels
        .into_iter()
        .map(|ch| ChannelSafe::from_with_role(ch, is_admin))
        .collect();
    let total = safe_channels.len() as i64;

    Ok(Json(ChannelListResponse {
        data: safe_channels,
        total,
    }))
}

pub async fn create_channel(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateChannelRequest>,
) -> AppResult<Json<ChannelSafe>> {
    let models_json = serde_json::to_string(&request.models).unwrap_or_else(|_| "[]".to_string());
    let mapping_json = serde_json::to_string(&request.model_mapping.unwrap_or_default())
        .unwrap_or_else(|_| "{}".to_string());
    let config_json = serde_json::to_string(&request.config.unwrap_or_default())
        .unwrap_or_else(|_| "{}".to_string());
    let groups_json = serde_json::to_string(&request.user_groups.unwrap_or_default())
        .unwrap_or_else(|_| "[]".to_string());
    let exclude_groups_json =
        serde_json::to_string(&request.exclude_user_groups.unwrap_or_default())
            .unwrap_or_else(|_| "[]".to_string());

    // Auto-generate unique 4-digit group_aid
    let mut group_aid_val = String::new();
    use rand::Rng;
    for _ in 0..10 {
        let aid: u32 = rand::thread_rng().gen_range(1000..10000);
        let aid_str = aid.to_string();
        let exists: i64 = sqlx::query_scalar(
            &state
                .db
                .format_query("SELECT COUNT(*) FROM channels WHERE group_aid = ?"),
        )
        .bind(&aid_str)
        .fetch_one(&state.db.pool)
        .await?;
        if exists == 0 {
            group_aid_val = aid_str;
            break;
        }
    }
    if group_aid_val.is_empty() {
        group_aid_val = rand::thread_rng().gen_range(1000..10000).to_string(); // fallback
    }

    // 校验 A：带素材绑定的分组不允许让同一等级被两条不同绑定覆盖（详见 relay::asset_route）。
    // 只在灰度开关打开后生效 —— 开关关闭时保存行为与改造前完全一致。
    if crate::relay::asset_route::by_group_enabled(&state.db).await {
        let included: Vec<String> = serde_json::from_str(&groups_json).unwrap_or_default();
        let excluded: Vec<String> = serde_json::from_str(&exclude_groups_json).unwrap_or_default();
        let problems = crate::relay::asset_route::check_group_invariant(
            &state.db,
            None,
            request.preset_id,
            &included,
            &excluded,
        )
        .await?;
        if let Some(first) = problems.into_iter().next() {
            return Err(AppError::BadRequest(format!("素材上游分组冲突：{first}")));
        }
    }

    let sql = r#"INSERT INTO channels (name, provider_type, base_url, api_key, models, model_mapping, user_groups, exclude_user_groups, group_aid, preset_id, category_id, sort_order, priority, weight, status, max_rps, quota_limit, quota_used, daily_quota_limit, weekly_quota_limit, monthly_quota_limit, config, rate)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id"#;

    let id: i64 = sqlx::query_scalar::<_, i64>(&state.db.format_query(sql))
        .bind(&request.name)
        .bind(&request.provider_type)
        .bind(&request.base_url)
        .bind(&request.api_key)
        .bind(&models_json)
        .bind(&mapping_json)
        .bind(&groups_json)
        .bind(&exclude_groups_json)
        .bind(&group_aid_val)
        .bind(request.preset_id)
        .bind(request.category_id)
        .bind(request.sort_order.unwrap_or(0))
        .bind(request.priority.unwrap_or(0))
        .bind(request.weight.unwrap_or(1))
        .bind(request.max_rps.unwrap_or(0))
        .bind(request.quota_limit.unwrap_or(-1.0))
        .bind(request.quota_used.unwrap_or(0.0))
        .bind(request.daily_quota_limit.unwrap_or(-1.0))
        .bind(request.weekly_quota_limit.unwrap_or(-1.0))
        .bind(request.monthly_quota_limit.unwrap_or(-1.0))
        .bind(&config_json)
        .bind(request.rate)
        .fetch_one(&state.db.pool)
        .await?;

    let channel: Channel =
        sqlx::query_as(&state.db.format_query("SELECT * FROM channels WHERE id = ?"))
            .bind(id)
            .fetch_one(&state.db.pool)
            .await?;

    crate::api::plugins::notify_marketplace_data_changed(&state).await;

    Ok(Json(ChannelSafe::from_with_role(channel, true)))
}

pub async fn update_channel(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(request): Json<UpdateChannelRequest>,
) -> AppResult<Json<ChannelSafe>> {
    // Current channel for partial updates
    let mut channel: Channel =
        sqlx::query_as(&state.db.format_query("SELECT * FROM channels WHERE id = ?"))
            .bind(id)
            .fetch_one(&state.db.pool)
            .await?;

    if let Some(name) = request.name {
        channel.name = name;
    }
    if let Some(provider_type) = request.provider_type {
        channel.provider_type = provider_type;
    }
    if let Some(base_url) = request.base_url {
        channel.base_url = base_url;
    }
    if let Some(api_key) = request.api_key {
        // 【防护】空值或含脱敏标记的值不覆盖原始密钥
        if !api_key.is_empty() && !api_key.contains("******") {
            channel.api_key = api_key;
        }
    }
    if let Some(models) = request.models {
        channel.models = serde_json::to_string(&models).unwrap_or_else(|_| "[]".to_string());
    }
    if let Some(mapping) = request.model_mapping {
        channel.model_mapping =
            serde_json::to_string(&mapping).unwrap_or_else(|_| "{}".to_string());
    }
    if let Some(user_groups) = request.user_groups {
        channel.user_groups =
            serde_json::to_string(&user_groups).unwrap_or_else(|_| "[]".to_string());
    }
    if let Some(exclude_user_groups) = request.exclude_user_groups {
        channel.exclude_user_groups =
            serde_json::to_string(&exclude_user_groups).unwrap_or_else(|_| "[]".to_string());
    }
    if let Some(group_aid) = request.group_aid {
        channel.group_aid = Some(group_aid);
    }
    if let Some(sort_order) = request.sort_order {
        channel.sort_order = sort_order;
    }
    if let Some(priority) = request.priority {
        channel.priority = priority;
    }
    if let Some(weight) = request.weight {
        channel.weight = weight;
    }
    if let Some(status) = request.status {
        channel.status = status;
    }
    if let Some(max_rps) = request.max_rps {
        channel.max_rps = Some(max_rps);
    }
    if let Some(quota_limit) = request.quota_limit {
        channel.quota_limit = quota_limit;
    }
    if let Some(quota_used) = request.quota_used {
        channel.quota_used = quota_used;
    }
    if let Some(daily) = request.daily_quota_limit {
        channel.daily_quota_limit = daily;
    }
    if let Some(weekly) = request.weekly_quota_limit {
        channel.weekly_quota_limit = weekly;
    }
    if let Some(monthly) = request.monthly_quota_limit {
        channel.monthly_quota_limit = monthly;
    }
    if let Some(config) = request.config {
        channel.config = serde_json::to_string(&config).unwrap_or_else(|_| "{}".to_string());
    }
    if let Some(rate) = request.rate {
        channel.rate = rate;
    }
    if let Some(category_id) = request.category_id {
        channel.category_id = category_id;
    }

    if let Some(preset_id) = request.preset_id {
        channel.preset_id = Some(preset_id);
    }

    let mut group_aid_val = channel.group_aid.clone().unwrap_or_default();
    if group_aid_val.is_empty() {
        use rand::Rng;
        for _ in 0..10 {
            let aid: u32 = rand::thread_rng().gen_range(1000..10000);
            let aid_str = aid.to_string();
            let exists: i64 = sqlx::query_scalar(
                &state
                    .db
                    .format_query("SELECT COUNT(*) FROM channels WHERE group_aid = ?"),
            )
            .bind(&aid_str)
            .fetch_one(&state.db.pool)
            .await?;
            if exists == 0 {
                group_aid_val = aid_str;
                break;
            }
        }
        if group_aid_val.is_empty() {
            group_aid_val = rand::thread_rng().gen_range(1000..10000).to_string();
            // fallback
        }
    }

    // 校验 A：同 create_channel。禁用的分组不进候选集，故跳过校验。
    if channel.status == 1 && crate::relay::asset_route::by_group_enabled(&state.db).await {
        let problems = crate::relay::asset_route::check_group_invariant(
            &state.db,
            Some(channel.id),
            channel.preset_id,
            &channel.get_user_groups(),
            &channel.get_exclude_user_groups(),
        )
        .await?;
        if let Some(first) = problems.into_iter().next() {
            return Err(AppError::BadRequest(format!("素材上游分组冲突：{first}")));
        }
    }

    // 诊断日志：记录关键字段变更，帮助排查上游切换后持续报错的问题
    {
        let old_ch: Option<(Option<i64>, String)> = sqlx::query_as(
            &state
                .db
                .format_query("SELECT preset_id, base_url FROM channels WHERE id = ?"),
        )
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await
        .ok()
        .flatten();
        let (old_preset, old_base) = old_ch.unwrap_or((None, String::new()));
        let sibling_count: i64 = sqlx::query_scalar(
            &state
                .db
                .format_query("SELECT COUNT(*) FROM channels WHERE group_aid = ? AND id != ?"),
        )
        .bind(&group_aid_val)
        .bind(id)
        .fetch_one(&state.db.pool)
        .await
        .unwrap_or(0);
        tracing::info!(
            "[Channel Update Debug] id={}, group_aid={}, preset_id: {:?} -> {:?}, base_url: '{}' -> '{}', 同组其他渠道数: {}",
            id, group_aid_val, old_preset, channel.preset_id, old_base, channel.base_url, sibling_count
        );
        if sibling_count > 0 {
            tracing::warn!(
                "[Channel Update Debug] ⚠️ 渠道 {} 所在分组 group_aid={} 存在 {} 条其他渠道记录，修改上游不会同步到它们！",
                id, group_aid_val, sibling_count
            );
        }
    }

    sqlx::query(
        &state.db.format_query(r#"UPDATE channels SET name = ?, provider_type = ?, base_url = ?, api_key = ?, models = ?, 
           model_mapping = ?, user_groups = ?, exclude_user_groups = ?, preset_id = ?, category_id = ?, sort_order = ?, priority = ?, weight = ?, status = ?, max_rps = ?, quota_limit = ?, quota_used = ?, daily_quota_limit = ?, weekly_quota_limit = ?, monthly_quota_limit = ?, config = ?, group_aid = ?, rate = ?, updated_at = CURRENT_TIMESTAMP
           WHERE id = ?"#)
    )
    .bind(&channel.name)
    .bind(&channel.provider_type)
    .bind(&channel.base_url)
    .bind(&channel.api_key)
    .bind(&channel.models)
    .bind(&channel.model_mapping)
    .bind(&channel.user_groups)
    .bind(&channel.exclude_user_groups)
    .bind(channel.preset_id)
    .bind(channel.category_id)
    .bind(channel.sort_order)
    .bind(channel.priority)
    .bind(channel.weight)
    .bind(channel.status)
    .bind(channel.max_rps)
    .bind(channel.quota_limit)
    .bind(channel.quota_used)
    .bind(channel.daily_quota_limit)
    .bind(channel.weekly_quota_limit)
    .bind(channel.monthly_quota_limit)
    .bind(&channel.config)
    .bind(&group_aid_val)
    .bind(channel.rate)
    .bind(id)
    .execute(&state.db.pool)
    .await?;

    if !group_aid_val.is_empty() {
        sqlx::query(&state.db.format_query(
            "UPDATE channels SET sort_order = ?, category_id = ? WHERE group_aid = ? AND id != ?",
        ))
        .bind(channel.sort_order)
        .bind(channel.category_id)
        .bind(&group_aid_val)
        .bind(id)
        .execute(&state.db.pool)
        .await
        .ok();
    }

    // 渠道配置变更后清除熔断记录，防止旧熔断阻止修正后的渠道被选中
    if !group_aid_val.is_empty() {
        state.failed_channels.remove(&group_aid_val);
        // 同时清除 HA 子渠道的熔断记录（前缀匹配 ha_group_{channel_id}_）
        let ha_prefix = format!("ha_group_{}_", id);
        state
            .failed_channels
            .retain(|k, _| !k.starts_with(&ha_prefix));
        tracing::info!(
            "[Channel Update] 渠道 {} (group_aid={}) 配置已更新，已清除关联的熔断记录",
            id,
            group_aid_val
        );
    }

    crate::api::plugins::notify_marketplace_data_changed(&state).await;

    Ok(Json(ChannelSafe::from_with_role(channel, true)))
}

pub async fn delete_channel(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    sqlx::query(&state.db.format_query("DELETE FROM channels WHERE id = ?"))
        .bind(id)
        .execute(&state.db.pool)
        .await?;

    crate::api::plugins::notify_marketplace_data_changed(&state).await;

    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn test_channel(
    State(state): State<Arc<AppState>>,
    options_claims: Option<axum::Extension<crate::auth::Claims>>,
    Path(id): Path<i64>,
    Json(req): Json<crate::models::TestChannelRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let mut channel = crate::relay::router::fetch_channel(&state, id, None)
        .await
        .ok_or_else(|| crate::error::AppError::NotFound("渠道不存在".to_string()))?;

    let mut channel_config_id: Option<i64> = None;

    if let Some(sub_id) = req.sub_channel_id {
        if channel.provider_type == "high_availability_group" {
            let sub_config: Option<crate::models::ChannelConfig> = sqlx::query_as(
                &state
                    .db
                    .format_query("SELECT * FROM channel_configs WHERE id = ?"),
            )
            .bind(sub_id)
            .fetch_optional(&state.db.pool)
            .await
            .unwrap_or(None);

            if let Some(cfg) = sub_config {
                // 测试指定子上游时校验其额度（站点时区，与线上扣费一致）
                let site_tz = crate::relay::relay_settings::get_cached_site_timezone(&state.db).await;
                let (_, now_week, now_month) = crate::models::quota_period_keys(&site_tz);
                if cfg.status != 1 {
                    return Err(crate::error::AppError::Forbidden(
                        "指定的上游渠道配置已禁用，测试请求被拦截".into(),
                    ));
                }
                if !cfg.has_available_quota(&site_tz, &now_week, &now_month) {
                    return Err(crate::error::AppError::Forbidden(
                        "指定的上游预设额度已耗尽，测试请求被拦截".into(),
                    ));
                }
                crate::relay::router::apply_ha_sub_mapped(&mut channel, &cfg);
                crate::relay::router::apply_volcengine_credential(&state, &mut channel).await;
                crate::relay::router::apply_comfyui_channel(&state, &mut channel).await;
                channel_config_id = Some(sub_id);
            } else {
                return Err(crate::error::AppError::NotFound(
                    "指定的高可用子上游渠道不存在".into(),
                ));
            }
        }
    }

    let tz_name = crate::relay::relay_settings::get_cached_site_timezone(&state.db).await;
    let (now_day, now_week, now_month) = crate::models::quota_period_keys(&tz_name);
    if let Err(msg) = channel.check_quota_limits(&now_day, &now_week, &now_month) {
        return Err(crate::error::AppError::Forbidden(format!(
            "该渠道{}，测试请求被拦截",
            msg
        )));
    }

    // 绑定上游预设时同步校验预设状态与额度
    if let Some(pid) = channel.preset_id {
        if let Ok(Some(preset)) = sqlx::query_as::<_, crate::models::ChannelConfig>(
            &state
                .db
                .format_query("SELECT * FROM channel_configs WHERE id = ?"),
        )
        .bind(pid)
        .fetch_optional(&state.db.pool)
        .await
        {
            if preset.status != 1 {
                return Err(crate::error::AppError::Forbidden(
                    "绑定的上游渠道配置已禁用，测试请求被拦截".into(),
                ));
            }
            if !preset.has_available_quota(&tz_name, &now_week, &now_month) {
                return Err(crate::error::AppError::Forbidden(
                    "绑定的上游预设额度已耗尽，测试请求被拦截".into(),
                ));
            }
        }
    }

    // preset / volc 已由 fetch_channel（及上方 HA 子配）水合，勿再查空父行覆盖
    let test_model = if let Some(m) = req.model {
        m
    } else {
        let model_mids = channel.get_models();
        if !model_mids.is_empty() {
            // models 存储的是 mid，需要反查 model_id
            let mid = &model_mids[0];
            let model_id: Option<String> = sqlx::query_scalar(
                &state
                    .db
                    .format_query("SELECT model_id FROM models WHERE mid = ?"),
            )
            .bind(mid)
            .fetch_optional(&state.db.pool)
            .await
            .unwrap_or(None);
            model_id.unwrap_or_else(|| mid.clone())
        } else {
            "gpt-3.5-turbo".to_string()
        }
    };

    // ── 获取该测试模型关联的计费规则（复用 relay 模块已有的模型查找逻辑） ──
    let db_model =
        crate::relay::proxy::find_active_model_exact(&state, &test_model, None, Some(&channel))
            .await;

    // 计算映射后的上游模型 ID（渠道测试无分辨率上下文）
    let (resolved_model, _) =
        crate::relay::router::resolve_model(&channel, &test_model, db_model.as_ref(), None);

    let start = std::time::Instant::now();

    // ── 获取该测试模型关联的计费规则（请求开始锁定峰谷倍率） ──
    let db_rule = crate::relay::proxy::get_model_billing_rule(
        &state,
        &test_model,
        Some(&channel),
        db_model.as_ref(),
    )
    .await;

    // ── 解析转发规则 ──
    let mut resolved: Option<crate::relay::forward::ResolvedForward> = None;
    let mut category = db_model
        .as_ref()
        .and_then(|m| m.type_name.clone())
        .unwrap_or_else(|| "聊天".to_string());
    let mut rule_is_stream = false;
    let mut db_forward_rule: Option<crate::models::ForwardRule> = None;

    if let Some(fid) = req.forward_rule_id {
        let rule: Option<crate::models::ForwardRule> = sqlx::query_as(
            &state
                .db
                .format_query("SELECT * FROM forward_rules WHERE id = ?"),
        )
        .bind(fid)
        .fetch_optional(&state.db.pool)
        .await?;
        if let Some(r) = rule {
            category = r.category.clone();
            if r.name.contains("流式") || r.name.to_lowercase().contains("stream") {
                rule_is_stream = true;
            }
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&r.config_json) {
                // 统一通过 parse_forward_config 解析，与 resolve_forward_rule 逻辑完全一致
                // 新增字段只需修改 parse_forward_config，此处无需变动
                let category_path = crate::relay::proxy::category_endpoint(Some(&category));
                let mid = db_model.as_ref().map(|m| m.mid.clone());
                let mut res = crate::relay::forward::parse_forward_config(
                    &config,
                    &category_path,
                    &r.eid,
                    mid,
                );
                crate::relay::forward::resolve_volcengine_media_enhance_path(
                    &mut res,
                    &resolved_model,
                );
                resolved = Some(res);
            }
            db_forward_rule = Some(r);
        }
    } else {
        // 如果没有指定 forward_rule_id，自动根据模型关联规则解析转发路径
        let entry_path = crate::relay::proxy::category_endpoint(Some(&category));
        resolved = crate::relay::forward::resolve_forward_rule(
            &state,
            &test_model,
            &category,
            &entry_path,
            Some(&channel),
            db_model.as_ref(),
        )
        .await;
    }

    let mut fwd = resolved.unwrap_or_else(|| {
        crate::relay::forward::infer_forward_from_base_url(
            &channel.base_url,
            &category,
            db_model.as_ref(),
        )
    });

    // ── 修正并校正 target_type ──
    crate::relay::forward::refine_target_type(&mut fwd, &channel.base_url);
    crate::relay::forward::apply_channel_provider(&mut fwd, &channel);

    let is_image = category == "图片";
    let is_video = category == "视频" || category == "视频增强";

    // 视频拨测：默认 480p + 5 秒；提交失败再升 720p 重试一次
    let video_resolutions: &[&str] = if is_video { &["480p", "720p"] } else { &[""] };

    let mut openai_body = serde_json::Value::Null;
    let mut request_data = serde_json::Value::Null;
    let mut masked_endpoint = String::new();
    let mut curl_cmd = String::new();
    let mut request_json = String::new();
    let mut raw_response_text = String::new();
    let mut response_res: Result<serde_json::Value, crate::error::AppError> =
        Ok(serde_json::json!({}));
    #[cfg(feature = "plugin_comfyui")]
    let mut comfy_ack: Option<crate::api::plugins::comfyui_bridge::ComfySubmitAck> = None;

    for (attempt_idx, resolution) in video_resolutions.iter().enumerate() {
        // ── 构建测试请求体（入参使用映射后的上游模型 ID：resolved_model） ──
        openai_body = if is_image {
            serde_json::json!({"model": resolved_model, "prompt": "Draw a picture of a cute white cat. 请画一只可爱的白色小猫。", "n": 1})
        } else if is_video {
            serde_json::json!({
                "model": resolved_model,
                "prompt": "A short video of a cute white cat walking",
                "resolution": *resolution,
                "duration": 5
            })
        } else {
            let st = rule_is_stream || fwd.upstream_path.contains("stream");
            serde_json::json!({"model": resolved_model, "messages": [{"role": "user", "content": "hi"}], "stream": st, "max_tokens": 5})
        };

        request_data = crate::relay::forward::transform_request_body(
            &fwd,
            &resolved_model,
            &openai_body,
            &category,
            db_rule.as_ref(),
            Some(&state.http_client),
        )
        .await;

        // 可灵视频/图片动态路由路径匹配
        crate::relay::forward::resolve_kling_dynamic_path(&mut fwd, &request_data);

        if fwd.target_type == "comfyui" {
            #[cfg(feature = "plugin_comfyui")]
            {
                let (wf_id, server_id) =
                    crate::api::plugins::comfyui_bridge::resolve_submit_target(
                        &state,
                        fwd.comfyui_workflow_id,
                        fwd.comfyui_server_id,
                        &fwd.comfyui_server_ids,
                        fwd.comfyui_dispatch.as_deref(),
                    )
                    .await?;
                request_json = serde_json::to_string(&openai_body).unwrap_or_default();
                masked_endpoint = format!("{}/prompt", channel.base_url.trim_end_matches('/'));
                curl_cmd = format!("# ComfyUI workflow_id={wf_id} server_id={server_id:?}");
                match crate::api::plugins::comfyui_bridge::submit_video(
                    &state,
                    wf_id,
                    &openai_body,
                    server_id,
                )
                .await
                {
                    Ok(ack) => {
                        let ack_json = serde_json::json!({
                            "id": ack.prompt_id,
                            "prompt_id": ack.prompt_id,
                            "status": "pending"
                        });
                        raw_response_text = ack_json.to_string();
                        response_res = Ok(ack_json);
                        request_json = ack.prompt_json.clone();
                        comfy_ack = Some(ack);
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        raw_response_text = msg.clone();
                        response_res = Ok(serde_json::json!({
                            "_upstream_status": 502,
                            "_upstream_error": {"error": msg}
                        }));
                    }
                }
                let send_failed = match &response_res {
                    Ok(v) => v.get("_upstream_error").is_some(),
                    Err(_) => true,
                };
                if !is_video || !send_failed || attempt_idx + 1 >= video_resolutions.len() {
                    break;
                }
                continue;
            }
            #[cfg(not(feature = "plugin_comfyui"))]
            {
                return Err(crate::error::AppError::BadRequest(
                    "ComfyUI 接入插件未编译".into(),
                ));
            }
        }

        // ── 构建 URL ──
        let endpoint = crate::relay::forward::build_upstream_url(
            &channel.base_url,
            &fwd,
            &resolved_model,
            &channel.api_key,
        );

        let builder = crate::services::http_client::with_upstream_timeout(
            crate::relay::forward::apply_request_auth(
                state
                    .http_client
                    .post(&endpoint)
                    .header("Content-Type", "application/json"),
                &fwd,
                &channel.api_key,
                &mut request_data,
                &channel.base_url,
            ),
        );

        // ── 生成 cURL 命令（在 apply_request_auth 之后，确保 body 包含可能注入的 SubAppId 等字段）──
        masked_endpoint = crate::relay::forward::mask_key_in_string(&endpoint, &channel.api_key);
        let auth_headers_for_curl =
            crate::relay::forward::build_auth_headers(&fwd, &channel.api_key, true);
        curl_cmd = format!("curl -X POST '{}' \\\n", masked_endpoint);
        curl_cmd.push_str("  -H 'Content-Type: application/json' \\\n");
        for (k, v) in &auth_headers_for_curl {
            let masked_v = crate::relay::forward::mask_key_in_string(v, &channel.api_key);
            curl_cmd.push_str(&format!("  -H '{}: {}' \\\n", k, masked_v));
        }
        request_json = serde_json::to_string(&request_data).unwrap_or_default();
        curl_cmd.push_str(&format!("  -d '{}'", request_json.replace("'", "\\'")));

        response_res = match builder.send().await {
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    let text = r.text().await.unwrap_or_default();
                    raw_response_text = text.clone();
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(v) => Ok(v),
                        Err(_) => Ok(serde_json::json!({
                            "status": "success",
                            "raw": text.chars().take(500).collect::<String>()
                        })),
                    }
                } else {
                    let status_val = status.as_u16();
                    let err_body = r.text().await.unwrap_or_default();
                    raw_response_text = err_body.clone();
                    let upstream_json = serde_json::from_str::<serde_json::Value>(&err_body)
                        .unwrap_or_else(|_| serde_json::json!({"raw_error": err_body}));
                    Ok(serde_json::json!({
                        "_upstream_status": status_val,
                        "_upstream_error": upstream_json
                    }))
                }
            }
            Err(e) => {
                raw_response_text = format!(
                    "{{\"connection_error\":\"{}\"}}",
                    e.to_string().replace('"', "\\\"")
                );
                Ok(serde_json::json!({
                    "_upstream_status": 0,
                    "_upstream_error": {"connection_error": e.to_string()}
                }))
            }
        };

        let send_failed = match &response_res {
            Ok(v) => v.get("_upstream_error").is_some(),
            Err(_) => true,
        };
        // 非视频只跑一轮；视频 480p 失败则升 720p 再试
        if !is_video || !send_failed || attempt_idx + 1 >= video_resolutions.len() {
            break;
        }
        tracing::warn!(
            "[TestChannel] 视频拨测 resolution={} 失败，改用 {} 重试",
            resolution,
            video_resolutions[attempt_idx + 1]
        );
    }

    // ── 响应格式转换（与 relay 保持一致，确保 task_id 可被正确提取）──
    let response_content_for_log = if fwd.target_type.starts_with("tencent_vod") {
        let (converted, _) =
            crate::relay::task::convert_tencent_post_response(&raw_response_text, &category);
        converted
    } else {
        raw_response_text
    };

    // ── 记录日志（字段与 relay 模块对齐，确保后续功能正常读取）──
    let latency_ms = start.elapsed().as_millis() as i32;
    let mut status_code = 200;
    let mut p_tokens = 0;
    let mut c_tokens = 0;
    let mut err_msg: Option<String> = None;

    match &response_res {
        Ok(v) => {
            if v.get("_upstream_error").is_some() {
                status_code = v
                    .get("_upstream_status")
                    .and_then(|s| s.as_i64())
                    .unwrap_or(502) as i32;
                err_msg = Some(serde_json::to_string(&v["_upstream_error"]).unwrap_or_default());
            } else if let Some(usage) = v.get("usage") {
                p_tokens = usage
                    .get("prompt_tokens")
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0) as i32;
                c_tokens = usage
                    .get("completion_tokens")
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0) as i32;
            }
        }
        Err(e) => {
            status_code = 500;
            err_msg = Some(e.to_string());
        }
    }

    let user_id_str = match &options_claims {
        Some(axum::Extension(c)) => c.sub.clone(),
        None => "0".to_string(),
    };

    // endpoint 格式与 relay 一致: "入口|上游路径"，便于正确推断类别
    let ep = format!(
        "test|{}",
        fwd.upstream_path.replace("${model}", &resolved_model)
    );
    let openai_body_str = serde_json::to_string(&openai_body).unwrap_or_default();
    let task_id =
        crate::relay::task::extract_task_id(&response_content_for_log).unwrap_or_default();

    // ── 预先计算渠道测试的计费明细依据（但不扣费） ──
    let preview_usage = crate::relay::usage_extractor::UsageTokens {
        prompt: p_tokens,
        completion: c_tokens,
        ..Default::default()
    };
    let (_, calc_detail) = crate::relay::compute_cost(
        db_model.as_ref(),
        db_rule.as_ref(),
        &preview_usage,
        1.0,
        &crate::relay::usage_extractor::ExtractedFeatures::default(),
    );
    let final_billing_detail = format!("[测试渠道，不扣费] {}", calc_detail);
    let billing_pid = db_rule.as_ref().map(|r| r.pid.clone());
    let forward_eid = db_forward_rule.as_ref().map(|r| r.eid.clone()).or_else(|| {
        if fwd.eid.is_empty() {
            None
        } else {
            Some(fwd.eid.clone())
        }
    });

    // ── 存入日志前对内容进行 base64 脱敏（复用 relay 模块统一脱敏逻辑） ──
    let sanitized_req = crate::relay::proxy::sanitize_base64(&openai_body_str);
    let sanitized_resp = crate::relay::proxy::sanitize_base64(&response_content_for_log);
    let sanitized_upstream_req = crate::relay::proxy::sanitize_base64(&request_json);

    // 与 relay::record_pending_log 一致：任务类用 tsk_，其余用 log_
    let log_id_str = {
        let prefix = if !category.is_empty() && category != "聊天" {
            "tsk_"
        } else {
            "log_"
        };
        format!("{}{}", prefix, ulid::Ulid::new().to_string().to_lowercase())
    };

    // 异步图/视频测试：先写「冻结」进行中态，供任务列表展示刷新按钮；
    // is_completed=1 避免被 TaskPoller 捡起重复轮询（本函数内同步 poll）。
    let will_poll = !task_id.is_empty() && (is_image || is_video) && status_code == 200;
    let insert_billing_detail = if will_poll {
        "[测试渠道，不扣费] 异步任务处理中(冻结)".to_string()
    } else {
        final_billing_detail.clone()
    };

    // INSERT 返回数字主键 id；同时写入业务 log_id（tsk_/log_）供列表展示与检索
    let db_row_id = sqlx::query_scalar::<_, i64>(&state.db.format_query(
        "INSERT INTO logs (log_id, user_id, channel_id, token_id, model, prompt_tokens, completion_tokens, \
         cost, latency_ms, status_code, endpoint, error_message, upstream_url, \
         request_content, response_content, upstream_req_content, billing_detail, action_type, task_id, \
         billing_pid, forward_eid, channel_config_id, is_ha, is_completed) \
         VALUES (?, ?, ?, 0, ?, ?, ?, 0.0, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1) RETURNING id"
    ))
        .bind(&log_id_str).bind(&user_id_str).bind(id).bind(&test_model)
        .bind(p_tokens).bind(c_tokens).bind(latency_ms)
        .bind(status_code).bind(&ep).bind(&err_msg).bind(&masked_endpoint)
        .bind(&sanitized_req).bind(&sanitized_resp)
        .bind(&sanitized_upstream_req).bind(&insert_billing_detail).bind(&category).bind(&task_id)
        .bind(billing_pid).bind(forward_eid).bind(channel_config_id.map(|cid| cid as i32))
        .bind(crate::relay::ha::channel_is_ha_flag(&channel))
        .fetch_optional(&state.db.pool).await.unwrap_or(None);

    #[cfg(feature = "plugin_comfyui")]
    if let (Some(lid), Some(ack)) = (db_row_id, comfy_ack.as_ref()) {
        crate::api::plugins::comfyui_bridge::link_job(&state, lid, ack).await?;
    }

    // ── 异步任务自动轮询（统一支持所有厂商：火山/腾讯云/DashScope/可灵等）──
    let mut response_res = response_res;
    if will_poll {
        if let Some((final_body, _final_status)) = crate::relay::task::poll_task_result(
            &state.http_client,
            &channel,
            &fwd,
            &task_id,
            crate::relay::task::PollTaskOpts {
                model: &resolved_model,
                category: &category,
                timeout_secs: 180,
                jimeng_ctx: None,
            },
        )
        .await
        {
            // 更新日志：终态响应 + 最终计费明细（去掉冻结标记）
            if let Some(lid) = db_row_id {
                let sanitized_final = crate::relay::proxy::sanitize_base64(&final_body);
                let _ = sqlx::query(&state.db.format_query(
                    "UPDATE logs SET response_content = ?, billing_detail = ?, is_completed = 1 WHERE id = ?",
                ))
                .bind(&sanitized_final)
                .bind(&final_billing_detail)
                .bind(lid)
                .execute(&state.db.pool)
                .await;
            }
            // 更新前端展示的响应数据为终态结果
            response_res = match serde_json::from_str::<serde_json::Value>(&final_body) {
                Ok(v) => Ok(v),
                Err(_) => {
                    Ok(serde_json::json!({"raw": final_body.chars().take(500).collect::<String>()}))
                }
            };
        } else if let Some(lid) = db_row_id {
            // 轮询超时/无终态：保留响应，去掉冻结以免任务列表永久「进行中」
            let _ =
                sqlx::query(&state.db.format_query(
                    "UPDATE logs SET billing_detail = ?, is_completed = 1 WHERE id = ?",
                ))
                .bind(&final_billing_detail)
                .bind(lid)
                .execute(&state.db.pool)
                .await;
        }
    }

    // ── 返回结果 ──
    match response_res {
        Ok(response_value) => {
            let is_upstream_err = response_value.get("_upstream_error").is_some();
            if is_upstream_err {
                let upstream_status = response_value
                    .get("_upstream_status")
                    .and_then(|s| s.as_i64())
                    .unwrap_or(0);
                Ok(Json(serde_json::json!({
                    "success": false, "err_msg": format!("上游返回 HTTP {}", upstream_status),
                    "latency": latency_ms, "channel_id": id, "curl_command": curl_cmd,
                    "request_data": request_data, "response_data": response_value,
                })))
            } else {
                Ok(Json(serde_json::json!({
                    "success": true, "latency": latency_ms, "channel_id": id,
                    "curl_command": curl_cmd, "request_data": request_data, "response_data": response_value,
                })))
            }
        }
        Err(e) => {
            let err_str = e.to_string();
            Ok(Json(serde_json::json!({
                "success": false, "err_msg": err_str.clone(), "latency": latency_ms,
                "channel_id": id, "curl_command": curl_cmd, "request_data": request_data,
                "response_data": { "error": err_str },
            })))
        }
    }
}

// ── 熔断状态查询与重置 ────────────────────────────────────────────

/// 查询渠道及其 HA 子渠道的熔断状态
pub async fn get_meltdown_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let channel: Channel =
        sqlx::query_as(&state.db.format_query("SELECT * FROM channels WHERE id = ?"))
            .bind(id)
            .fetch_one(&state.db.pool)
            .await?;

    let now = std::time::Instant::now();
    // 顺带清掉已过期熔断项，避免管理端展示与内存堆积
    crate::relay::ha::scrub_failed_channels(&state);
    let group_aid = channel.group_aid.clone().unwrap_or_default();

    // 1. 查询渠道自身的熔断状态
    let channel_meltdown = if !group_aid.is_empty() {
        if let Some(blocked_until) = state.failed_channels.get(&group_aid) {
            if *blocked_until.value() > now {
                let remaining = blocked_until.value().duration_since(now).as_secs();
                serde_json::json!({ "is_melted": true, "remaining_seconds": remaining, "key": group_aid })
            } else {
                serde_json::json!({ "is_melted": false, "remaining_seconds": 0 })
            }
        } else {
            serde_json::json!({ "is_melted": false, "remaining_seconds": 0 })
        }
    } else {
        serde_json::json!({ "is_melted": false, "remaining_seconds": 0 })
    };

    // 2. 查询 HA 子渠道的熔断状态
    let mut sub_channels = Vec::new();
    if channel.provider_type == "high_availability_group" {
        let config: serde_json::Value =
            serde_json::from_str(&channel.config).unwrap_or(serde_json::json!({}));

        let sub_ids: Vec<i64> = config
            .get("sub_channels")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
            .unwrap_or_default();

        // 批量查询子渠道名称
        let mut sub_names: std::collections::HashMap<i64, String> =
            std::collections::HashMap::new();
        if !sub_ids.is_empty() {
            let placeholders: Vec<String> = sub_ids.iter().map(|_| "?".to_string()).collect();
            let sql = format!(
                "SELECT id, name FROM channel_configs WHERE id IN ({})",
                placeholders.join(",")
            );
            let formatted_sql = state.db.format_query(&sql);
            let mut query = sqlx::query_as::<_, (i64, String)>(&formatted_sql);
            for sid in &sub_ids {
                query = query.bind(sid);
            }
            if let Ok(rows) = query.fetch_all(&state.db.pool).await {
                for (sid, name) in rows {
                    sub_names.insert(sid, name);
                }
            }
        }

        for sub_id in &sub_ids {
            let ha_key = format!("ha_group_{}_config_{}", id, sub_id);
            let name = sub_names
                .get(sub_id)
                .cloned()
                .unwrap_or_else(|| format!("渠道 {}", sub_id));

            if let Some(blocked_until) = state.failed_channels.get(&ha_key) {
                if *blocked_until.value() > now {
                    let remaining = blocked_until.value().duration_since(now).as_secs();
                    sub_channels.push(serde_json::json!({
                        "config_id": sub_id,
                        "name": name,
                        "is_melted": true,
                        "remaining_seconds": remaining,
                        "key": ha_key,
                    }));
                } else {
                    sub_channels.push(serde_json::json!({
                        "config_id": sub_id,
                        "name": name,
                        "is_melted": false,
                        "remaining_seconds": 0,
                    }));
                }
            } else {
                sub_channels.push(serde_json::json!({
                    "config_id": sub_id,
                    "name": name,
                    "is_melted": false,
                    "remaining_seconds": 0,
                }));
            }
        }
    }

    Ok(Json(serde_json::json!({
        "channel_meltdown": channel_meltdown,
        "sub_channels": sub_channels,
    })))
}

/// 手动重置渠道及其 HA 子渠道的熔断状态
pub async fn reset_meltdown(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let channel: Channel =
        sqlx::query_as(&state.db.format_query("SELECT * FROM channels WHERE id = ?"))
            .bind(id)
            .fetch_one(&state.db.pool)
            .await?;

    let group_aid = channel.group_aid.clone().unwrap_or_default();
    let mut cleared_count: u32 = 0;

    // 1. 清除渠道自身的熔断记录
    if !group_aid.is_empty() {
        if state.failed_channels.remove(&group_aid).is_some() {
            cleared_count += 1;
        }
    }

    // 2. 清除所有 HA 子渠道的熔断记录
    let ha_prefix = format!("ha_group_{}_", id);
    let keys_to_remove: Vec<String> = state
        .failed_channels
        .iter()
        .filter(|entry| entry.key().starts_with(&ha_prefix))
        .map(|entry| entry.key().clone())
        .collect();

    for key in &keys_to_remove {
        state.failed_channels.remove(key);
        cleared_count += 1;
    }

    tracing::info!(
        "[Meltdown Reset] 管理员手动重置渠道 {} (group_aid={}) 的熔断状态，共清除 {} 条记录",
        id,
        group_aid,
        cleared_count
    );

    Ok(Json(serde_json::json!({
        "success": true,
        "cleared_count": cleared_count,
    })))
}

/// 手动清零渠道分组已用额度（总/日/月）
pub async fn reset_quota(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    let result = sqlx::query(
        &state
            .db
            .format_query(&crate::models::channel_quota::reset_quota_sql("channels")),
    )
    .bind(id)
    .execute(&state.db.pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(crate::error::AppError::NotFound("渠道不存在".into()));
    }

    tracing::info!("[Channel Quota Reset] 管理员手动清零渠道 {} 的已用额度", id);
    Ok(Json(serde_json::json!({ "success": true })))
}
