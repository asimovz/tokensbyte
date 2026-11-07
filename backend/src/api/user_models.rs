/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 用户可访问模型反查（只读，配置层）。
//!
//! 判定复刻 `relay/router.rs` 里 `models` / `user_groups` / `exclude_user_groups` 三处匹配，
//! 但**不查额度、熔断、上游预设**：本视图回答「这个等级在哪些渠道里被配了」，
//! 不回答「现在调用一定成功」。选渠规则若变更，需同步本文件的 `channel_matches_user`。

use crate::error::{AppError, AppResult};
use crate::relay::ha::parse_sub_channel_policy;
use crate::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const HA_PROVIDER: &str = "high_availability_group";

/// 渠道一行的只读投影（全部取成文本，避开 int4/int8 漂移）。
struct ChanRow {
    id: String,
    name: String,
    provider_type: String,
    models: String,
    user_groups: String,
    exclude_user_groups: String,
    config: String,
}

/// 上游全局配置一行的只读投影，用于 HA 组成员名称与 P/W 回落。
struct CfgRow {
    id: String,
    name: String,
    priority: i64,
    weight: i64,
}

/// JSON 文本列 -> `Vec<String>`；畸形或非字符串元素按容错丢弃。
fn parse_str_array(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<Value>>(raw)
        .map(|arr| {
            arr.into_iter()
                .filter_map(|v| match v {
                    Value::String(s) => Some(s),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 线上选渠把「空数组」当通配，判定条件是字面量 `'[]'`；畸形文本不算通配。
fn is_empty_json_array(raw: &str) -> bool {
    raw.replace(' ', "") == "[]"
}

/// 该用户等级（group_key 或等级 ID 任一写法）能否用这条渠道。
/// 返回 `Some(true)` = 等级白名单命中，`Some(false)` = 不限等级人人可用，`None` = 不可用。
fn channel_matches_user(
    chan: &ChanRow,
    keys: &HashSet<String>,
) -> Option<bool> {
    let excludes = parse_str_array(&chan.exclude_user_groups);
    if excludes.iter().any(|e| keys.contains(e)) {
        return None;
    }
    if is_empty_json_array(&chan.user_groups) {
        return Some(false);
    }
    let groups = parse_str_array(&chan.user_groups);
    if groups.iter().any(|g| keys.contains(g)) {
        return Some(true);
    }
    None
}

/// `channels.models` 的元素既可能是 mid，也可能是历史写法的 model_id；
/// 空数组（`[]`）= 该渠道接受所有模型。解析结果统一落到启用模型的 mid 上。
fn expand_model_entries(
    chan: &ChanRow,
    mid_set: &HashSet<String>,
    name_to_mids: &HashMap<String, Vec<String>>,
    all_mids: &[String],
) -> Vec<String> {
    let entries = parse_str_array(&chan.models);
    if entries.is_empty() {
        return if is_empty_json_array(&chan.models) {
            all_mids.to_vec()
        } else {
            Vec::new()
        };
    }
    let mut out = Vec::new();
    for e in entries {
        if mid_set.contains(&e) {
            out.push(e);
        } else if let Some(mids) = name_to_mids.get(&e) {
            out.extend(mids.iter().cloned());
        }
    }
    // 同一渠道里 mid 与 model_id 混写可能撞车，只算一次
    out.sort();
    out.dedup();
    out
}

/// 组内成员的生效优先级/权重：组内值优先，缺键回落上游全局值（与选渠同源）。
fn sub_channels_of(chan: &ChanRow, cfgs: &HashMap<String, CfgRow>) -> Vec<Value> {
    let config: Value = serde_json::from_str(&chan.config).unwrap_or(Value::Null);
    let ids: Vec<String> = config
        .get("sub_channels")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| match v {
                    Value::Number(n) => Some(n.to_string()),
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return Vec::new();
    }

    let policy = parse_sub_channel_policy(&chan.config);
    let policy_obj = config.get("sub_channel_policy").and_then(Value::as_object);

    ids.iter()
        .filter_map(|sid| {
            let cfg = cfgs.get(sid)?;
            let binding = policy_obj.and_then(|p| p.get(sid)).and_then(Value::as_object);
            let from_group = |field: &str| {
                binding
                    .and_then(|b| b.get(field))
                    .and_then(Value::as_i64)
                    .is_some()
            };
            let priority = policy.priority_of(sid_parse(sid), cfg.priority as i32);
            let weight = policy.weight_of(sid_parse(sid), cfg.weight as i32);
            Some(json!({
                "config_id": sid,
                "name": cfg.name,
                "priority": priority,
                "weight": weight,
                "priority_from": if from_group("priority") { "group" } else { "upstream" },
                "weight_from": if from_group("weight") { "group" } else { "upstream" },
            }))
        })
        .collect()
}

fn sid_parse(sid: &str) -> i64 {
    sid.parse().unwrap_or(0)
}

/// `GET /users/{id}/models`：某用户（按其等级）配置的模型 + 折扣 + 渠道/渠道分组。
pub async fn list_user_models(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let user: Option<(String, String, String, String, String, String, String)> = sqlx::query_as(
        &state.db.format_query(
            "SELECT COALESCE(CAST(u.username AS VARCHAR), ''), \
                    COALESCE(CAST(u.uid AS VARCHAR), ''), \
                    COALESCE(CAST(u.nickname AS VARCHAR), ''), \
                    COALESCE(CAST(u.user_group AS VARCHAR), ''), \
                    COALESCE(CAST(u.model_discounts AS VARCHAR), '{}'), \
                    COALESCE(CAST(ul.id AS VARCHAR), '0'), \
                    COALESCE(CAST(ul.name AS VARCHAR), '') \
             FROM users u LEFT JOIN user_levels ul ON u.user_group = ul.group_key \
             WHERE u.id = ?",
        ),
    )
    .bind(&id)
    .fetch_optional(&state.db.pool)
    .await?;

    let Some((username, uid, nickname, group_key, md_raw, level_id, level_name)) = user else {
        return Err(AppError::NotFound("用户不存在".to_string()));
    };

    // 与选渠一致：等级白名单同时接受 group_key 与等级 ID 两种写法
    let mut keys: HashSet<String> = HashSet::new();
    if !group_key.is_empty() {
        keys.insert(group_key.clone());
    }
    if level_id != "0" {
        keys.insert(level_id.clone());
    }

    let discounts: HashMap<String, f64> =
        serde_json::from_str(&md_raw).unwrap_or_default();

    let chan_rows: Vec<(String, String, String, String, String, String, String)> = sqlx::query_as(
        &state.db.format_query(
            "SELECT COALESCE(CAST(id AS VARCHAR), ''), COALESCE(CAST(name AS VARCHAR), ''), \
                    COALESCE(CAST(provider_type AS VARCHAR), ''), COALESCE(CAST(models AS VARCHAR), ''), \
                    COALESCE(CAST(user_groups AS VARCHAR), ''), COALESCE(CAST(exclude_user_groups AS VARCHAR), ''), \
                    COALESCE(CAST(config AS VARCHAR), '') \
             FROM channels WHERE status = 1",
        ),
    )
    .fetch_all(&state.db.pool)
    .await?;

    let model_rows: Vec<(String, String)> = sqlx::query_as(
        &state
            .db
            .format_query("SELECT COALESCE(CAST(mid AS VARCHAR), ''), COALESCE(CAST(model_id AS VARCHAR), '') FROM models WHERE is_active = 1"),
    )
    .fetch_all(&state.db.pool)
    .await?;

    let cfg_rows: Vec<(String, String, String, String)> = sqlx::query_as(
        &state.db.format_query(
            "SELECT COALESCE(CAST(id AS VARCHAR), ''), COALESCE(CAST(name AS VARCHAR), ''), \
                    COALESCE(CAST(priority AS VARCHAR), '0'), COALESCE(CAST(weight AS VARCHAR), '0') \
             FROM channel_configs",
        ),
    )
    .fetch_all(&state.db.pool)
    .await?;

    let mut mid_to_name: HashMap<String, String> = HashMap::new();
    let mut name_to_mids: HashMap<String, Vec<String>> = HashMap::new();
    let mut all_mids: Vec<String> = Vec::new();
    for (mid, model_id) in &model_rows {
        mid_to_name.insert(mid.clone(), model_id.clone());
        name_to_mids.entry(model_id.clone()).or_default().push(mid.clone());
        all_mids.push(mid.clone());
    }
    let mid_set: HashSet<String> = all_mids.iter().cloned().collect();

    let cfgs: HashMap<String, CfgRow> = cfg_rows
        .into_iter()
        .map(|(id, name, p, w)| {
            (
                id.clone(),
                CfgRow {
                    id,
                    name,
                    priority: p.parse().unwrap_or(0),
                    weight: w.parse().unwrap_or(0),
                },
            )
        })
        .collect();

    // mid -> 该等级可用的渠道
    #[derive(Default)]
    struct Agg {
        channels: Vec<Value>,
        has_open: bool,
    }
    let mut rows: HashMap<String, Agg> = HashMap::new();

    let chans: Vec<ChanRow> = chan_rows
        .into_iter()
        .map(
            |(id, name, provider_type, models, user_groups, exclude_user_groups, config)| ChanRow {
                id,
                name,
                provider_type,
                models,
                user_groups,
                exclude_user_groups,
                config,
            },
        )
        .collect();

    for c in chans {
        let matched = match channel_matches_user(&c, &keys) {
            Some(m) => m,
            None => continue,
        };
        let is_ha = c.provider_type == HA_PROVIDER;
        let view = json!({
            "id": c.id,
            "name": c.name,
            "is_ha_group": is_ha,
            "match": if matched { "level" } else { "open" },
            "sub_channels": if is_ha { sub_channels_of(&c, &cfgs) } else { Vec::<Value>::new() },
        });
        for mid in expand_model_entries(&c, &mid_set, &name_to_mids, &all_mids) {
            let row = rows.entry(mid).or_default();
            row.has_open = row.has_open || !matched;
            row.channels.push(view.clone());
        }
    }

    let mut data: Vec<Value> = rows
        .iter()
        .map(|(mid, agg)| {
            let level_only = !agg.has_open;
            json!({
                "mid": mid,
                "model_id": mid_to_name.get(mid).cloned().unwrap_or_default(),
                "match": if level_only { "level" } else { "open" },
                "discount": discounts.get(mid).copied(),
                "channel_count": agg.channels.len(),
                "channels": agg.channels,
            })
        })
        .collect();

    data.sort_by(|a, b| {
        let rank = |v: &Value| if v["match"] == "level" { 0 } else { 1 };
        rank(a).cmp(&rank(b)).then_with(|| {
            b["channel_count"]
                .as_i64()
                .unwrap_or(0)
                .cmp(&a["channel_count"].as_i64().unwrap_or(0))
        }).then_with(|| a["model_id"].as_str().unwrap_or("").cmp(b["model_id"].as_str().unwrap_or("")))
    });

    let accessible = data.len();
    let level_only_count = data.iter().filter(|v| v["match"] == "level").count();
    let discounted = data.iter().filter(|v| v["discount"] != Value::Null).count();
    // 配了折扣但该等级没有任何渠道能挂上 -> 前端提示「配了也用不上」
    let unreachable: Vec<Value> = discounts
        .keys()
        .filter(|mid| !rows.contains_key(mid.as_str()))
        .map(|mid| {
            json!({
                "mid": mid,
                "model_id": mid_to_name.get(mid).cloned().unwrap_or_default(),
                "discount": discounts.get(mid).copied(),
            })
        })
        .collect();

    Ok(Json(json!({
        "user": {
            "id": id,
            "uid": uid,
            "username": username,
            "nickname": nickname,
            "level_id": level_id,
            "level_name": if level_name.is_empty() { Value::Null } else { json!(level_name) },
            "user_group": group_key,
        },
        "summary": {
            "total_active_models": all_mids.len(),
            "accessible": accessible,
            "level_only": level_only_count,
            "open": accessible - level_only_count,
            "discounted": discounted,
            "no_channel": all_mids.len() - accessible,
        },
        "data": data,
        "unreachable_discounts": unreachable,
    })))
}
