/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! Shared proxy utilities — user context, billing, logging.
//! All relay handlers reuse these to avoid code duplication.

use super::router;
use crate::error::{AppError, AppResult};
use crate::models::ApiToken;
use crate::models::Channel;
use crate::AppState;
use regex::Regex;
use std::sync::Arc;

// ── User Context ────────────────────────────────────────────────

#[derive(Clone)]
pub struct UserContext {
    pub user_group: String,
    pub level_id: String,
    pub balance: f64,
    pub discount: f64,
    pub discount_type: i32,
    /// 用户模型单独折扣(JSON: {"mid": discount})，优先于等级折扣
    pub model_discounts: Option<String>,
}

impl UserContext {
    /// 仅折扣字段参与计费时的轻量上下文（异步任务结算等）
    pub fn from_discounts(
        discount: f64,
        discount_type: i32,
        model_discounts: Option<String>,
    ) -> Self {
        Self {
            user_group: String::new(),
            level_id: String::new(),
            balance: 0.0,
            discount,
            discount_type,
            model_discounts,
        }
    }
}

pub async fn get_user_context(state: &AppState, user_id: &str) -> AppResult<UserContext> {
    // 查询用户信息、等级折扣、折扣模式、模型单独折扣（用于计费时优先匹配用户模型折扣）
    let (g, l_id, b, gb, cl, d, dt, md): (String, i64, f64, f64, f64, f64, i32, Option<String>) = sqlx::query_as(
        &state.db.format_query(
            "SELECT u.user_group, COALESCE(ul.id, 0), u.balance, u.gift_balance, u.credit_limit, COALESCE(ul.discount, 1.0), COALESCE(ul.discount_type, 0), u.model_discounts \
             FROM users u LEFT JOIN user_levels ul ON u.user_group = ul.group_key \
             WHERE u.id = ?"
        )
    )
    .bind(user_id)
    .fetch_one(&state.db.pool)
    .await?;
    Ok(UserContext {
        user_group: g,
        level_id: l_id.to_string(),
        // 可用额 = 系统 + 赠送 + 信控；统一 6 位，避免浮点残渣误判为「有余额」
        balance: crate::money::round_money(b + gb + cl),
        discount: d,
        discount_type: dt,
        model_discounts: md,
    })
}

/// 统一折扣策略（MIN + MAX 两步）：
/// 1. 根据等级 discount_type 取对应折扣来源的最小值：
///    - discount_type = 1 (全站折扣): MIN(用户模型单独折扣, 全站折扣)
///    - discount_type = 2 (等级折扣): MIN(用户模型单独折扣, 用户等级折扣)
///    - discount_type = 0 (不选择/默认): MIN(用户模型单独折扣, 全站折扣, 用户等级折扣)
/// 2. 折扣限价约束：MAX(最低折扣, 模型限价)，保证折扣不低于限价
pub fn resolve_discount(
    db_model: Option<&crate::models::Model>,
    level_discount: f64,
    user_model_discount: Option<f64>,
    discount_type: i32,
) -> (f64, &'static str) {
    let mut min_discount = f64::MAX;
    let mut source = "等级折扣";

    // 1. 用户模型单独折扣
    if let Some(umd) = user_model_discount {
        min_discount = umd;
        source = "用户模型折扣";
    }

    // 2. 按 discount_type 比对等级折扣或全站折扣 (0=全取, 1=仅全站, 2=仅等级)
    if discount_type != 1 && level_discount < min_discount {
        min_discount = level_discount;
        source = "等级折扣";
    }

    if discount_type != 2 {
        if let Some(m) = db_model {
            if m.global_discount_enabled == 1 && m.global_discount < min_discount {
                min_discount = m.global_discount;
                source = "全站折扣";
            }
        }
    }

    // 兜底保护：若全站折扣未开启且无其他可用折扣，回退等级折扣
    if min_discount == f64::MAX {
        min_discount = level_discount;
        source = "等级折扣";
    }

    // 3. 模型折扣限价约束 MAX(最低折扣, 模型限价)
    if let Some(m) = db_model {
        if m.site_discount_enabled == 1 && min_discount < m.site_discount {
            return (m.site_discount, "折扣限价");
        }
    }

    (min_discount, source)
}

/// 从用户 model_discounts JSON 中提取指定模型(mid)的单独折扣
pub fn parse_user_model_discount(model_discounts: &Option<String>, mid: &str) -> Option<f64> {
    let json_str = model_discounts.as_ref()?;
    let map: std::collections::HashMap<String, f64> = serde_json::from_str(json_str).ok()?;
    map.get(mid).copied()
}

// ── Model Lookup (支持同名模型按类型区分) ────────────────────────

/// 轻量查询模型关联的计费规则详情结构体（完整 BillingRule 实体）。
pub async fn get_model_billing_rule(
    state: &AppState,
    model_id: &str,
    channel: Option<&crate::models::Channel>,
    db_model: Option<&crate::models::Model>,
) -> Option<crate::models::BillingRule> {
    let rule_id = if let Some(m) = db_model {
        m.billing_rule_id?
    } else {
        let model = find_active_model_exact(state, model_id, None, channel).await?;
        model.billing_rule_id?
    };
    let mut rule: crate::models::BillingRule = sqlx::query_as(
        &state
            .db
            .format_query("SELECT * FROM billing_rules WHERE id = ? AND is_active = 1"),
    )
    .bind(rule_id)
    .fetch_optional(&state.db.pool)
    .await
    .unwrap_or(None)?;

    // 请求开始时刻锁定峰谷倍率到 applied_multiplier（结算时不再按当前时钟重算）
    let default_site_tz = super::relay_settings::get_cached_site_timezone(&state.db).await;
    rule.lock_time_multiplier(&default_site_tz);

    Some(rule)
}

/// 按 model_id 查找活跃模型，可选传入 category 以区分同名但不同类型的模型。
/// category: Some("图片") / Some("视频") / Some("聊天") / None（不限类型）
pub async fn find_active_model_exact(
    state: &AppState,
    model_id: &str,
    category: Option<&str>,
    channel: Option<&crate::models::Channel>,
) -> Option<crate::models::Model> {
    // category 过滤条件（参数化绑定防止 SQL 注入）
    let cat_filter = if category.is_some() {
        " AND t.name = ?"
    } else {
        ""
    };

    // 1. 获取所有匹配的活跃模型候选（ORDER BY m.id 保证多候选时返回顺序确定性）
    let sql = format!(
        "SELECT m.*, t.name AS type_name FROM models m LEFT JOIN model_types t ON m.type_id = t.id WHERE m.model_id = ? AND m.is_active = 1{} ORDER BY m.id",
        cat_filter
    );
    let formatted_sql = state.db.format_query(&sql);
    let mut query = sqlx::query_as(&formatted_sql).bind(model_id);
    if let Some(cat) = category {
        query = query.bind(cat);
    }
    let mut candidates: Vec<crate::models::Model> =
        query.fetch_all(&state.db.pool).await.unwrap_or_default();

    if candidates.is_empty() && category.is_some() {
        let fallback_sql = "SELECT m.*, t.name AS type_name FROM models m LEFT JOIN model_types t ON m.type_id = t.id WHERE m.model_id = ? AND m.is_active = 1 ORDER BY m.id";
        candidates = sqlx::query_as(&state.db.format_query(fallback_sql))
            .bind(model_id)
            .fetch_all(&state.db.pool)
            .await
            .unwrap_or_default();
    }

    if candidates.is_empty() {
        return None;
    }

    // 2. 如果提供了渠道，且存在多个候选模型，尝试精确匹配渠道包含的 mid
    if let Some(ch) = channel {
        let ch_models = ch.get_models(); // 可能是 mid 数组，也可能是旧版的 model_id 数组
        if !ch_models.is_empty() {
            // 优先匹配 mid（精确锁定唯一模型记录及其关联计费规则）
            if let Some(exact) = candidates.iter().find(|m| ch_models.contains(&m.mid)) {
                return Some(exact.clone());
            }
            // 兜底匹配 model_id
            if let Some(exact) = candidates.iter().find(|m| ch_models.contains(&m.model_id)) {
                return Some(exact.clone());
            }
        }
    }

    // 3. 默认返回第一个（ORDER BY m.id 保证确定性）
    Some(candidates.into_iter().next().unwrap())
}

/// 根据 mid 查找处于激活状态的模型数据
pub async fn find_active_model_by_mid(state: &AppState, mid: &str) -> Option<crate::models::Model> {
    let sql = "SELECT m.*, t.name AS type_name FROM models m LEFT JOIN model_types t ON m.type_id = t.id WHERE m.mid = ? AND m.is_active = 1 LIMIT 1";
    sqlx::query_as(&state.db.format_query(sql))
        .bind(mid)
        .fetch_optional(&state.db.pool)
        .await
        .unwrap_or(None)
}

// ── Access Check ────────────────────────────────────────────────

/// 根据 category 推断标准 endpoint 路径（用于错误日志记录）
pub fn category_endpoint(category: Option<&str>) -> &'static str {
    match category {
        Some("图片") => "/v1/images/generations",
        Some("视频") | Some("视频增强") => "/v1/video/generations",
        Some("音频") => "/v1/audio/speech",
        Some("向量") => "/v1/embeddings",
        Some("排序") => "/v1/rerank",
        _ => "/v1/chat/completions",
    }
}

/// 入口类型与模型真实类型是否互通（目前仅视频 ↔ 视频增强）
#[inline]
fn category_compatible(expected: &str, resolved: &str) -> bool {
    expected == resolved
        || (expected == "视频" && resolved == "视频增强")
        || (expected == "视频增强" && resolved == "视频")
}

/// 类型隔离失败文案：真实类型 + 实际入口（action_type 另记入口，见 check_access）
#[inline]
fn type_mismatch_message(model: &str, resolved_cat: &str, expected_cat: &str) -> String {
    format!(
        "模型 '{}' 为 '{}' 类型，不支持当前 '{}' 接口请求",
        model, resolved_cat, expected_cat
    )
}

/// 路径→类型兜底（仅鉴权中间件 / 历史日志补全等「业务模块尚未透传」场景）。
/// 业务失败日志应优先透传模块已知的 category，勿依赖本函数。
pub fn action_type_from_path(endpoint: &str) -> Option<&'static str> {
    let ep = endpoint
        .split('|')
        .next()
        .unwrap_or(endpoint)
        .to_ascii_lowercase();
    // 更具体的规则在前
    const RULES: &[(&str, &str)] = &[
        ("enhance-video", "视频增强"),
        ("erase-video", "视频增强"),
        ("contents/generations", "视频"),
        ("video-generation", "视频"),
        ("video-synthesis", "视频"),
        ("/videos/", "视频"),
        ("/video/", "视频"),
        ("multimodal-generation", "图片"),
        ("/images/", "图片"),
        ("/image/", "图片"),
        ("/audio/", "音频"),
        ("/tts", "音频"),
        ("/speech", "音频"),
        ("embedding", "向量"),
        ("rerank", "排序"),
        ("/chat/", "聊天"),
        ("/messages", "聊天"),
        ("/responses", "聊天"),
        ("v1beta/models", "聊天"),
    ];
    RULES.iter().find(|(k, _)| ep.contains(k)).map(|(_, v)| *v)
}

/// Token 模型权限校验（渠道选择 **之前** 调用，快速拦截未授权模型）。
/// `action_type`: 调用方已知类别（如 "图片"），失败落库时透传，保证日志 Tab 定位正确。
pub async fn check_model_permission(
    state: &Arc<AppState>,
    token: &ApiToken,
    model: &str,
    endpoint: &str,
    action_type: Option<&str>,
) -> AppResult<()> {
    if !token.is_model_allowed(model) {
        let msg = format!("Model {} not allowed for this token", model);
        record_error_log(
            state,
            &token.user_id,
            None,
            Some(token.id),
            model,
            403,
            endpoint,
            &msg,
            None,
            action_type,
        )
        .await;
        return Err(AppError::Forbidden(msg));
    }

    Ok(())
}

/// 类型安全隔离 + 预扣费余额检查。
/// 调用方需在渠道选择 **之前** 自行执行 `check_model_permission()` 权限拦截，
/// 本函数只负责模型类别校验和预扣费，channel 用于精确匹配同名模型的预扣费金额。
/// 返回 `(pre_deduction, db_model)`：pre_deduction 为预扣费金额，db_model 为已查询的模型记录，
/// 调用方可将 db_model 传递给下游函数（如 resolve_forward_rule / record_pending_log）复用，避免重复查库。
pub async fn check_access(
    state: &Arc<AppState>,
    token: &ApiToken,
    model: &str,
    ctx: &UserContext,
    category: Option<&str>,
    channel: Option<&crate::models::Channel>,
) -> AppResult<(f64, Option<crate::models::Model>, String)> {
    check_access_with_model(state, token, model, ctx, category, channel, None).await
}

/// 支持透传预查模型实体的安全隔离扣费校验，规避 find_active_model_exact 内部的二次查表
pub async fn check_access_with_model(
    state: &Arc<AppState>,
    token: &ApiToken,
    model: &str,
    ctx: &UserContext,
    category: Option<&str>,
    channel: Option<&crate::models::Channel>,
    pre_fetched_model: Option<crate::models::Model>,
) -> AppResult<(f64, Option<crate::models::Model>, String)> {
    let db_model = if let Some(m) = pre_fetched_model {
        Some(m)
    } else {
        find_active_model_exact(state, model, category, channel).await
    };

    // 获取真实分类
    let resolved_cat = if let Some(ref m) = db_model {
        m.type_name
            .clone()
            .unwrap_or_else(|| category.unwrap_or("").to_string())
    } else {
        category.unwrap_or("").to_string()
    };

    let ep = category_endpoint(category);
    let ch_id = channel.map(|c| c.id);
    let up_url = channel.map(|c| c.base_url.as_str());

    // 类型安全隔离：action_type 记入口 expected（Tab=endpoint），文案带模型真实类型 resolved
    if let Some(expected_cat) = category {
        if db_model.is_some() && !category_compatible(expected_cat, &resolved_cat) {
            let msg = type_mismatch_message(model, &resolved_cat, expected_cat);
            record_error_log(
                state,
                &token.user_id,
                ch_id,
                Some(token.id),
                model,
                400,
                ep,
                &msg,
                up_url,
                Some(expected_cat),
            )
            .await;
            return Err(AppError::BadRequest(msg));
        }
    }

    let pre_deduction =
        crate::money::round_money(db_model.as_ref().map(|m| m.pre_deduction).unwrap_or(0.0));
    // ctx.balance = 系统+赠送+信控（get_user_context 已 round）
    let avail = ctx.balance;
    let insufficient = if pre_deduction > 0.0 {
        avail < pre_deduction
    } else {
        avail <= 0.0
    };
    if insufficient {
        let msg = if pre_deduction > 0.0 {
            let currency_unit = crate::api::settings::get_currency_settings(state)
                .await
                .currency_unit;
            format!("账户余额不足{}{}", pre_deduction, currency_unit)
        } else {
            "余额不足".to_string()
        };
        record_error_log(
            state,
            &token.user_id,
            ch_id,
            Some(token.id),
            model,
            402,
            ep,
            &msg,
            up_url,
            Some(&resolved_cat),
        )
        .await;
        return Err(AppError::PaymentRequired(msg));
    }

    // 低余额：限制未完成视频路数（金额门禁已过；不二次扣在途预扣）
    // 入口类别或模型真实类型任含「视频」即生效（含视频增强）
    let is_video = category.is_some_and(|c| c.contains("视频")) || resolved_cat.contains("视频");
    if is_video {
        if let Err(e) =
            super::relay_settings::enforce_video_inflight_gate(&state.db, &token.user_id, avail)
                .await
        {
            if let AppError::TooManyRequests(msg) = &e {
                record_error_log(
                    state,
                    &token.user_id,
                    ch_id,
                    Some(token.id),
                    model,
                    429,
                    ep,
                    msg,
                    up_url,
                    Some(&resolved_cat),
                )
                .await;
            }
            return Err(e);
        }
    }

    Ok((pre_deduction, db_model, resolved_cat))
}

// ── Channel Selection ───────────────────────────────────────────

pub async fn select_channel_for_model(
    state: &Arc<AppState>,
    token: &ApiToken,
    model: &str,
    user_group: &str,
    level_id: &str,
    endpoint: &str,
    exclude_aids: &[String],
    log_miss: bool,
    action_type: Option<&str>,
) -> AppResult<Channel> {
    select_channel_with_db(
        state,
        token,
        model,
        user_group,
        level_id,
        endpoint,
        None,
        exclude_aids,
        log_miss,
        action_type,
    )
    .await
}

/// 渠道选择（支持透传 Model 实体，提取其 mid 规避 select_channel 内部的查表动作）。
/// `log_miss`: 选渠失败时是否写入日志。failover 循环中若已有上游错误应传 false，终态为 No available channels 时传 true。
/// `action_type`: 调用方已知类别，选渠失败落库时透传。
pub async fn select_channel_with_db(
    state: &Arc<AppState>,
    token: &ApiToken,
    model: &str,
    user_group: &str,
    level_id: &str,
    endpoint: &str,
    db_model: Option<&crate::models::Model>,
    exclude_aids: &[String],
    log_miss: bool,
    action_type: Option<&str>,
) -> AppResult<Channel> {
    let mids = db_model.map(|m| vec![m.mid.clone()]);
    let (allow_ha, _) = super::ha::policy(state, token.high_availability).await;
    match router::select_channel(
        state,
        model,
        user_group,
        level_id,
        exclude_aids,
        mids.as_deref(),
        allow_ha,
    )
    .await
    {
        Ok(ch) => Ok(ch),
        Err(e) => {
            if log_miss {
                let msg = if let AppError::NotFound(ref m) = e {
                    m.clone()
                } else {
                    e.to_string()
                };
                record_error_log(
                    state,
                    &token.user_id,
                    None,
                    Some(token.id),
                    model,
                    404,
                    endpoint,
                    &msg,
                    None,
                    action_type,
                )
                .await;
            }
            Err(e)
        }
    }
}

// ── Record Usage & Billing ──────────────────────────────────────

use super::url_utils::join_url;

/// 事务化预扣费：FOR UPDATE 锁行防并发；有 pending 日志时同事务写入 cost/pre_deduct_gift，
/// 保证崩溃后孤儿清理/启动恢复可按日志退款（避免「钱包已扣、日志 cost=0」）。
/// 返回赠送钱包实扣金额（供 `pre_deduct_gift` 落库）。
pub async fn pre_deduct(
    state: &Arc<AppState>,
    user_id: &str,
    amount: f64,
    pending_log_id: Option<i64>,
) -> Result<f64, sqlx::Error> {
    if amount <= 0.0 {
        return Ok(0.0);
    }
    let amount = crate::money::round_money(amount);
    let mut tx = state.db.pool.begin().await?;
    let (bal, gift, credit): (f64, f64, f64) = sqlx::query_as(&state.db.format_query(
        "SELECT balance, gift_balance, credit_limit FROM users WHERE id = ? FOR UPDATE",
    ))
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;

    if crate::money::round_money(bal + gift + credit) < amount {
        tx.rollback().await?;
        return Err(sqlx::Error::RowNotFound);
    }
    let (gift_deducted, balance_deducted) = crate::money::split_gift_first(amount, gift);

    sqlx::query(&state.db.format_query(
        "UPDATE users SET balance = balance - ?, gift_balance = gift_balance - ? WHERE id = ?",
    ))
    .bind(balance_deducted)
    .bind(gift_deducted)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    // 写入预扣凭证；HA 失败复用同一 pending 时重置为处理中，供崩溃补偿/孤儿清理命中。
    // 冻结成功后不会再 pre_deduct，无需对冻结态做分支防护。
    if let Some(log_id) = pending_log_id {
        let touched = sqlx::query(&state.db.format_query(
            "UPDATE logs SET cost = ?, pre_deduct_gift = ?, status_code = 0, is_completed = 0, \
             error_message = NULL, billing_detail = '请求处理中' WHERE id = ?",
        ))
        .bind(amount)
        .bind(gift_deducted)
        .bind(log_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if touched == 0 {
            tx.rollback().await?;
            tracing::error!(
                "[PreDeduct] pending log {} 不存在，回滚预扣 user={}",
                log_id,
                user_id
            );
            return Err(sqlx::Error::Protocol(
                "pre_deduct: pending log missing".into(),
            ));
        }
    } else {
        tracing::warn!(
            "[PreDeduct] 无 pending_log_id，仅扣钱包无法崩溃退款 user={} amount={:.6}",
            user_id,
            amount
        );
    }

    tx.commit().await?;
    Ok(gift_deducted)
}

/// 预扣费；余额不足时写 402 日志并返回 PaymentRequired（金额≤0 跳过；角色无豁免）
pub async fn pre_deduct_or_intercept(
    state: &Arc<AppState>,
    token: &ApiToken,
    channel: &crate::models::Channel,
    model: &str,
    pre_deduction: f64,
    ep: &str,
    start_time: std::time::Instant,
    is_stream: i32,
    request_content_str: &str,
    upstream_body_str: &str,
    ep_tag: Option<String>,
    pending_log_id: Option<i64>,
    db_model: Option<&crate::models::Model>,
    category: Option<&str>,
) -> AppResult<f64> {
    if pre_deduction <= 0.0 {
        return Ok(0.0);
    }
    match pre_deduct(state, &token.user_id, pre_deduction, pending_log_id).await {
        Ok(gift) => Ok(gift),
        Err(e) => {
            let err_msg = match &e {
                sqlx::Error::RowNotFound => "余额不足".to_string(),
                sqlx::Error::Protocol(msg) if msg.contains("pending log missing") => {
                    "预扣记账失败，请重试".to_string()
                }
                _ => format!("预扣费失败: {:?}", e),
            };
            tracing::error!("[PreDeduct] 预扣费失败 用户ID={}: {:?}", token.user_id, e);
            let latency_ms = start_time.elapsed().as_millis() as u32;
            let is_balance = matches!(e, sqlx::Error::RowNotFound);
            let status_code = if is_balance { 402 } else { 500 };
            let _ = record_zero_cost_fail(ZeroCostUpstreamFail {
                state,
                token,
                channel,
                model,
                prefer_http_status: Some(status_code),
                endpoint: ep,
                latency_ms,
                is_stream,
                request_content: request_content_str.to_string(),
                response_body: err_msg.clone(),
                response_content: Some(err_msg.clone()),
                upstream_req_content: Some(upstream_body_str.to_string()),
                billing_detail: ep_tag,
                hint_category: category,
                pending_log_id,
                billing_model_hint: None,
                db_model,
                client_msg: Some(&err_msg),
                pre_deducted: 0.0,
                pre_deduct_gift: 0.0,
            })
            .await;
            Err(if is_balance {
                AppError::PaymentRequired("余额不足".to_string())
            } else {
                AppError::Internal(err_msg)
            })
        }
    }
}

// ── 预记录日志（请求前写入） ────────────────────────────────────
//
// 【一条日志原则】每个模型请求全生命周期只产生一条日志记录：
//   1. 请求发送前：调用 record_pending_log 插入 status_code=0 的"处理中"日志
//   2. 请求完成后：调用 record_and_bill* 系列函数时传入 pending_log_id，
//      通过 UPDATE 更新该日志行的最终状态、响应、计费等信息
//   3. 其他开发者/AI 在新增模型请求端点时必须遵循此原则，不得额外 INSERT 日志
//

/// Base64 数据脱敏：将请求/响应内容中的 base64 长串替换为占位符，减少日志体积。
/// 供预记录和最终记录共用，保证数据处理一致性。
pub fn sanitize_base64(text: &str) -> String {
    // 规则 1: data URI 格式 (data:image/png;base64,...)
    let re_data_uri = Regex::new(r"data:[^;]+;base64,[A-Za-z0-9+/=]{100,}").unwrap();
    let text = re_data_uri.replace_all(text, "base64数据").to_string();
    // 规则 2: 纯 base64 长串 (无 data: 前缀，如 b64_json / inline_data 字段)
    let re_raw_b64 = Regex::new(r#""[A-Za-z0-9+/]{200,}={0,2}""#).unwrap();
    re_raw_b64.replace_all(&text, "\"base64数据\"").to_string()
}

/// 预记录日志参数（命名字段，避免位置参数踩坑）
pub struct PendingLog<'a> {
    pub state: &'a Arc<AppState>,
    pub user_id: &'a str,
    pub token_id: i64,
    pub model: &'a str,
    pub endpoint: &'a str,
    pub is_stream: i32,
    pub request_content: Option<&'a str>,
    pub upstream_url: Option<&'a str>,
    pub channel: &'a crate::models::Channel,
    pub billing_model_hint: Option<&'a str>,
    pub plugin_tag: Option<&'a str>,
    pub category: Option<&'a str>,
    pub db_model: Option<&'a crate::models::Model>,
    pub forward_eid: Option<&'a str>,
    pub requested_log_id: Option<&'a str>,
}

/// 在上游请求发送前预记录一条"处理中"日志（status_code=0），返回 logs 主键 id。
/// 使用户能立即在日志页面看到请求记录，而不必等待上游响应。
/// 存入的信息包括：用户信息、渠道、模型、请求参数、端点、流式标志等。
/// 预记录阶段不存储 upstream_req_content（上游请求参数），因为此时请求尚未真正发送给上游，
/// 该字段在请求完成后由 record_and_bill_inner UPDATE 写入。
/// 预记录阶段即执行 URL 密钥脱敏、Base64 脱敏和上下文开关控制，与最终日志保持数据安全一致性。
pub async fn record_pending_log(p: PendingLog<'_>) -> Option<i64> {
    let PendingLog {
        state,
        user_id,
        token_id,
        model,
        endpoint,
        is_stream,
        request_content,
        upstream_url,
        channel,
        billing_model_hint,
        plugin_tag,
        category,
        db_model,
        forward_eid,
        requested_log_id,
    } = p;
    // 计费模型提示：插件（如快乐小马）解析后的实际模型，用于正确查询元信息
    let meta_model = billing_model_hint.unwrap_or(model);
    let (mut action_type, billing_pid, enable_log) =
        resolve_model_meta(state, meta_model, category, Some(channel), db_model).await;
    // 元信息未解析到类型时透传调用方 category（业务模块已知，无需再猜 endpoint）
    if action_type.is_empty() {
        if let Some(cat) = category.map(str::trim).filter(|c| !c.is_empty()) {
            action_type = cat.to_string();
        }
    }
    let log_id_prefix = if !action_type.is_empty() && action_type != "聊天" {
        "tsk_"
    } else {
        "log_"
    };
    let generated_log_id = requested_log_id.map(|s| s.to_string()).unwrap_or_else(|| {
        format!(
            "{}{}",
            log_id_prefix,
            ulid::Ulid::new().to_string().to_lowercase()
        )
    });
    let forward_eid: Option<String> = forward_eid.filter(|s| !s.is_empty()).map(|s| s.to_string());

    let channel_config_id = super::ha::resolve_log_config_id(state, channel).await;
    let is_ha = super::ha::channel_is_ha_flag(channel);
    let masked_url: Option<String> =
        upstream_url.map(|u| super::forward::mask_key_in_string(u, &channel.api_key));

    let stored_req: Option<String> = if enable_log > 0 {
        request_content.map(sanitize_base64)
    } else {
        None
    };

    let sql = state.db.format_query(
        "INSERT INTO logs (log_id, user_id, channel_id, token_id, model, prompt_tokens, completion_tokens, \
         cached_tokens, cost, status_code, endpoint, error_message, latency_ms, \
         request_content, response_content, is_stream, upstream_url, \
         billing_detail, task_id, action_type, billing_pid, forward_eid, plugin_tag, channel_config_id, is_ha) \
         VALUES (?, ?, ?, ?, ?, 0, 0, 0, 0.0, 0, ?, NULL, 0, ?, NULL, ?, ?, \
                 '请求处理中', '', ?, ?, ?, ?, ?, ?) RETURNING id"
    );

    let (sys_ep, _upstream_ep) = if endpoint.contains('|') {
        let parts: Vec<&str> = endpoint.splitn(2, '|').collect();
        (parts[0], parts[1])
    } else {
        (endpoint, endpoint)
    };

    let res = sqlx::query_scalar::<_, i64>(&sql)
        .bind(&generated_log_id)
        .bind(user_id)
        .bind(channel.id)
        .bind(token_id)
        .bind(model)
        .bind(sys_ep)
        .bind(stored_req.as_deref())
        .bind(is_stream)
        .bind(masked_url.as_deref())
        .bind(&action_type)
        .bind(&billing_pid)
        .bind(&forward_eid)
        .bind(plugin_tag.unwrap_or(""))
        .bind(channel_config_id)
        .bind(is_ha)
        .fetch_one(&state.db.pool)
        .await;

    match res {
        Ok(id) => {
            tracing::info!(
                "[PendingLog] ID={} 日志号={} 模型={} 端点={}",
                id,
                generated_log_id,
                model,
                sys_ep
            );
            Some(id)
        }
        Err(e) => {
            tracing::error!("[PendingLog] 预记录失败: {:?}", e);
            None
        }
    }
}

/// 从 models 表解析模型元信息（action_type / billing_pid / enable_log_content）
/// 供预记录和最终记录复用，避免代码重复。
/// channel: 可选渠道信息，用于同 model_id 多条记录时精确匹配渠道绑定的 mid，
///          确保 billing_pid 与实际计费规则一致。
/// db_model: 调用方已查询的 Model 记录（主键定位，极快且无歧义）。传 None 时按 model_id 查询。
/// forward_eid 不在此函数查询——各端点在 resolve_forward_rule 时已获取，直接透传给 record_pending_log。
async fn resolve_model_meta(
    state: &AppState,
    model_name: &str,
    hint_category: Option<&str>,
    channel: Option<&crate::models::Channel>,
    db_model: Option<&crate::models::Model>,
) -> (String, Option<String>, i32) {
    let mut action_type = String::new();
    let mut billing_pid: Option<String> = None;
    let mut enable_log: i32 = 0;

    // 统一 JOIN 查询，根据是否有 db_model 选择最优 WHERE 条件
    use sqlx::Row;
    let base_select = "SELECT m.mid, m.enable_log_content, \
         t.name as category_name, b.pid as billing_pid \
         FROM models m \
         LEFT JOIN model_types t ON m.type_id = t.id \
         LEFT JOIN billing_rules b ON m.billing_rule_id = b.id";

    let row = if let Some(m) = db_model {
        // 主键精确定位（一次查询、一行结果、无歧义）
        let sql = format!("{} WHERE m.id = ?", base_select);
        sqlx::query(&state.db.format_query(&sql))
            .bind(m.id)
            .fetch_optional(&state.db.pool)
            .await
            .unwrap_or(None)
    } else {
        // 按 model_id 查询 + 类别过滤 + 渠道精确匹配
        let cat_filter = if let Some(cat) = hint_category {
            format!(" AND t.name = '{}'", cat)
        } else {
            String::new()
        };
        let sql = format!(
            "{} WHERE m.model_id = ? AND m.is_active = 1{} ORDER BY m.id",
            base_select, cat_filter
        );
        let mut rows = sqlx::query(&state.db.format_query(&sql))
            .bind(model_name)
            .fetch_all(&state.db.pool)
            .await
            .unwrap_or_default();

        if rows.is_empty() && hint_category.is_some() {
            let fallback_sql = format!(
                "{} WHERE m.model_id = ? AND m.is_active = 1 ORDER BY m.id",
                base_select
            );
            rows = sqlx::query(&state.db.format_query(&fallback_sql))
                .bind(model_name)
                .fetch_all(&state.db.pool)
                .await
                .unwrap_or_default();
        }

        if rows.is_empty() {
            // 模型未入库时仍保留调用方类别提示，避免失败日志 action_type 为空
            if let Some(cat) = hint_category.filter(|c| !c.is_empty()) {
                action_type = cat.to_string();
            }
            return (action_type, billing_pid, enable_log);
        }

        // 优先通过渠道精确匹配，确保 billing_pid 与计费路径一致
        // 渠道 models 字段可能是 mid 或 model_id 格式，两种均尝试匹配
        let target_row = if let Some(ch) = channel {
            let ch_models = ch.get_models();
            if !ch_models.is_empty() {
                rows.iter()
                    .position(|r| {
                        let mid: String = r.try_get("mid").unwrap_or_default();
                        ch_models.contains(&mid)
                    })
                    .or_else(|| {
                        rows.iter()
                            .position(|_| ch_models.contains(&model_name.to_string()))
                    })
            } else {
                None
            }
        } else {
            None
        };
        let idx = target_row.unwrap_or(0);
        Some(rows.into_iter().nth(idx).unwrap())
    };

    let row = match row {
        Some(r) => r,
        None => {
            if let Some(cat) = hint_category.filter(|c| !c.is_empty()) {
                action_type = cat.to_string();
            }
            return (action_type, billing_pid, enable_log);
        }
    };

    action_type = row.try_get("category_name").unwrap_or_default();
    if action_type.is_empty() {
        if let Some(cat) = hint_category.filter(|c| !c.is_empty()) {
            action_type = cat.to_string();
        }
    }
    billing_pid = row.try_get("billing_pid").unwrap_or(None);
    enable_log = row.try_get("enable_log_content").unwrap_or(0);

    tracing::info!(
        "[ModelMeta] 模型={} 类别={} PID={} 日志内容开关={} 来源={}",
        model_name,
        action_type,
        billing_pid.as_deref().unwrap_or("-"),
        enable_log,
        if db_model.is_some() { "pk" } else { "query" }
    );

    (action_type, billing_pid, enable_log)
}

pub async fn record_error_log(
    state: &Arc<AppState>,
    user_id: &str,
    channel_id: Option<i64>,
    token_id: Option<i64>,
    model: &str,
    status_code: u16,
    endpoint: &str,
    error_msg: &str,
    upstream_url: Option<&str>,
    action_type: Option<&str>,
) {
    let db_error_msg = extract_error_message(error_msg);
    // 优先用调用方透传的类型；仅未透传时（鉴权中间件）才按路径兜底
    let resolved_type = action_type
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| action_type_from_path(endpoint).map(|s| s.to_string()))
        .unwrap_or_default();
    let sql = state.db.format_query(
        "INSERT INTO logs (log_id, user_id, channel_id, token_id, model, prompt_tokens, completion_tokens, cached_tokens, cost, status_code, endpoint, error_message, latency_ms, request_content, response_content, is_stream, upstream_url, action_type, is_completed) VALUES (?, ?, ?, ?, ?, 0, 0, 0, 0.0, ?, ?, ?, 0, NULL, NULL, 0, ?, ?, 1)"
    );
    let cid = channel_id.unwrap_or(0);
    let tid = token_id.unwrap_or(0);
    let log_prefix = if !resolved_type.is_empty() && resolved_type != "聊天" {
        "tsk_"
    } else {
        "log_"
    };
    let error_log_id = format!(
        "{}{}",
        log_prefix,
        ulid::Ulid::new().to_string().to_lowercase()
    );

    let res = sqlx::query(&sql)
        .bind(&error_log_id)
        .bind(user_id)
        .bind(cid)
        .bind(tid)
        .bind(model)
        .bind(status_code as i32)
        .bind(endpoint)
        .bind(&db_error_msg)
        .bind(upstream_url.unwrap_or(""))
        .bind(&resolved_type)
        .execute(&state.db.pool)
        .await;

    if let Err(e) = res {
        tracing::error!("[ErrorLog] 记录错误日志失败: {:?}", e);
    }
}

/// 最终记账/更新日志参数
pub struct BillRecord<'a> {
    pub state: &'a Arc<AppState>,
    pub token: &'a ApiToken,
    pub channel: &'a crate::models::Channel,
    pub model: &'a str,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub cached_tokens: i32,
    pub cost: f64,
    pub pre_deducted: f64,
    pub pre_deduct_gift: f64,
    pub status_code: u16,
    pub endpoint: &'a str,
    pub error_msg: Option<&'a str>,
    pub latency_ms: u32,
    pub is_stream: i32,
    pub request_content: Option<String>,
    pub response_content: Option<String>,
    pub upstream_req_content: Option<String>,
    pub billing_detail: Option<String>,
    pub hint_category: Option<&'a str>,
    pub pending_log_id: Option<i64>,
    pub billing_model_hint: Option<&'a str>,
    pub plugin_tag: Option<&'a str>,
    pub db_model: Option<&'a crate::models::Model>,
    /// 请求开始已锁定的峰谷倍率；`None` 时由本函数按当前规则快照写入 billing_features
    pub time_multiplier: Option<f64>,
}

/// 计费记录统一入口
/// 【一条日志原则】pending_log_id 有值时 UPDATE 预记录行，无值时 INSERT 新行
/// billing_model_hint: 插件解析后的实际模型（用于正确查询 billing_pid 等元信息），普通场景传 None
/// plugin_tag: INSERT 时写入；UPDATE 时非空则覆盖（级联需回写 s1_task_id）
/// db_model: 调用方已查询的 Model 记录，传入后 resolve_model_meta 走主键精确定位，避免重复查库
/// channel: 已水合渠道（含最终 base_url/api_key/yid），禁止再查空父行覆盖
pub async fn record_and_bill_inner(p: BillRecord<'_>) {
    let BillRecord {
        state,
        token,
        channel,
        model: model_name,
        prompt_tokens,
        completion_tokens,
        cached_tokens,
        cost,
        pre_deducted,
        pre_deduct_gift,
        status_code,
        endpoint,
        error_msg,
        latency_ms,
        is_stream,
        request_content,
        response_content,
        upstream_req_content,
        billing_detail,
        hint_category,
        pending_log_id,
        billing_model_hint,
        plugin_tag,
        db_model,
        time_multiplier: bill_time_multiplier,
    } = p;
    let pre_deducted = crate::money::round_money(pre_deducted);
    let pre_deduct_gift = crate::money::round_money(pre_deduct_gift);
    // 实时 TPM 观测（与计费路径同点，零写库）
    let live_total_tokens =
        (prompt_tokens.max(0) as u64).saturating_add(completion_tokens.max(0) as u64);
    crate::middleware::live_metrics::record_tokens(&token.user_id, token.id, live_total_tokens);

    let extracted_error_msg = error_msg.map(|msg| extract_error_message(msg));
    let db_error_msg = extracted_error_msg.as_deref();
    let channel_id = channel.id;

    let meta_model = billing_model_hint.unwrap_or(model_name);
    let (category, billing_pid, enable_log) =
        resolve_model_meta(state, meta_model, hint_category, Some(channel), db_model).await;

    // HA: group_aid；物理: preset_id；内存 yid 补全 config_id
    let channel_config_id = super::ha::resolve_log_config_id(state, channel).await;
    let is_ha = super::ha::channel_is_ha_flag(channel);

    let filter_content = |content: Option<String>, respect_log_flag: bool| -> Option<String> {
        let text = content?;
        if respect_log_flag && enable_log == 0 {
            return None;
        }
        Some(sanitize_base64(&text))
    };

    // ── 计费特征快照 ──
    let billing_features_json: Option<String> = {
        let mut feat = request_content
            .as_ref()
            .and_then(|rc| serde_json::from_str::<serde_json::Value>(rc).ok())
            .map(|json| crate::relay::usage_extractor::extract_request_features(&json));
        if let Some(upstream_feat) = upstream_req_content
            .as_ref()
            .and_then(|uc| serde_json::from_str::<serde_json::Value>(uc).ok())
            .map(|json| crate::relay::usage_extractor::extract_request_features(&json))
        {
            if let Some(ref mut f) = feat {
                f.merge(upstream_feat);
            } else {
                feat = Some(upstream_feat);
            }
        }
        if let Some(ref resp) = response_content {
            if let Ok(resp_json) = serde_json::from_str::<serde_json::Value>(resp) {
                let resp_feat = crate::relay::usage_extractor::extract_request_features(&resp_json);
                if let Some(ref mut f) = feat {
                    f.merge(resp_feat);
                } else {
                    feat = Some(resp_feat);
                }
            }
            let usage = crate::relay::usage_extractor::parse_usage(resp);
            crate::relay::usage_extractor::enrich_features_from_usage(
                feat.get_or_insert_with(Default::default),
                &usage,
            );
        }
        // 级联：从 plugin_tag.cascade 补 version/resolution 到计费特征（不改用户入参）
        if let Some(tag) = plugin_tag {
            if let Some(ver) = crate::relay::cascade::cascade_json_str(tag, "/cascade/version") {
                feat.get_or_insert_with(Default::default).version = Some(ver);
            }
            if let Some(res) = crate::relay::cascade::cascade_json_str(tag, "/cascade/resolution") {
                feat.get_or_insert_with(Default::default).resolution = Some(res);
            }
        }
        // 峰谷倍率快照：优先用调用方传入的请求开始锁定值；否则按当前规则锁定写入
        if feat.as_ref().and_then(|f| f.time_multiplier).is_none() {
            let locked = if let Some(tm) = bill_time_multiplier {
                Some(tm)
            } else {
                get_model_billing_rule(state, model_name, Some(channel), db_model)
                    .await
                    .map(|r| r.applied_multiplier)
            };
            if let Some(tm) = locked {
                feat.get_or_insert_with(Default::default).time_multiplier = Some(tm);
            }
        }
        feat.and_then(|f| serde_json::to_string(&f).ok())
    };

    let req_content = filter_content(request_content, true);
    let upstream_req = filter_content(upstream_req_content, true);

    let resp_content = if enable_log == 0 {
        if category == "视频" || category == "图片" {
            filter_content(response_content, false)
        } else {
            if let Some(ref text) = response_content {
                let usage_json = crate::relay::usage_extractor::extract_usage_json_string(text);
                if usage_json.is_some() {
                    usage_json
                } else if category == "聊天" || category == "文本" {
                    Some("[]".to_string())
                } else {
                    filter_content(Some(text.clone()), false)
                }
            } else {
                None
            }
        }
    } else {
        filter_content(response_content, false)
    };

    // 直接复用已水合 Channel 的 base_url/api_key（含 HA 子配 / preset / volc 覆盖）
    let (system_endpoint, upstream_ep) = if endpoint.contains('|') {
        let parts: Vec<&str> = endpoint.splitn(2, '|').collect();
        (parts[0], parts[1])
    } else {
        (endpoint, endpoint)
    };

    let mut final_endpoint = upstream_ep.to_string();
    if !final_endpoint.starts_with("http") && !channel.base_url.is_empty() {
        final_endpoint = join_url(&channel.base_url, &final_endpoint);
    }
    if !channel.api_key.is_empty() {
        final_endpoint = super::forward::mask_key_in_string(&final_endpoint, &channel.api_key);
    }
    if !final_endpoint.starts_with("http") {
        if let Some(log_id) = pending_log_id {
            if let Ok(Some(prev)) = sqlx::query_scalar::<_, String>(
                &state
                    .db
                    .format_query("SELECT COALESCE(upstream_url, '') FROM logs WHERE id = ?"),
            )
            .bind(log_id)
            .fetch_optional(&state.db.pool)
            .await
            {
                if prev.starts_with("http") {
                    final_endpoint = prev;
                }
            }
        }
    }

    let res: Result<(), sqlx::Error> = async {
        let mut tx = state.db.pool.begin().await?;
        // 从响应体自动提取异步任务 ID（复用 response_formatter::extract_async_task_id 统一逻辑）
        // 提前解析提取，用于判断当前是否为异步任务预扣冻结阶段
        let task_id = resp_content
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .map(|v| super::response_formatter::extract_async_task_id(&v))
            .unwrap_or_default();

        // 异步任务预扣冻结判定：任务 ID 非空且计费详情中包含“冻结”
        let is_freeze = !task_id.is_empty() && billing_detail.as_deref().map_or(false, |d| d.contains("冻结"));

        // 始终更新令牌最后使用时间
        sqlx::query(&state.db.format_query(
            "UPDATE api_tokens SET last_used_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = ?"
        ))
        .bind(token.id)
        .execute(&mut *tx)
        .await?;

        let (settled_cost, apply_balance) = crate::money::settlement_delta(cost, pre_deducted);
        if settled_cost > 0.0 || pre_deducted > 0.0 {
            let site_tz = crate::relay::relay_settings::get_cached_site_timezone(&state.db).await;
            let tz = crate::api::date_helper::resolve_user_timedisplay_name(
                &state.db,
                &token.user_id,
                &site_tz,
            )
            .await;

            if settled_cost > 0.0 {
                let _added = super::token_quota::consume_async_or_sync(
                    state,
                    &mut tx,
                    token,
                    settled_cost,
                    &tz,
                )
                .await?;
            }

            if apply_balance > 0.0 {
                sqlx::query(&state.db.format_query(
                    "UPDATE users SET
                     balance = CASE WHEN gift_balance >= ? THEN balance ELSE balance - (? - gift_balance) END,
                     gift_used_quota = gift_used_quota + ? + CASE WHEN gift_balance >= ? THEN ? ELSE gift_balance END,
                     gift_balance = CASE WHEN gift_balance >= ? THEN gift_balance - ? ELSE 0 END,
                     used_quota = used_quota + ?,
                     updated_at = CURRENT_TIMESTAMP
                     WHERE id = ?",
                ))
                .bind(apply_balance).bind(apply_balance)
                .bind(pre_deduct_gift).bind(apply_balance).bind(apply_balance)
                .bind(apply_balance).bind(apply_balance).bind(settled_cost).bind(&token.user_id)
                .execute(&mut *tx)
                .await?;
            } else if apply_balance < 0.0 {
                let refund = -apply_balance;
                let gift_cost = settled_cost.min(pre_deduct_gift);
                let gift_refund = pre_deduct_gift - gift_cost;
                let balance_refund = refund - gift_refund;
                sqlx::query(&state.db.format_query(
                    "UPDATE users SET balance = balance + ?, gift_balance = gift_balance + ?, used_quota = used_quota + ?, gift_used_quota = gift_used_quota + ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                ))
                .bind(balance_refund)
                .bind(gift_refund)
                .bind(settled_cost)
                .bind(gift_cost)
                .bind(&token.user_id)
                .execute(&mut *tx)
                .await?;
            } else {
                // apply==0：冻结阶段不累加 used_quota；同步精确匹配则累加
                let (add_used, add_gift) = if is_freeze {
                    (0.0, 0.0)
                } else {
                    (settled_cost, settled_cost.min(pre_deduct_gift))
                };
                sqlx::query(&state.db.format_query(
                    "UPDATE users SET used_quota = used_quota + ?, gift_used_quota = gift_used_quota + ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                ))
                .bind(add_used)
                .bind(add_gift)
                .bind(&token.user_id)
                .execute(&mut *tx)
                .await?;
            }

            if channel_id > 0 && settled_cost > 0.0 {
                super::channel_quota::consume_channel(
                    &state.db, &mut tx, channel_id, settled_cost, &site_tz,
                )
                .await?;
            }
            if let Some(cfg_id) = channel_config_id {
                if cfg_id > 0 && settled_cost > 0.0 {
                    super::channel_quota::consume_config(
                        &state.db, &mut tx, cfg_id as i64, settled_cost, &site_tz,
                    )
                    .await?;
                }
            }
        }

        let db_post_response = if is_freeze {
            resp_content.clone()
        } else {
            None
        };

        let final_action_type = if !category.is_empty() {
            category.clone()
        } else {
            hint_category.unwrap_or("").to_string()
        };

        // 【一条日志原则】有 pending_log_id 时 UPDATE 预记录行，否则 INSERT 新行
        // 成功写成功子渠；HA 全失败由 ha.fail 按首败一次落库
        // CAS status_code=0：避免孤儿清理/启动恢复已关单后退款后又被结算扣费覆盖
        if let Some(log_id) = pending_log_id {
            let touched = sqlx::query(&state.db.format_query(
                "UPDATE logs SET channel_id = ?, model = ?, \
                 prompt_tokens = ?, completion_tokens = ?, cached_tokens = ?, \
                 cost = ?, status_code = ?, endpoint = ?, error_message = ?, latency_ms = ?, \
                 request_content = ?, response_content = ?, post_response = ?, upstream_url = ?, \
                 upstream_req_content = ?, billing_detail = ?, \
                 task_id = CASE WHEN ? = '' OR ? IS NULL THEN task_id ELSE ? END, \
                 action_type = ?, billing_pid = ?, \
                 billing_features = ?, pre_deduct_gift = ?, is_completed = ?, \
                 channel_config_id = ?, is_ha = ?, \
                 plugin_tag = CASE WHEN ? = '' THEN plugin_tag ELSE ? END \
                 WHERE id = ? AND status_code = 0",
            ))
            .bind(channel_id)
            .bind(model_name)
            .bind(prompt_tokens)
            .bind(completion_tokens)
            .bind(cached_tokens)
            .bind(settled_cost)
            .bind(status_code as i32)
            .bind(system_endpoint)
            .bind(db_error_msg)
            .bind(latency_ms as i32)
            .bind(&req_content)
            .bind(&resp_content)
            .bind(&db_post_response)
            .bind(&final_endpoint)
            .bind(&upstream_req)
            .bind(&billing_detail)
            .bind(&task_id).bind(&task_id).bind(&task_id)
            .bind(&final_action_type)
            .bind(&billing_pid)
            .bind(&billing_features_json)
            .bind(pre_deduct_gift)
            .bind(if is_freeze { 0i16 } else { 1i16 })  // is_completed: 冻结任务=0(待结算), 同步请求=1(已完成)
            .bind(channel_config_id)
            .bind(is_ha)
            .bind(plugin_tag.unwrap_or(""))
            .bind(plugin_tag.unwrap_or(""))
            .bind(log_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if touched == 0 {
                // 已被孤儿清理/启动恢复/并发结案；勿与预扣余额不足的 RowNotFound 混淆
                return Err(sqlx::Error::Protocol("pending_cas_miss".into()));
            }
        } else {
            let fb_prefix = if !final_action_type.is_empty() && final_action_type != "聊天" { "tsk_" } else { "log_" };
            let fallback_log_id = format!("{}{}", fb_prefix, ulid::Ulid::new().to_string().to_lowercase());
            sqlx::query(&state.db.format_query(
                "INSERT INTO logs (log_id, user_id, channel_id, token_id, model, prompt_tokens, completion_tokens, cached_tokens, cost, status_code, endpoint, error_message, latency_ms, request_content, response_content, post_response, is_stream, upstream_url, upstream_req_content, billing_detail, task_id, action_type, billing_pid, forward_eid, billing_features, pre_deduct_gift, plugin_tag, is_completed, channel_config_id, is_ha) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            ))
            .bind(&fallback_log_id)
            .bind(&token.user_id)
            .bind(channel_id)
            .bind(token.id)
            .bind(model_name)
            .bind(prompt_tokens)
            .bind(completion_tokens)
            .bind(cached_tokens)
            .bind(settled_cost)
            .bind(status_code as i32)
            .bind(system_endpoint)
            .bind(db_error_msg)
            .bind(latency_ms as i32)
            .bind(&req_content)
            .bind(&resp_content)
            .bind(&db_post_response)
            .bind(is_stream)
            .bind(&final_endpoint)
            .bind(&upstream_req)
            .bind(&billing_detail)
            .bind(&task_id)
            .bind(&final_action_type)
            .bind(&billing_pid)
            .bind::<Option<String>>(None)  // forward_eid: 预记录阶段已写入，无预记录时留空
            .bind(&billing_features_json)
            .bind(pre_deduct_gift)
            .bind(plugin_tag.unwrap_or(""))
            .bind(if is_freeze { 0i16 } else { 1i16 })  // is_completed: 冻结任务=0(待结算), 同步请求=1(已完成)
            .bind(channel_config_id)
            .bind(is_ha)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    .await;
    if let Err(e) = res {
        tracing::error!("[RelayUsage] 记录使用日志失败: {:?}", e);
        // 结算事务失败：若已预扣且日志仍为处理中，立即 CAS 退款（不必等孤儿任务）
        // CAS 未命中说明他处已关单/退款，禁止再补偿
        let cas_miss = matches!(&e, sqlx::Error::Protocol(m) if m == "pending_cas_miss");
        if pre_deducted > 0.0 && !cas_miss {
            const DETAIL: &str = "计费落库失败，预扣费已退回";
            if let Some(log_id) = pending_log_id {
                let _ = close_pending_and_refund(state, log_id, 500, DETAIL, DETAIL).await;
            } else if let Err(e) = refund_wallet_sql(
                &state.db,
                &state.db.pool,
                &token.user_id,
                pre_deducted,
                pre_deduct_gift,
            )
            .await
            {
                tracing::error!(
                    "[BillCompensate] 无 pending 日志钱包退款失败 user={} amount={:.6}: {:?}",
                    token.user_id,
                    pre_deducted,
                    e
                );
            }
        }
    } else if crate::money::round_money(cost) > 0.0 {
        // 异步检查低余额提醒，不阻塞计费路径
        crate::services::notification::spawn_low_balance_check(
            Arc::clone(state),
            token.user_id.clone(),
        );
    }
}

/// 锁定仍为 pending 的日志预扣凭证；无行则 Ok(None)。
async fn lock_pending_prepay(
    db: &crate::db::Database,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    log_id: i64,
) -> Result<Option<(String, f64, f64)>, sqlx::Error> {
    sqlx::query_as(&db.format_query(
        "SELECT user_id, cost, pre_deduct_gift FROM logs \
         WHERE id = ? AND status_code = 0 FOR UPDATE",
    ))
    .bind(log_id)
    .fetch_optional(&mut **tx)
    .await
}

/// `close=None`：仅退预扣并清零 cost，保持 pending；`close=Some`：关单并退预扣。
async fn settle_pending_prepay(
    state: &Arc<AppState>,
    log_id: i64,
    close: Option<(i32, &str, &str)>,
) -> bool {
    let tag = if close.is_some() {
        "PendingClose"
    } else {
        "HA"
    };
    let mut tx = match state.db.pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!("[{tag}] 开启事务失败 日志ID={}: {:?}", log_id, e);
            return false;
        }
    };

    let row = match lock_pending_prepay(&state.db, &mut tx, log_id).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("[{tag}] 锁定日志失败 日志ID={}: {:?}", log_id, e);
            let _ = tx.rollback().await;
            return false;
        }
    };
    let Some((user_id, cost, pre_deduct_gift)) = row else {
        let _ = tx.rollback().await;
        return false;
    };
    let cost = crate::money::round_money(cost);
    let pre_deduct_gift = crate::money::round_money(pre_deduct_gift);

    if close.is_none() && cost <= 0.0 && pre_deduct_gift <= 0.0 {
        let _ = tx.rollback().await;
        return false;
    }

    let upd = match close {
        Some((status_code, error_message, billing_detail)) => sqlx::query(&state.db.format_query(
            "UPDATE logs SET status_code = ?, cost = 0.0, pre_deduct_gift = 0.0, \
             error_message = ?, billing_detail = ?, is_completed = 1 \
             WHERE id = ? AND status_code = 0",
        ))
        .bind(status_code)
        .bind(error_message)
        .bind(billing_detail)
        .bind(log_id)
        .execute(&mut *tx)
        .await,
        None => sqlx::query(&state.db.format_query(
            "UPDATE logs SET cost = 0.0, pre_deduct_gift = 0.0 WHERE id = ? AND status_code = 0",
        ))
        .bind(log_id)
        .execute(&mut *tx)
        .await,
    };
    if let Err(e) = upd {
        tracing::error!("[{tag}] 更新日志失败 日志ID={}: {:?}", log_id, e);
        let _ = tx.rollback().await;
        return false;
    }

    if let Err(e) = refund_wallet_sql(&state.db, &mut *tx, &user_id, cost, pre_deduct_gift).await {
        tracing::error!(
            "[{tag}] 退款失败 用户ID={} 日志ID={}: {:?}",
            user_id,
            log_id,
            e
        );
        let _ = tx.rollback().await;
        return false;
    }

    if let Err(e) = tx.commit().await {
        tracing::error!("[{tag}] 提交事务失败 日志ID={}: {:?}", log_id, e);
        return false;
    }

    if let Some((status_code, _, _)) = close {
        tracing::info!(
            "[PendingClose] 已关闭 日志ID={} 状态码={} 用户ID={} 金额={:.6} 赠送={:.6}",
            log_id,
            status_code,
            user_id,
            cost,
            pre_deduct_gift
        );
    } else {
        tracing::info!(
            "[HA] 续试退预扣 日志ID={} 金额={:.6} 赠送={:.6}",
            log_id,
            cost,
            pre_deduct_gift
        );
    }
    true
}

/// CAS 关闭 status_code=0 的 pending：同行锁读 cost/pre_deduct_gift 后退双钱包。
async fn close_pending_and_refund(
    state: &Arc<AppState>,
    log_id: i64,
    status_code: i32,
    error_message: &str,
    billing_detail: &str,
) -> bool {
    settle_pending_prepay(
        state,
        log_id,
        Some((status_code, error_message, billing_detail)),
    )
    .await
}

async fn refund_wallet_sql<'e, E>(
    db: &crate::db::Database,
    executor: E,
    user_id: &str,
    cost: f64,
    pre_deduct_gift: f64,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    if cost <= 0.0 && pre_deduct_gift <= 0.0 {
        return Ok(());
    }
    // 赠送退回不超过 cost，避免脏数据导致 balance 被扣成负向
    let cost = crate::money::round_money(cost.max(0.0));
    let gift_refund = crate::money::round_money(pre_deduct_gift.min(cost).max(0.0));
    let balance_refund = crate::money::round_money(cost - gift_refund);
    sqlx::query(&db.format_query(
        "UPDATE users SET balance = balance + ?, gift_balance = gift_balance + ?, \
         updated_at = CURRENT_TIMESTAMP WHERE id = ?",
    ))
    .bind(balance_refund)
    .bind(gift_refund)
    .bind(user_id)
    .execute(executor)
    .await?;
    Ok(())
}

/// 清理孤儿预记录日志（status_code=0 且超过 30 分钟）
pub async fn cleanup_orphan_pending_logs(state: &Arc<AppState>) {
    let orphans: Vec<i64> = match sqlx::query_scalar(&state.db.format_query(
        "SELECT id FROM logs \
             WHERE is_completed = 0 AND status_code = 0 \
             AND created_at < CURRENT_TIMESTAMP - INTERVAL '30 minutes'",
    ))
    .fetch_all(&state.db.pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("[OrphanCleanup] 查询孤儿日志失败: {:?}", e);
            return;
        }
    };
    if orphans.is_empty() {
        return;
    }
    tracing::info!(
        "[OrphanCleanup] 发现 {} 条孤儿日志，开始清理",
        orphans.len()
    );
    for log_id in orphans {
        let _ = close_pending_and_refund(
            state,
            log_id,
            408,
            "请求处理超时或连接中断",
            "孤儿日志清理，预扣费已退回",
        )
        .await;
    }
}

/// 启动时恢复中断遗留的处理中日志（异步冻结为 status=200，不会命中）
pub async fn recover_interrupted_logs(state: &Arc<AppState>) {
    let orphans: Vec<i64> = match sqlx::query_scalar(&state.db.format_query(
        "SELECT id FROM logs WHERE is_completed = 0 AND status_code = 0",
    ))
    .fetch_all(&state.db.pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("[StartupRecover] 查询中断日志失败: {:?}", e);
            return;
        }
    };
    if orphans.is_empty() {
        return;
    }
    tracing::info!(
        "[StartupRecover] 发现 {} 条上次中断遗留的处理中日志",
        orphans.len()
    );
    for log_id in orphans {
        let _ = close_pending_and_refund(
            state,
            log_id,
            503,
            "服务升级重启，请求被中断",
            "服务升级中断，预扣费已退回",
        )
        .await;
    }
}

/// 错误信息敏感词脱敏：URL 域名替换为 ***（保留协议和路径），密钥替换为 ***。
/// 仅用于普通用户端返回和日志展示，管理员端保留原始信息用于排查。
pub fn sanitize_error_message(msg: &str) -> String {
    // URL 域名脱敏：https://api.example.com/v1/... → https://***/v1/...
    let re_url = Regex::new(r"(https?://)([^/\s)\]},]+)").unwrap();
    let result = re_url.replace_all(msg, "${1}***").to_string();
    // API 密钥脱敏：sk-xxxx 等格式
    let re_key = Regex::new(r"\bsk-[a-zA-Z0-9]{8,}\b").unwrap();
    re_key.replace_all(&result, "***").to_string()
}

/// 上游失败对外 HTTP 状态：仅保留 4xx/5xx，其余按 502（含非 HTTP 业务码）。
#[inline]
pub fn normalize_error_http_status(status: u16) -> u16 {
    if (400..600).contains(&status) {
        status
    } else {
        502
    }
}

/// 异步轮询传输层可重试 HTTP（业务失败走 body.status=failed，不经此判定）。
#[inline]
pub fn is_poll_transport_retryable(status: u16) -> bool {
    matches!(
        normalize_error_http_status(status),
        429 | 500 | 502 | 503 | 504
    )
}

/// 上游失败对外错误：日志与客户端共用同一 HTTP 状态码（4xx/5xx 透出，其余按 502）
/// 错误体经 `format_as_openai_error` 收成标准 OpenAI error；`headers` 可选透传上游诊断头
pub fn upstream_fail(
    status: u16,
    raw_msg: &str,
    headers: Option<axum::http::HeaderMap>,
) -> crate::error::AppError {
    let msg = sanitize_error_message(&norm_err_msg(raw_msg));
    crate::error::AppError::UpstreamHttpError(normalize_error_http_status(status), msg, headers)
}

/// 零费用/失败结算参数（检测仍由调用方完成；本结构负责记账，可选再包成 `upstream_fail`）。
///
/// - `prefer_http_status = Some(s)`：传输失败 / HTTP 非 2xx / 调用方已定码，状态用 `s`，日志文案用 `upstream_error_text`
/// - `prefer_http_status = None`：HTTP 200 业务失败，状态码与日志文案从 `response_body` 推断/提取
/// - `client_msg`：客户端文案；`None` 时与日志 `error_msg` 相同
/// - `pre_deducted` / `pre_deduct_gift`：预扣后失败退费场景传入已预扣额；上游失败尚未预扣时保持 `0.0`
pub struct ZeroCostUpstreamFail<'a> {
    pub state: &'a Arc<AppState>,
    pub token: &'a ApiToken,
    pub channel: &'a crate::models::Channel,
    pub model: &'a str,
    pub prefer_http_status: Option<u16>,
    pub endpoint: &'a str,
    pub latency_ms: u32,
    pub is_stream: i32,
    pub request_content: String,
    pub response_body: String,
    pub response_content: Option<String>,
    pub upstream_req_content: Option<String>,
    pub billing_detail: Option<String>,
    pub hint_category: Option<&'a str>,
    pub pending_log_id: Option<i64>,
    pub billing_model_hint: Option<&'a str>,
    pub db_model: Option<&'a crate::models::Model>,
    pub client_msg: Option<&'a str>,
    pub pre_deducted: f64,
    pub pre_deduct_gift: f64,
}

/// 失败记账（cost=0）：返回 `(status_code, client_msg)`，由调用方决定 `upstream_fail` / `BadRequest` 等。
/// `status_code` 已经过 `normalize_error_http_status`，与后续 `upstream_fail` / HA first_fail 一致。
pub async fn record_zero_cost_fail(p: ZeroCostUpstreamFail<'_>) -> (u16, String) {
    let raw_status = p
        .prefer_http_status
        .unwrap_or_else(|| infer_error_status_code_from_str(&p.response_body));
    let status_code = normalize_error_http_status(raw_status);
    let error_msg = match p.prefer_http_status {
        Some(_) => upstream_error_text(status_code, &p.response_body),
        None => extract_error_message(&p.response_body),
    };
    let client_owned = p.client_msg.unwrap_or(&error_msg).to_string();
    record_and_bill_inner(BillRecord {
        state: p.state,
        token: p.token,
        channel: p.channel,
        model: p.model,
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: 0,
        cost: 0.0,
        pre_deducted: p.pre_deducted,
        pre_deduct_gift: p.pre_deduct_gift,
        status_code,
        endpoint: p.endpoint,
        error_msg: Some(&error_msg),
        latency_ms: p.latency_ms,
        is_stream: p.is_stream,
        request_content: Some(p.request_content),
        response_content: p.response_content,
        upstream_req_content: p.upstream_req_content,
        billing_detail: p.billing_detail,
        hint_category: p.hint_category,
        pending_log_id: p.pending_log_id,
        billing_model_hint: p.billing_model_hint,
        plugin_tag: None,
        db_model: p.db_model,
        time_multiplier: None,
    })
    .await;
    (status_code, client_owned)
}

/// HA 续试：按行内预扣凭证退钱包并清零 cost，保持 pending（status_code=0）供下一子渠结算
pub async fn refund_pending(state: &Arc<AppState>, log_id: i64) {
    let _ = settle_pending_prepay(state, log_id, None).await;
}

/// JSON 则走统一 OpenAI 错误规范化；非 JSON 原样返回
fn norm_err_msg(raw_msg: &str) -> String {
    let raw = raw_msg.find('{').map(|i| &raw_msg[i..]).unwrap_or(raw_msg);
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| super::response_formatter::format_as_openai_error(&v))
        .unwrap_or_else(|| raw_msg.to_string())
}

/// 上游错误文案：空 body 时补默认句，供日志与 `upstream_fail` 共用
#[inline]
pub fn upstream_error_text(status: u16, body: &str) -> String {
    if body.trim().is_empty() {
        format!("Upstream HTTP error {}", status)
    } else {
        body.to_string()
    }
}

/// 从可能为 JSON 格式的错误响应体中提取最核心的错误文本信息
pub fn extract_error_message(resp_body: &str) -> String {
    let raw = resp_body
        .find('{')
        .map(|i| &resp_body[i..])
        .unwrap_or(resp_body);
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(msg) = super::response_formatter::extract_error_message_from_value(&json) {
            return msg;
        }
    }
    resp_body.to_string()
}

/// 推断业务 HTTP 状态码（error.code → 文案关键词），结果保证为 4xx/5xx。
pub fn infer_error_status_code(body: &serde_json::Value) -> u16 {
    let raw = if let Some(code) = super::response_formatter::extract_error_code_from_value(body) {
        if let Some(status) = classify_error_code(&code) {
            status
        } else {
            classify_error_text(
                &super::response_formatter::extract_error_message_from_value(body)
                    .unwrap_or_default(),
            )
        }
    } else {
        classify_error_text(
            &super::response_formatter::extract_error_message_from_value(body).unwrap_or_default(),
        )
    };
    normalize_error_http_status(raw)
}

/// 文本错误快捷推断：尝试 JSON，否则文案关键词；结果保证 4xx/5xx。
pub fn infer_error_status_code_from_str(err: &str) -> u16 {
    let raw = err.find('{').map(|i| &err[i..]).unwrap_or(err);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        return infer_error_status_code(&v);
    }
    normalize_error_http_status(classify_error_text(err))
}

/// 按 error.code 字符串分类 HTTP 状态码。
/// 数字仅在合法 HTTP 错误区间（400–599）时采纳；厂商业务码（如 MiniMax 2013）返回 None，
/// 由调用方按 message 再分级。语义字符串码（PolicyViolation 等）正常映射。
fn classify_error_code(code: &str) -> Option<u16> {
    if let Ok(n) = code.parse::<u16>() {
        return if (400..=599).contains(&n) {
            Some(n)
        } else {
            None
        };
    }
    let c = code.to_lowercase();
    // 402：欠费/余额不足（顺序须在 403 之前，防止 overdue 含 forbidden 被误分）
    if c.contains("overdue")
        || c.contains("balance")
        || c.contains("payment")
        || c.contains("insufficient_quota")
        || c.contains("insufficient_fund")
        || c.contains("billing")
    {
        return Some(402);
    }
    // 403：内容安全 / 政策违规 / 权限不足（permission 须排在 auth 之前）
    if c.contains("sensitive")
        || c.contains("policy")
        || c.contains("violation")
        || c.contains("safety")
        || c.contains("copyright")
        || c.contains("block")
        || c.contains("moderation")
        || c.contains("censor")
        || c.contains("permission")
        || c.contains("forbidden")
        || c.contains("access_denied")
    {
        return Some(403);
    }
    // 鉴权/身份认证失败
    if c.contains("auth")
        || c.contains("unauthorized")
        || c.contains("invalid_key")
        || c.contains("credential")
        || c.contains("unauthenticated")
        || c.contains("revoked")
    {
        return Some(401);
    }
    // 限流/超额
    if c.contains("rate")
        || c.contains("limit")
        || c.contains("quota")
        || c.contains("throttl")
        || c.contains("exceeded")
    {
        return Some(429);
    }
    // 超时/不可用
    if c.contains("timeout")
        || c.contains("gateway")
        || c.contains("unavailable")
        || c.contains("overload")
    {
        return Some(504);
    }
    // 上游服务内部错误
    if c.contains("internal") || c.contains("server_error") || c.contains("service_error") {
        return Some(500);
    }
    // 有语义 code 但未命中以上分类 → 客户端类业务错误
    Some(400)
}

/// message 文本关键词分类 HTTP 状态码（无结构化 error.code 时的兜底，私有辅助）
fn classify_error_text(msg: &str) -> u16 {
    let m = msg.to_lowercase();
    // 402：欠费/余额不足（须在 403 之前，防止被权限分支误拦）
    if m.contains("overdue")
        || m.contains("out of budget")
        || m.contains("insufficient_balance")
        || m.contains("insufficient fund")
        || m.contains("payment required")
        || m.contains("balance")
        || m.contains("payment")
        || m.contains("欠费")
        || m.contains("余额不足")
        || m.contains("余额不够")
    {
        return 402;
    }
    // 403：内容安全/政策违规/权限不足（permission 须在 auth 之前："not authorized" 含 auth 子串）
    if m.contains("safety")
        || m.contains("censor")
        || m.contains("policy")
        || m.contains("violation")
        || m.contains("block")
        || m.contains("sensitive")
        || m.contains("moderation")
        || m.contains("content_filter")
        || m.contains("permission")
        || m.contains("forbidden")
        || m.contains("not authorized")
        || m.contains("access denied")
        || m.contains("敏感")
        || m.contains("违规")
        || m.contains("安全")
        || m.contains("政策")
        || m.contains("审核")
        || m.contains("无权限")
        || m.contains("没有权限")
    {
        return 403;
    }
    // 401：鉴权/授权失败
    if m.contains("auth")
        || m.contains("unauthorized")
        || m.contains("api_key")
        || m.contains("credential")
        || m.contains("invalid_key")
        || m.contains("bad_key")
        || m.contains("revoked")
        || m.contains("unauthenticated")
        || m.contains("鉴权")
        || m.contains("密钥")
        || m.contains("授权")
    {
        return 401;
    }
    // 429：限流/超额
    if m.contains("too many requests")
        || m.contains("too_many_requests")
        || m.contains("limit")
        || m.contains("quota")
        || m.contains("exceeded")
        || m.contains("rate")
        || m.contains("insufficient")
        || m.contains("额度")
        || m.contains("限流")
        || m.contains("并发")
        || m.contains("超出")
        || m.contains("不足")
    {
        return 429;
    }
    // 504：超时/网关/连接中断
    if m.contains("timeout")
        || m.contains("gateway")
        || m.contains("connect")
        || m.contains("disconnect")
        || m.contains("abort")
        || m.contains("unreachable")
        || m.contains("超时")
        || m.contains("网关")
        || m.contains("中断")
    {
        return 504;
    }
    // 500：上游服务器内部故障
    if m.contains("internal")
        || m.contains("server")
        || m.contains("failed")
        || m.contains("error")
        || m.contains("bug")
        || m.contains("crash")
        || m.contains("故障")
        || m.contains("服务器错误")
        || m.contains("执行失败")
        || m.contains("异常")
    {
        return 500;
    }
    400
}

/// 并发获取所有视频 URL 的时长之和（8 秒全局超时兜底）
pub async fn sum_remote_videos_duration(client: &reqwest::Client, urls: &[String]) -> f64 {
    if urls.is_empty() {
        return 0.0;
    }
    let probe = async {
        let futs = urls.iter().map(|u| probe_video_duration(client, u));
        futures::future::join_all(futs).await.into_iter().sum()
    };
    tokio::time::timeout(std::time::Duration::from_secs(8), probe)
        .await
        .unwrap_or(0.0)
}

/// 局部辅助：流式获取指定 Range 的数据，一旦解析出时长立刻返回，支持 UA 伪装防拦截
async fn fetch_and_parse(
    client: &reqwest::Client,
    url: &str,
    range: &str,
) -> Option<(f64, Option<u64>, Vec<u8>)> {
    use futures::StreamExt;

    let resp = client.get(url)
        .header("Range", range)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(4))
        .send().await.ok()?;

    let status = resp.status().as_u16();
    if status != 200 && status != 206 {
        tracing::warn!(
            "[VideoDuration] HTTP 状态码非预期: {} 状态码={}",
            url,
            status
        );
        return None;
    }

    let total = resp
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|cr| cr.split('/').last()?.trim().parse::<u64>().ok());

    let mut buf = Vec::with_capacity(8192);
    let mut stream = resp.bytes_stream();
    while let Some(Ok(chunk)) = stream.next().await {
        buf.extend_from_slice(&chunk);
        if let Some(d) = parse_video_duration(&buf) {
            return Some((d, total, buf));
        }
        if buf.len() >= 32768 {
            break;
        }
    }
    Some((0.0, total, buf))
}

/// HTTP Range 流式探测单个远程 MP4 视频时长，解析出 duration 立即终止连接
async fn probe_video_duration(client: &reqwest::Client, url: &str) -> f64 {
    let start = std::time::Instant::now();

    // 1. 发起头部 Range 请求，拉取并流式解析前 32KB
    let (dur, total_size, head_buf) = match fetch_and_parse(client, url, "bytes=0-32767").await {
        Some(res) => res,
        None => {
            tracing::warn!("[VideoDuration] 头部请求异常: {}", url);
            return 0.0;
        }
    };

    if dur > 0.0 {
        tracing::info!(
            "[VideoDuration] 头部解析成功: {} 时长={} 耗时={:?}",
            url,
            dur,
            start.elapsed()
        );
        return dur;
    }

    // 2. 如果头部未找到且知道总大小，发起第二个 Range 请求，拉取并流式解析尾部 32KB (处理非 faststart 视频)
    if let Some(total) = total_size {
        if total > 32768 {
            let range = format!("bytes={}-{}", total - 32768, total - 1);
            if let Some((tail_dur, _, _)) = fetch_and_parse(client, url, &range).await {
                if tail_dur > 0.0 {
                    tracing::info!(
                        "[VideoDuration] 尾部解析成功: {} 时长={} 耗时={:?}",
                        url,
                        tail_dur,
                        start.elapsed()
                    );
                    return tail_dur;
                }
            }
        }
    }

    // 3. 兜底处理 (若全部步骤都没找到，则宣告失败)
    tracing::warn!(
        "[VideoDuration] 探测失败(非标准或元数据过大): {} 大小={} 字节 总大小={:?} 耗时={:?}",
        url,
        head_buf.len(),
        total_size,
        start.elapsed()
    );
    0.0
}

/// 通用视频时长解析入口，支持 MP4/MOV、WEBM/MKV、AVI、FLV
fn parse_video_duration(data: &[u8]) -> Option<f64> {
    parse_mp4_duration(data)
        .or_else(|| parse_webm_duration(data))
        .or_else(|| parse_avi_duration(data))
        .or_else(|| parse_flv_duration(data))
        .filter(|d| *d > 0.0 && d.is_finite() && *d < 86400.0)
}

/// 解析 MP4/MOV 提取视频时长（秒）
fn parse_mp4_duration(data: &[u8]) -> Option<f64> {
    let moov_pos = data.windows(4).position(|w| w == b"moov")?;
    let moov_body = &data[moov_pos + 4..];
    let mvhd_pos = moov_body.windows(4).position(|w| w == b"mvhd")?;
    let mvhd_body = &moov_body[mvhd_pos + 4..];
    let (ts_off, dur_off, dur_len) = if mvhd_body.first().copied()? == 0 {
        (12, 16, 4)
    } else {
        (20, 24, 8)
    };
    if mvhd_body.len() < dur_off + dur_len {
        return None;
    }
    let timescale = u32::from_be_bytes(mvhd_body[ts_off..ts_off + 4].try_into().ok()?) as f64;
    let duration = if dur_len == 4 {
        u32::from_be_bytes(mvhd_body[dur_off..dur_off + 4].try_into().ok()?) as f64
    } else {
        u64::from_be_bytes(mvhd_body[dur_off..dur_off + 8].try_into().ok()?) as f64
    };
    if timescale > 0.0 {
        Some(duration / timescale)
    } else {
        None
    }
}

/// 解析 WEBM/MKV (EBML 容器) 提取视频时长（秒）
fn parse_webm_duration(data: &[u8]) -> Option<f64> {
    data.windows(4)
        .position(|w| w == &[0x1A, 0x45, 0xDF, 0xA3])?;
    let info_pos = data
        .windows(4)
        .position(|w| w == &[0x15, 0x49, 0xA9, 0x66])?;
    let info_body = &data[info_pos + 4..];

    let ts_pos = info_body
        .windows(3)
        .position(|w| w == &[0x2A, 0xD7, 0xB1])?;
    let (timescale, _) = parse_ebml_vint(&info_body[ts_pos + 3..])?;

    let dur_pos = info_body.windows(2).position(|w| w == &[0x44, 0x89])?;
    let dur_body = &info_body[dur_pos + 2..];
    let (dur_size, dur_size_len) = parse_ebml_vint(dur_body)?;
    if dur_body.len() < dur_size_len + dur_size as usize {
        return None;
    }

    let val_bytes = &dur_body[dur_size_len..dur_size_len + dur_size as usize];
    let duration_ms = if dur_size == 4 {
        f32::from_be_bytes(val_bytes.try_into().ok()?) as f64
    } else if dur_size == 8 {
        f64::from_be_bytes(val_bytes.try_into().ok()?) as f64
    } else {
        return None;
    };

    if timescale > 0 {
        Some((duration_ms * timescale as f64) / 1_000_000_000.0)
    } else {
        Some(duration_ms / 1000.0)
    }
}

/// 解析 AVI (RIFF 容器) 提取视频时长（秒）
fn parse_avi_duration(data: &[u8]) -> Option<f64> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"AVI " {
        return None;
    }
    let avih_pos = data.windows(4).position(|w| w == b"avih")?;
    let avih_body = &data[avih_pos + 8..];
    if avih_body.len() < 20 {
        return None;
    }
    let us_per_frame = u32::from_le_bytes(avih_body[0..4].try_into().ok()?) as f64;
    let total_frames = u32::from_le_bytes(avih_body[16..20].try_into().ok()?) as f64;
    if us_per_frame > 0.0 {
        Some((us_per_frame * total_frames) / 1_000_000.0)
    } else {
        None
    }
}

/// 解析 FLV (AMF 容器) 提取视频时长（秒）
fn parse_flv_duration(data: &[u8]) -> Option<f64> {
    if data.len() < 4 || &data[0..3] != b"FLV" {
        return None;
    }
    let dur_pos = data.windows(8).position(|w| w == b"duration")?;
    let val_type_pos = dur_pos + 8;
    if data.len() >= val_type_pos + 9 && data[val_type_pos] == 0x00 {
        let duration =
            f64::from_be_bytes(data[val_type_pos + 1..val_type_pos + 9].try_into().ok()?);
        if duration > 0.0 && duration.is_finite() {
            return Some(duration);
        }
    }
    None
}

/// 解析 EBML VINT (可变长度整数)
fn parse_ebml_vint(data: &[u8]) -> Option<(u64, usize)> {
    let first = *data.first()?;
    let zeros = first.leading_zeros() as usize;
    if zeros >= 8 {
        return None;
    }
    let len = zeros + 1;
    if data.len() < len {
        return None;
    }
    let mut val = (first & (0xFF >> len)) as u64;
    for i in 1..len {
        val = (val << 8) | data[i] as u64;
    }
    Some((val, len))
}

/// 高效精确：复用系统特征识别结构来提取输入视频 URL 列表
pub fn extract_request_video_urls(body: &serde_json::Value) -> Vec<String> {
    let mut urls = Vec::new();

    // 1. 顶层 videos 数组
    if let Some(arr) = body.get("videos").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str().filter(|s| !s.is_empty()) {
                urls.push(s.to_string());
            } else if let Some(u) = item
                .get("url")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                urls.push(u.to_string());
            } else if let Some(u) = item
                .get("video_url")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                urls.push(u.to_string());
            }
        }
    }

    // 2. 火山方舟 content[].video_url 结构
    if let Some(content) = body.get("content").and_then(|c| c.as_array()) {
        for item in content {
            if let Some(t) = item.get("type").and_then(|v| v.as_str()) {
                if t.contains("video") {
                    if let Some(video_obj) = item.get("video_url") {
                        if let Some(u) = video_obj.as_str().filter(|s| !s.is_empty()) {
                            urls.push(u.to_string());
                        } else if let Some(u) = video_obj
                            .get("url")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            urls.push(u.to_string());
                        }
                    }
                }
            }
        }
    }

    urls.sort();
    urls.dedup();
    urls
}
