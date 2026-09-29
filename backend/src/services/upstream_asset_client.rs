/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 上游渠道素材接口客户端（Bearer + Action/Version 查询参数）
//!
//! 用于 upstream_asset_relay：对绑定渠道 base_url(+可选 path) 发起 CreateAsset/GetAsset/DeleteAsset。

use crate::relay::url_utils::join_url;
use crate::services::volcengine::normalize_ark_asset_id;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const ASSET_API_VERSION: &str = "2024-01-01";

/// 写入 `plugin_api_logs` / `plugin_assets.source` 的来源标识
pub const LOG_SOURCE: &str = "upstream_relay_convert";
/// plugins.name
pub const PLUGIN_NAME: &str = "upstream_asset_relay";

/// 绑定隔离命名空间：`uar:{binding_id}`
pub fn binding_ns(binding_id: i64) -> String {
    format!("uar:{}", binding_id)
}

/// 从 `uar:{id}` 解析绑定 ID
fn parse_binding_id_from_ns(plugin_ns: &str) -> Option<i64> {
    plugin_ns
        .strip_prefix("uar:")
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|id| *id > 0)
}

/// 拼装素材接口基础 endpoint（不含 Action/Version）。
/// - `asset_base_path` 为空：直接使用渠道 `base_url`（去尾 `/`），禁止走 join_url("",)。
/// - `asset_base_path` 是绝对 URL（`http://` / `https://` 开头）：直接作为 endpoint，
///   原样保留（含尾斜杠——乐信素材路径 `/assets/` 必须带尾 `/`），不与 `base_url` 拼接。
///   用于素材域名与模型域名分离的上游（如乐信
///   视频 `https://api.591ll.com/ai/api` / 素材 `https://mintel.591ll.com/render/api`），
///   凭证仍取自同一渠道配置。join_url 是纯字符串拼接、不识别绝对 URL，
///   不在此前置拦住会拼出 `.../ai/api/https:/mintel...` 这种静默 404 的地址。
/// - 其余情况：按相对路径拼在 `base_url` 之后。
pub fn build_asset_endpoint(base_url: &str, asset_base_path: &str) -> String {
    let path = asset_base_path.trim();
    if path.is_empty() {
        return base_url.trim_end_matches('/').to_string();
    }
    if is_absolute_url(path) {
        return path.to_string();
    }
    join_url(base_url, path)
}

/// 判定 `asset_base_path` 是否已写成绝对 URL（大小写不敏感，避开 `HTTPS://` 绕过）
pub fn is_absolute_url(path: &str) -> bool {
    let lower = path.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// 在 endpoint 上追加 Action/Version；若已有 query 则用 `&`。
pub fn append_action_query(endpoint: &str, action: &str) -> String {
    let sep = if endpoint.contains('?') { '&' } else { '?' };
    format!(
        "{}{}Action={}&Version={}",
        endpoint, sep, action, ASSET_API_VERSION
    )
}

/// 从响应中取字段：优先 `Result.<key>`，其次顶层 `<key>`。
pub fn extract_result_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .pointer(&format!("/Result/{}", key))
        .and_then(|v| v.as_str())
        .or_else(|| value.get(key).and_then(|v| v.as_str()))
}

fn spawn_api_log(
    db: crate::db::Database,
    user_id: String,
    plugin_name: String,
    action: String,
    request_payload: String,
    response_payload: String,
    status_code: i32,
) {
    tokio::spawn(async move {
        let _ = sqlx::query(&db.format_query(
            "INSERT INTO plugin_api_logs (user_id, plugin_name, api_endpoint, request_payload, response_payload, status_code, source) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        ))
        .bind(&user_id)
        .bind(&plugin_name)
        .bind(&action)
        .bind(&request_payload)
        .bind(&response_payload)
        .bind(status_code)
        .bind(LOG_SOURCE)
        .execute(&db.pool)
        .await;
    });
}

/// 绑定级鉴权配置。`auth_mode` 空或 `bearer` → 用 `api_key` 走 Bearer（存量默认）；
/// `volc_v4` → 用 AK/SK 做火山 Signature V4（复用 volcengine_sign），供乐信等
/// 以 ark V4 鉴权、域名却非火山官方的上游。region/service 空时默认 cn-beijing / ark。
#[derive(Clone, Debug, Default)]
pub struct BindingAuth {
    pub auth_mode: String,
    pub api_key: String,
    pub access_key: String,
    pub secret_key: String,
    pub sign_region: String,
    pub sign_service: String,
}

impl BindingAuth {
    pub fn is_v4(&self) -> bool {
        self.auth_mode.eq_ignore_ascii_case("volc_v4")
    }
    fn region(&self) -> &str {
        let r = self.sign_region.trim();
        if r.is_empty() { "cn-beijing" } else { r }
    }
    fn service(&self) -> &str {
        let s = self.sign_service.trim();
        if s.is_empty() { "ark" } else { s }
    }
}

/// load_binding_endpoints 的单绑定结果：endpoint + 鉴权配置。
#[derive(Clone, Debug)]
pub struct BindingEndpoint {
    pub endpoint: String,
    pub auth: BindingAuth,
}

/// Bearer / V4 调用上游素材 Action 所需上下文（避免过多参数）。
/// `auth` 为 None 或 bearer 模式时走 Bearer（api_key）；volc_v4 时走 AK/SK 签名。
pub struct UpstreamCallCtx<'a> {
    pub http: &'a reqwest::Client,
    pub db: &'a crate::db::Database,
    pub user_id: &'a str,
    pub plugin_name: &'a str,
    pub endpoint_base: &'a str,
    pub api_key: &'a str,
    pub auth: Option<&'a BindingAuth>,
}

/// 按鉴权模式给请求 builder 施加鉴权头。
/// volc_v4：解析 url 得 host/path/query，对 (method, url, body_bytes) 计算火山 V4 签名，
/// payload_hash 与实际发送 body 字节严格一致（调用方须用同一 bytes 作为 body）；
/// 否则回退 Bearer。Content-Type 由调用方设置（V4 canonical_headers 依赖 application/json）。
fn apply_auth(
    builder: reqwest::RequestBuilder,
    method: &reqwest::Method,
    url: &str,
    body_bytes: &[u8],
    ctx: &UpstreamCallCtx<'_>,
) -> Result<reqwest::RequestBuilder> {
    if let Some(a) = ctx.auth {
        if a.is_v4() {
            let parsed = reqwest::Url::parse(url)
                .map_err(|e| anyhow!("上游素材 URL 解析失败: {} - {}", e, url))?;
            let host = match parsed.port() {
                Some(p) => format!("{}:{}", parsed.host_str().unwrap_or_default(), p),
                None => parsed.host_str().unwrap_or_default().to_string(),
            };
            let path = parsed.path();
            let query = parsed.query().unwrap_or("");
            let (authorization, x_date, payload_hash) =
                crate::services::volcengine::volcengine_sign(
                    &a.access_key,
                    &a.secret_key,
                    method.as_str(),
                    &host,
                    path,
                    query,
                    a.service(),
                    a.region(),
                    body_bytes,
                );
            return Ok(builder
                .header("X-Date", &x_date)
                .header("X-Content-Sha256", &payload_hash)
                .header("Authorization", authorization));
        }
    }
    Ok(builder.header("Authorization", format!("Bearer {}", ctx.api_key)))
}

/// Bearer 调用上游素材 Action（带日志），返回完整 JSON。
pub async fn call_action_logged(
    ctx: &UpstreamCallCtx<'_>,
    action: &str,
    body: &Value,
) -> Result<Value> {
    let url = append_action_query(ctx.endpoint_base, action);
    let req_payload = body.to_string();
    let body_bytes = serde_json::to_vec(body).unwrap_or_default();
    let builder = ctx
        .http
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body_bytes.clone());
    let builder = apply_auth(builder, &reqwest::Method::POST, &url, &body_bytes, ctx)?;
    let res = crate::services::http_client::with_upstream_timeout(builder)
        .send()
        .await?;
    let status_code = res.status().as_u16() as i32;
    let text = res.text().await.unwrap_or_default();
    spawn_api_log(
        ctx.db.clone(),
        ctx.user_id.to_string(),
        ctx.plugin_name.to_string(),
        action.to_string(),
        req_payload,
        text.clone(),
        status_code,
    );
    if !(200..300).contains(&status_code) {
        return Err(anyhow!("上游素材接口错误: {} - {}", status_code, text));
    }
    serde_json::from_str(&text).map_err(|e| anyhow!("解析上游素材响应失败: {} - {}", e, text))
}

/// 通用 Bearer 调用（指定 HTTP 方法 + 相对路径 + 可选请求体），供 asset_api_profile 描述符分支使用。
/// `method` 为 reqwest::Method；`body` 为 None 时不发送请求体（GET/DELETE）。
/// 与 call_action_logged 一致记录完整日志。
pub async fn call_upstream_http(
    ctx: &UpstreamCallCtx<'_>,
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value> {
    let url = join_url(ctx.endpoint_base, path);
    let req_payload = body.map(|b| b.to_string()).unwrap_or_default();
    let body_bytes = body
        .map(|b| serde_json::to_vec(b).unwrap_or_default())
        .unwrap_or_default();
    let sign_method = method.clone();
    let mut builder = ctx.http.request(method, &url);
    if body.is_some() {
        builder = builder
            .header("Content-Type", "application/json")
            .body(body_bytes.clone());
    }
    builder = apply_auth(builder, &sign_method, &url, &body_bytes, ctx)?;
    let res = crate::services::http_client::with_upstream_timeout(builder)
        .send()
        .await?;
    let status_code = res.status().as_u16() as i32;
    let text = res.text().await.unwrap_or_default();
    spawn_api_log(
        ctx.db.clone(),
        ctx.user_id.to_string(),
        ctx.plugin_name.to_string(),
        format!("{} {}", ctx.plugin_name, path),
        req_payload,
        text.clone(),
        status_code,
    );
    if !(200..300).contains(&status_code) {
        return Err(anyhow!("上游素材接口错误: {} - {}", status_code, text));
    }
    serde_json::from_str(&text).map_err(|e| anyhow!("解析上游素材响应失败: {} - {}", e, text))
}

/// 读取绑定的 asset_api_profile 描述符（无配置返回 None）。
pub async fn load_binding_profile(
    db: &crate::db::Database,
    binding_id: i64,
) -> Option<crate::relay::asset_api_profile::AssetApiProfile> {
    let sql = db.format_query(
        "SELECT asset_api_profile FROM upstream_asset_bindings WHERE id = ?",
    );
    let row: Option<(Option<String>,)> = sqlx::query_as(&sql)
        .bind(binding_id)
        .fetch_optional(&db.pool)
        .await
        .unwrap_or(None);
    let raw = row.and_then(|(p,)| p).unwrap_or_default();
    crate::relay::asset_api_profile::AssetApiProfile::parse(&raw)
}

#[derive(sqlx::FromRow)]
struct BindingCredRow {
    id: i64,
    asset_base_path: String,
    base_url: Option<String>,
    api_key: Option<String>,
    auth_mode: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
    sign_region: Option<String>,
    sign_service: Option<String>,
}

/// 一次查出多个绑定的 endpoint + 鉴权配置，避免按绑定 N 次查询。
/// 供 ark_asset_proxy 上游素材透传分支复用（凭证实时读库，换上游改库即生效）。
pub async fn load_binding_endpoints(
    db: &crate::db::Database,
    binding_ids: &[i64],
) -> HashMap<i64, BindingEndpoint> {
    let mut out = HashMap::new();
    if binding_ids.is_empty() {
        return out;
    }
    let ph = binding_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql = db.format_query(&format!(
        "SELECT b.id, b.asset_base_path, c.base_url, c.api_key, \
                b.auth_mode, b.access_key, b.secret_key, b.sign_region, b.sign_service \
         FROM upstream_asset_bindings b \
         LEFT JOIN channel_configs c ON c.id = b.channel_config_id \
         WHERE b.id IN ({ph}) AND COALESCE(c.status, 1) = 1"
    ));
    let mut q = sqlx::query_as::<_, BindingCredRow>(&sql);
    for id in binding_ids {
        q = q.bind(id);
    }
    let Ok(rows) = q.fetch_all(&db.pool).await else {
        return out;
    };
    for row in rows {
        let base_url = row.base_url.unwrap_or_default();
        if base_url.trim().is_empty() {
            continue;
        }
        let auth = BindingAuth {
            auth_mode: row.auth_mode.unwrap_or_default(),
            api_key: row.api_key.unwrap_or_default(),
            access_key: row.access_key.unwrap_or_default(),
            secret_key: row.secret_key.unwrap_or_default(),
            sign_region: row.sign_region.unwrap_or_default(),
            sign_service: row.sign_service.unwrap_or_default(),
        };
        // V4 需 AK/SK；Bearer 需 api_key。缺凭证的绑定视为不可用（与旧行为一致：不静默 401）
        if auth.is_v4() {
            if auth.access_key.trim().is_empty() || auth.secret_key.trim().is_empty() {
                continue;
            }
        } else if auth.api_key.trim().is_empty() {
            continue;
        }
        out.insert(
            row.id,
            BindingEndpoint {
                endpoint: build_asset_endpoint(&base_url, &row.asset_base_path),
                auth,
            },
        );
    }
    out
}

struct UpstreamDeleteJob {
    binding_id: i64,
    plugin_ns: String,
    endpoint: String,
    api_key: String,
    auth: BindingAuth,
    asset_ids: Vec<String>,
}

/// DeleteAsset 条间间隔，与方舟清理一致，避免打满上游流控。
const UPSTREAM_DELETE_GAP_MS: u64 = 500;

/// 删库前解析凭证并异步 DeleteAsset；失败仅 info，不阻塞本地删除。
/// `items` 为 `(asset_id, plugin_ns)`。条间节流；遇限流停止剩余。
pub async fn spawn_best_effort_delete_items(
    http: reqwest::Client,
    db: crate::db::Database,
    operator_uid: String,
    items: Vec<(String, String)>,
) {
    let jobs = prepare_delete_jobs(&db, items).await;
    if jobs.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let _ = run_delete_jobs(&http, &db, &operator_uid, jobs).await;
    });
}

/// 同步节流 DeleteAsset；`true` 表示遇限流应停止本轮清理（勿再删本地）。
pub async fn delete_items_throttled(
    http: reqwest::Client,
    db: crate::db::Database,
    operator_uid: &str,
    items: Vec<(String, String)>,
) -> bool {
    let jobs = prepare_delete_jobs(&db, items).await;
    if jobs.is_empty() {
        return false;
    }
    run_delete_jobs(&http, &db, operator_uid, jobs).await
}

async fn prepare_delete_jobs(
    db: &crate::db::Database,
    items: Vec<(String, String)>,
) -> Vec<UpstreamDeleteJob> {
    let mut by_ns: HashMap<String, HashSet<String>> = HashMap::new();
    for (aid, ns) in items {
        let id = normalize_ark_asset_id(&aid);
        if !id.is_empty() {
            by_ns.entry(ns).or_default().insert(id.to_string());
        }
    }
    if by_ns.is_empty() {
        return Vec::new();
    }

    let mut ns_binding: Vec<(String, i64, HashSet<String>)> = Vec::with_capacity(by_ns.len());
    let mut binding_ids = Vec::new();
    let mut seen_bid = HashSet::new();
    for (ns, aids) in by_ns {
        let Some(binding_id) = parse_binding_id_from_ns(&ns) else {
            tracing::info!(
                "[UpstreamAsset] DeleteAsset 跳过: 无法解析绑定 ns={} ({} 个)",
                ns,
                aids.len()
            );
            continue;
        };
        if seen_bid.insert(binding_id) {
            binding_ids.push(binding_id);
        }
        ns_binding.push((ns, binding_id, aids));
    }
    if ns_binding.is_empty() {
        return Vec::new();
    }

    let creds = load_binding_endpoints(db, &binding_ids).await;
    let mut jobs = Vec::with_capacity(ns_binding.len());
    for (ns, binding_id, aids) in ns_binding {
        let Some(be) = creds.get(&binding_id).cloned() else {
            tracing::info!(
                "[UpstreamAsset] DeleteAsset 跳过: 绑定#{} 渠道凭证不可用 ({} 个)",
                binding_id,
                aids.len()
            );
            continue;
        };
        jobs.push(UpstreamDeleteJob {
            binding_id,
            plugin_ns: ns,
            api_key: be.auth.api_key.clone(),
            endpoint: be.endpoint,
            auth: be.auth,
            asset_ids: aids.into_iter().collect(),
        });
    }
    jobs
}

/// 执行 DeleteAsset；`true` = 遇限流已中止。
async fn run_delete_jobs(
    http: &reqwest::Client,
    db: &crate::db::Database,
    operator_uid: &str,
    jobs: Vec<UpstreamDeleteJob>,
) -> bool {
    let gap = std::time::Duration::from_millis(UPSTREAM_DELETE_GAP_MS);
    let mut first = true;
    for job in jobs {
        let ctx = UpstreamCallCtx {
            http,
            db,
            user_id: operator_uid,
            plugin_name: &job.plugin_ns,
            endpoint_base: &job.endpoint,
            api_key: &job.api_key,
            auth: Some(&job.auth),
        };
        for aid in job.asset_ids {
            if !first {
                tokio::time::sleep(gap).await;
            }
            first = false;
            let body = json!({ "Id": aid });
            match call_action_logged(&ctx, "DeleteAsset", &body).await {
                Ok(_) => tracing::info!(
                    "[UpstreamAsset] DeleteAsset 成功: {} (绑定#{})",
                    aid,
                    job.binding_id
                ),
                Err(e) => {
                    let msg = e.to_string();
                    if crate::services::volcengine::VolcClient::is_api_rate_limited(&msg) {
                        tracing::info!(
                            "[UpstreamAsset] DeleteAsset 限流，本轮停止: {} (绑定#{}) - {}",
                            aid,
                            job.binding_id,
                            msg
                        );
                        return true;
                    }
                    tracing::info!(
                        "[UpstreamAsset] DeleteAsset 失败(不影响本地删除): {} (绑定#{}) - {}",
                        aid,
                        job.binding_id,
                        msg
                    );
                }
            }
        }
    }
    false
}
