/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

pub mod live_metrics;
pub mod rate_limit;
use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::header,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::auth;
use crate::error::AppError;
use crate::AppState;

/// API Key 脱敏：保留前8后4位，中间用 *** 替代
fn mask_key(key: &str) -> String {
    if key.len() <= 12 {
        return "***".to_string();
    }
    format!("{}***{}", &key[..8], &key[key.len() - 4..])
}

/// Token 查询命中结果
enum TokenMatch {
    Active(crate::models::ApiToken, String),
    Disabled(crate::models::ApiToken, String),
}

/// 按调用方给定的顺序尝试候选 key（Anthropic 规则下 x-api-key 优先），
/// 优先返回首个命中且启用的 token 及命中的 key；
/// 若候选 key 均未命中启用的 token，则返回首个命中的已禁用 token（上层需返回 403）；
/// 全部未命中返回 None
async fn lookup_api_token(
    state: &Arc<AppState>,
    candidate_keys: &[String],
) -> Result<Option<TokenMatch>, sqlx::Error> {
    let mut disabled: Option<(crate::models::ApiToken, String)> = None;
    for key in candidate_keys {
        match sqlx::query_as::<_, crate::models::ApiToken>(
            &state
                .db
                .format_query("SELECT * FROM api_tokens WHERE token_key = ?"),
        )
        .bind(key)
        .fetch_optional(&state.db.pool)
        .await?
        {
            Some(t) if t.is_active != 0 => return Ok(Some(TokenMatch::Active(t, key.clone()))),
            Some(t) => {
                if disabled.is_none() {
                    disabled = Some((t, key.clone()));
                }
            }
            None => continue,
        }
    }
    Ok(disabled.map(|(t, k)| TokenMatch::Disabled(t, k)))
}

/// Extract user claims from JWT token in Authorization header
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let auth_header = match request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        Some(h) => h,
        None => return AppError::Unauthorized.into_response(),
    };

    let token = match auth_header.strip_prefix("Bearer ") {
        Some(t) => t,
        None => return AppError::Unauthorized.into_response(),
    };

    let claims = match auth::validate_token(token, &state.config.jwt_secret) {
        Ok(c) => c,
        Err(_) => return AppError::Unauthorized.into_response(),
    };

    // 高频只读观测接口：仅校验 JWT，跳过 is_active 查库（减轻看板轮询对连接池压力）
    let path = request.uri().path();
    if path.ends_with("/metrics/live") {
        request.extensions_mut().insert(claims);
        return next.run(request).await;
    }

    // Verify user still exists and is active
    let is_active: Result<Option<i64>, sqlx::Error> = sqlx::query_scalar(
        &state
            .db
            .format_query("SELECT is_active FROM users WHERE id = ?"),
    )
    .bind(&claims.sub)
    .fetch_optional(&state.db.pool)
    .await;

    match is_active {
        Ok(Some(active)) if active != 0 => {
            request.extensions_mut().insert(claims);
            next.run(request).await
        }
        Ok(Some(_)) | Ok(None) => AppError::Unauthorized.into_response(),
        Err(e) => {
            tracing::error!("Database error in auth_middleware: {}", e);
            AppError::Internal("Database connection error".to_string()).into_response()
        }
    }
}

/// Require admin role
pub async fn admin_middleware(request: Request, next: Next) -> Response {
    let claims = match request.extensions().get::<auth::Claims>() {
        Some(c) => c,
        None => return AppError::Unauthorized.into_response(),
    };

    if claims.role != "admin" {
        return AppError::Forbidden("Admin access required".to_string()).into_response();
    }

    next.run(request).await
}

/// Normalize vendor-specific API auth formats to standard Authorization header
/// Supports: x-api-key / X-Api-Key (Anthropic & Volcengine)
///           x-goog-api-key (Google Gemini)
///           ?key=xxx query parameter (Google Gemini)
fn normalize_request_auth(request: &mut Request) {
    if request.headers().get(header::AUTHORIZATION).is_none() {
        // 1. Try x-api-key or X-Api-Key (Anthropic & Volcengine)
        if let Some(key) = request
            .headers()
            .get("x-api-key")
            .or_else(|| request.headers().get("X-Api-Key"))
            .and_then(|v| v.to_str().ok())
        {
            if let Ok(val) = format!("Bearer {}", key).parse() {
                request.headers_mut().insert(header::AUTHORIZATION, val);
                return;
            }
        }

        // 2. Try x-goog-api-key (Google Gemini)
        if let Some(key) = request
            .headers()
            .get("x-goog-api-key")
            .and_then(|v| v.to_str().ok())
        {
            if let Ok(val) = format!("Bearer {}", key).parse() {
                request.headers_mut().insert(header::AUTHORIZATION, val);
                return;
            }
        }

        // 3. Try ?key= query parameter (Google Gemini)
        if let Some(query) = request.uri().query() {
            for pair in query.split('&') {
                if let Some(key) = pair.strip_prefix("key=") {
                    if let Ok(val) = format!("Bearer {}", key).parse() {
                        request.headers_mut().insert(header::AUTHORIZATION, val);
                    }
                    break;
                }
            }
        }
    }
}

/// Extract API token (sk-xxx) for relay endpoints
pub async fn api_key_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    // 规范化各种厂商的认证头部/参数为标准 Authorization 格式
    normalize_request_auth(&mut request);

    let path = request
        .extensions()
        .get::<axum::extract::OriginalUri>()
        .map(|uri| uri.path().to_string())
        .unwrap_or_else(|| request.uri().path().to_string());
    // 余额查询等轻量只读接口无需记录错误日志
    let skip_log = path.ends_with("/balance");
    let auth_header: Option<String> = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    // Anthropic 规则鉴权优先级：x-api-key 最高；携带时优先按 x-api-key 鉴权，
    // 未命中（或无 x-api-key）再按 Authorization Bearer 鉴权。
    let x_api_key: Option<String> = request
        .headers()
        .get("x-api-key")
        .or_else(|| request.headers().get("X-Api-Key"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(str::to_string);

    let bearer_key: Option<String> = auth_header
        .as_deref()
        .and_then(|h| h.strip_prefix("Bearer "))
        .filter(|k| !k.is_empty())
        .map(str::to_string);

    // 候选 key 组装：x-api-key 优先，Authorization 次之（相同自动去重）
    let mut candidate_keys: Vec<String> = Vec::new();
    match (&x_api_key, &bearer_key) {
        (Some(xk), Some(b)) => {
            candidate_keys.push(xk.clone());
            if xk != b {
                candidate_keys.push(b.clone());
            }
        }
        (Some(xk), None) => candidate_keys.push(xk.clone()),
        (None, Some(b)) => candidate_keys.push(b.clone()),
        (None, None) => {
            if !skip_log && !path.ends_with("/health") && !path.ends_with("favicon.ico") {
                tracing::warn!("[Auth] {} | 缺少 Authorization 请求头", path);
                crate::relay::proxy::record_error_log(
                    &state,
                    "unknown",
                    None,
                    None,
                    "unknown",
                    401,
                    &path,
                    "Missing Authorization Header",
                    None,
                    None,
                )
                .await;
            }
            return AppError::AuthFailed("Missing Authorization Header".to_string())
                .into_response();
        }
    }

    // Authorization 存在但格式非法（非 Bearer 或空）：
    // 仅当 x-api-key 也不可用时才拒绝；否则改按 x-api-key 鉴权，避免误伤 Anthropic 客户端。
    if let Some(h) = auth_header.as_deref() {
        if bearer_key.is_none() {
            if x_api_key.is_none() {
                tracing::warn!(
                    "[Auth] {} | Bearer 格式错误, header={}",
                    path,
                    &h[..h.len().min(20)]
                );
                if !skip_log {
                    crate::relay::proxy::record_error_log(
                        &state,
                        "unknown",
                        None,
                        None,
                        "unknown",
                        401,
                        &path,
                        "Invalid Bearer Token Format",
                        None,
                        None,
                    )
                    .await;
                }
                return AppError::AuthFailed("Invalid Bearer Token Format".to_string())
                    .into_response();
            }
            tracing::warn!("[Auth] {} | Authorization 格式异常，改用 x-api-key 鉴权", path);
        }
    }

    let token: crate::models::ApiToken = match lookup_api_token(&state, &candidate_keys).await {
        Ok(Some(TokenMatch::Active(t, matched_key))) => {
            let via_x_api_key = x_api_key.as_deref() == Some(matched_key.as_str());
            if via_x_api_key {
                tracing::info!(
                    "[Auth] {} | x-api-key 验证通过: key={}, token_id={}, user={}",
                    path,
                    mask_key(&matched_key),
                    t.id,
                    t.user_id
                );
            } else if x_api_key.is_some() {
                tracing::info!(
                    "[Auth] {} | x-api-key 未命中, 回退 Authorization 验证通过: key={}, token_id={}, user={}",
                    path,
                    mask_key(&matched_key),
                    t.id,
                    t.user_id
                );
            } else {
                tracing::info!(
                    "[Auth] {} | 令牌验证通过: key={}, token_id={}, user={}",
                    path,
                    mask_key(&matched_key),
                    t.id,
                    t.user_id
                );
            }
            t
        }
        Ok(Some(TokenMatch::Disabled(t, matched_key))) => {
            tracing::warn!(
                "[Auth] {} | 令牌已禁用: key={}, token_id={}, user={}",
                path,
                mask_key(&matched_key),
                t.id,
                t.user_id
            );
            if !skip_log {
                crate::relay::proxy::record_error_log(
                    &state,
                    &t.user_id,
                    None,
                    Some(t.id),
                    "unknown",
                    403,
                    &path,
                    "Token disabled",
                    None,
                    None,
                )
                .await;
            }
            return AppError::Forbidden("Token disabled".to_string()).into_response();
        }
        Ok(None) => {
            tracing::warn!(
                "[Auth] {} | 无效 API Key: key={}",
                path,
                mask_key(&candidate_keys[0])
            );
            if !skip_log {
                crate::relay::proxy::record_error_log(
                    &state,
                    "unknown",
                    None,
                    None,
                    "unknown",
                    401,
                    &path,
                    "Invalid API Key",
                    None,
                    None,
                )
                .await;
            }
            return AppError::AuthFailed("Invalid API Key".to_string()).into_response();
        }
        Err(e) => return AppError::Internal(format!("Database error: {}", e)).into_response(),
    };

    // Check only_playground / only_playground_2026 restrict
    let only_pg = token.only_playground == 1;
    let only_pg2026 = token.only_playground_2026 == 1;
    if only_pg || only_pg2026 {
        let x_playground = request
            .headers()
            .get("x-playground")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let x_playground_2026 = request
            .headers()
            .get("x-playground-2026")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let pg_ok = x_playground == "1" || x_playground == "true";
        let pg2026_ok = x_playground_2026 == "1" || x_playground_2026 == "true";
        let allowed = match (only_pg, only_pg2026) {
            (true, true) => pg_ok || pg2026_ok,
            (true, false) => pg_ok,
            (false, true) => pg2026_ok,
            (false, false) => true,
        };
        if !allowed {
            let msg = if only_pg && only_pg2026 {
                "该令牌仅能在创作中心或创作中心2026内使用"
            } else if only_pg2026 {
                "该令牌仅能在创作中心2026内使用"
            } else {
                "该令牌仅能在创作中心内使用"
            };
            if !skip_log {
                crate::relay::proxy::record_error_log(
                    &state,
                    &token.user_id,
                    None,
                    Some(token.id),
                    "unknown",
                    403,
                    &path,
                    "This token is restricted to Playground use only",
                    None,
                    None,
                )
                .await;
            }
            return AppError::Forbidden(msg.to_string()).into_response();
        }
    }

    // Check expiry
    if token.is_expired() {
        if !skip_log {
            crate::relay::proxy::record_error_log(
                &state,
                &token.user_id,
                None,
                Some(token.id),
                "unknown",
                403,
                &path,
                "Token expired",
                None,
                None,
            )
            .await;
        }
        return AppError::Forbidden("Token expired".to_string()).into_response();
    }

    // 额度检查：GET（轮询/余额）与 DELETE（取消任务以释放冻结）在超额时仍放行
    let skip_quota_check = matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::DELETE
    );
    if !skip_quota_check {
        let site_tz = crate::relay::relay_settings::get_cached_site_timezone(&state.db).await;
        // 计费自然日以用户 timedisplay 为准（非站点全局、非 timesystem）
        let timedisplay = crate::api::date_helper::resolve_user_timedisplay_name(
            &state.db,
            &token.user_id,
            &site_tz,
        )
        .await;

        // 内存拦截器：DashMap miss 时从 DB hydration，覆盖日/周/月/总额度
        let limits = crate::relay::quota_memory::limits_from_token(&token);
        if let Err(e) = state
            .quota_memory
            .check_quota(&state.db, token.id, &timedisplay, &limits)
            .await
        {
            let err_msg = e.to_string();
            if !skip_log {
                crate::relay::proxy::record_error_log(
                    &state,
                    &token.user_id,
                    None,
                    Some(token.id),
                    "unknown",
                    403,
                    &path,
                    &err_msg,
                    None,
                    None,
                )
                .await;
            }
            return AppError::Forbidden(err_msg).into_response();
        }
    }

    // Check IP Whitelist
    if !token.allowed_ips.is_empty() {
        let client_ip: &str = request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .or_else(|| {
                request
                    .headers()
                    .get("x-real-ip")
                    .and_then(|v| v.to_str().ok())
            })
            .unwrap_or("127.0.0.1");

        let allowed: Vec<&str> = token.allowed_ips.split(',').collect();
        let mut is_allowed = false;
        for ip in allowed {
            if client_ip == ip.trim() {
                is_allowed = true;
                break;
            }
        }
        if !is_allowed {
            let msg = format!("IP {} not whitelisted", client_ip);
            if !skip_log {
                crate::relay::proxy::record_error_log(
                    &state,
                    &token.user_id,
                    None,
                    Some(token.id),
                    "unknown",
                    403,
                    &path,
                    &msg,
                    None,
                    None,
                )
                .await;
            }
            return AppError::Forbidden(msg).into_response();
        }
    }

    // Check Rate Limits
    if token.rps_limit > 0 {
        if !state.rate_limiter.check_rps(token.id, token.rps_limit) {
            if !skip_log {
                crate::relay::proxy::record_error_log(
                    &state,
                    &token.user_id,
                    None,
                    Some(token.id),
                    "unknown",
                    429,
                    &path,
                    "RPS limit exceeded",
                    None,
                    None,
                )
                .await;
            }
            return AppError::TooManyRequests("RPS limit exceeded".to_string()).into_response();
        }
    }

    if token.rpm_limit > 0 {
        if !state.rate_limiter.check_rpm(token.id, token.rpm_limit) {
            if !skip_log {
                crate::relay::proxy::record_error_log(
                    &state,
                    &token.user_id,
                    None,
                    Some(token.id),
                    "unknown",
                    429,
                    &path,
                    "RPM limit exceeded",
                    None,
                    None,
                )
                .await;
            }
            return AppError::TooManyRequests("RPM limit exceeded".to_string()).into_response();
        }
    }

    // 实时吞吐观测（QPS/RPM/Task）；Guard 挂到 Response 直至 body 结束
    let (global_guard, user_guard) = live_metrics::begin_request(&token.user_id, token.id);
    request.extensions_mut().insert(token);
    let mut response = next.run(request).await;
    response
        .extensions_mut()
        .insert(live_metrics::LiveMetricsTaskGuards::new(
            global_guard,
            user_guard,
        ));
    response
}
