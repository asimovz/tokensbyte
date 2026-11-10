/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

#![allow(dead_code)]
use crate::error::{AppError, AppResult};
use crate::models::Channel;
use crate::AppState;
use rand::Rng;
use std::sync::Arc;

/// 同档加权随机：`weight` 越大被选概率越高；负权重按 0 计，合计 ≤0 时取首项。
fn pick_weighted_by<T, F>(items: &[T], weight_of: F) -> &T
where
    F: Fn(&T) -> i32,
{
    debug_assert!(!items.is_empty());
    let weight = |item: &T| weight_of(item).max(0);
    let total_weight: i32 = items.iter().map(|i| weight(i)).sum();
    if total_weight <= 0 {
        return &items[0];
    }
    let mut rng = rand::rngs::OsRng;
    let random_value = rng.gen_range(0..total_weight);
    let mut current_sum = 0;
    for item in items {
        current_sum += weight(item);
        if random_value < current_sum {
            return item;
        }
    }
    &items[0]
}

/// Select the best channel for a given model based on priority and load balancing.
/// `allow_ha`: 是否允许选中高可用虚拟组（插件+令牌，见 `ha::policy`）。
pub async fn select_channel(
    state: &Arc<AppState>,
    model: &str,
    user_group: &str,
    level_id: &str,
    exclude_aids: &[String],
    mids: Option<&[String]>,
    allow_ha: bool,
) -> AppResult<Channel> {
    tracing::info!(
        "[SelectChannel] 开始 模型={} 分组={} 等级={} 已排除={:?} 允许HA={}",
        model,
        user_group,
        level_id,
        exclude_aids,
        allow_ha
    );

    // 1. 查找请求 model_id 对应的所有 mid（若传入了已解析好的 mids 数组，则免于数据库查询）
    let owned_mids;
    let mids_ref = if let Some(m) = mids {
        m
    } else {
        owned_mids = sqlx::query_scalar(
            &state
                .db
                .format_query("SELECT mid FROM models WHERE model_id = ? AND is_active = 1"),
        )
        .bind(model)
        .fetch_all(&state.db.pool)
        .await
        .unwrap_or_default();
        &owned_mids
    };

    // 2. 构建匹配条件：匹配 mid 列表中的任意值，同时兼容旧格式（直接存 model_id 的渠道）
    // 使用参数化绑定防止 SQL 注入（model/mids/exclude_aids 均通过 .bind() 传入）
    let mut bind_values: Vec<String> = Vec::new();

    // models LIKE 条件：每个 model/mid 生成一个 "models LIKE ?" 占位符
    // 绑定值使用 %"model"% 格式，匹配 JSON 数组中任意位置的模型名
    let mut model_conditions = vec!["models LIKE ?".to_string()];
    bind_values.push(format!("%\"{}\"%", model));
    for mid in mids_ref {
        model_conditions.push("models LIKE ?".to_string());
        bind_values.push(format!("%\"{}\"%", mid));
    }
    let model_clause = format!("(({}) OR models = '[]')", model_conditions.join(" OR "));

    // exclude_aids：仅把物理渠 group_aid 推进 SQL；ha_group_* 由内存过滤/子渠逻辑处理
    let sql_excludes: Vec<&String> = exclude_aids
        .iter()
        .filter(|a| !a.starts_with("ha_group_"))
        .collect();
    let exclude_clause = if sql_excludes.is_empty() {
        String::new()
    } else {
        let placeholders = sql_excludes
            .iter()
            .map(|_| "?")
            .collect::<Vec<&str>>()
            .join(",");
        format!("AND group_aid NOT IN ({})", placeholders)
    };

    let sql = format!(
        r#"SELECT * FROM channels 
           WHERE status = 1 
           AND (quota_limit < 0 OR quota_used < quota_limit)
           AND (daily_quota_limit < 0 OR (CASE WHEN COALESCE(last_reset_day, '') <> ? THEN 0 ELSE daily_quota_used END) < daily_quota_limit)
           AND (weekly_quota_limit < 0 OR (CASE WHEN COALESCE(last_reset_week, '') <> ? THEN 0 ELSE weekly_quota_used END) < weekly_quota_limit)
           AND (monthly_quota_limit < 0 OR (CASE WHEN COALESCE(last_reset_month, '') <> ? THEN 0 ELSE monthly_quota_used END) < monthly_quota_limit)
           AND {}
           {}
           AND (user_groups LIKE ? OR user_groups LIKE ? OR user_groups = '[]')
           ORDER BY priority DESC"#,
        model_clause, exclude_clause
    );

    let tz_name = crate::relay::relay_settings::get_cached_site_timezone(&state.db).await;
    let (now_day, now_week, now_month) = crate::models::quota_period_keys(&tz_name);

    let formatted_sql = state.db.format_query(&sql);
    let mut query = sqlx::query_as(&formatted_sql);
    // 绑定日/周/月额度懒重置键
    query = query.bind(&now_day);
    query = query.bind(&now_week);
    query = query.bind(&now_month);
    // 绑定 models LIKE 参数
    for val in &bind_values {
        query = query.bind(val);
    }
    // 绑定 exclude_aids 参数（仅物理 group_aid）
    for aid in &sql_excludes {
        query = query.bind(aid.as_str());
    }
    // 绑定 user_groups LIKE 参数
    query = query.bind(format!("%\"{}\"%", user_group));
    query = query.bind(format!("%\"{}\"%", level_id));
    let channels: Vec<Channel> = query.fetch_all(&state.db.pool).await?;

    tracing::info!(
        "[SelectChannel] 库候选数={} 渠道id={:?}",
        channels.len(),
        channels.iter().map(|c| c.id).collect::<Vec<i64>>()
    );

    // 记录过滤前库候选数：区分「该模型无路由」(404) 与「有渠道但被熔断/额度全过滤」(429)
    let db_candidate_n = channels.len();

    // 预加载非 HA 渠道绑定的上游预设，过滤禁用 / 额度耗尽的预设
    let preset_ids: Vec<i64> = channels
        .iter()
        .filter(|c| c.provider_type != "high_availability_group")
        .filter_map(|c| c.preset_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let mut unusable_presets: std::collections::HashSet<i64> = std::collections::HashSet::new();
    if !preset_ids.is_empty() {
        let presets: Vec<crate::models::ChannelConfig> = sqlx::query_as(
            &state
                .db
                .format_query("SELECT * FROM channel_configs WHERE id = ANY(?)"),
        )
        .bind(&preset_ids)
        .fetch_all(&state.db.pool)
        .await
        .unwrap_or_default();
        for p in &presets {
            if p.status != 1 || !p.has_available_quota(&tz_name, &now_week, &now_month) {
                unusable_presets.insert(p.id);
            }
        }
        // 预设记录缺失也视为不可用
        for pid in &preset_ids {
            if !presets.iter().any(|p| p.id == *pid) {
                unusable_presets.insert(*pid);
            }
        }
    }

    // 过滤：熔断 / 黑名单 / 不允许 HA / 整组 exclude（ha_group_{id}）/ 预设禁用或额度耗尽
    let mut channels: Vec<Channel> = channels
        .into_iter()
        .filter(|c| {
            if c.provider_type == "high_availability_group" {
                if !allow_ha {
                    return false;
                }
                let group_key = format!("ha_group_{}", c.id);
                if exclude_aids.iter().any(|a| a == &group_key) {
                    return false;
                }
            } else if let Some(pid) = c.preset_id {
                if unusable_presets.contains(&pid) {
                    return false;
                }
            }
            let excludes = c.get_exclude_user_groups();
            if !excludes.is_empty()
                && (excludes.contains(&user_group.to_string())
                    || excludes.contains(&level_id.to_string()))
            {
                return false;
            }
            if let Some(ref aid) = c.group_aid {
                if crate::relay::ha::is_melted_down(state, aid) {
                    return false;
                }
            }
            true
        })
        .collect();

    tracing::info!(
        "[SelectChannel] 过滤后={} 渠道id={:?}",
        channels.len(),
        channels.iter().map(|c| c.id).collect::<Vec<i64>>()
    );

    if channels.is_empty() {
        let err_msg = format!("No available channels found for model {}", model);
        tracing::warn!(
            "[SelectChannel] 未命中 模型={} 分组={} 等级={}",
            model,
            user_group,
            level_id
        );
        // 有候选但被熔断/额度/黑名单全过滤 → 暂时无容量，429 让客户端退避重试；
        // 库里根本没有该 模型+分组 候选 → 才是真正的 404「无此模型」
        return Err(if db_candidate_n == 0 {
            AppError::NotFound(err_msg)
        } else {
            AppError::TooManyRequests(err_msg)
        });
    }

    // 物理渠 / HA 子渠：最高 priority 档内按 weight 比例随机分流
    // 顺序不变量：HA 子配注入并清 preset_id →（仅剩 preset 时）查 preset → 最后 volc
    let mut ch = loop {
        let highest_priority = channels[0].priority;
        let top_tier: Vec<Channel> = channels
            .iter()
            .take_while(|c| c.priority == highest_priority)
            .cloned()
            .collect();
        let picked = pick_weighted_by(&top_tier, |c| c.weight).clone();

        if picked.provider_type != "high_availability_group" {
            break picked;
        }

        let sub_channel_ids: Vec<i64> = serde_json::from_str::<serde_json::Value>(&picked.config)
            .ok()
            .and_then(|v| v.get("sub_channels").cloned())
            .and_then(|v| serde_json::from_value::<Vec<i64>>(v).ok())
            .unwrap_or_default();

        if sub_channel_ids.is_empty() {
            return Err(AppError::NotFound(format!(
                "高可用虚拟渠道组 (ID: {}) 配置异常，未绑定任何上游子渠道",
                picked.id
            )));
        }

        let mut sub_configs: Vec<crate::models::ChannelConfig> = sqlx::query_as(
            &state
                .db
                .format_query("SELECT * FROM channel_configs WHERE id = ANY(?)"),
        )
        .bind(&sub_channel_ids)
        .fetch_all(&state.db.pool)
        .await?;

        let group_id = picked.id;
        sub_configs = sub_configs
            .into_iter()
            .filter(|sub_c| {
                if sub_c.status != 1 {
                    return false;
                }
                let config_key = format!("ha_group_{}_config_{}", group_id, sub_c.id);
                if exclude_aids.contains(&config_key) {
                    return false;
                }
                if crate::relay::ha::is_melted_down(state, &config_key) {
                    return false;
                }
                // 上游预设额度耗尽则不可选
                sub_c.has_available_quota(&tz_name, &now_week, &now_month)
            })
            .collect();

        if sub_configs.is_empty() {
            // 整组不可用：剔除后重选，避免误报盖住其它可用物理渠
            channels.retain(|c| c.id != group_id);
            if channels.is_empty() {
                let err_msg = format!("No available channels found for model {}", model);
                tracing::warn!("[SelectChannel] HA组 {} 子渠已耗尽", group_id);
                return Err(AppError::TooManyRequests(err_msg));
            }
            continue;
        }

        // 子渠：最高 priority 档；同档按 weight 比例随机（绑定序仅作 weight≤0 时的兜底顺序）
        // 组内策略键 channels.config.sub_channel_policy 存在则覆盖上游全局 P/W，解析失败逐条回落
        let sub_policy = crate::relay::ha::parse_sub_channel_policy(&picked.config);
        let highest_sub_priority = sub_configs
            .iter()
            .map(|c| sub_policy.priority_of(c.id, c.priority))
            .max()
            .unwrap_or(0);
        let top_subs: Vec<&_> = sub_channel_ids
            .iter()
            .filter_map(|&id| {
                sub_configs
                    .iter()
                    .find(|c| c.id == id && sub_policy.priority_of(c.id, c.priority) == highest_sub_priority)
            })
            .collect();
        if top_subs.is_empty() {
            channels.retain(|c| c.id != group_id);
            if channels.is_empty() {
                return Err(AppError::TooManyRequests(format!(
                    "No available channels found for model {}",
                    model
                )));
            }
            continue;
        }
        let selected_sub = *pick_weighted_by(&top_subs, |c| sub_policy.weight_of(c.id, c.weight));

        let mut resolved = picked;
        apply_ha_sub_mapped(&mut resolved, selected_sub);
        break resolved;
    };

    // 4. Resolve preset (channel config template)
    if let Some(pid) = ch.preset_id {
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
                tracing::warn!("[SelectChannel] 预设已禁用 渠道={} 预设id={}", ch.id, pid);
                return Err(AppError::NotFound(format!(
                    "上游渠道配置已禁用 (preset_id={})",
                    pid
                )));
            }
            tracing::info!(
                "[SelectChannel] 套用预设 渠道={} 子渠标识={:?} 预设id={} {} -> {}",
                ch.id,
                ch.group_aid,
                pid,
                ch.base_url,
                preset.base_url
            );
            apply_config_base(&mut ch, &preset);
        } else {
            tracing::warn!(
                "[SelectChannel] 预设缺失 渠道={} 子渠标识={:?} 预设id={}",
                ch.id,
                ch.group_aid,
                pid
            );
        }
    }

    // 5. 画质增强凭证集成：从 config 中的凭证 ID 实时查询最新密钥（保证插件端修改凭证后渠道分组数据一致）
    apply_volcengine_credential(state, &mut ch).await;
    apply_comfyui_channel(state, &mut ch).await;

    tracing::info!(
        "[SelectChannel] 选中 渠道id={} 上游YID={} 名称='{}' 类型={} 子渠标识={:?} 地址={}",
        ch.id,
        crate::relay::ha::yid_label(ch.yid.as_deref()),
        ch.name,
        ch.provider_type,
        ch.group_aid,
        ch.base_url
    );

    Ok(ch)
}

/// 将 channel_configs 的 base/key/rate/yid 写入内存 Channel（选渠 / 重载共用）
#[inline]
fn apply_config_base(ch: &mut crate::models::Channel, cfg: &crate::models::ChannelConfig) {
    ch.base_url = cfg.base_url.clone();
    ch.api_key = cfg.api_key.clone();
    ch.rate = cfg.rate;
    ch.yid = Some(cfg.yid.clone());
}

/// HA 子配注入：写 base/key/rate/yid/name/provider/group_aid，清 preset_id
/// `name` 用子配名（内存态，供 ha_usage_logs）；父渠 DB 名不变，logs JOIN 仍显示组名
#[inline]
fn apply_ha_sub(ch: &mut crate::models::Channel, cfg: &crate::models::ChannelConfig) {
    let group_id = ch.id;
    apply_config_base(ch, cfg);
    ch.name = cfg.name.clone();
    ch.provider_type = cfg.provider_type.clone();
    ch.group_aid = Some(format!("ha_group_{}_config_{}", group_id, cfg.id));
    ch.preset_id = None; // 防止后续父行 preset 覆盖子配
}

/// HA 子配 + `ha_model_mapping`（选渠 / 重载 / 渠道测试唯一入口，避免漏映射）
#[inline]
pub(crate) fn apply_ha_sub_mapped(
    ch: &mut crate::models::Channel,
    cfg: &crate::models::ChannelConfig,
) {
    apply_ha_sub(ch, cfg);
    apply_ha_model_mapping(ch, cfg.id);
}

/// 按子配 id 叠加父渠 `ha_model_mapping`（保证异步 settle / 测试与同步选渠一致）
#[inline]
fn apply_ha_model_mapping(ch: &mut crate::models::Channel, sub_id: i64) {
    let Ok(config_val) = serde_json::from_str::<serde_json::Value>(&ch.config) else {
        return;
    };
    let Some(ha_mapping) = config_val
        .get("ha_model_mapping")
        .and_then(|v| v.as_object())
    else {
        return;
    };
    let mut base_mapping: std::collections::HashMap<String, String> =
        serde_json::from_str(&ch.model_mapping).unwrap_or_default();
    let sub_id_str = sub_id.to_string();
    for (model_id, sub_map) in ha_mapping {
        if let Some(alias) = sub_map.get(&sub_id_str).and_then(|v| v.as_str()) {
            if !alias.is_empty() {
                base_mapping.insert(model_id.clone(), alias.to_string());
            }
        }
    }
    ch.model_mapping = serde_json::to_string(&base_mapping).unwrap_or_else(|_| "{}".to_string());
}

/// 将已查到的子配/预设写入内存 Channel（HA 走 apply_ha_sub_mapped；物理走 base）
#[inline]
fn apply_reload_cfg(ch: &mut crate::models::Channel, cfg: &crate::models::ChannelConfig) {
    if ch.provider_type == "high_availability_group" {
        apply_ha_sub_mapped(ch, cfg);
    } else {
        apply_config_base(ch, cfg);
        ch.preset_id = Some(cfg.id);
    }
}

/// 按 ID 加载并水合渠道，产出与 `select_channel` 一致的运行时 Channel（含最终 base_url/api_key/yid/rate/映射）。
/// `channel_config_id`：日志中的子配快照；HA 异步轮询必传，以还原当时选中的子配。
pub async fn fetch_channel(
    state: &crate::AppState,
    channel_id: i64,
    channel_config_id: Option<i32>,
) -> Option<Channel> {
    let mut ch: Channel =
        sqlx::query_as(&state.db.format_query("SELECT * FROM channels WHERE id = ?"))
            .bind(channel_id)
            .fetch_optional(&state.db.pool)
            .await
            .ok()??;

    hydrate_for_reload(state, &mut ch, channel_config_id).await;
    Some(ch)
}

/// 重载路径水合：channel_config_id → 父行 preset_id → volc。
/// 与 select 不同：此处无 HA 加权选子，只能靠日志快照还原当时子配。
async fn hydrate_for_reload(
    state: &crate::AppState,
    ch: &mut crate::models::Channel,
    channel_config_id: Option<i32>,
) {
    if let Some(cid) = channel_config_id {
        if let Ok(Some(cfg)) = sqlx::query_as::<_, crate::models::ChannelConfig>(
            &state
                .db
                .format_query("SELECT * FROM channel_configs WHERE id = ?"),
        )
        .bind(cid as i64)
        .fetch_optional(&state.db.pool)
        .await
        {
            apply_reload_cfg(ch, &cfg);
            apply_volcengine_credential(state, ch).await;
            apply_comfyui_channel(state, ch).await;
            return;
        }
    }
    if let Some(pid) = ch.preset_id {
        if let Ok(Some(preset)) = sqlx::query_as::<_, crate::models::ChannelConfig>(
            &state
                .db
                .format_query("SELECT * FROM channel_configs WHERE id = ?"),
        )
        .bind(pid)
        .fetch_optional(&state.db.pool)
        .await
        {
            apply_config_base(ch, &preset);
        }
    }
    apply_volcengine_credential(state, ch).await;
    apply_comfyui_channel(state, ch).await;
}

/// 解析最终上游模型名（全站唯一入口）。
/// 优先级：分辨率映射 > 渠道映射（含 HA 子渠明文）> 模型表别名 > 原始 id。
/// `resolution`：已规范化更佳；`None` 则跳过分辨率映射。
pub fn resolve_model(
    channel: &Channel,
    requested_model: &str,
    db_model: Option<&crate::models::Model>,
    resolution: Option<&str>,
) -> (String, Option<String>) {
    if let Some(res) = resolution {
        if let Some((alias, res_key)) = resolve_res_alias(channel, requested_model, res) {
            let src = format!("分辨率映射@{}", res_key);
            tracing::debug!("[ModelMap] {} {} → {}", src, requested_model, alias);
            return (alias, Some(src));
        }
    }
    let resolved = channel.resolve_model(requested_model);
    if resolved != requested_model {
        return (resolved, Some("渠道映射".into()));
    }
    if let Some(m) = db_model {
        if !m.model_id_alias.is_empty() {
            return (m.model_id_alias.clone(), Some("模型映射".into()));
        }
    }
    (resolved, None)
}

/// 图片/视频请求入口：从 body 抽分辨率后走 [`resolve_model`]（聊天/语音等勿用）
#[inline]
pub fn resolve_model_body(
    channel: &Channel,
    requested_model: &str,
    db_model: Option<&crate::models::Model>,
    body: Option<&serde_json::Value>,
) -> (String, Option<String>) {
    let res = body.and_then(crate::relay::usage_extractor::extract_resolution);
    resolve_model(channel, requested_model, db_model, res.as_deref())
}

/// 结算等场景：仅图片/视频类别才带上已提取分辨率做映射
#[inline]
pub fn mapping_resolution<'a>(
    category: Option<&str>,
    resolution: Option<&'a str>,
) -> Option<&'a str> {
    category
        .filter(|c| c.contains("图片") || c.contains("视频"))
        .and(resolution)
}

/// `config.res_model_mapping`: `{ model_id: { "default"|sub_id: { "480p"|"1k": alias } } }`
/// 回退：子渠该档 → 默认别名该档 →（未命中则走明文别名）
fn resolve_res_alias(
    channel: &Channel,
    requested_model: &str,
    resolution: &str,
) -> Option<(String, String)> {
    let config: serde_json::Value = serde_json::from_str(&channel.config).ok()?;
    let scopes = config
        .get("res_model_mapping")?
        .get(requested_model)?
        .as_object()?;
    let res_key = crate::relay::usage_extractor::normalize_resolution_label(resolution);
    if res_key.is_empty() {
        return None;
    }
    let pick = |scope: &str| -> Option<String> {
        let map = scopes.get(scope)?.as_object()?;
        res_alias_in_map(map, &res_key)
    };
    if let Some(sid) = crate::relay::ha::ha_config_id_from_aid(channel) {
        if let Some(alias) = pick(&sid.to_string()) {
            return Some((alias, res_key));
        }
    }
    pick("default").map(|alias| (alias, res_key))
}

#[inline]
fn res_alias_in_map(
    map: &serde_json::Map<String, serde_json::Value>,
    res_key: &str,
) -> Option<String> {
    if let Some(a) = map
        .get(res_key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(a.to_string());
    }
    // 兼容历史未规范 key（如 "480" / "720P"）
    map.iter().find_map(|(k, v)| {
        if crate::relay::usage_extractor::normalize_resolution_label(k) != res_key {
            return None;
        }
        v.as_str()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    })
}

/// 画质增强凭证集成：根据 config 中的凭证 ID 实时查询 plugin_configs 表获取最新密钥并覆盖到渠道
/// （供 select_channel / fetch_channel / 渠道测试共用）
pub(crate) async fn apply_volcengine_credential(
    state: &crate::AppState,
    ch: &mut crate::models::Channel,
) {
    if ch.provider_type != "volcengine" {
        return;
    }
    let cred_id = match serde_json::from_str::<serde_json::Value>(&ch.config)
        .ok()
        .and_then(|cfg| {
            cfg.get("volcengine_enhance_credential_id")?
                .as_str()
                .map(|s| s.to_string())
        }) {
        Some(id) => id,
        None => return,
    };
    let keys_str: Option<String> = sqlx::query_scalar(
        &state.db.format_query(
            "SELECT config_value FROM plugin_configs WHERE plugin_name = 'volcengine_enhance' AND config_key = 'keys'"
        )
    ).fetch_optional(&state.db.pool).await.ok().flatten();
    if let Some(ref ks) = keys_str {
        if let Ok(keys) = serde_json::from_str::<Vec<serde_json::Value>>(ks) {
            if let Some(k) = keys
                .iter()
                .find(|k| k.get("id").and_then(|v| v.as_str()) == Some(&cred_id))
            {
                ch.api_key = k
                    .get("api_key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                ch.base_url = k
                    .get("base_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("https://mediakit.cn-beijing.volces.com")
                    .to_string();
            }
        }
    }
}

#[cfg(feature = "plugin_comfyui")]
pub(crate) async fn apply_comfyui_channel(state: &crate::AppState, ch: &mut crate::models::Channel) {
    if ch.provider_type != "comfyui" {
        return;
    }
    let cfg = serde_json::from_str::<serde_json::Value>(&ch.config).unwrap_or(serde_json::Value::Null);
    let server_id = crate::relay::forward::parse_comfyui_server_ids(&cfg)
        .first()
        .copied();
    let workflow_id = cfg.get("comfyui_workflow_id").and_then(|v| v.as_i64());
    let url: Option<String> = if let Some(sid) = server_id {
        sqlx::query_scalar(&state.db.format_query(
            "SELECT base_url FROM comfyui_servers WHERE id = ? AND is_active = 1",
        ))
        .bind(sid)
        .fetch_optional(&state.db.pool)
        .await
        .ok()
        .flatten()
    } else if let Some(wid) = workflow_id {
        sqlx::query_scalar(&state.db.format_query(
            "SELECT s.base_url FROM comfyui_workflows w \
             INNER JOIN comfyui_servers s ON s.id = w.server_id \
             WHERE w.id = ? AND w.is_active = 1 AND s.is_active = 1",
        ))
        .bind(wid)
        .fetch_optional(&state.db.pool)
        .await
        .ok()
        .flatten()
    } else {
        None
    };
    if let Some(u) = url {
        ch.base_url = u;
    }
}

#[cfg(not(feature = "plugin_comfyui"))]
pub(crate) async fn apply_comfyui_channel(_: &crate::AppState, _: &mut crate::models::Channel) {}
