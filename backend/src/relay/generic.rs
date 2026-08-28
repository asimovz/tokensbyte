/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! Relay: 通用透传处理器
//! 处理 Embedding（向量）和 Rerank（排序）模型请求。
//! 请求体直接透传，仅替换 model 字段；响应直接返回上游 JSON。
//! 遵循与 audio.rs 一致的 7 步流水线模式。

use super::{forward, proxy, router, upstream_headers, usage_extractor};
use crate::models::ApiToken;
use crate::{
    error::{AppError, AppResult},
    AppState,
};
use axum::{
    extract::{Extension, OriginalUri, State},
    response::Response,
    Json,
};
use std::sync::Arc;

// ── 类别推断 ────────────────────────────────────────────────────

/// 本 handler 仅服务向量/排序两类
fn infer_category(path: &str) -> &'static str {
    if path.contains("rerank") {
        "排序"
    } else {
        "向量"
    }
}

// ── 主处理函数 ──────────────────────────────────────────────────

/// 通用透传处理器 — Embedding / Rerank
pub async fn generic_relay(
    State(state): State<Arc<AppState>>,
    Extension(token): Extension<ApiToken>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<serde_json::Value>,
) -> AppResult<Response> {
    let raw_path = uri.path();
    let entry_path = raw_path.to_string();
    let category = infer_category(raw_path);
    let request_content_str = serde_json::to_string(&body).unwrap_or_default();

    let model = body["model"]
        .as_str()
        .ok_or_else(|| AppError::BadRequest("Missing required parameter: model".to_string()))?;

    // ── 1. Token 模型权限校验（渠道选择前快速拦截） ──
    proxy::check_model_permission(&state, &token, model, &entry_path, Some(category)).await?;
    //start patch @bobcat
    let mut replace = crate::patch::maybe_replace(&state, &token.user_id, model, Some(category)).await;
    //end patch

    // ── 2. 用户上下文 ──
    let ctx = proxy::get_user_context(&state, &token.user_id).await?;

    // ── 3. 渠道选择 + HA failover ──
    let mut ha = crate::relay::ha::HaAttempt::begin(&state, token.high_availability).await;

    while ha.cont() {
        let start_time = std::time::Instant::now();
        let channel = match crate::patch::select_channel_for_replace(
            &state,
            &token,
            &mut replace,
            &ctx.user_group,
            &ctx.level_id,
            &entry_path,
            &ha.exclude_aids,
            !ha.had_upstream,
            ha.had_upstream,
            Some(category),
        )
        .await
        {
            Ok(c) => c,
            Err(e) => {
                ha.on_select_err(e);
                break;
            }
        };

        // ── 4. 预扣费检查 ──
        let (pre_deduction, db_model, resolved_cat) =
            match proxy::check_access(&state, &token, model, &ctx, Some(category), Some(&channel))
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    ha.on_access_err(e);
                    break;
                }
            };

        //start patch @bobcat
        let route_model = replace.route();
        let route_db_model = replace
            .fwd_db_model(&state, Some(category), Some(&channel), db_model.as_ref())
            .await;
        //end patch

        // ── 5. 转发规则解析 ──
        let mut resolved = match forward::resolve_forward_rule(
            &state,
            route_model,
            &resolved_cat,
            &entry_path,
            Some(&channel),
            route_db_model.as_ref(),
        )
        .await
        {
            Some(r) => r,
            None => {
                if forward::model_has_forward_rules(&state, route_model).await {
                    ha.on_access_err(AppError::BadRequest(format!(
                        "模型 '{}' 不支持当前接口，请检查模型对应的转发规则",
                        route_model
                    )));
                    break;
                }
                forward::infer_forward_from_base_url(
                    &channel.base_url,
                    &resolved_cat,
                    route_db_model.as_ref(),
                )
            }
        };
        forward::refine_target_type(&mut resolved, &channel.base_url);

        // 模型映射：向量/排序无分辨率档，跳过 body 解析
        let (final_resolved_model, mapping_source) =
            router::resolve_model(&channel, route_model, route_db_model.as_ref(), None);

        // 查询计费规则（供计费阶段使用）
        let mut db_rule =
            proxy::get_model_billing_rule(&state, model, Some(&channel), db_model.as_ref()).await;

        // ── 6. 请求体透传（仅替换 model 字段） ──
        let mut upstream_body = body.clone();
        upstream_body["model"] = serde_json::json!(&final_resolved_model);

        let url = forward::build_upstream_url(
            &channel.base_url,
            &resolved,
            &final_resolved_model,
            &channel.api_key,
        );

        // 【一条日志原则】请求前预记录日志
        let ep = format!(
            "{}|{}",
            raw_path,
            resolved
                .upstream_path
                .replace("${model}", &final_resolved_model)
        );

        tracing::info!(
            "[Generic] 模型={} 类别={} 目标类型={} URL={}",
            model,
            category,
            resolved.target_type,
            url
        );

        if ha.pending_log_id.is_none() {
            ha.set_pending(
                proxy::record_pending_log(proxy::PendingLog {
                    state: &state,
                    user_id: &token.user_id,
                    token_id: token.id,
                    model: model,
                    endpoint: &ep,
                    is_stream: 0,
                    request_content: Some(&request_content_str),
                    upstream_url: Some(&url),
                    channel: &channel,
                    billing_model_hint: None,
                    plugin_tag: None,
                    category: Some(resolved_cat.as_str()),
                    db_model: db_model.as_ref(),
                    forward_eid: Some(&resolved.eid),
                    requested_log_id: None,
                })
                .await,
            );
        }

        // 【连接保护】上游请求+预扣+落库放独立 task，客户端断开后仍能完成
        let pending_log_id = ha.pending_log_id;
        let timeout_ctx = ha.timeout_ctx();
        let fail_buf = ha.buf();

        let result_rx = super::spawn_protected({
            let state = state.clone();
            let token = token.clone();
            let channel = channel.clone();
            let model = model.to_string();
            let request_content_str = request_content_str.clone();
            let ctx = ctx.clone();
            let url = url.clone();
            let ep = ep.clone();
            let db_model = db_model.clone();
            let resolved_cat = resolved_cat.clone();
            async move {
                // 构建并发送上游请求（统一鉴权 + 设置请求体）；预扣在业务成功后再执行（对齐 chat）
                let builder = state
                    .http_client
                    .post(&url)
                    .header("Content-Type", "application/json");
                let builder = crate::services::http_client::with_timeout(
                    forward::apply_request_auth(
                        builder,
                        &resolved,
                        &channel.api_key,
                        &mut upstream_body,
                        &channel.base_url,
                    ),
                    timeout_ctx.resolve(),
                );
                let resp = match builder.send().await {
                    Ok(resp) => resp,
                    Err(e) => {
                        let err_msg = e.to_string();
                        let latency_ms = start_time.elapsed().as_millis() as u32;
                        let bill = crate::relay::ha::FailBill::transport(
                            latency_ms,
                            err_msg,
                            &request_content_str,
                            upstream_body.to_string(),
                        );
                        return Err(crate::relay::ha::HaAttempt::park(&fail_buf, bill, None));
                    }
                };

                let status = resp.status().as_u16();
                if !resp.status().is_success() {
                    let upstream_hdrs = resp.headers().clone();
                    let err = resp.text().await.unwrap_or_default();
                    let latency_ms = start_time.elapsed().as_millis() as u32;
                    let bill = crate::relay::ha::FailBill::http(
                        latency_ms,
                        status,
                        err,
                        &request_content_str,
                        upstream_body.to_string(),
                    );
                    return Err(crate::relay::ha::HaAttempt::park(
                        &fail_buf,
                        bill,
                        Some(upstream_hdrs),
                    ));
                }

                // 读取响应体文本
                let upstream_hdrs = resp.headers().clone();
                let mut resp_text = resp.text().await.unwrap_or_default();

                // 上游 body 级错误检测（HTTP 200 但业务失败，在预扣费之前拦截）
                let (converted, post_err) = forward::check_upstream_post_error(
                    &resolved.target_type,
                    &resp_text,
                    resolved_cat.as_str(),
                    false,
                );
                resp_text = converted;
                if let Some(err_response) = post_err {
                    let latency_ms = start_time.elapsed().as_millis() as u32;
                    let bill = crate::relay::ha::FailBill::biz(
                        latency_ms,
                        resp_text,
                        err_response,
                        request_content_str,
                        upstream_body.to_string(),
                    );
                    return Err(crate::relay::ha::HaAttempt::park(
                        &fail_buf,
                        bill,
                        Some(upstream_hdrs),
                    ));
                }

                let pre_deduct_gift = proxy::pre_deduct_or_intercept(
                    &state,
                    &token,
                    &channel,
                    &model,
                    pre_deduction,
                    &ep,
                    start_time,
                    0,
                    &request_content_str,
                    &upstream_body.to_string(),
                    None,
                    pending_log_id,
                    db_model.as_ref(),
                    Some(resolved_cat.as_str()),
                )
                .await?;

                // 提取 usage tokens
                let mut usage = usage_extractor::parse_usage(&resp_text);
                // 差额修正：部分 rerank 模型（如阿里 qwen3-vl-rerank）返回的 total_tokens 大于 input_tokens，
                // 或部分模型仅返回 total_tokens。为确保以 total_tokens 作为总消耗准确计费，
                // 当 total 大于 prompt、completion 与 image_tokens 之和时，将差额统一补入 prompt 用量中。
                if usage.total > 0
                    && usage.total > usage.prompt + usage.completion + usage.image_tokens
                {
                    usage.prompt = usage.total - usage.completion - usage.image_tokens;
                }

                // ── 计费结算 ──
                let latency_ms = start_time.elapsed().as_millis() as u32;
                let features = usage_extractor::ExtractedFeatures::default();

                let (cost, billing_detail) = crate::relay::calculate_relay_cost(
                    &state,
                    db_model.as_ref(),
                    db_rule.as_mut(),
                    &channel,
                    &ctx,
                    &usage,
                    &features,
                    mapping_source.as_deref(),
                    &model,
                    &final_resolved_model,
                )
                .await;

                proxy::record_and_bill_inner(proxy::BillRecord {
                    state: &state,
                    token: &token,
                    channel: &channel,
                    model: &model,
                    prompt_tokens: usage.prompt,
                    completion_tokens: usage.completion,
                    cached_tokens: usage.cached,
                    cost: cost,
                    pre_deducted: pre_deduction,
                    pre_deduct_gift: pre_deduct_gift,
                    status_code: 200,
                    endpoint: &ep,
                    error_msg: None,
                    latency_ms: latency_ms,
                    is_stream: 0,
                    request_content: Some(request_content_str),
                    response_content: Some(resp_text.clone()),
                    upstream_req_content: Some(upstream_body.to_string()),
                    billing_detail: Some(billing_detail),
                    hint_category: Some(resolved_cat.as_str()),
                    pending_log_id: pending_log_id,
                    billing_model_hint: None,
                    plugin_tag: None,
                    db_model: db_model.as_ref(),
                    time_multiplier: db_rule.as_ref().map(|r| r.applied_multiplier),
                })
                .await;

                // 直接透传上游 JSON 响应（含诊断响应头）
                Ok(upstream_headers::json_with_upstream_headers(
                    &upstream_hdrs,
                    resp_text,
                ))
            }
        });

        match result_rx.await {
            Ok(result) => match result {
                Ok(resp) => {
                    let ms = start_time.elapsed().as_millis() as u32;
                    ha.ok(&state, &channel, &url, ms).await;
                    return Ok(resp);
                }
                Err(e) => {
                    if ha
                        .fail(
                            &crate::relay::ha::HaBillCtx::new(&state, &token, model, &ep)
                                .category(resolved_cat.as_str())
                                .db(db_model.as_ref()),
                            &channel,
                            e,
                            Some(&url),
                        )
                        .await
                    {
                        ha.bump();
                        continue;
                    }
                    break;
                }
            },
            Err(_) => {
                ha.last_err = AppError::Internal("请求处理任务异常终止".into());
                break;
            }
        }
    } // end while

    Err(ha
        .finish(
            &crate::relay::ha::HaBillCtx::new(&state, &token, model, &entry_path)
                .category(category),
        )
        .await)
}
