/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 素材上游「按渠道分组推导」的唯一判定处。
//!
//! 推导链：用户等级 → 可达渠道分组(`channels.user_groups`) → 分组凭证(`preset_id`)
//! → 素材绑定(`upstream_asset_bindings.channel_config_id`)。
//!
//! 之所以能省掉 `asset_binding_levels` 那条手工映射边，靠的是三条已确认的业务不变量：
//! 1. 素材上游必须与模型上游同源（同一等级不会「模型走 A、素材走 B」）；
//! 2. 一个凭证只允许一条启用中的素材绑定（由校验 B + 部分唯一索引硬保证）；
//! 3. 同一等级下每个视频模型只绑定一个渠道分组，不做主备/分流的多分组。
//!
//! 高可用分组不参与推导：前端保存时 `isHaGroup ⇒ preset_id = null`（Channels.tsx:759），
//! 运行时选渠也会把子配注入并清空 `preset_id`（router.rs apply_ha_sub），
//! 故 JOIN `b.channel_config_id = ch.preset_id` 对其天然无结果，无需特判。
//!
//! 候选数 > 1 一律由调用方判为错误并列出候选，绝不按 id/priority 猜一家：
//! 猜中的上游会把素材资产散落到非预期账号下，事后只能靠上游账单反查。

use crate::db::Database;
use crate::error::AppError;
use std::collections::HashMap;

/// 灰度开关（`relay_settings.asset_route_by_group`）。关：native.rs 只认等级映射，行为与改造前一致。
pub async fn by_group_enabled(db: &Database) -> bool {
    crate::relay::relay_settings::get_cached_asset_route_by_group(db).await
}

/// 一个「带启用素材绑定的分组」行：推导与校验的公共数据单元。
#[derive(Debug, Clone, sqlx::FromRow)]
struct GroupRow {
    channel_id: i64,
    channel_name: String,
    #[allow(dead_code)]
    preset_id: i64,
    binding_id: i64,
    binding_name: String,
    user_groups: String,
    exclude_user_groups: String,
}

/// 一次取出全部候选行：分组启用 + 凭证启用 + 该凭证上有启用中的绑定。
///
/// 不做额度/熔断/`priority` 判定：用户当月额度用完不该导致既有素材库查不出、删不掉。
/// 要切断某等级的素材上游，用显式的「停用分组」动作。
async fn binding_groups(db: &Database) -> Result<Vec<GroupRow>, AppError> {
    let sql = db.format_query(
        "SELECT ch.id AS channel_id, ch.name AS channel_name, ch.preset_id, \
                b.id AS binding_id, b.name AS binding_name, \
                ch.user_groups, ch.exclude_user_groups \
         FROM channels ch \
         JOIN upstream_asset_bindings b ON b.channel_config_id = ch.preset_id AND b.is_active = 1 \
         LEFT JOIN channel_configs c ON c.id = ch.preset_id \
         WHERE ch.status = 1 AND ch.preset_id IS NOT NULL \
           AND ch.provider_type <> 'high_availability_group' \
           AND COALESCE(c.status, 1) = 1",
    );
    sqlx::query_as::<_, GroupRow>(&sql)
        .fetch_all(&db.pool)
        .await
        .map_err(|e| AppError::Internal(format!("查询素材上游分组候选失败: {e}")))
}

/// 等级可达判定，口径与 `relay::router` 选渠一致：
/// `included` 为空 = 对所有等级开放（`user_groups = '[]'`）；黑名单命中则优先排除。
/// 命中值既可能是 `user_levels.group_key`，也可能是等级 id 字符串，故两者都传进来比。
fn level_reachable(keys: &[String], included: &[String], excluded: &[String]) -> bool {
    let open = included.is_empty() || included.iter().any(|v| keys.iter().any(|k| k == v));
    if !open {
        return false;
    }
    !excluded.iter().any(|v| keys.iter().any(|k| k == v))
}

/// 行级判定。脏 JSON 不视为「全开放」——`serde_json::from_str` 失败时按不覆盖处理，
/// 避免一处手工写坏的配置把素材范围放大到全部等级。
fn row_reachable(keys: &[String], row: &GroupRow) -> bool {
    let Ok(included) = serde_json::from_str::<Vec<String>>(&row.user_groups) else {
        return false;
    };
    let excluded: Vec<String> = serde_json::from_str(&row.exclude_user_groups).unwrap_or_default();
    level_reachable(keys, &included, &excluded)
}

/// 单条候选：某分组为某等级带进来的一条绑定。
#[derive(Debug, Clone)]
pub struct Candidate {
    pub binding_id: i64,
    pub binding_name: String,
    pub channel_id: i64,
    pub channel_name: String,
}

/// 推导结果。歧义与「推不出」必须分开：前者是配置坏了要人修，后者是回落旧链路的信号。
pub enum Outcome {
    /// 唯一命中
    Hit(Candidate),
    /// 多条不同绑定同时可达 —— 配置违反不变量
    Ambiguous(Vec<Candidate>),
    /// 该等级没有任何带绑定的可达分组
    Empty,
}

/// 按等级键（group_key + 等级 id 字符串）求候选，并按 `binding_id` 去重。
///
/// 去重维度是绑定而非分组：两个分组引用同一凭证时，候选仍是同一条绑定，
/// 不构成歧义（校验 B 已在配置侧阻断同凭证多绑定，此处不能因此报错）。
pub async fn resolve_by_group(
    db: &Database,
    keys: &[String],
) -> Result<Outcome, AppError> {
    let rows = binding_groups(db).await?;
    // 保留首个到达顺序（SQL 侧按 priority DESC, id 排序，仅作展示用，不参与消歧）
    let mut by_binding: HashMap<i64, Candidate> = HashMap::new();
    for r in rows.iter().filter(|r| row_reachable(keys, r)) {
        by_binding
            .entry(r.binding_id)
            .or_insert_with(|| Candidate {
                binding_id: r.binding_id,
                binding_name: r.binding_name.clone(),
                channel_id: r.channel_id,
                channel_name: r.channel_name.clone(),
            });
    }
    let mut cands: Vec<Candidate> = by_binding.into_values().collect();
    cands.sort_by_key(|c| c.binding_id);
    Ok(match cands.len() {
        0 => Outcome::Empty,
        1 => Outcome::Hit(cands.remove(0)),
        _ => Outcome::Ambiguous(cands),
    })
}

/// 候选清单的人读文案，供错误消息与日志复用。
pub fn describe(cands: &[Candidate]) -> String {
    cands
        .iter()
        .map(|c| format!("分组#{}「{}」→绑定#{}「{}」", c.channel_id, c.channel_name, c.binding_id, c.binding_name))
        .collect::<Vec<_>>()
        .join("；")
}

/// 视频侧回落：按本次请求实际落到的凭证反查绑定。
///
/// 转发规则里显式配置的 `upstream_asset_binding_id` 优先级更高，由调用方先判。
pub async fn binding_for_preset(db: &Database, preset_id: i64) -> Option<i64> {
    let sql = db.format_query(
        "SELECT b.id, b.name FROM upstream_asset_bindings b \
         LEFT JOIN channel_configs c ON c.id = b.channel_config_id \
         WHERE b.channel_config_id = ? AND b.is_active = 1 AND COALESCE(c.status, 1) = 1 \
         ORDER BY b.id",
    );
    let rows: Vec<(i64, String)> = sqlx::query_as(&sql)
        .bind(preset_id)
        .fetch_all(&db.pool)
        .await
        .unwrap_or_default();
    match rows.len() {
        0 => None,
        1 => Some(rows[0].0),
        n => {
            // 唯一索引建立后理论不可达；历史数据未清时按最小 id 走并显式告警，不静默
            tracing::warn!(
                "[UarRoute] 凭证#{} 上存在 {} 条启用绑定（{}），取最小 id；请停用多余绑定",
                preset_id,
                n,
                rows.iter()
                    .map(|(id, name)| format!("#{}「{}」", id, name))
                    .collect::<Vec<_>>()
                    .join("；")
            );
            Some(rows[0].0)
        }
    }
}

/// 某凭证上是否有启用中的绑定（校验 A 用来判断「本次分组改动会不会影响素材路由」）。
async fn binding_on_preset(db: &Database, preset_id: i64) -> Option<(i64, String)> {
    let sql = db.format_query(
        "SELECT id, name FROM upstream_asset_bindings \
         WHERE channel_config_id = ? AND is_active = 1 ORDER BY id LIMIT 1",
    );
    sqlx::query_as::<_, (i64, String)>(&sql)
        .bind(preset_id)
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten()
}

/// 校验 A：保存渠道分组时，检查这次改动会不会破坏「任一等级的候选绑定唯一」。
///
/// 返回文案列表（空 = 无问题）。调用方决定阻断还是告警。
/// 判定只针对「本次改动涉及的等级」，不去全库扫冲突，避免历史脏数据把无关分组也保存不了。
pub async fn check_group_invariant(
    db: &Database,
    self_channel_id: Option<i64>,
    preset_id: Option<i64>,
    user_groups: &[String],
    exclude_groups: &[String],
) -> Result<Vec<String>, AppError> {
    let Some(pid) = preset_id else {
        return Ok(Vec::new()); // 高可用分组 / 未选凭证：不参与素材推导
    };
    let Some((bid, bname)) = binding_on_preset(db, pid).await else {
        return Ok(Vec::new()); // 该凭证没挂素材绑定：纯模型分组，改它不影响素材
    };

    let mut out = Vec::new();

    // 规则 1：带素材绑定的分组不允许对所有等级开放。
    // '[]' 会让这条绑定服务全部等级，任何其它带绑定的分组都与之冲突，且冲突面随新增分组增长。
    if user_groups.is_empty() && exclude_groups.is_empty() {
        out.push(format!(
            "该分组对所有用户等级开放，但其凭证已挂素材绑定#{bid}「{bname}」；\
             素材上游会因此服务全部等级，之后再建任何带绑定的分组都会冲突。\
             请在【可访问用户等级】里显式指定等级。"
        ));
    }

    let others = binding_groups(db).await?;
    #[derive(sqlx::FromRow)]
    struct LevelRow {
        id: i64,
        group_key: String,
        name: String,
    }
    let sql = db.format_query("SELECT id, group_key, name FROM user_levels ORDER BY id");
    let levels: Vec<LevelRow> = sqlx::query_as::<_, LevelRow>(&sql)
        .fetch_all(&db.pool)
        .await
        .unwrap_or_default();

    for lv in &levels {
        let keys = [lv.group_key.clone(), lv.id.to_string()];
        // 本次保存后，该等级是否命中「这个分组」
        if !level_reachable(&keys, user_groups, exclude_groups) {
            continue;
        }
        let conflicting: Vec<&GroupRow> = others
            .iter()
            .filter(|o| Some(o.channel_id) != self_channel_id)
            .filter(|o| o.binding_id != bid) // 同一条绑定不算冲突
            .filter(|o| row_reachable(&keys, o))
            .collect();
        if conflicting.is_empty() {
            continue;
        }
        out.push(format!(
            "等级「{}」(#{}) 同时被本分组(绑定#{bid}「{bname}」)与 {} 覆盖，\
             素材请求将无法唯一确定上游；请只保留一个带素材绑定的分组覆盖该等级。",
            lv.name,
            lv.id,
            conflicting
                .iter()
                .map(|o| format!("分组#{}「{}」(绑定#{}「{}」)", o.channel_id, o.channel_name, o.binding_id, o.binding_name))
                .collect::<Vec<_>>()
                .join("、")
        ));
    }

    // 冲突面可能很大（16 个等级全中），截断避免响应里塞长文本
    out.truncate(8);
    Ok(out)
}

/// 校验 B：同一凭证是否已有别的启用绑定（创建/编辑绑定时阻断）。
pub async fn check_binding_unique_for_config(
    db: &Database,
    self_binding_id: Option<i64>,
    channel_config_id: i64,
) -> Result<Option<String>, AppError> {
    #[derive(sqlx::FromRow)]
    struct Other {
        id: i64,
        name: String,
    }
    let sql = db.format_query(
        "SELECT id, name FROM upstream_asset_bindings \
         WHERE channel_config_id = ? AND is_active = 1 AND id <> ? ORDER BY id",
    );
    let rows: Vec<Other> = sqlx::query_as::<_, Other>(&sql)
        .bind(channel_config_id)
        .bind(self_binding_id.unwrap_or(0))
        .fetch_all(&db.pool)
        .await
        .map_err(|e| AppError::Internal(format!("查询同凭证绑定失败: {e}")))?;
    Ok(rows.first().map(|o| {
        format!(
            "该上游凭证已存在启用中的素材绑定「{}」(#{})；一个凭证只允许一条启用绑定，\
             且素材上游必须与模型上游同源。如需替换请先停用或编辑原绑定。",
            o.name, o.id
        )
    }))
}
