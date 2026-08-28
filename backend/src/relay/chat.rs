/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

// ── 聊天 & Responses API 处理 ──────────────────────────────────
// 统一管理 Chat Completions 和 Responses API 的请求处理逻辑

use super::{forward, proxy, router, stream, upstream_headers, usage_extractor};
use crate::error::{AppError, AppResult};
use crate::models::ApiToken;
use crate::AppState;
use axum::{
    extract::{Extension, OriginalUri, State},
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;

// ── Chat Completions (/v1/chat/completions) ──────────────────

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Extension(token): Extension<ApiToken>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<serde_json::Value>,
) -> AppResult<Response> {
    let raw_path = uri.path();
    let request_content_str = serde_json::to_string(&body).unwrap_or_default();
    let model = body["model"]
        .as_str()
        .ok_or_else(|| AppError::BadRequest("Missing required parameter: model".to_string()))?;
    let is_stream = body["stream"].as_bool().unwrap_or(false);

    let ctx = proxy::get_user_context(&state, &token.user_id).await?;
    proxy::check_model_permission(&state, &token, model, "/v1/chat/completions", Some("聊天"))
        .await?;
    //start patch @bobcat
    let mut replace = crate::patch::maybe_replace(&state, &token.user_id, model, Some("聊天")).await;
    //end patch

    // 【一条日志原则】HA 重试复用同一条 pending，避免产生多条
    let mut ha = crate::relay::ha::HaAttempt::begin(&state, token.high_availability).await;

    while ha.cont() {
        let start_time = std::time::Instant::now();

        // 1. 选择渠道
        //start patch @bobcat
        let channel = match crate::patch::select_channel_for_replace(
            &state,
            &token,
            &mut replace,
            &ctx.user_group,
            &ctx.level_id,
            raw_path,
            &ha.exclude_aids,
            !ha.had_upstream,
            ha.had_upstream,
            Some("聊天"),
        )
        .await
        //end patch
        {
            Ok(c) => c,
            Err(e) => {
                ha.on_select_err(e);
                break;
            }
        };

        // 2. 预扣费检查（带 channel 精确匹配同名模型；同时获取 Model 供下游复用）
        let (pre_deduction, db_model, resolved_cat) =
            match proxy::check_access(&state, &token, model, &ctx, Some("聊天"), Some(&channel))
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
            .fwd_db_model(&state, Some("聊天"), Some(&channel), db_model.as_ref())
            .await;
        //end patch

        // 模型映射：聊天无分辨率档，跳过 body 解析
        let (resolved_model, mapping_source) =
            router::resolve_model(&channel, route_model, route_db_model.as_ref(), None);

        // 3. 解析转发规则（复用 db_model 避免重查 models 表）
        let resolved = match forward::resolve_forward_rule(
            &state,
            route_model,
            &resolved_cat,
            raw_path,
            Some(&channel),
            route_db_model.as_ref(),
        )
        .await
        {
            Some(r) => r,
            None => {
                if forward::model_has_forward_rules(&state, route_model).await {
                    // 业务侧错误，不可 HA 续试（continue 不 bump 会空转）
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

        let target_type = resolved.target_type.clone();
        // 转发规则路径含 streamGenerateContent 时上游始终返回 SSE，强制走流式路径
        let is_stream = is_stream || resolved.upstream_path.contains("streamGenerateContent");
        let mut db_rule =
            proxy::get_model_billing_rule(&state, model, Some(&channel), db_model.as_ref()).await;
        let upstream_body: serde_json::Value = forward::transform_request_body(
            &resolved,
            &resolved_model,
            &body,
            "聊天",
            db_rule.as_ref(),
            Some(&state.http_client),
        )
        .await;
        let url = forward::build_upstream_url(
            &channel.base_url,
            &resolved,
            &resolved_model,
            &channel.api_key,
        );
        let auth_headers = forward::build_auth_headers(&resolved, &channel.api_key, true);

        tracing::info!(
            "[Chat] 尝试={} 模型={} 目标类型={} 鉴权={} 地址={} 渠道id={}",
            ha.attempt,
            model,
            target_type,
            resolved.auth_type,
            url,
            channel.id
        );

        let resolved_upstream_path = resolved.upstream_path.replace("${model}", &resolved_model);
        let masked_url = forward::mask_key_in_string(&url, &channel.api_key);
        let ep = format!("{}|{}", raw_path, masked_url);
        let bill_ctx = crate::relay::ha::HaBillCtx::new(&state, &token, model, &ep)
            .category("聊天")
            .db(db_model.as_ref());
        // pending_log_id 将在后续与网络请求一起并发执行
        if is_stream {
            let mut stream_body = upstream_body.clone();
            // Gemini 流式通过 URL 切换为 :streamGenerateContent?alt=sse 实现，请求体不接受 stream 字段
            if target_type != "gemini" {
                stream_body["stream"] = serde_json::json!(true);
            }
            let mut final_upstream_path = resolved_upstream_path.clone();
            let stream_url = if target_type == "gemini" {
                final_upstream_path = final_upstream_path
                    .replace(":generateContent", ":streamGenerateContent")
                    + "?alt=sse";
                let mut final_url =
                    super::url_utils::join_url(&channel.base_url, &final_upstream_path);
                if resolved.auth_type == "query_key" {
                    if final_url.contains('?') {
                        final_url = format!("{}&key={}", final_url, channel.api_key);
                    } else {
                        final_url = format!("{}?key={}", final_url, channel.api_key);
                    }
                }
                final_url
            } else {
                url.clone()
            };

            let final_upstream_path = forward::mask_key_in_string(&stream_url, &channel.api_key);

            let stream_builder = state
                .http_client
                .post(&stream_url)
                .header("Content-Type", "application/json");
            let stream_builder = auth_headers
                .into_iter()
                .fold(stream_builder, |b, (k, v)| b.header(k, v));

            let existing_pending = ha.pending_log_id;
            let pending_log_future = async {
                if let Some(id) = existing_pending {
                    Some(id)
                } else {
                    proxy::record_pending_log(proxy::PendingLog {
                        state: &state,
                        user_id: &token.user_id,
                        token_id: token.id,
                        model: model,
                        endpoint: &ep,
                        is_stream: 1,
                        request_content: Some(&request_content_str),
                        upstream_url: Some(&url),
                        channel: &channel,
                        billing_model_hint: None,
                        plugin_tag: None,
                        category: Some("聊天"),
                        db_model: db_model.as_ref(),
                        forward_eid: Some(&resolved.eid),
                        requested_log_id: None,
                    })
                    .await
                }
            };

            let send_future = stream_builder.json(&stream_body).send();
            let (log_res, resp_res) = tokio::join!(pending_log_future, send_future);
            ha.set_pending(log_res);

            let resp = match resp_res {
                Ok(r) => r,
                Err(e) => {
                    let err_msg = e.to_string();
                    let latency_ms = start_time.elapsed().as_millis() as u32;
                    tracing::warn!("[Chat] 流式连接错误: {}", err_msg);
                    if chat_on_upstream_fail(
                        &mut ha,
                        &bill_ctx,
                        &channel,
                        crate::relay::ha::FailBill::transport(
                            latency_ms,
                            err_msg.clone(),
                            &request_content_str,
                            upstream_body.to_string(),
                        )
                        .content(None)
                        .client(err_msg.clone())
                        .stream(1),
                        Some(&url),
                        None,
                    )
                    .await
                    {
                        ha.bump();
                        continue;
                    }
                    break;
                }
            };

            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let upstream_hdrs = resp.headers().clone();
                let err = resp.text().await.unwrap_or_default();
                let display_err = proxy::upstream_error_text(status, &err);
                let latency_ms = start_time.elapsed().as_millis() as u32;
                tracing::warn!("[Chat] 流式上游错误 {}: {}", status, display_err);
                if chat_on_upstream_fail(
                    &mut ha,
                    &bill_ctx,
                    &channel,
                    crate::relay::ha::FailBill::http(
                        latency_ms,
                        status,
                        err.clone(),
                        &request_content_str,
                        upstream_body.to_string(),
                    )
                    .body(display_err.clone())
                    .content(Some(err))
                    .client(display_err)
                    .stream(1),
                    Some(&url),
                    Some(upstream_hdrs),
                )
                .await
                {
                    ha.bump();
                    continue;
                }
                break;
            }

            let prompt_tokens = estimate_prompt_tokens(&body);
            let pre_deduct_gift = match proxy::pre_deduct_or_intercept(
                &state,
                &token,
                &channel,
                model,
                pre_deduction,
                &ep,
                start_time,
                1,
                &request_content_str,
                &upstream_body.to_string(),
                None,
                ha.pending_log_id,
                db_model.as_ref(),
                Some("聊天"),
            )
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    ha.on_access_err(e);
                    break;
                }
            };

            let ms = start_time.elapsed().as_millis() as u32;
            ha.ok(&state, &channel, &url, ms).await;
            return Ok(stream::handle_chat_stream(
                state.clone(),
                token.clone(),
                channel.clone(),
                model.to_string(),
                resp,
                ctx.clone(),
                prompt_tokens,
                request_content_str.clone(),
                start_time,
                target_type,
                final_upstream_path,
                Some(upstream_body.to_string()),
                pre_deduction,
                pre_deduct_gift,
                raw_path.to_string(),
                None,
                ha.pending_log_id,
                db_model,
                db_rule,
            )
            .await
            .into_response());
        } else {
            let builder = state
                .http_client
                .post(&url)
                .header("Content-Type", "application/json");
            let builder = auth_headers
                .into_iter()
                .fold(builder, |b, (k, v)| b.header(k, v));
            let builder = crate::services::http_client::with_timeout(builder, ha.attempt_timeout());

            let existing_pending = ha.pending_log_id;
            let pending_log_future = async {
                if let Some(id) = existing_pending {
                    Some(id)
                } else {
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
                        category: Some("聊天"),
                        db_model: db_model.as_ref(),
                        forward_eid: Some(&resolved.eid),
                        requested_log_id: None,
                    })
                    .await
                }
            };

            let send_future = builder.json(&upstream_body).send();
            let (log_res, resp_res) = tokio::join!(pending_log_future, send_future);
            ha.set_pending(log_res);

            let resp = match resp_res {
                Ok(r) => r,
                Err(e) => {
                    let err_msg = e.to_string();
                    let latency_ms = start_time.elapsed().as_millis() as u32;
                    tracing::warn!("[Chat] 连接错误: {}", err_msg);
                    if chat_on_upstream_fail(
                        &mut ha,
                        &bill_ctx,
                        &channel,
                        crate::relay::ha::FailBill::transport(
                            latency_ms,
                            err_msg.clone(),
                            &request_content_str,
                            upstream_body.to_string(),
                        )
                        .content(None)
                        .client(err_msg.clone())
                        .stream(0),
                        Some(&url),
                        None,
                    )
                    .await
                    {
                        ha.bump();
                        continue;
                    }
                    break;
                }
            };

            let status = resp.status().as_u16();
            if !resp.status().is_success() {
                let upstream_hdrs = resp.headers().clone();
                let err = resp.text().await.unwrap_or_default();
                let display_err = proxy::upstream_error_text(status, &err);
                let latency_ms = start_time.elapsed().as_millis() as u32;
                tracing::warn!("[Chat] 上游错误 {}: {}", status, display_err);
                if chat_on_upstream_fail(
                    &mut ha,
                    &bill_ctx,
                    &channel,
                    crate::relay::ha::FailBill::http(
                        latency_ms,
                        status,
                        err.clone(),
                        &request_content_str,
                        upstream_body.to_string(),
                    )
                    .body(display_err.clone())
                    .content(Some(err))
                    .client(display_err)
                    .stream(0),
                    Some(&url),
                    Some(upstream_hdrs),
                )
                .await
                {
                    ha.bump();
                    continue;
                }
                break;
            }

            if upstream_headers::is_stream_content_type(resp.headers()) {
                let prompt_tokens = estimate_prompt_tokens(&body);
                let final_upstream_path = resolved_upstream_path.clone();
                let pre_deduct_gift = match proxy::pre_deduct_or_intercept(
                    &state,
                    &token,
                    &channel,
                    model,
                    pre_deduction,
                    &ep,
                    start_time,
                    1,
                    &request_content_str,
                    &upstream_body.to_string(),
                    None,
                    ha.pending_log_id,
                    db_model.as_ref(),
                    Some("聊天"),
                )
                .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        ha.on_access_err(e);
                        break;
                    }
                };
                let ms = start_time.elapsed().as_millis() as u32;
                ha.ok(&state, &channel, &url, ms).await;
                return Ok(stream::handle_chat_stream(
                    state.clone(),
                    token.clone(),
                    channel.clone(),
                    model.to_string(),
                    resp,
                    ctx.clone(),
                    prompt_tokens,
                    request_content_str.clone(),
                    start_time,
                    target_type,
                    final_upstream_path,
                    Some(upstream_body.to_string()),
                    pre_deduction,
                    pre_deduct_gift,
                    raw_path.to_string(),
                    None,
                    ha.pending_log_id,
                    db_model,
                    db_rule,
                )
                .await
                .into_response());
            }

            let upstream_hdrs = resp.headers().clone();
            let data = resp.bytes().await.unwrap_or_default();
            let mut response_content_str = String::from_utf8_lossy(&data).to_string();

            // 上游 body 级错误检测（HTTP 200 但业务失败，在预扣费之前拦截）
            let (converted, post_err) = forward::check_upstream_post_error(
                &target_type,
                &response_content_str,
                resolved_cat.as_str(),
                false,
            );
            response_content_str = converted;
            if let Some(_err_response) = post_err {
                let latency_ms = start_time.elapsed().as_millis() as u32;
                let err_text = proxy::extract_error_message(&response_content_str);
                tracing::warn!("[Chat] 上游响应体错误: {}", err_text);
                if chat_on_upstream_fail(
                    &mut ha,
                    &bill_ctx,
                    &channel,
                    crate::relay::ha::FailBill::biz(
                        latency_ms,
                        response_content_str.clone(),
                        err_text.clone(),
                        &request_content_str,
                        upstream_body.to_string(),
                    )
                    .stream(0),
                    Some(&url),
                    Some(upstream_hdrs),
                )
                .await
                {
                    ha.bump();
                    continue;
                }
                break;
            }

            let usage_tokens = usage_extractor::parse_usage(&response_content_str);
            let prompt_tokens = usage_tokens.prompt;
            let completion_tokens = usage_tokens.completion;
            let cached_tokens = usage_tokens.cached;

            let mut features = usage_extractor::extract_request_features(&body);
            usage_extractor::enrich_features_from_usage(&mut features, &usage_tokens);

            let pre_deduct_gift = match proxy::pre_deduct_or_intercept(
                &state,
                &token,
                &channel,
                model,
                pre_deduction,
                &ep,
                start_time,
                0,
                &request_content_str,
                &upstream_body.to_string(),
                None,
                ha.pending_log_id,
                db_model.as_ref(),
                Some("聊天"),
            )
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    ha.on_access_err(e);
                    break;
                }
            };

            let (quota_used, detail) = super::calculate_relay_cost(
                &state,
                db_model.as_ref(),
                db_rule.as_mut(),
                &channel,
                &ctx,
                &usage_tokens,
                &features,
                mapping_source.as_deref(),
                &model,
                &resolved_model,
            )
            .await;
            let latency_ms = start_time.elapsed().as_millis() as u32;

            // 【连接保护】计费放入独立 task，客户端断开后仍完成
            {
                let state = state.clone();
                let token = token.clone();
                let channel = channel.clone();
                let model = model.to_string();
                let ep = ep.clone();
                let request_content = request_content_str.clone();
                let response_content = response_content_str.clone();
                let upstream_req = upstream_body.to_string();
                let dm = db_model.clone();
                let pending_log_id = ha.pending_log_id;
                let locked_tm = db_rule.as_ref().map(|r| r.applied_multiplier);
                tokio::spawn(async move {
                    proxy::record_and_bill_inner(proxy::BillRecord {
                        state: &state,
                        token: &token,
                        channel: &channel,
                        model: &model,
                        prompt_tokens: prompt_tokens,
                        completion_tokens: completion_tokens,
                        cached_tokens: cached_tokens,
                        cost: quota_used,
                        pre_deducted: pre_deduction,
                        pre_deduct_gift: pre_deduct_gift,
                        status_code: 200,
                        endpoint: &ep,
                        error_msg: None,
                        latency_ms,
                        is_stream: 0,
                        request_content: Some(request_content),
                        response_content: Some(response_content),
                        upstream_req_content: Some(upstream_req),
                        billing_detail: Some(detail),
                        hint_category: Some("聊天"),
                        pending_log_id: pending_log_id,
                        billing_model_hint: None,
                        plugin_tag: None,
                        db_model: dm.as_ref(),
                        time_multiplier: locked_tm,
                    })
                    .await;
                });
            }

            let final_body = if raw_path.ends_with("/messages") {
                response_content_str.clone()
            } else {
                transform_chat_response(&response_content_str, &target_type, model)
            };

            let ms = start_time.elapsed().as_millis() as u32;
            ha.ok(&state, &channel, &url, ms).await;
            return Ok(upstream_headers::json_with_upstream_headers(
                &upstream_hdrs,
                final_body,
            ));
        } // end if is_stream
    } // end while ha.cont()

    Err(ha
        .finish(&crate::relay::ha::HaBillCtx::new(&state, &token, model, raw_path).category("聊天"))
        .await)
}

// ── Responses API (/v1/responses, /api/v3/responses) ─────────
// 直接透传请求体到上游，不做格式转换，复用聊天类别的计费和日志体系

pub async fn responses_create(
    State(state): State<Arc<AppState>>,
    Extension(token): Extension<ApiToken>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<serde_json::Value>,
) -> AppResult<Response> {
    let raw_path = uri.path();
    let request_content_str = serde_json::to_string(&body).unwrap_or_default();
    let model = body["model"]
        .as_str()
        .ok_or_else(|| AppError::BadRequest("Missing required parameter: model".to_string()))?;
    let is_stream = body["stream"].as_bool().unwrap_or(false);

    let ctx = proxy::get_user_context(&state, &token.user_id).await?;
    proxy::check_model_permission(&state, &token, model, "/v1/responses", Some("聊天")).await?;
    //start patch @bobcat
    let mut replace = crate::patch::maybe_replace(&state, &token.user_id, model, Some("聊天")).await;
    //end patch
    let mut ha = crate::relay::ha::HaAttempt::begin(&state, token.high_availability).await;

    while ha.cont() {
        let start_time = std::time::Instant::now();
        let channel = match crate::patch::select_channel_for_replace(
            &state,
            &token,
            &mut replace,
            &ctx.user_group,
            &ctx.level_id,
            raw_path,
            &ha.exclude_aids,
            !ha.had_upstream,
            ha.had_upstream,
            Some("聊天"),
        )
        .await
        {
            Ok(c) => c,
            Err(e) => {
                ha.on_select_err(e);
                break;
            }
        };
        let (pre_deduction, db_model, resolved_cat) =
            match proxy::check_access(&state, &token, model, &ctx, Some("聊天"), Some(&channel))
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
            .fwd_db_model(&state, Some("聊天"), Some(&channel), db_model.as_ref())
            .await;
        //end patch
        // 模型映射：Responses 无分辨率档，跳过 body 解析
        let (resolved_model, mapping_source) =
            router::resolve_model(&channel, route_model, route_db_model.as_ref(), None);

        // 解析转发规则：复用聊天类别，兜底使用 /v1/responses 路径
        let resolved = match forward::resolve_forward_rule(
            &state,
            route_model,
            &resolved_cat,
            raw_path,
            Some(&channel),
            route_db_model.as_ref(),
        )
        .await
        {
            Some(r) => r,
            None => {
                let url_lower = channel.base_url.to_lowercase();
                if url_lower.contains("volces.com") || url_lower.contains("volcengine") {
                    forward::make_forward("volcengine_chat", "/api/v3/responses", "bearer")
                } else {
                    forward::default_openai_forward("/v1/responses")
                }
            }
        };

        let mut db_rule =
            proxy::get_model_billing_rule(&state, model, Some(&channel), db_model.as_ref()).await;

        // 构建上游请求体：仅替换 model 字段，其余透传
        let mut upstream_body = body.clone();
        upstream_body["model"] = serde_json::json!(resolved_model);

        let url = forward::build_upstream_url(
            &channel.base_url,
            &resolved,
            &resolved_model,
            &channel.api_key,
        );
        let auth_headers = forward::build_auth_headers(&resolved, &channel.api_key, true);

        tracing::info!(
            "[Responses] 模型={} 映射模型={} URL={}",
            model,
            resolved_model,
            url
        );

        let resolved_upstream_path = resolved.upstream_path.replace("${model}", &resolved_model);
        let ep = format!("{}|{}", raw_path, resolved_upstream_path);

        // 【连接保护】请求发送+响应处理+预扣+计费放独立 task，客户端断开后仍能完成
        let mut features = usage_extractor::extract_request_features(&body);
        let upstream_body_str = upstream_body.to_string();
        let pending_log_id = ha.pending_log_id;
        let timeout_ctx = ha.timeout_ctx();
        let fail_buf = ha.buf();

        let result_rx = super::spawn_protected({
            let state = state.clone();
            let token = token.clone();
            let request_content_str = request_content_str.clone();
            let channel = channel.clone();
            let model = model.to_string();
            let url = url.clone();
            let ep = ep.clone();
            let resolved_eid = resolved.eid.clone();
            let db_model = db_model.clone();
            let ctx = ctx.clone();
            let raw_path = raw_path.to_string();
            let mapping_source = mapping_source;
            async move {
                let existing_pending = pending_log_id;

                // pending_log_id + Result 一并返回，供外层 HA 复用
                let pending_log_future = async {
                    if let Some(id) = existing_pending {
                        Some(id)
                    } else {
                        proxy::record_pending_log(proxy::PendingLog {
                            state: &state,
                            user_id: &token.user_id,
                            token_id: token.id,
                            model: &model,
                            endpoint: &ep,
                            is_stream: if is_stream { 1 } else { 0 },
                            request_content: Some(&request_content_str),
                            upstream_url: Some(&url),
                            channel: &channel,
                            billing_model_hint: None,
                            plugin_tag: None,
                            category: Some("聊天"),
                            db_model: db_model.as_ref(),
                            forward_eid: Some(&resolved_eid),
                            requested_log_id: None,
                        })
                        .await
                    }
                };

                // 统一构建请求（流式/非流式共用 builder，仅请求体不同）
                let mut req_body = upstream_body;
                if is_stream {
                    req_body["stream"] = serde_json::json!(true);
                }

                let builder = state
                    .http_client
                    .post(&url)
                    .header("Content-Type", "application/json");
                let builder = auth_headers
                    .into_iter()
                    .fold(builder, |b, (k, v)| b.header(k, v));
                let builder = crate::services::http_client::with_timeout_if(
                    builder,
                    !is_stream,
                    timeout_ctx.resolve(),
                );
                let send_future = builder.json(&req_body).send();

                let (log_res, resp_res) = tokio::join!(pending_log_future, send_future);
                let pending_log_id = log_res.or(existing_pending);

                let resp = match resp_res {
                    Ok(r) => r,
                    Err(e) => {
                        let err_msg = e.to_string();
                        let latency_ms = start_time.elapsed().as_millis() as u32;
                        let bill = crate::relay::ha::FailBill::transport(
                            latency_ms,
                            err_msg,
                            &request_content_str,
                            upstream_body_str.clone(),
                        )
                        .stream(if is_stream { 1 } else { 0 });
                        return (
                            pending_log_id,
                            Err(crate::relay::ha::HaAttempt::park(&fail_buf, bill, None)),
                        );
                    }
                };

                if !resp.status().is_success() {
                    let status = resp.status().as_u16();
                    let upstream_hdrs = resp.headers().clone();
                    let err_body = resp.text().await.unwrap_or_default();
                    let latency_ms = start_time.elapsed().as_millis() as u32;
                    let bill = crate::relay::ha::FailBill::http(
                        latency_ms,
                        status,
                        err_body,
                        &request_content_str,
                        upstream_body_str.clone(),
                    )
                    .stream(if is_stream { 1 } else { 0 });
                    return (
                        pending_log_id,
                        Err(crate::relay::ha::HaAttempt::park(
                            &fail_buf,
                            bill,
                            Some(upstream_hdrs),
                        )),
                    );
                }

                // 判断是否为流式响应（请求流式 或 上游实际返回 SSE）
                let actual_stream = is_stream || upstream_headers::is_sse(resp.headers());

                if actual_stream {
                    // 流式路径：预扣费后交给 handle_responses_stream（内部有独立 worker 处理流+计费）
                    let pre_deduct_gift = proxy::pre_deduct_or_intercept(
                        &state,
                        &token,
                        &channel,
                        &model,
                        pre_deduction,
                        &ep,
                        start_time,
                        1,
                        &request_content_str,
                        &upstream_body_str,
                        None,
                        pending_log_id,
                        db_model.as_ref(),
                        Some("聊天"),
                    )
                    .await;
                    let pre_deduct_gift = match pre_deduct_gift {
                        Ok(v) => v,
                        Err(e) => return (pending_log_id, Err(e)),
                    };

                    (
                        pending_log_id,
                        Ok(stream::handle_responses_stream(
                            state.clone(),
                            token,
                            channel,
                            model.clone(),
                            resp,
                            ctx.clone(),
                            request_content_str,
                            start_time,
                            resolved_upstream_path,
                            Some(upstream_body_str),
                            pre_deduction,
                            pre_deduct_gift,
                            raw_path,
                            pending_log_id,
                            db_model,
                            db_rule,
                        )
                        .await
                        .into_response()),
                    )
                } else {
                    // 非流式：直接透传响应，提取 usage 计费
                    let upstream_hdrs = resp.headers().clone();
                    let data = resp.bytes().await.unwrap_or_default();
                    let mut response_content_str = String::from_utf8_lossy(&data).to_string();

                    // 上游 body 级错误检测（HTTP 200 但业务失败，在预扣费之前拦截）
                    let (converted, post_err) = forward::check_upstream_post_error(
                        &resolved.target_type,
                        &response_content_str,
                        resolved_cat.as_str(),
                        false,
                    );
                    response_content_str = converted;
                    if let Some(err_response) = post_err {
                        let latency_ms = start_time.elapsed().as_millis() as u32;
                        let bill = crate::relay::ha::FailBill::biz(
                            latency_ms,
                            response_content_str,
                            err_response,
                            &request_content_str,
                            upstream_body_str.clone(),
                        );
                        return (
                            pending_log_id,
                            Err(crate::relay::ha::HaAttempt::park(
                                &fail_buf,
                                bill,
                                Some(upstream_hdrs),
                            )),
                        );
                    }

                    let usage_tokens = usage_extractor::parse_usage(&response_content_str);
                    let prompt_tokens = usage_tokens.prompt;
                    let completion_tokens = usage_tokens.completion;
                    let cached_tokens = usage_tokens.cached;

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
                        &upstream_body_str,
                        None,
                        pending_log_id,
                        db_model.as_ref(),
                        Some("聊天"),
                    )
                    .await;
                    let pre_deduct_gift = match pre_deduct_gift {
                        Ok(v) => v,
                        Err(e) => return (pending_log_id, Err(e)),
                    };

                    usage_extractor::enrich_features_from_usage(&mut features, &usage_tokens);

                    let (quota_used, detail) = crate::relay::calculate_relay_cost(
                        &state,
                        db_model.as_ref(),
                        db_rule.as_mut(),
                        &channel,
                        &ctx,
                        &usage_tokens,
                        &features,
                        mapping_source.as_deref(),
                        &model,
                        &resolved_model,
                    )
                    .await;

                    let latency_ms = start_time.elapsed().as_millis() as u32;

                    proxy::record_and_bill_inner(proxy::BillRecord {
                        state: &state,
                        token: &token,
                        channel: &channel,
                        model: &model,
                        prompt_tokens: prompt_tokens,
                        completion_tokens: completion_tokens,
                        cached_tokens: cached_tokens,
                        cost: quota_used,
                        pre_deducted: pre_deduction,
                        pre_deduct_gift: pre_deduct_gift,
                        status_code: 200,
                        endpoint: &ep,
                        error_msg: None,
                        latency_ms,
                        is_stream: 0,
                        request_content: Some(request_content_str),
                        response_content: Some(response_content_str.clone()),
                        upstream_req_content: Some(upstream_body_str),
                        billing_detail: Some(detail),
                        hint_category: Some("聊天"),
                        pending_log_id: pending_log_id,
                        billing_model_hint: None,
                        plugin_tag: None,
                        db_model: db_model.as_ref(),
                        time_multiplier: db_rule.as_ref().map(|r| r.applied_multiplier),
                    })
                    .await;

                    // Responses API 直接透传上游响应，不做格式转换
                    (
                        pending_log_id,
                        Ok(upstream_headers::json_with_upstream_headers(
                            &upstream_hdrs,
                            response_content_str,
                        )),
                    )
                }
            }
        });

        match result_rx.await {
            Ok((returned_log_id, result)) => {
                ha.set_pending(returned_log_id);
                match result {
                    Ok(resp) => {
                        let ms = start_time.elapsed().as_millis() as u32;
                        ha.ok(&state, &channel, &url, ms).await;
                        return Ok(resp);
                    }
                    Err(e) => {
                        if ha
                            .fail(
                                &crate::relay::ha::HaBillCtx::new(&state, &token, model, &ep)
                                    .category("聊天")
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
                }
            }
            Err(_) => {
                ha.last_err = AppError::Internal("请求处理任务异常终止".into());
                break;
            }
        }
    } // end while

    Err(ha
        .finish(&crate::relay::ha::HaBillCtx::new(&state, &token, model, raw_path).category("聊天"))
        .await)
}

// ── 公共辅助函数 ──────────────────────────────────────────────

/// 粗略估算 prompt tokens（兼容 Chat 的 messages 和 Responses 的 input）
pub fn estimate_prompt_tokens(body: &serde_json::Value) -> i32 {
    let mut total_chars = 0;
    // Chat Completions: messages 数组
    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in messages {
            if let Some(s) = msg.get("content").and_then(|c| c.as_str()) {
                total_chars += s.len();
            }
        }
    }
    // Responses API: input 字段（string 或 array）
    if let Some(input) = body.get("input") {
        if let Some(s) = input.as_str() {
            total_chars += s.len();
        } else if let Some(arr) = input.as_array() {
            for item in arr {
                if let Some(s) = item.get("text").and_then(|t| t.as_str()) {
                    total_chars += s.len();
                } else if let Some(s) = item.get("content").and_then(|c| c.as_str()) {
                    total_chars += s.len();
                }
            }
        }
    }
    // instructions 字段
    if let Some(s) = body.get("instructions").and_then(|i| i.as_str()) {
        total_chars += s.len();
    }
    (total_chars as f64 / 4.0).ceil() as i32
}

/// 将上游非 OpenAI 格式响应转换为 OpenAI 格式
fn transform_chat_response(response: &str, target_type: &str, model: &str) -> String {
    match target_type {
        "anthropic" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(response) {
                let content = v
                    .get("content")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| {
                        arr.iter()
                            .filter(|c| c.get("type").and_then(|t| t.as_str()) == Some("text"))
                            .map(|c| c.get("text").and_then(|t| t.as_str()).unwrap_or(""))
                            .next()
                    })
                    .unwrap_or("");
                let usage_tokens = usage_extractor::parse_usage(response);
                let mut usage = serde_json::json!({
                    "prompt_tokens": usage_tokens.prompt,
                    "completion_tokens": usage_tokens.completion,
                    "total_tokens": usage_tokens.total,
                    "cache_creation": v.get("usage").and_then(|c| c.get("cache_creation"))
                });
                // 映射 Anthropic 缓存字段到 OpenAI prompt_tokens_details
                if usage_tokens.cached > 0 || usage_tokens.cache_creation > 0 {
                    usage["prompt_tokens_details"] = serde_json::json!({
                        "cached_tokens": usage_tokens.cached,
                        "cache_creation_tokens": usage_tokens.cache_creation
                    });
                }
                return serde_json::to_string(&serde_json::json!({
                    "id": v.get("id").and_then(|i| i.as_str()).unwrap_or(""),
                    "object": "chat.completion",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
                    "usage": usage
                })).unwrap_or_else(|_| response.to_string());
            }
            response.to_string()
        }
        "gemini" | "gemini_image" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(response) {
                let content = v
                    .get("candidates")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("content"))
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.get(0))
                    .and_then(|p| p.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                let finish = v
                    .get("candidates")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("finishReason"))
                    .and_then(|f| f.as_str())
                    .unwrap_or("stop");
                let usage_tokens = usage_extractor::parse_usage(response);
                let mut usage = serde_json::json!({
                    "prompt_tokens": usage_tokens.prompt,
                    "completion_tokens": usage_tokens.completion,
                    "total_tokens": usage_tokens.total,
                });
                if usage_tokens.cached > 0 {
                    usage["prompt_tokens_details"] = serde_json::json!({
                        "cached_tokens": usage_tokens.cached
                    });
                }
                return serde_json::to_string(&serde_json::json!({
                    "id": uuid::Uuid::new_v4().to_string(),
                    "object": "chat.completion",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": finish}],
                    "usage": usage
                })).unwrap_or_else(|_| response.to_string());
            }
            response.to_string()
        }
        _ => {
            // 兜底：检测上游是否返回了 Anthropic 原生格式（type:"message"），自动转为 OpenAI 格式
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(response) {
                if v.get("type").and_then(|t| t.as_str()) == Some("message") {
                    return transform_chat_response(response, "anthropic", model);
                }
            }
            response.to_string()
        }
    }
}

// ── 辅助提炼函数（精简冗余、解耦核心流程） ──────────────────────────

/// 上游失败：暂存账单 → [`HaAttempt::fail`]（HA 中间不写 logs）
async fn chat_on_upstream_fail(
    ha: &mut crate::relay::ha::HaAttempt,
    ctx: &crate::relay::ha::HaBillCtx<'_>,
    channel: &crate::models::Channel,
    bill: crate::relay::ha::FailBill,
    url: Option<&str>,
    headers: Option<axum::http::HeaderMap>,
) -> bool {
    let err = crate::relay::ha::HaAttempt::park(&ha.buf(), bill, headers);
    ha.fail(ctx, channel, err, url).await
}
