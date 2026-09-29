/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 火山方舟视频素材 URL/base64→素材ID 自动转换模块
//!
//! 转发规则启用 `asset_convert` 或 `upstream_asset_convert` 时，在请求发往上游前扫描
//! content 中的 image_url/video_url/audio_url，注册为 `asset://<ASSET_ID>`。
//!
//! - http(s) URL：直接 CreateAsset（不下载整文件）
//! - base64：`data:` URI 或纯 base64 → 解码 → TOS 临时 URL → CreateAsset(URL) → 清理临时对象
//!   （两路径共用同一套文件处理；CreateAsset 始终只收公网 URL）
//!
//! 去重：URL 用 Range 元数据指纹（失败降级 URL 串）；base64 用内容 SHA-256。

use crate::services::upstream_asset_client as uac;
use crate::AppState;
use sha2::{Digest, Sha256};
use std::time::Duration;

/// content 元素中需要扫描转换的 URL 类型映射
/// (content.type 值, 内部 URL 对象 key, 火山方舟 AssetType)
const URL_TYPE_MAP: &[(&str, &str, &str)] = &[
    ("image_url", "image_url", "Image"),
    ("video_url", "video_url", "Video"),
    ("audio_url", "audio_url", "Audio"),
];

/// 日志用短 URL：data URI / 纯 base64 标注为 base64；过长则按字符边界截断。
fn shorten_url_for_log(url: &str) -> String {
    if is_base64_media(url) {
        return "base64数据".to_string();
    }
    if url.len() > 80 {
        let pos = url
            .char_indices()
            .nth(80)
            .map(|(i, _)| i)
            .unwrap_or(url.len());
        format!("{}...", &url[..pos])
    } else {
        url.to_string()
    }
}

#[inline]
fn is_http_media_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// data URI，或纯 base64 候选（与 `forward::parse_image_data` 一致：排除含 `.`/`:` 的 URL/路径形态）
fn is_base64_media(s: &str) -> bool {
    let s = s.trim();
    if s.starts_with("data:") {
        return true;
    }
    // http(s)/asset:// 等均含 ':'，自然排除
    s.len() >= 32 && !s.contains('.') && !s.contains(':')
}

/// base64 data URI 前缀与文件扩展名的映射
const BASE64_MIME_EXT: &[(&str, &str)] = &[
    ("data:image/png", "png"),
    ("data:image/jpeg", "jpg"),
    ("data:image/jpg", "jpg"),
    ("data:image/gif", "gif"),
    ("data:image/webp", "webp"),
    ("data:image/bmp", "bmp"),
    ("data:video/mp4", "mp4"),
    ("data:video/webm", "webm"),
    ("data:video/mov", "mov"),
    ("data:audio/mp3", "mp3"),
    ("data:audio/wav", "wav"),
    ("data:audio/mpeg", "mp3"),
    ("data:audio/ogg", "ogg"),
];

/// CreateAsset 的 Name：取 URL 末段文件名；无则 `tb_{type}_{hash8}`（代理/部分上游要求非空）
fn derive_create_asset_name(url: &str, asset_type: &str) -> String {
    let bare = url.split(['?', '#']).next().unwrap_or(url);
    let name = bare.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
    let name = if name.is_empty() || name == bare {
        let dig = Sha256::digest(url.as_bytes());
        format!(
            "tb_{}_{:02x}{:02x}{:02x}{:02x}",
            asset_type.to_ascii_lowercase(),
            dig[0],
            dig[1],
            dig[2],
            dig[3]
        )
    } else {
        name.to_string()
    };
    crate::services::volcengine::clamp_create_asset_name(&name)
}

/// 收集 content[] 中待转换项：(索引, url_key, asset_type, url, 日志短串)；跳过 asset://
fn collect_content_convert_tasks(
    content_arr: &[serde_json::Value],
) -> Vec<(usize, String, String, String, String)> {
    let mut tasks = Vec::new();
    for (idx, item) in content_arr.iter().enumerate() {
        let item_type = match item.get("type").and_then(|t| t.as_str()) {
            Some(t) => t,
            None => continue,
        };
        let (url_key, asset_type) = match URL_TYPE_MAP.iter().find(|(t, _, _)| *t == item_type) {
            Some((_, uk, at)) => (*uk, *at),
            None => continue,
        };
        let url_val = match item
            .get(url_key)
            .and_then(|u| u.get("url"))
            .and_then(|u| u.as_str())
        {
            Some(u) => u,
            None => continue,
        };
        if url_val.starts_with("asset://") {
            continue;
        }
        tasks.push((
            idx,
            url_key.to_string(),
            asset_type.to_string(),
            url_val.to_string(),
            shorten_url_for_log(url_val),
        ));
    }
    tasks
}

/// 写入 asset:// 并追加成功日志（含缓存标记）
fn push_convert_ok(
    content_arr: &mut [serde_json::Value],
    logs: &mut Vec<String>,
    idx: usize,
    url_key: &str,
    asset_type: &str,
    url_short: &str,
    asset_id: &str,
    cached: bool,
) {
    if let Some(url_obj) = content_arr
        .get_mut(idx)
        .and_then(|item| item.get_mut(url_key))
        .and_then(|u| u.as_object_mut())
    {
        url_obj.insert(
            "url".to_string(),
            serde_json::Value::String(format!("asset://{}", asset_id)),
        );
    }
    let tag = if cached { " [命中缓存]" } else { "" };
    logs.push(format!(
        "[{}] {} ✓ asset://{}{}",
        asset_type, url_short, asset_id, tag
    ));
}

/// 扫描 upstream_body 的 content 数组，将网络 URL / base64 数据转换为火山方舟素材 ID。
///
/// - http/https URL：Range 元数据指纹去重 → URL 去重 → CreateAsset（不下载整文件）
/// - data: / 纯 base64：解码 → SHA-256 哈希去重 → TOS 临时上传 → CreateAsset → 删除临时文件
/// - 已是 asset:// 前缀的跳过
/// - 转换失败时记录失败原因，由调用方决定是否拦截
/// 返回值: (转换日志, 失败原因列表)
pub async fn convert_content_urls(
    state: &AppState,
    user_id: &str,
    plugin_ns: &str,
    body: &mut serde_json::Value,
    moderation: bool,
) -> (Vec<String>, Vec<String>) {
    let mut logs: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // 检查对应的素材资产管理插件是否启用
    let plugin_enabled: bool = sqlx::query_scalar::<_, i64>(
        &state
            .db
            .format_query("SELECT is_enabled FROM plugins WHERE name = ?"),
    )
    .bind(plugin_ns)
    .fetch_optional(&state.db.pool)
    .await
    .ok()
    .flatten()
    .map(|v| v == 1)
    .unwrap_or(false);

    if !plugin_enabled {
        tracing::info!(
            "[AssetConvert] 素材资产管理插件({}) 未启用，跳过素材转换",
            plugin_ns
        );
        logs.push(format!("素材转换跳过: 插件({})未启用", plugin_ns));
        return (logs, errors);
    }

    // 加载 volcengine 审核配置（素材资产管理插件）
    let mut volc_config = match crate::api::plugins::get_volc_config(state, plugin_ns).await {
        Some(vc) => vc,
        None => {
            tracing::info!(
                "[AssetConvert] 素材资产管理插件({}) 未配置审核凭证，跳过素材转换",
                plugin_ns
            );
            logs.push("素材转换跳过: 未配置审核凭证".to_string());
            return (logs, errors);
        }
    };

    // 获取 content 数组（可变引用）
    let content_arr = match body.get_mut("content").and_then(|c| c.as_array_mut()) {
        Some(arr) => arr,
        None => return (logs, errors),
    };

    let client = crate::services::volcengine::VolcClient::new(volc_config.clone())
        .with_logger(state.db.clone(), user_id.to_string())
        .with_source("relay_convert")
        .with_plugin_name(plugin_ns);

    // 确保有可用的 Group ID，如果没有则尝试自动创建并保存
    if !ensure_group_id(state, &client, &mut volc_config, plugin_ns).await {
        logs.push("素材转换失败: 无法获取或创建素材组ID".to_string());
        errors.push("素材转换失败: 无法获取或创建素材组ID".to_string());
        return (logs, errors);
    }

    // 预加载 TOS 配置（base64 场景需要）
    let tos_config = crate::api::plugins::get_tos_config(state, plugin_ns).await;

    let tasks = collect_content_convert_tasks(content_arr);
    if tasks.is_empty() {
        return (logs, errors);
    }

    // 并发处理所有素材转换任务，大幅缩短多资源场景总耗时
    let mut futures = Vec::new();
    for (idx, url_key, asset_type, url_val, url_short) in tasks {
        let state_clone = state;
        let client_clone = client.clone();
        let mut volc_config_clone = volc_config.clone();
        let tos_config_clone = tos_config.clone();
        let user_id_owned = user_id.to_string();
        let plugin_ns_owned = plugin_ns.to_string();

        let fut = async move {
            // 返回 (asset_id, cached) — cached=true 表示复用了已有素材，未重新提交火山方舟
            let asset_result: Result<(String, bool), String> = if is_http_media_url(&url_val) {
                convert_url_resource(
                    state_clone,
                    &client_clone,
                    &mut volc_config_clone,
                    &user_id_owned,
                    &plugin_ns_owned,
                    &url_val,
                    &asset_type,
                    moderation,
                )
                .await
            } else if is_base64_media(&url_val) {
                convert_base64_with_create(
                    state_clone,
                    &tos_config_clone,
                    &user_id_owned,
                    &plugin_ns_owned,
                    "relay_convert",
                    &url_val,
                    &asset_type,
                    |tmp_url| {
                        let client = client_clone;
                        let mut volc = volc_config_clone;
                        let ns = plugin_ns_owned.clone();
                        let at = asset_type.clone();
                        async move {
                            create_asset(
                                state_clone,
                                &client,
                                &mut volc,
                                &ns,
                                &tmp_url,
                                &at,
                                moderation,
                            )
                            .await
                        }
                    },
                )
                .await
            } else {
                Err("不支持的格式".to_string())
            };
            (idx, url_key, asset_type, url_short, asset_result)
        };
        futures.push(fut);
    }

    // 收集并发结果
    let results = futures::future::join_all(futures).await;
    for (idx, url_key, asset_type, url_short, asset_result) in results {
        match asset_result {
            Ok((aid, cached)) => {
                push_convert_ok(
                    content_arr,
                    &mut logs,
                    idx,
                    &url_key,
                    &asset_type,
                    &url_short,
                    &aid,
                    cached,
                );
            }
            Err(reason) => {
                // 提取火山引擎错误中的 Message 字段用于日志摘要，完整错误由 errors 传递
                let brief = reason
                    .find('{')
                    .and_then(|i| serde_json::from_str::<serde_json::Value>(&reason[i..]).ok())
                    .and_then(|j| {
                        j.pointer("/ResponseMetadata/Error/Message")
                            .or_else(|| j.pointer("/Error/Message"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| reason.clone());
                logs.push(format!(
                    "[{}] {} ✗ 转换失败: {}",
                    asset_type, url_short, brief
                ));
                errors.push(reason);
            }
        }
    }
    (logs, errors)
}

/// 处理网络 URL 资源：Range 元数据指纹去重 → URL 去重 → 直接 CreateAsset（不下载整文件）
/// 返回 (asset_id, cached) — cached=true 表示复用了已有素材
async fn convert_url_resource(
    state: &AppState,
    client: &crate::services::volcengine::VolcClient,
    volc_config: &mut crate::services::volcengine::VolcConfig,
    user_id: &str,
    plugin_ns: &str,
    url: &str,
    asset_type: &str,
    moderation: bool,
) -> Result<(String, bool), String> {
    let meta_fp = fetch_meta_fingerprint(&state.http_client, url).await;
    if let Some(aid) =
        lookup_cached_converted_asset(state, url, plugin_ns, "relay_convert", meta_fp.as_deref())
            .await
    {
        tracing::info!("[AssetConvert] 命中缓存，复用素材: {} -> {}", url, aid);
        return Ok((aid, true));
    }

    // 未命中任何去重层，直接提交 URL 给火山方舟 CreateAsset（由火山方舟自行下载处理）
    match create_asset(
        state,
        client,
        volc_config,
        plugin_ns,
        url,
        asset_type,
        moderation,
    )
    .await
    {
        Ok(aid) => {
            let fp_ref = meta_fp.as_deref();
            insert_asset_record_with_source(
                state,
                user_id,
                asset_type,
                url,
                &aid,
                None,
                fp_ref,
                plugin_ns,
                "relay_convert",
            )
            .await;
            tracing::info!("[AssetConvert] 新素材注册成功: {} -> {}", url, aid);
            Ok((aid, false))
        }
        Err(reason) => {
            tracing::warn!("[AssetConvert] 素材注册失败: {} - URL: {}", reason, url);
            Err(reason)
        }
    }
}

/// base64 → 哈希去重 → TOS 临时 URL → create(tmp_url) → 落库 → 清理 TOS
/// CreateAsset 回调只收公网 URL，与官方接口约束一致；插件/上游路径共用本函数。
async fn convert_base64_with_create<F, Fut>(
    state: &AppState,
    tos_config: &Option<crate::services::tos::TosConfig>,
    user_id: &str,
    plugin_ns: &str,
    source: &str,
    data_uri: &str,
    asset_type: &str,
    create_from_url: F,
) -> Result<(String, bool), String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let (bytes, ext) = decode_base64_data(data_uri, asset_type)
        .ok_or_else(|| "base64 数据解码失败".to_string())?;

    let content_hash = hex::encode(Sha256::digest(&bytes));

    if let Some(aid) = query_by_hash_with_source(state, &content_hash, plugin_ns, source).await {
        tracing::info!(
            "[AssetConvert] base64 哈希命中，复用素材: 哈希={:.16}... -> {}",
            content_hash,
            aid
        );
        return Ok((aid, true));
    }

    let tos_cfg = tos_config
        .as_ref()
        .ok_or_else(|| "base64 转换需要 TOS 配置，但未配置存储".to_string())?;

    let tmp_filename = format!("{}.{}", &content_hash[..16], ext);
    let tmp_object_key = tos_cfg.full_key(&format!("_tmp_asset_convert/{}", tmp_filename));

    let tmp_url = crate::services::tos::upload_file(
        tos_cfg,
        &tmp_object_key,
        bytes,
        &format!("{}/{}", asset_type.to_lowercase(), ext),
        None,
    )
    .await
    .map_err(|e| format!("TOS 临时文件上传失败: {}", e))?;

    tracing::info!("[AssetConvert] base64 临时文件已上传: {}", tmp_url);

    let create_result = create_from_url(tmp_url.clone()).await;
    schedule_tos_temp_cleanup(tos_cfg.clone(), tmp_object_key);

    let aid = create_result?;
    insert_asset_record_with_source(
        state,
        user_id,
        asset_type,
        &tmp_url,
        &aid,
        Some(&content_hash),
        None,
        plugin_ns,
        source,
    )
    .await;
    tracing::info!(
        "[AssetConvert] base64 素材注册成功: base64_{}.{} -> {}",
        &content_hash[..8],
        ext,
        aid
    );
    Ok((aid, false))
}

fn schedule_tos_temp_cleanup(tos_cfg: crate::services::tos::TosConfig, tmp_object_key: String) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        match crate::services::tos::delete_file(&tos_cfg, &tmp_object_key).await {
            Ok(_) => tracing::info!("[AssetConvert] TOS 临时文件已清理: {}", tmp_object_key),
            Err(e) => tracing::warn!(
                "[AssetConvert] TOS 临时文件清理失败(非致命): {} - {}",
                tmp_object_key,
                e
            ),
        }
    });
}

// ========== 内部工具函数 ==========

/// 从 Content-Range 取整文件总长（`bytes 0-0/N` / `bytes */N`）；`*` 或非数字则 None。
fn content_range_total(cr: &str) -> Option<&str> {
    let total = cr.rsplit('/').next()?.trim();
    if total.is_empty() || total == "*" {
        return None;
    }
    if total.bytes().all(|b| b.is_ascii_digit()) {
        Some(total)
    } else {
        None
    }
}

/// GET Range 取元数据指纹（兼容坏 HEAD 的 CDN），指纹公式与历史一致：
/// SHA-256(URL域名+路径 | 整文件长度 | ETag | Last-Modified)
/// 整文件长度优先取 Content-Range 总长；源站忽略 Range 回 200 时回退 Content-Length。
/// 超时 10 秒，失败返回 None（调用方降级到 URL 字符串匹配）。
async fn fetch_meta_fingerprint(http_client: &reqwest::Client, url: &str) -> Option<String> {
    let url_short = shorten_url_for_log(url);

    let resp = match http_client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .timeout(Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let kind = if e.is_timeout() {
                "超时"
            } else if e.is_connect() {
                "连接失败"
            } else if e.is_request() {
                "请求构造/发送失败"
            } else {
                "其它错误"
            };
            tracing::warn!(
                "[AssetConvert] Range 元数据请求失败({}): {} - {}",
                kind,
                url_short,
                e
            );
            return None;
        }
    };

    let status = resp.status();
    if !status.is_success() {
        tracing::warn!(
            "[AssetConvert] Range 元数据状态码异常({}), 降级 URL 去重: {}",
            status,
            url_short
        );
        return None;
    }

    let headers = resp.headers();
    let etag = headers
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let last_modified = headers
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let content_range = headers
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let hdr_len = headers
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // 与历史 HEAD 指纹对齐：哈希里的长度必须是整文件大小，不是分片 Content-Length
    let full_len = content_range_total(content_range)
        .map(|s| s.to_string())
        .or_else(|| {
            if status.as_u16() == 200 && !hdr_len.is_empty() {
                Some(hdr_len.to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    // 丢弃最多 1 字节 body，避免占用连接
    let _ = resp.bytes().await;

    if full_len.is_empty() && etag.is_empty() && last_modified.is_empty() {
        tracing::info!(
            "[AssetConvert] Range 无有效标识字段(Length/ETag/Last-Modified), 降级 URL 去重: {}",
            url_short
        );
        return None;
    }

    tracing::info!(
        "[AssetConvert] Range 元数据: 长度={}, ETag={}, 修改时间={} | {}",
        if full_len.is_empty() { "-" } else { &full_len },
        if etag.is_empty() { "-" } else { &etag },
        if last_modified.is_empty() {
            "-"
        } else {
            &last_modified
        },
        url_short
    );

    let url_base = url
        .split('?')
        .next()
        .unwrap_or(url)
        .split('#')
        .next()
        .unwrap_or(url);

    let mut hasher = Sha256::new();
    hasher.update(url_base.as_bytes());
    hasher.update(b"|");
    hasher.update(full_len.as_bytes());
    hasher.update(b"|");
    hasher.update(etag.as_bytes());
    hasher.update(b"|");
    hasher.update(last_modified.as_bytes());

    Some(hex::encode(hasher.finalize()))
}

async fn query_by_fingerprint_with_source(
    state: &AppState,
    fingerprint: &str,
    plugin_ns: &str,
    source: &str,
) -> Option<String> {
    sqlx::query_as::<_, (String,)>(
        &state.db.format_query(
            "SELECT asset_id FROM plugin_assets WHERE meta_fingerprint = ? AND source = ? AND asset_id IS NOT NULL AND plugin_ns = ? LIMIT 1"
        )
    )
    .bind(fingerprint)
    .bind(source)
    .bind(plugin_ns)
    .fetch_optional(&state.db.pool)
    .await
    .ok()
    .flatten()
    .map(|row| row.0)
}

/// 基于 file_url 查询已有素材 ID（仅指纹不可用时的 L2 兜底）
async fn query_by_url_with_source(
    state: &AppState,
    url: &str,
    plugin_ns: &str,
    source: &str,
) -> Option<String> {
    sqlx::query_as::<_, (String,)>(&state.db.format_query(
        "SELECT asset_id FROM plugin_assets \
             WHERE file_url = ? AND source = ? AND asset_id IS NOT NULL AND plugin_ns = ? LIMIT 1",
    ))
    .bind(url)
    .bind(source)
    .bind(plugin_ns)
    .fetch_optional(&state.db.pool)
    .await
    .ok()
    .flatten()
    .map(|row| row.0)
}

/// L1 指纹命中则复用；指纹已算出但对不上 → 跳过 URL、重新注册（防同 URL 内容变更误复用）；
/// 仅指纹算不出时才走 L2 URL。
async fn lookup_cached_converted_asset(
    state: &AppState,
    url: &str,
    plugin_ns: &str,
    source: &str,
    meta_fp: Option<&str>,
) -> Option<String> {
    if let Some(fp) = meta_fp {
        if let Some(aid) = query_by_fingerprint_with_source(state, fp, plugin_ns, source).await {
            return Some(aid);
        }
        tracing::info!(
            "[AssetConvert] 元数据指纹未命中(内容可能已变化)，跳过 URL 复用并重新注册: {}",
            shorten_url_for_log(url)
        );
        return None;
    }

    query_by_url_with_source(state, url, plugin_ns, source).await
}

/// 解码 base64：支持 `data:*;base64,...` 与纯 base64；返回 (字节, 扩展名)
fn decode_base64_data(input: &str, asset_type: &str) -> Option<(Vec<u8>, String)> {
    use base64::Engine;
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let decode = |s: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(s))
            .ok()
    };

    let default_ext = match asset_type {
        "Video" => "mp4",
        "Audio" => "mp3",
        _ => "png",
    };

    let (bytes, ext) = if input.starts_with("data:") {
        let comma_pos = input.find(',')?;
        let header = &input[..comma_pos];
        let bytes = decode(super::forward::b64_data(input).trim())?;
        let ext = BASE64_MIME_EXT
            .iter()
            .find(|(prefix, _)| header.starts_with(prefix))
            .map(|(_, e)| (*e).to_string())
            .unwrap_or_else(|| ext_from_magic(&bytes, default_ext));
        (bytes, ext)
    } else {
        let bytes = decode(input)?;
        let ext = ext_from_magic(&bytes, default_ext);
        (bytes, ext)
    };

    if bytes.is_empty() {
        return None;
    }
    Some((bytes, ext))
}

/// 魔数推扩展名；未知则回退到 content.type 对应的默认后缀
fn ext_from_magic(bytes: &[u8], default_ext: &str) -> String {
    let ext = match bytes {
        [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, ..] => "png",
        [0xFF, 0xD8, 0xFF, ..] => "jpg",
        [0x47, 0x49, 0x46, 0x38, ..] => "gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        [_, _, _, _, b'f', b't', b'y', b'p', ..] => "mp4",
        [0x1A, 0x45, 0xDF, 0xA3, ..] => "webm",
        [b'I', b'D', b'3', ..] | [0xFF, 0xFB, ..] | [0xFF, 0xF3, ..] | [0xFF, 0xF2, ..] => "mp3",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'A', b'V', b'E', ..] => "wav",
        [b'O', b'g', b'g', b'S', ..] => "ogg",
        _ => default_ext,
    };
    ext.to_string()
}

async fn query_by_hash_with_source(
    state: &AppState,
    content_hash: &str,
    plugin_ns: &str,
    source: &str,
) -> Option<String> {
    sqlx::query_as::<_, (String,)>(
        &state.db.format_query(
            "SELECT asset_id FROM plugin_assets WHERE content_hash = ? AND source = ? AND asset_id IS NOT NULL AND plugin_ns = ? LIMIT 1"
        )
    )
    .bind(content_hash)
    .bind(source)
    .bind(plugin_ns)
    .fetch_optional(&state.db.pool)
    .await
    .ok()
    .flatten()
    .map(|row| row.0)
}

#[allow(clippy::too_many_arguments)]
async fn insert_asset_record_with_source(
    state: &AppState,
    user_id: &str,
    asset_type: &str,
    file_url: &str,
    asset_id: &str,
    content_hash: Option<&str>,
    meta_fingerprint: Option<&str>,
    plugin_ns: &str,
    source: &str,
) {
    let at_lower = asset_type.to_lowercase();
    let fname = derive_create_asset_name(file_url, asset_type);
    if let Err(e) = sqlx::query(
        &state.db.format_query(
            "INSERT INTO plugin_assets (user_id, asset_type, source, status, file_name, file_url, asset_id, category, content_hash, meta_fingerprint, plugin_ns) \
             VALUES (?, ?, ?, 'approved', ?, ?, ?, '转换素材', ?, ?, ?)"
        )
    )
    .bind(user_id)
    .bind(&at_lower)
    .bind(source)
    .bind(&fname)
    .bind(file_url)
    .bind(asset_id)
    .bind(content_hash)
    .bind(meta_fingerprint)
    .bind(plugin_ns)
    .execute(&state.db.pool)
    .await
    {
        tracing::warn!(
            "[AssetConvert] 写入 plugin_assets 失败(将导致无法复用缓存): {} | URL={} 素材ID={} 命名空间={} 来源={}",
            e,
            file_url,
            asset_id,
            plugin_ns,
            source
        );
    }
}

/// 调用 CreateAsset API 注册素材，并轮询等待素材处理完成（Active 状态）
/// 视频资源处理时间较长，自动根据素材类型调整超时（Image: 30s, Video/Audio: 60s）
async fn create_asset(
    state: &AppState,
    client: &crate::services::volcengine::VolcClient,
    volc_config: &mut crate::services::volcengine::VolcConfig,
    plugin_ns: &str,
    url: &str,
    asset_type: &str,
    moderation: bool,
) -> Result<String, String> {
    let group_id = volc_config.group_id.clone().unwrap_or_default();

    let asset_mod = if moderation {
        Some(crate::services::volcengine::AssetModerationConfig {
            strategy: "Skip".to_string(),
        })
    } else {
        None
    };

    let mut req = crate::services::volcengine::CreateAssetRequest {
        group_id: group_id.clone(),
        url: url.to_string(),
        asset_type: asset_type.to_string(),
        name: Some(derive_create_asset_name(url, asset_type)),
        project_name: Some(volc_config.project_name.clone()),
        moderation: asset_mod.clone(),
    };

    let mut asset_id_res = client
        .call_api::<_, crate::services::volcengine::CreateAssetResponse>(
            "ark",
            &volc_config.region,
            "CreateAsset",
            "2024-01-01",
            crate::services::volcengine::CreateAssetRequest {
                group_id: req.group_id.clone(),
                url: req.url.clone(),
                asset_type: req.asset_type.clone(),
                name: req.name.clone(),
                project_name: req.project_name.clone(),
                moderation: req.moderation.clone(),
            },
        )
        .await;

    // 错误处理：如果是无效的素材组（比如换了 Access Key），尝试重新生成一次
    if let Err(e) = &asset_id_res {
        let e_lower = e.to_string().to_lowercase();
        // 启发式判断：如果错误提示与 group、权限有关，则尝试重置 GroupID
        // 避免因为单纯的图片 URL 无效或网络超时导致滥建素材组
        if e_lower.contains("group") || e_lower.contains("auth") {
            tracing::warn!("[AssetConvert] CreateAsset 失败，可能由于 AccessKey 变更导致原 GroupID 无效，准备重试。原错误: {}", e);

            // 防止高并发下产生多个冗余 Group，先从数据库重新拉取一次最新配置，判断是否已被其他并发请求刷新
            if let Some(latest_cfg) = crate::api::plugins::get_volc_config(state, plugin_ns).await {
                if latest_cfg.group_id.is_some() && latest_cfg.group_id != Some(group_id.clone()) {
                    tracing::info!(
                        "[AssetConvert] 发现其他并发请求已更新素材组 ID，直接复用: {:?}",
                        latest_cfg.group_id
                    );
                    volc_config.group_id = latest_cfg.group_id;
                } else {
                    // 数据库里的配置未变，说明确实需要当前请求去申请一个新的
                    volc_config.group_id = None;
                    ensure_group_id(state, client, volc_config, plugin_ns).await;
                }
            } else {
                volc_config.group_id = None;
                ensure_group_id(state, client, volc_config, plugin_ns).await;
            }

            req.group_id = volc_config.group_id.clone().unwrap_or_default();
            asset_id_res = client
                .call_api::<_, crate::services::volcengine::CreateAssetResponse>(
                    "ark",
                    &volc_config.region,
                    "CreateAsset",
                    "2024-01-01",
                    req,
                )
                .await;
        }
    }

    let asset_id = asset_id_res.map_err(|e| format!("素材转换失败: {}", e))?.id;

    // 视频/音频资源处理时间较长，动态调整轮询超时
    // Image: 60s, Audio: 120s, Video: 180s（视频文件体积大，火山端处理更耗时）
    let max_wait_secs: u64 = match asset_type {
        "Image" => 60,
        "Audio" => 120,
        _ => 180,
    };
    const POLL_INTERVAL_SECS: u64 = 3;
    let max_attempts = max_wait_secs / POLL_INTERVAL_SECS;
    let mut last_poll_error: Option<String> = None;

    for attempt in 0..max_attempts {
        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;

        let get_req = crate::services::volcengine::GetAssetRequest {
            id: asset_id.clone(),
            project_name: Some(volc_config.project_name.clone()),
        };

        match client
            .call_api::<_, crate::services::volcengine::GetAssetResponse>(
                "ark",
                &volc_config.region,
                "GetAsset",
                "2024-01-01",
                get_req,
            )
            .await
        {
            Ok(res) => match res.status.as_str() {
                "Active" => {
                    tracing::info!(
                        "[AssetConvert] 素材就绪: {} (等待 {}s)",
                        asset_id,
                        (attempt + 1) * POLL_INTERVAL_SECS
                    );
                    return Ok(asset_id);
                }
                "Failed" => {
                    let reason = if let Some(ref err) = res.error {
                        if !err.message.is_empty() {
                            err.message.clone()
                        } else if !err.code.is_empty() {
                            err.code.clone()
                        } else {
                            "审核未通过".to_string()
                        }
                    } else {
                        let fail_code = res.fail_code.as_deref().unwrap_or("");
                        let fail_reason = res.fail_reason.as_deref().unwrap_or("");
                        match (fail_code.is_empty(), fail_reason.is_empty()) {
                            (false, false) => format!("[{}] {}", fail_code, fail_reason),
                            (false, true) => format!("[{}]", fail_code),
                            (true, false) => fail_reason.to_string(),
                            (true, true) => "审核未通过".to_string(),
                        }
                    };
                    tracing::error!("[AssetConvert] 素材处理失败: {} - {}", asset_id, reason);
                    return Err(format!("素材处理失败({}): {}", asset_id, reason));
                }
                status => {
                    tracing::info!(
                        "[AssetConvert] 素材处理中: {} 状态={} (第{}/{}次)",
                        asset_id,
                        status,
                        attempt + 1,
                        max_attempts
                    );
                }
            },
            Err(e) => {
                let err_str = e.to_string();
                tracing::warn!(
                    "[AssetConvert] GetAsset 查询失败: {} - {}",
                    asset_id,
                    err_str
                );
                last_poll_error = Some(err_str);
            }
        }
    }

    // 超时时包含最后一次轮询错误原因，便于用户排查
    let timeout_msg = if let Some(ref poll_err) = last_poll_error {
        format!(
            "素材处理超时({}s): {}, 错误: {}",
            max_wait_secs, asset_id, poll_err
        )
    } else {
        format!("素材处理超时({}s): {}", max_wait_secs, asset_id)
    };
    Err(timeout_msg)
}

/// 自动保证 Group ID 存在，未设置时调用 API 自动生成并持久化
async fn ensure_group_id(
    state: &crate::AppState,
    client: &crate::services::volcengine::VolcClient,
    volc_config: &mut crate::services::volcengine::VolcConfig,
    plugin_ns: &str,
) -> bool {
    // 如果已经有非空的 ID 则直接通过
    if volc_config
        .group_id
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        return true;
    }

    // 调用 API 默认生成一个
    let req = crate::services::volcengine::CreateAssetGroupRequest {
        name: "tokensbyte_auto_generated_group".to_string(),
        description: "由 Tokensbyte 系统自动生成的转换素材专用群组".to_string(),
        group_type: Some("AIGC".to_string()),
        project_name: Some(volc_config.project_name.clone()),
    };

    match client
        .call_api::<_, crate::services::volcengine::CreateAssetGroupResponse>(
            "ark",
            &volc_config.region,
            "CreateAssetGroup",
            "2024-01-01",
            req,
        )
        .await
    {
        Ok(res) => {
            let new_sg_id = res.id;
            tracing::info!("[AssetConvert] 成功自动生成 Ark 素材组 ID: {}", new_sg_id);
            volc_config.group_id = Some(new_sg_id.clone());

            // 存入数据库
            let update_res = sqlx::query(
                &state.db.format_query("UPDATE plugin_configs SET config_value = ?, updated_at = CURRENT_TIMESTAMP WHERE plugin_name = ? AND config_key = 'volc_group_id'")
            )
            .bind(&new_sg_id)
            .bind(plugin_ns)
            .execute(&state.db.pool)
            .await;

            if let Ok(r) = update_res {
                if r.rows_affected() == 0 {
                    let _ = sqlx::query(
                        &state.db.format_query("INSERT INTO plugin_configs (plugin_name, config_key, config_value) VALUES (?, 'volc_group_id', ?)")
                    )
                    .bind(plugin_ns)
                    .bind(&new_sg_id)
                    .execute(&state.db.pool)
                    .await;
                }
            }
            true
        }
        Err(e) => {
            tracing::error!(
                "[AssetConvert] 自动生成 Ark 素材组失败，未满足必需属性，拦截执行: {}",
                e
            );
            false
        }
    }
}

/// 上游渠道素材转换：扫描 content[]，经绑定渠道 Bearer CreateAsset → asset://
/// 与 convert_content_urls 正交；插件未启用时 soft-skip（errors 空）。
pub async fn convert_content_urls_via_upstream(
    state: &AppState,
    user_id: &str,
    binding_id: i64,
    body: &mut serde_json::Value,
) -> (Vec<String>, Vec<String>) {
    let mut logs: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    #[derive(sqlx::FromRow)]
    struct BindingRow {
        plugin_enabled: i64,
        binding_found: Option<i64>,
        is_active: Option<i32>,
        asset_base_path: Option<String>,
        group_id: Option<String>,
        channel_config_id: Option<i64>,
        base_url: Option<String>,
        api_key: Option<String>,
        #[sqlx(default)]
        config_status: Option<i32>,
    }

    // 以 plugins 为主表一次取出启用状态与绑定/渠道，语义与「先查插件再查绑定」一致
    let row: Option<BindingRow> = sqlx::query_as(&state.db.format_query(
        "SELECT p.is_enabled AS plugin_enabled, b.id AS binding_found, b.is_active, \
                b.asset_base_path, b.group_id, b.channel_config_id, c.base_url, c.api_key, \
                c.status AS config_status \
         FROM plugins p \
         LEFT JOIN upstream_asset_bindings b ON b.id = ? \
         LEFT JOIN channel_configs c ON c.id = b.channel_config_id \
         WHERE p.name = ?",
    ))
    .bind(binding_id)
    .bind(uac::PLUGIN_NAME)
    .fetch_optional(&state.db.pool)
    .await
    .ok()
    .flatten();

    // 插件记录不存在或未启用 → 同样跳过
    let Some(mut row) = row.filter(|r| r.plugin_enabled == 1) else {
        tracing::info!("[UpstreamAsset] 插件未启用，跳过素材转换");
        logs.push("上游素材转换跳过: 插件未启用".to_string());
        return (logs, errors);
    };
    if row.binding_found.is_none() {
        errors.push(format!(
            "上游素材转换失败: 绑定#{} 不存在或上游渠道配置已删除",
            binding_id
        ));
        return (logs, errors);
    }
    if row.is_active != Some(1) {
        logs.push(format!("上游素材转换跳过: 绑定#{} 已停用", binding_id));
        return (logs, errors);
    }
    if row.config_status.unwrap_or(1) != 1 {
        errors.push(format!(
            "上游素材转换失败: 绑定#{} 关联的上游渠道配置已禁用",
            binding_id
        ));
        return (logs, errors);
    }
    let base_url = row.base_url.take().unwrap_or_default();
    let api_key = row.api_key.take().unwrap_or_default();
    let channel_config_id = row.channel_config_id.unwrap_or(0);
    if base_url.trim().is_empty() || api_key.trim().is_empty() {
        errors.push(format!(
            "上游素材转换失败: 上游渠道配置#{} 缺少 base_url 或 api_key",
            channel_config_id
        ));
        return (logs, errors);
    }
    let asset_base_path = row.asset_base_path.take().unwrap_or_default();
    let mut group_id_opt = row.group_id.take();

    let content_arr = match body.get_mut("content").and_then(|c| c.as_array_mut()) {
        Some(arr) => arr,
        None => return (logs, errors),
    };

    let plugin_ns = uac::binding_ns(binding_id);
    let endpoint = uac::build_asset_endpoint(&base_url, &asset_base_path);
    let call_ctx = uac::UpstreamCallCtx {
        http: &state.http_client,
        db: &state.db,
        user_id,
        plugin_name: &plugin_ns,
        endpoint_base: &endpoint,
        api_key: &api_key,
        // 视频转素材流程暂用渠道 api_key 走 Bearer；乐信等 V4 上游的素材直连走 native passthrough
        auth: None,
    };

    // 确保 GroupId
    if group_id_opt
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        match ensure_upstream_group_id(state, &call_ctx, binding_id).await {
            Ok(gid) => group_id_opt = Some(gid),
            Err(e) => {
                errors.push(e);
                return (logs, errors);
            }
        }
    }

    let tasks = collect_content_convert_tasks(content_arr);
    if tasks.is_empty() {
        return (logs, errors);
    }

    // base64 与插件路径相同：依赖系统/插件 TOS（upstream_asset_relay 无独立 TOS 时回退系统配置）
    let tos_config = crate::api::plugins::get_tos_config(state, uac::PLUGIN_NAME).await;
    let group_id = group_id_opt.unwrap_or_default();

    for (idx, url_key, asset_type, url_val, url_short) in tasks {
        let asset_result = if is_http_media_url(&url_val) {
            convert_url_via_upstream(state, &call_ctx, &group_id, &url_val, &asset_type).await
        } else if is_base64_media(&url_val) {
            convert_base64_with_create(
                state,
                &tos_config,
                user_id,
                &plugin_ns,
                uac::LOG_SOURCE,
                &url_val,
                &asset_type,
                |tmp_url| {
                    let ctx = &call_ctx;
                    let gid = group_id.as_str();
                    let at = asset_type.as_str();
                    async move { create_asset_via_upstream(ctx, gid, &tmp_url, at).await }
                },
            )
            .await
        } else {
            Err("不支持的格式".to_string())
        };

        match asset_result {
            Ok((aid, cached)) => {
                push_convert_ok(
                    content_arr,
                    &mut logs,
                    idx,
                    &url_key,
                    &asset_type,
                    &url_short,
                    &aid,
                    cached,
                );
            }
            Err(reason) => {
                logs.push(format!("[{}] {} ✗ {}", asset_type, url_short, reason));
                errors.push(reason);
            }
        }
    }

    (logs, errors)
}

async fn ensure_upstream_group_id(
    state: &AppState,
    ctx: &uac::UpstreamCallCtx<'_>,
    binding_id: i64,
) -> Result<String, String> {
    let body = serde_json::json!({
        "Name": "tokensbyte_upstream_auto_group",
        "Description": "由火山视频转素材ID自动创建的素材组",
        "GroupType": "AIGC"
    });
    let res = uac::call_action_logged(ctx, "CreateAssetGroup", &body)
        .await
        .map_err(|e| format!("创建上游素材组失败: {}", e))?;

    let gid = uac::extract_result_field(&res, "Id")
        .ok_or_else(|| "创建上游素材组失败: 响应缺少 Id".to_string())?
        .to_string();

    let _ = sqlx::query(
        &state.db.format_query(
            "UPDATE upstream_asset_bindings SET group_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        ),
    )
    .bind(&gid)
    .bind(binding_id)
    .execute(&state.db.pool)
    .await;

    tracing::info!(
        "[UpstreamAsset] 自动创建素材组并回写绑定#{}: {}",
        binding_id,
        gid
    );
    Ok(gid)
}

async fn convert_url_via_upstream(
    state: &AppState,
    ctx: &uac::UpstreamCallCtx<'_>,
    group_id: &str,
    url: &str,
    asset_type: &str,
) -> Result<(String, bool), String> {
    let meta_fp = fetch_meta_fingerprint(ctx.http, url).await;
    if let Some(aid) = lookup_cached_converted_asset(
        state,
        url,
        ctx.plugin_name,
        uac::LOG_SOURCE,
        meta_fp.as_deref(),
    )
    .await
    {
        tracing::info!("[UpstreamAsset] 命中缓存，复用素材: {} -> {}", url, aid);
        return Ok((aid, true));
    }

    let asset_id = create_asset_via_upstream(ctx, group_id, url, asset_type).await?;

    insert_asset_record_with_source(
        state,
        ctx.user_id,
        asset_type,
        url,
        &asset_id,
        None,
        meta_fp.as_deref(),
        ctx.plugin_name,
        uac::LOG_SOURCE,
    )
    .await;

    Ok((asset_id, false))
}

/// 上游 CreateAsset(URL) + 轮询 Active（不含缓存/落库，供 URL 与 base64 共用）
async fn create_asset_via_upstream(
    ctx: &uac::UpstreamCallCtx<'_>,
    group_id: &str,
    url: &str,
    asset_type: &str,
) -> Result<String, String> {
    let mut body = serde_json::json!({
        "URL": url,
        "AssetType": asset_type,
        "Name": derive_create_asset_name(url, asset_type),
        "GroupId": group_id,
    });
    if let Some(obj) = body.as_object_mut() {
        if group_id.trim().is_empty() {
            obj.remove("GroupId");
        }
    }

    let create_res = uac::call_action_logged(ctx, "CreateAsset", &body)
        .await
        .map_err(|e| format!("素材注册失败: {}", e))?;

    let asset_id = uac::extract_result_field(&create_res, "Id")
        .ok_or_else(|| "素材注册失败: 响应缺少 Id".to_string())?
        .to_string();

    poll_upstream_asset_active(ctx, &asset_id, asset_type).await?;
    Ok(asset_id)
}

async fn poll_upstream_asset_active(
    ctx: &uac::UpstreamCallCtx<'_>,
    asset_id: &str,
    asset_type: &str,
) -> Result<(), String> {
    let max_wait_secs: u64 = match asset_type {
        "Image" => 60,
        "Audio" => 120,
        _ => 180,
    };
    const POLL_INTERVAL_SECS: u64 = 3;
    let max_attempts = max_wait_secs / POLL_INTERVAL_SECS;
    let mut last_err: Option<String> = None;

    for attempt in 0..max_attempts {
        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
        let body = serde_json::json!({ "Id": asset_id });
        match uac::call_action_logged(ctx, "GetAsset", &body).await {
            Ok(res) => {
                let status = uac::extract_result_field(&res, "Status").unwrap_or("");
                if status.eq_ignore_ascii_case("Active") {
                    tracing::info!(
                        "[UpstreamAsset] 素材就绪: {} ({}s)",
                        asset_id,
                        (attempt + 1) * POLL_INTERVAL_SECS
                    );
                    return Ok(());
                }
                if status.eq_ignore_ascii_case("Failed") {
                    let reason = uac::extract_result_field(&res, "FailReason")
                        .or_else(|| {
                            res.pointer("/Result/Error/Message")
                                .and_then(|v| v.as_str())
                        })
                        .unwrap_or("审核未通过");
                    return Err(format!("素材处理失败({}): {}", asset_id, reason));
                }
            }
            Err(e) => {
                last_err = Some(e.to_string());
            }
        }
    }

    Err(if let Some(e) = last_err {
        format!(
            "素材处理超时({}s): {}, 错误: {}",
            max_wait_secs, asset_id, e
        )
    } else {
        format!("素材处理超时({}s): {}", max_wait_secs, asset_id)
    })
}
