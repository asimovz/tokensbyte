/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

#[cfg(feature = "commercial_plugins")]
pub mod asset_convert;
// 上游素材 API 描述符引擎：绑定级 asset_api_profile 声明式协议适配（非火山上游如 fantaframe/cmcc）
pub mod asset_api_profile;
// 素材上游按渠道分组推导：等级 → 分组 → 凭证 → 绑定（含配置侧唯一性校验）
pub mod asset_route;
pub mod billing_pipeline;
pub mod cascade;
pub mod channel_quota;
pub mod forward;
pub mod ha;
pub mod image;
pub mod native;
pub mod proxy;
pub mod quota_memory;
pub mod relay_settings;
pub mod router;
pub mod stream;
pub mod task;
pub mod token_quota;
pub mod upstream_headers;
// 用户级安全限制：IP 白名单（含 CIDR），60s TTL 缓存供中继热路径读取
pub mod user_limits;
pub mod url_utils;
pub mod usage_extractor;
pub mod usage_stats;
pub mod vendor_callback;
pub mod video;

#[cfg(not(feature = "commercial_plugins"))]
pub mod asset_convert {
    use crate::AppState;
    pub async fn convert_content_urls(
        _state: &AppState,
        _user_id: &str,
        _plugin_ns: &str,
        _body: &mut serde_json::Value,
        _moderation: bool,
    ) -> (Vec<String>, Vec<String>) {
        (Vec::new(), Vec::new())
    }

    pub async fn convert_content_urls_via_upstream(
        _state: &AppState,
        _user_id: &str,
        _binding_id: i64,
        _body: &mut serde_json::Value,
    ) -> (Vec<String>, Vec<String>) {
        (Vec::new(), Vec::new())
    }
}
pub mod audio;
pub mod balance;
pub mod chat;
pub mod generic;
pub mod model_list;
pub mod response_formatter;
pub mod tos_persist;

use std::future::Future;

/// 连接保护：独立 task 执行 `fut`，结果经 oneshot 回传；客户端断开不影响 fut 跑完
#[inline]
pub fn spawn_protected<T: Send + 'static>(
    fut: impl Future<Output = T> + Send + 'static,
) -> tokio::sync::oneshot::Receiver<T> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = tx.send(fut.await);
    });
    rx
}

/// 底座费用 × res_mul；倍率为 1 时原样返回（无 token 时的时长等计费兜底）
pub fn scale_cost_by_res_mul(
    cost: f64,
    detail: String,
    map: &std::collections::HashMap<String, f64>,
    resolution: &str,
) -> (f64, String) {
    let mult = forward::lookup_res_mul(map, resolution);
    if (mult - 1.0).abs() <= 1e-9 {
        return (crate::money::round_money(cost), detail);
    }
    let factor = format!(" * {:.2}倍(级联res_mul:{})", mult, resolution);
    let detail = match detail.find(" | ") {
        Some(idx) => format!("{}{}{}", &detail[..idx], factor, &detail[idx..]),
        None => format!("{}{}", detail, factor),
    };
    (crate::money::round_money(cost * mult), detail)
}

/// 计算最终的消费金额和计费详情文本（包含后置时间段倍率折算，解耦高可用插件并提炼冗余逻辑）
pub async fn calculate_relay_cost(
    state: &crate::AppState,
    db_model: Option<&crate::models::Model>,
    db_rule: Option<&mut crate::models::BillingRule>,
    channel: &crate::models::Channel,
    ctx: &crate::relay::proxy::UserContext,
    usage: &usage_extractor::UsageTokens,
    features: &usage_extractor::ExtractedFeatures,
    mapping_source: Option<&str>,
    model_name: &str,
    resolved_model: &str,
) -> (f64, String) {
    let is_ha_enabled = relay_settings::get_cached_ha_enabled(&state.db).await;

    let umd = db_model
        .and_then(|m| crate::relay::proxy::parse_user_model_discount(&ctx.model_discounts, &m.mid));
    let (final_discount, discount_source) =
        crate::relay::proxy::resolve_discount(db_model, ctx.discount, umd, ctx.discount_type);

    let applied_discount = if is_ha_enabled {
        final_discount * channel.rate
    } else {
        final_discount
    };

    let (mut cost, mut detail) = compute_cost_raw(
        db_model,
        db_rule.as_deref(),
        usage,
        applied_discount,
        features,
    );

    // 将折扣名字直接融合进 {:.2}倍率 描述中
    let discount_target = format!("{:.2}倍率", applied_discount);
    let discount_replace = format!("{:.2}倍率({})", applied_discount, discount_source);
    detail = detail.replace(&discount_target, &discount_replace);

    // 后置时间段倍率：优先 billing_features 快照（异步结算），否则用请求开始锁定的 applied_multiplier
    if let Some(rule) = db_rule {
        let (new_cost, new_detail) = apply_locked_time_multiplier(cost, detail, rule, features);
        cost = new_cost;
        detail = new_detail;
    }
    if is_ha_enabled && channel.rate != 1.0 {
        detail = format!("{} * {:.2}倍(渠道倍率)", detail, channel.rate);
    }
    if let Some(src) = mapping_source {
        detail.push_str(&format!(" | {}: {} ➞ {}", src, model_name, resolved_model));
    }

    (crate::money::round_money(cost), detail)
}

/// 应用已锁定的峰谷时段倍率（请求开始 / billing_features 快照），防重复乘价与重复明细
pub fn apply_locked_time_multiplier(
    mut cost: f64,
    mut detail: String,
    rule: &mut crate::models::BillingRule,
    features: &usage_extractor::ExtractedFeatures,
) -> (f64, String) {
    // 快照优先（异步任务结算），否则用请求开始时写入的 applied_multiplier
    let time_multiplier = features.time_multiplier.unwrap_or(rule.applied_multiplier);
    rule.applied_multiplier = time_multiplier;

    if (time_multiplier - 1.0).abs() <= 0.00001 {
        return (cost, detail);
    }
    if rule.is_multiplier_applied {
        // 已乘过价：不再改金额，也不再追加明细文案
        return (cost, detail);
    }
    cost *= time_multiplier;
    rule.is_multiplier_applied = true;
    detail = format!("{} * {:.2}倍(时段倍率)", detail, time_multiplier);
    (cost, detail)
}

/// 统一计费逻辑（返回金额已 round 到 6 位小数）
/// 若 `db_rule.applied_multiplier` 已在请求开始锁定（或 features.time_multiplier 有快照），会后置乘时段倍率
pub fn compute_cost(
    db_model: Option<&crate::models::Model>,
    db_rule: Option<&crate::models::BillingRule>,
    usage: &usage_extractor::UsageTokens,
    discount: f64,
    features: &usage_extractor::ExtractedFeatures,
) -> (f64, String) {
    let (cost, detail) = compute_cost_raw(db_model, db_rule, usage, discount, features);
    let (cost, detail) = if let Some(rule) = db_rule {
        let tm = features.time_multiplier.unwrap_or(rule.applied_multiplier);
        if (tm - 1.0).abs() > 0.00001 {
            (cost * tm, format!("{} * {:.2}倍(时段倍率)", detail, tm))
        } else {
            (cost, detail)
        }
    } else {
        (cost, detail)
    };
    (crate::money::round_money(cost), detail)
}

/// 统一计费逻辑（未舍入，供内部组合后再 round）
fn compute_cost_raw(
    _db_model: Option<&crate::models::Model>,
    db_rule: Option<&crate::models::BillingRule>,
    usage: &usage_extractor::UsageTokens,
    discount: f64,
    features: &usage_extractor::ExtractedFeatures,
) -> (f64, String) {
    // 从 UsageTokens 解构所需字段（简化后续计算代码）
    let prompt_tokens = usage.prompt;
    let completion_tokens = usage.completion;
    let cached_tokens = usage.cached;
    let cache_write_tokens = usage.cache_write;
    let audio_tokens = usage.audio_tokens;
    let audio_cached_tokens = usage.audio_cached_tokens;
    let rule = match db_rule {
        Some(r) => r,
        None => {
            // 没有配置计费规则，走默认基础计费 1M 万字 = 1美金 等价
            let total = prompt_tokens + completion_tokens;
            let cost = (total as f64 / 1_000_000.0) * discount;
            return (
                cost,
                format!(
                    "无规则默认计费: {}总Tokens * 1/1M * {:.2}倍率",
                    total, discount
                ),
            );
        }
    };

    let apply_web_search = |token_cost_raw: f64, token_detail_raw: String| -> (f64, String) {
        let web_search_count = features.web_search.unwrap_or(0) as f64;
        let web_search_rate =
            if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                ext.get("web_search_rate")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            } else {
                0.0
            };
        if web_search_count > 0.0 && web_search_rate > 0.0 {
            let ws_cost_raw = web_search_count * web_search_rate / 1000.0;
            let cost = (token_cost_raw + ws_cost_raw) * discount;
            let detail = format!(
                "[ {} + 联网搜索: {}次*{}元/千次 ] * {:.2}倍率",
                token_detail_raw, web_search_count, web_search_rate, discount
            );
            (cost, detail)
        } else {
            let cost = token_cost_raw * discount;
            let detail = format!("{} * {:.2}倍率", token_detail_raw, discount);
            (cost, detail)
        }
    };

    // 供解析分辨率费率使用的辅助结构体
    #[derive(serde::Deserialize)]
    struct ResolutionTier {
        pub resolution: String,
        pub rate: f64,
        #[serde(default = "crate::relay::default_tier_enabled")]
        pub enabled: bool,
        /// 图片：有图（图生图）倍率，仅 image_resolution 使用
        #[serde(default)]
        pub image_ref_multiplier: Option<f64>,
        /// 输入侧单价：image_resolution_io 按张、video_seconds_io 按秒
        #[serde(default)]
        pub input_rate: f64,
    }

    /// 供视频画质（分辨率及帧率阶梯）计费使用的辅助结构体
    #[derive(serde::Deserialize)]
    struct VideoQualityTier {
        pub resolution: String,
        pub fps_range: String,
        pub rate: f64,
        #[serde(default = "crate::relay::default_tier_enabled")]
        pub enabled: bool,
    }

    /// 供按分辨率像素计费使用的辅助结构体
    #[derive(serde::Deserialize)]
    struct SizeTier {
        pub size: String,
        #[serde(default)]
        pub rate: f64,
        #[serde(default = "crate::relay::default_tier_enabled")]
        pub enabled: bool,
        /// 有图（图生图）倍率
        #[serde(default)]
        pub image_ref_multiplier: Option<f64>,
        /// 是否启用画质独立定价
        #[serde(default)]
        pub quality_pricing: bool,
        /// 低画质单价（quality_pricing 开启时生效）
        #[serde(default)]
        pub rate_low: Option<f64>,
        /// 中画质单价（quality_pricing 开启时生效）
        #[serde(default)]
        pub rate_medium: Option<f64>,
        /// 高画质单价（quality_pricing 开启时生效）
        #[serde(default)]
        pub rate_high: Option<f64>,
    }

    match rule.billing_type.as_str() {
        "requests" => {
            let mut rate = rule.fixed_rate;
            let mut count = 1.0;
            let mut detail_desc = "固定按次计费".to_string();

            if rule.billing_rule == "per_image" {
                // 按张计费：严格使用从响应提取的实际图片数量；无有效图则不计费（由上层判失败）
                count = match features.image_count {
                    Some(c) if c > 0 => c as f64,
                    _ => {
                        tracing::warn!(
                            "[Billing] per_image 规则未获取到有效图片数量，按 0 张不计费"
                        );
                        0.0
                    }
                };
                detail_desc = "按张返回计费".to_string();

                if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                    // 提示词扩写倍率
                    if features.prompt_extend {
                        if let Some(m) =
                            ext.get("prompt_extend_multiplier").and_then(|v| v.as_f64())
                        {
                            rate *= m;
                            detail_desc.push_str(&format!(" [提示词扩写 x{}]", m));
                        }
                    }
                    // 有图倍率（区分文生图/图生图）
                    if features.has_image_ref {
                        if let Some(m) = ext.get("image_ref_multiplier").and_then(|v| v.as_f64()) {
                            rate *= m;
                            detail_desc.push_str(&format!(" [图生图 x{}]", m));
                        }
                    }
                }
            } else if rule.billing_rule == "image_resolution" {
                count = features.image_count.map(|c| c.max(1) as f64).unwrap_or(1.0);
                detail_desc = format!("分辨率匹配计费(默认单价: {})", rate);
                let res = features.resolution.as_deref().unwrap_or("1k");
                if let Ok(tiers) = serde_json::from_str::<Vec<ResolutionTier>>(&rule.pricing_tiers)
                {
                    let mut matched = false;
                    let mut max_rate: Option<f64> = None;
                    let mut max_res = String::new();
                    for tier in &tiers {
                        if !tier.enabled {
                            continue;
                        }
                        if max_rate.map_or(true, |mr| tier.rate > mr) {
                            max_rate = Some(tier.rate);
                            max_res = tier.resolution.clone();
                        }
                        if tier.resolution.eq_ignore_ascii_case(res) {
                            rate = tier.rate;
                            detail_desc = format!("命中分辨率阶梯 {} 单价: {:.6}", res, rate);
                            if features.has_image_ref {
                                if let Some(m) = tier.image_ref_multiplier.filter(|&m| m != 1.0) {
                                    rate *= m;
                                    detail_desc.push_str(&format!(" [图生图 x{}]", m));
                                }
                            }
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        if let Some(mr) = max_rate {
                            rate = mr;
                            detail_desc = format!(
                                "分辨率{}未命中,兜底最高阶梯({} 单价:{:.6})",
                                res, max_res, mr
                            );
                        }
                    }
                }

                // 提示词扩写倍率（全局，非 per-tier）
                if features.prompt_extend {
                    if let Ok(ext) =
                        serde_json::from_str::<serde_json::Value>(&rule.extended_config)
                    {
                        if let Some(m) =
                            ext.get("prompt_extend_multiplier").and_then(|v| v.as_f64())
                        {
                            rate *= m;
                            detail_desc.push_str(&format!(" [提示词扩写 x{}]", m));
                        }
                    }
                }
            } else if rule.billing_rule == "image_resolution_io" {
                // 输入张数 × 输入单价 + 生成张数 × 生成单价，两侧单价取自同一分辨率档位
                let output_n = features.image_count.map(|c| c.max(1) as f64).unwrap_or(1.0);
                let input_n = features.image_ref_count.unwrap_or(0).max(0) as f64;
                let res = features.resolution.as_deref().unwrap_or("1k");
                let mut input_rate = 0.0;
                let mut output_rate = rate;
                detail_desc = format!("分辨率K输入/生成匹配(默认生成单价: {})", rate);
                if let Ok(tiers) = serde_json::from_str::<Vec<ResolutionTier>>(&rule.pricing_tiers)
                {
                    let enabled: Vec<&ResolutionTier> = tiers.iter().filter(|t| t.enabled).collect();
                    let picked = enabled
                        .iter()
                        .copied()
                        .find(|t| t.resolution.eq_ignore_ascii_case(res))
                        .or_else(|| {
                            enabled.iter().copied().max_by(|a, b| {
                                a.rate.partial_cmp(&b.rate).unwrap_or(std::cmp::Ordering::Equal)
                            })
                        });
                    if let Some(t) = picked {
                        input_rate = t.input_rate;
                        output_rate = t.rate;
                        detail_desc = if t.resolution.eq_ignore_ascii_case(res) {
                            format!("命中分辨率阶梯 {}", t.resolution)
                        } else {
                            format!("分辨率{}未命中,兜底最高阶梯({})", res, t.resolution)
                        };
                    }
                }
                let cost = (input_n * input_rate + output_n * output_rate) * discount;
                return (
                    cost,
                    format!(
                        "{} -> (输入:{}张*{:.6} + 生成:{}张*{:.6})*{:.2}倍率",
                        detail_desc,
                        input_n as i32,
                        input_rate,
                        output_n as i32,
                        output_rate,
                        discount
                    ),
                );
            } else if rule.billing_rule == "image_size_pixel" {
                // 按分辨率像素计费：匹配 size 参数（如 1024x1024），支持画质独立定价
                count = features.image_count.map(|c| c.max(1) as f64).unwrap_or(1.0);
                detail_desc = format!("分辨率像素匹配计费(默认单价: {})", rate);
                let raw_size = features.size.as_deref().unwrap_or("");
                let normalized = normalize_pixel_size(raw_size);

                /// 从匹配到的档位中提取最终费率：画质独立定价时按 quality 选取，否则使用 rate
                fn resolve_tier_rate(tier: &SizeTier, quality: &Option<String>) -> (f64, String) {
                    if tier.quality_pricing {
                        let q = quality.as_deref().unwrap_or("medium");
                        let r = match q {
                            "low" => tier.rate_low.unwrap_or(0.0),
                            "high" => tier.rate_high.unwrap_or(0.0),
                            _ => tier.rate_medium.unwrap_or(0.0), // medium 或未知画质默认取中画质
                        };
                        (r, format!("画质:{}", q))
                    } else {
                        (tier.rate, String::new())
                    }
                }

                /// 取所有启用档位中的最高费率（用于未匹配时兜底）
                fn max_tier_rate(
                    tiers: &[SizeTier],
                    quality: &Option<String>,
                ) -> Option<(f64, String)> {
                    tiers
                        .iter()
                        .filter(|t| t.enabled)
                        .map(|t| {
                            let (r, _) = resolve_tier_rate(t, quality);
                            (r, t.size.clone())
                        })
                        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                }

                if !normalized.is_empty() {
                    if let Ok(tiers) = serde_json::from_str::<Vec<SizeTier>>(&rule.pricing_tiers) {
                        let mut matched = false;
                        for tier in &tiers {
                            if !tier.enabled {
                                continue;
                            }
                            if normalize_pixel_size(&tier.size) == normalized {
                                let (r, q_desc) = resolve_tier_rate(tier, &features.quality);
                                rate = r;
                                detail_desc = if q_desc.is_empty() {
                                    format!("命中分辨率像素 {} 单价: {:.6}", normalized, rate)
                                } else {
                                    format!(
                                        "命中分辨率像素 {} {} 单价: {:.6}",
                                        normalized, q_desc, rate
                                    )
                                };
                                if features.has_image_ref {
                                    if let Some(m) = tier.image_ref_multiplier.filter(|&m| m != 1.0)
                                    {
                                        rate *= m;
                                        detail_desc.push_str(&format!(" [图生图 x{}]", m));
                                    }
                                }
                                matched = true;
                                break;
                            }
                        }
                        if !matched {
                            if let Some((mr, ms)) = max_tier_rate(&tiers, &features.quality) {
                                rate = mr;
                                detail_desc = format!(
                                    "分辨率像素{}未命中,兜底最高阶梯({} 单价:{:.6})",
                                    normalized, ms, mr
                                );
                            }
                        }
                    }
                } else {
                    // size 不是有效像素格式，兜底最高价
                    if let Ok(tiers) = serde_json::from_str::<Vec<SizeTier>>(&rule.pricing_tiers) {
                        if let Some((mr, ms)) = max_tier_rate(&tiers, &features.quality) {
                            rate = mr;
                            detail_desc = format!(
                                "size({})无法解析,兜底最高阶梯({} 单价:{:.6})",
                                raw_size, ms, mr
                            );
                        }
                    }
                }
                // 提示词扩写倍率
                if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                    if features.prompt_extend {
                        if let Some(m) =
                            ext.get("prompt_extend_multiplier").and_then(|v| v.as_f64())
                        {
                            rate *= m;
                            detail_desc.push_str(&format!(" [提示词扩写 x{}]", m));
                        }
                    }
                }
            } else if rule.billing_rule == "vidu_image" {
                // 腾讯云 Vidu 图片精确查表：属性×分辨率 → 元/张
                count = features.image_count.map(|c| c.max(1) as f64).unwrap_or(1.0);
                detail_desc = format!("Vidu图片查表(默认单价: {})", rate);
                if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                    let res = features.resolution.as_deref().unwrap_or("1k");
                    let ref_count = features.image_ref_count.unwrap_or(0);
                    let attr = if !features.has_image_ref {
                        "text"
                    } else if ref_count <= 1 {
                        "img2img"
                    } else if ref_count <= 3 {
                        "ref_1_3"
                    } else {
                        "ref_4_7"
                    };
                    let table_key = format!("{}|{}", attr, res);
                    if let Some(tr) = lookup_price_table(&ext, &table_key) {
                        rate = tr;
                        detail_desc = format!(
                            "Vidu图片查表({} 单价:{})",
                            translate_billing_key(&table_key),
                            rate
                        );
                    } else {
                        // 未命中：兜底使用 price_table 中同属性的最高价
                        if let Some(pt) = ext.get("price_table").and_then(|p| p.as_object()) {
                            let prefix = format!("{}|", attr);
                            let max_price = pt
                                .iter()
                                .filter(|(k, _)| {
                                    k.to_lowercase().starts_with(&prefix.to_lowercase())
                                })
                                .filter_map(|(k, v)| v.as_f64().map(|p| (k.clone(), p)))
                                .max_by(|a, b| {
                                    a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                                });
                            if let Some((mk, mp)) = max_price {
                                rate = mp;
                                detail_desc = format!(
                                    "Vidu图片{}未命中,兜底最高价({} 单价:{})",
                                    translate_billing_key(&table_key),
                                    translate_billing_key(&mk),
                                    mp
                                );
                            }
                        }
                    }
                }
            } else if rule.billing_rule == "characters" {
                // 按字符数计费（语音合成等）：text_characters / 10000 * fixed_rate * discount
                // fixed_rate 单位：元/万字符（如 2.8 表示 2.8元/万字符）
                let chars = features.text_characters.unwrap_or(0) as f64;
                count = chars / 10000.0;
                detail_desc = format!("按字符计费({}字符)", chars as i32);
            } else if rule.billing_rule == "volc_seedream_pro" {
                // 火山 Seedream Pro：输入图超额（free_image_count，缺省 1=首张免费）
                // + 输出图按总像素万阶梯单价

                // 1) 输入图费：
                let free_images = free_image_count_from_ext(&rule.extended_config, 1);
                let input_images = features.image_ref_count.unwrap_or(0);
                let billable_inputs = (input_images - free_images).max(0);
                let input_rate = rule.prompt_rate;
                let input_cost = billable_inputs as f64 * input_rate;

                // 2) 输出图费：
                let image_count = features.image_count.map(|c| c.max(1) as f64).unwrap_or(1.0);
                let raw_size = features.size.as_deref().unwrap_or("1024x1024");

                // 解析宽和高计算总像素（万像素）
                let mut total_pixels_wan = 104.8576; // 默认 1024 * 1024 / 10000.0
                if let Some((w, h)) = raw_size.split_once('x').and_then(|(ws, hs)| {
                    let w = ws.trim().parse::<f64>().ok()?;
                    let h = hs.trim().parse::<f64>().ok()?;
                    Some((w, h))
                }) {
                    total_pixels_wan = (w * h) / 10000.0;
                }

                // 定义阶梯结构
                #[derive(serde::Deserialize)]
                struct VolcSeedreamTier {
                    pub max_pixels_wan: f64,
                    pub rate: f64,
                    #[serde(default = "crate::relay::default_tier_enabled")]
                    pub enabled: bool,
                }

                let mut output_rate = rule.fixed_rate; // 默认费率兜底
                let mut match_desc = "默认输出单价".to_string();

                if let Ok(tiers) =
                    serde_json::from_str::<Vec<VolcSeedreamTier>>(&rule.pricing_tiers)
                {
                    let mut matched = false;
                    let mut matched_tier: Option<&VolcSeedreamTier> = None;

                    // 升序排列
                    let mut sorted_tiers = tiers;
                    sorted_tiers.sort_by(|a, b| {
                        a.max_pixels_wan
                            .partial_cmp(&b.max_pixels_wan)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });

                    for tier in &sorted_tiers {
                        if !tier.enabled {
                            continue;
                        }
                        if total_pixels_wan <= tier.max_pixels_wan {
                            matched_tier = Some(tier);
                            matched = true;
                            break;
                        }
                    }

                    if !matched {
                        matched_tier = sorted_tiers.iter().filter(|t| t.enabled).last();
                    }

                    if let Some(tier) = matched_tier {
                        output_rate = tier.rate;
                        match_desc = format!(
                            "命中阶梯(<= {:.0}万像素 单价: {:.6})",
                            tier.max_pixels_wan, tier.rate
                        );
                    }
                }

                let output_cost = image_count * output_rate;
                let total_cost = (input_cost + output_cost) * discount;

                let detail = format!(
                    "火山SeedreamPro计费 -> (输入图超额:{}张(共{}张,免费{}张)*{}元/张 + 输出图: {}张*[总像素: {:.2}万, {}])*{:.2}倍率",
                    billable_inputs,
                    input_images,
                    free_images,
                    input_rate,
                    image_count,
                    total_pixels_wan,
                    match_desc,
                    discount
                );
                return (total_cost, detail);
            }
            let cost = count * rate * discount;
            (
                cost,
                format!(
                    "{} -> ({}量 * {:.6}单价 * {:.2}倍率)",
                    detail_desc, count, rate, discount
                ),
            )
        }
        "duration" => {
            let dur = features.duration_seconds.unwrap_or(0.0);
            let mut rate = rule.duration_rate;
            let mut detail_desc = "固定按秒时长计费".to_string();

            // 分辨率阶梯匹配（video_resolution / minimax_h3 共用）
            let match_res_rate = |resolution: Option<&str>,
                                  allow_max_without_res: bool|
             -> (f64, String) {
                if resolution.is_none() && !allow_max_without_res {
                    return (rate, format!("默认单价: {}", rate));
                }
                let Ok(tiers) = serde_json::from_str::<Vec<ResolutionTier>>(&rule.pricing_tiers)
                else {
                    return (rate, format!("默认单价: {}", rate));
                };
                let mut max_rate: Option<f64> = None;
                let mut max_res = String::new();
                for tier in &tiers {
                    if !tier.enabled {
                        continue;
                    }
                    if max_rate.map_or(true, |mr| tier.rate > mr) {
                        max_rate = Some(tier.rate);
                        max_res = tier.resolution.clone();
                    }
                    if let Some(res) = resolution {
                        if tier.resolution.eq_ignore_ascii_case(res) {
                            return (tier.rate, format!("命中分辨率 {} 单价: {}", res, tier.rate));
                        }
                    }
                }
                if let Some(mr) = max_rate {
                    let desc = match resolution {
                        Some(res) => {
                            format!("分辨率{}未命中，兜底最高阶梯({} 单价:{})", res, max_res, mr)
                        }
                        None => format!("无分辨率，兜底最高阶梯({} 单价:{})", max_res, mr),
                    };
                    return (mr, desc);
                }
                (rate, format!("默认单价: {}", rate))
            };

            if rule.billing_rule == "video_resolution" {
                detail_desc = format!("视频分辨率阶梯找寻(默认单价: {})", rate);
                if let Some(res) = features.resolution.as_deref() {
                    let (r, desc) = match_res_rate(Some(res), false);
                    rate = r;
                    detail_desc = desc;
                }
            } else if rule.billing_rule == "minimax_h3" {
                // 分辨率秒单价 × total_seconds + 输入图超额（free_image_count，缺省 5）
                let (r, desc) = match_res_rate(features.resolution.as_deref(), true);
                rate = r;
                detail_desc = format!("视频秒价+输入图 {}", desc);

                let free_images = free_image_count_from_ext(&rule.extended_config, 5);
                let input_images = features.image_ref_count.unwrap_or(0).max(0);
                let billable_images = (input_images - free_images).max(0);
                let image_rate = rule.prompt_rate;
                let total_cost = (dur * rate + billable_images as f64 * image_rate) * discount;
                return (
                    total_cost,
                    format!(
                        "{} -> ({:.2}秒*{} + 输入图超额:{}张(共{}张,免费{}张)*{}元/张)*{:.2}倍率",
                        detail_desc,
                        dur,
                        rate,
                        billable_images,
                        input_images,
                        free_images,
                        image_rate,
                        discount
                    ),
                );
            } else if rule.billing_rule == "video_seconds_io" {
                // 生成秒 × 生成秒单价 + 输入参考秒 × 输入秒单价 + 输入图超额（free_image_count，缺省 5）
                let in_dur = features.video_ref_seconds.unwrap_or(0.0);
                let res = features.resolution.as_deref();
                let mut in_rate = 0.0;
                let mut out_rate = rate;
                detail_desc = format!("视频秒价(输入/生成)+输入图 默认单价: {}", rate);
                if let Ok(tiers) = serde_json::from_str::<Vec<ResolutionTier>>(&rule.pricing_tiers)
                {
                    let enabled: Vec<&ResolutionTier> = tiers.iter().filter(|t| t.enabled).collect();
                    let picked = res
                        .and_then(|r| {
                            enabled
                                .iter()
                                .copied()
                                .find(|t| t.resolution.eq_ignore_ascii_case(r))
                        })
                        .or_else(|| {
                            enabled.iter().copied().max_by(|a, b| {
                                a.rate.partial_cmp(&b.rate).unwrap_or(std::cmp::Ordering::Equal)
                            })
                        });
                    if let Some(t) = picked {
                        in_rate = t.input_rate;
                        out_rate = t.rate;
                        let tier_desc = if res.is_some_and(|r| t.resolution.eq_ignore_ascii_case(r)) {
                            format!("命中分辨率 {}", t.resolution)
                        } else {
                            match res {
                                Some(r) => {
                                    format!("分辨率{}未命中，兜底最高阶梯({})", r, t.resolution)
                                }
                                None => format!("无分辨率，兜底最高阶梯({})", t.resolution),
                            }
                        };
                        detail_desc = format!("视频秒价(输入/生成)+输入图 {}", tier_desc);
                    }
                }
                let free_images = free_image_count_from_ext(&rule.extended_config, 5);
                let input_images = features.image_ref_count.unwrap_or(0).max(0);
                let billable_images = (input_images - free_images).max(0);
                let image_rate = rule.prompt_rate;
                let total_cost =
                    (dur * out_rate + in_dur * in_rate + billable_images as f64 * image_rate)
                        * discount;
                return (
                    total_cost,
                    format!(
                        "{} -> (生成:{:.2}秒*{} + 输入:{:.2}秒*{} + 输入图超额:{}张(共{}张,免费{}张)*{}元/张)*{:.2}倍率",
                        detail_desc,
                        dur,
                        out_rate,
                        in_dur,
                        in_rate,
                        billable_images,
                        input_images,
                        free_images,
                        image_rate,
                        discount
                    ),
                );
            } else if rule.billing_rule == "video_quality" {
                detail_desc = format!("视频画质阶梯找寻(默认单价: {})", rate);
                if let Some(res) = &features.resolution {
                    if let Ok(tiers) =
                        serde_json::from_str::<Vec<VideoQualityTier>>(&rule.pricing_tiers)
                    {
                        let fps_val = features.fps.unwrap_or(30.0);
                        let is_high_fps = fps_val > 30.0;

                        let mut target_res = "unknown";
                        let mut short_side: Option<i32> = None;
                        let res_normalized = res.to_lowercase().replace("*", "x");

                        if res_normalized.contains("720p") || res_normalized == "720" {
                            target_res = "720p";
                            short_side = Some(720);
                        } else if res_normalized.contains("1080p") || res_normalized == "1080" {
                            target_res = "1080p";
                            short_side = Some(1080);
                        } else if res_normalized.contains("2k") {
                            target_res = "2k";
                            short_side = Some(1440);
                        } else if res_normalized.contains("4k") {
                            target_res = "4k";
                            short_side = Some(2160);
                        } else {
                            let parts: Vec<&str> = res_normalized.split('x').collect();
                            let mut side: Option<i32> = None;
                            if parts.len() == 2 {
                                if let (Ok(w), Ok(h)) = (
                                    parts[0].trim().parse::<i32>(),
                                    parts[1].trim().parse::<i32>(),
                                ) {
                                    side = Some(std::cmp::min(w, h));
                                }
                            }
                            if side.is_none() {
                                let num_str: String = res_normalized
                                    .chars()
                                    .filter(|c| c.is_ascii_digit())
                                    .collect();
                                if let Ok(num) = num_str.parse::<i32>() {
                                    if num > 100 {
                                        side = Some(num);
                                    }
                                }
                            }

                            if let Some(s) = side {
                                short_side = Some(s);
                                target_res = if s <= 720 {
                                    "720p"
                                } else if s <= 1080 {
                                    "1080p"
                                } else if s <= 1440 {
                                    "2k"
                                } else {
                                    "4k"
                                };
                            }
                        }

                        let mut matched = false;
                        let mut max_rate: Option<f64> = None;
                        let mut max_tier_desc = String::new();

                        for tier in &tiers {
                            if !tier.enabled {
                                continue;
                            }
                            if max_rate.map_or(true, |mr| tier.rate > mr) {
                                max_rate = Some(tier.rate);
                                max_tier_desc = format!("{} {}", tier.resolution, tier.fps_range);
                            }

                            let res_match = tier.resolution.eq_ignore_ascii_case(target_res);
                            let fps_match = if is_high_fps {
                                tier.fps_range.contains(">30") || tier.fps_range.contains("> 30")
                            } else {
                                tier.fps_range.contains("<30")
                                    || tier.fps_range.contains("<=30")
                                    || tier.fps_range.contains("<= 30")
                            };

                            if res_match && fps_match {
                                rate = tier.rate;
                                if let Some(side) = short_side {
                                    detail_desc = format!(
                                        "命中视频画质阶梯 [{}] [帧率: {}] (解析短边: {}) 单价: {}",
                                        target_res, tier.fps_range, side, rate
                                    );
                                } else {
                                    detail_desc = format!(
                                        "命中视频画质阶梯 [{}] [帧率: {}] 单价: {}",
                                        target_res, tier.fps_range, rate
                                    );
                                }
                                matched = true;
                                break;
                            }
                        }

                        if !matched {
                            if let Some(mr) = max_rate {
                                rate = mr;
                                if let Some(side) = short_side {
                                    detail_desc = format!("视频画质阶梯未命中 [{}] (解析短边: {}, 帧率: {})，兜底最高阶梯({} 单价:{})", target_res, side, fps_val, max_tier_desc, mr);
                                } else {
                                    detail_desc = format!("视频画质阶梯未命中 [{}] (帧率: {})，兜底最高阶梯({} 单价:{})", target_res, fps_val, max_tier_desc, mr);
                                }
                            }
                        }
                    }
                }
            } else if rule.billing_rule == "kling_video" {
                // 可灵视频按秒计费：price_table 精确查表优先，倍率乘法兜底
                detail_desc = format!("可灵视频按秒计费(基准: {})", rate);
                if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                    let mapped_mode;
                    let mut raw_mode = "std";
                    if let Some(ref m) = features.mode {
                        raw_mode = m;
                    } else if let Some(ref res) = features.resolution {
                        // 新可灵无 mode，仅有 resolution（720p/1080p/…）时映射到查表键
                        let res_lower = res.to_lowercase();
                        if res_lower == "720p" {
                            mapped_mode = "std".to_string();
                        } else if res_lower == "1080p" {
                            mapped_mode = "pro".to_string();
                        } else {
                            mapped_mode = res_lower;
                        }
                        raw_mode = &mapped_mode;
                    }

                    let raw_sound = features.sound.as_deref().unwrap_or("off");
                    let raw_video = if features.has_video { "yes" } else { "no" };

                    // 维度开关：关闭时锁定为默认值，不参与差异化计费
                    let final_mode = if ext
                        .get("enable_mode")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true)
                    {
                        raw_mode
                    } else {
                        "std"
                    };
                    let final_sound = if ext
                        .get("enable_sound")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true)
                    {
                        raw_sound
                    } else {
                        "off"
                    };
                    let final_video = if ext
                        .get("enable_video_ref")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        raw_video
                    } else {
                        "no"
                    };

                    // 优先：精确查表（price_table）
                    let table_key = format!("{}|{}|{}", final_mode, final_sound, final_video);
                    let table_rate = lookup_price_table(&ext, &table_key);

                    if let Some(tr) = table_rate {
                        rate = tr;
                        detail_desc = format!(
                            "可灵视频查表({} 单价:{})",
                            translate_billing_key(&table_key),
                            rate
                        );
                    } else {
                        // 兜底：倍率乘法（兼容旧版规则）
                        let mode_mult = ext
                            .get("mode_multipliers")
                            .and_then(|m| m.get(final_mode))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(1.0);
                        let sound_mult = ext
                            .get("sound_multipliers")
                            .and_then(|m| m.get(final_sound))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(1.0);
                        rate *= mode_mult * sound_mult;
                        detail_desc = format!(
                            "可灵视频倍率(mode:{}x{} sound:{}x{} 单价:{})",
                            final_mode, mode_mult, final_sound, sound_mult, rate
                        );
                    }
                }
            } else if rule.billing_rule == "vidu_video" {
                // 腾讯云 Vidu 视频精确查表：属性×分辨率 → 元/秒
                detail_desc = format!("Vidu视频查表(基准: {})", rate);
                if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                    let res = features.resolution.as_deref().unwrap_or("720p");
                    let attr = if features.has_image_ref {
                        "image"
                    } else if features.has_video {
                        "ref"
                    } else {
                        "text"
                    };
                    let table_key = format!("{}|{}", attr, res);
                    if let Some(tr) = lookup_price_table(&ext, &table_key) {
                        rate = tr;
                        detail_desc = format!(
                            "Vidu视频查表({} 单价:{})",
                            translate_billing_key(&table_key),
                            rate
                        );
                    } else {
                        // 未命中：兜底使用 price_table 中同属性的最高价
                        if let Some(pt) = ext.get("price_table").and_then(|p| p.as_object()) {
                            let prefix = format!("{}|", attr);
                            let max_price = pt
                                .iter()
                                .filter(|(k, _)| {
                                    k.to_lowercase().starts_with(&prefix.to_lowercase())
                                })
                                .filter_map(|(k, v)| v.as_f64().map(|p| (k.clone(), p)))
                                .max_by(|a, b| {
                                    a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                                });
                            if let Some((mk, mp)) = max_price {
                                rate = mp;
                                detail_desc = format!(
                                    "Vidu视频{}未命中,兜底最高价({} 单价:{})",
                                    translate_billing_key(&table_key),
                                    translate_billing_key(&mk),
                                    mp
                                );
                            }
                        }
                    }
                    // 错峰模式：优先检查请求体的 OutputConfig.OffPeak，其次 service_tier=flex
                    let is_offpeak = features.service_tier.as_deref() == Some("flex");
                    if is_offpeak {
                        let offpeak_discount = ext
                            .get("offpeak_discount")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.5);
                        rate *= offpeak_discount;
                        detail_desc.push_str(&format!(" [错峰 x{}]", offpeak_discount));
                    }
                }
            } else if rule.billing_rule == "volc_enhance_cascade" {
                // 级联画质增强查表：version(fast|standard|pro|ai，优先规则 res_enhance，缺省 standard)×分辨率×是否有视频输入
                detail_desc = format!("火山级联画质增强查表(基准: {})", rate);
                if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                    let raw_version = features.version.as_deref().unwrap_or("standard");
                    // 与级联 cascade.resolution / 阶段二目标缺省一致
                    let raw_res = features.resolution.as_deref().unwrap_or("720p");
                    let raw_video = if features.has_video { "yes" } else { "no" };

                    let table_key = format!("{}|{}|{}", raw_version, raw_res, raw_video);
                    if let Some(tr) = lookup_price_table(&ext, &table_key) {
                        rate = tr;
                        detail_desc = format!(
                            "火山级联画质增强查表({} 单价:{})",
                            translate_billing_key(&table_key),
                            rate
                        );
                    } else {
                        // 未命中：兜底使用 price_table 中同属性版本的最高价
                        if let Some(pt) = ext.get("price_table").and_then(|p| p.as_object()) {
                            let prefix = format!("{}|", raw_version);
                            let max_price = pt
                                .iter()
                                .filter(|(k, _)| {
                                    k.to_lowercase().starts_with(&prefix.to_lowercase())
                                })
                                .filter_map(|(k, v)| v.as_f64().map(|p| (k.clone(), p)))
                                .max_by(|a, b| {
                                    a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                                });
                            if let Some((mk, mp)) = max_price {
                                rate = mp;
                                detail_desc = format!(
                                    "火山级联画质增强{}未命中,兜底最高价({} 单价:{})",
                                    translate_billing_key(&table_key),
                                    translate_billing_key(&mk),
                                    mp
                                );
                            }
                        }
                    }
                }
            }

            let cost = dur * rate * discount;
            (
                cost,
                format!(
                    "{} -> ({:.2}秒 * {}单价 * {:.2}倍率)",
                    detail_desc, dur, rate, discount
                ),
            )
        }
        _ => {
            // tokens 计费
            let mut p_rate = rule.prompt_rate;
            let mut c_rate = rule.completion_rate;
            let mut is_overridden = false;
            let mut detail_desc = "标准 Tokens 计费".to_string();

            if rule.billing_rule == "seedance2.0" {
                if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                    if let Some(rates) = ext.get("resolution_rates") {
                        let res_key = features
                            .resolution
                            .as_deref()
                            .unwrap_or("720p")
                            .to_lowercase();
                        let tier = rates.get(&res_key).or_else(|| rates.get("720p"));
                        let (rate_field, video_label) = if features.has_video {
                            ("with_video", "含视频")
                        } else {
                            ("without_video", "无视频")
                        };
                        if let Some(rate) = tier
                            .and_then(|t| t.get(rate_field))
                            .and_then(|v| v.as_f64())
                        {
                            p_rate = rate;
                            c_rate = rate;
                            is_overridden = true;
                            detail_desc = format!(
                                "Seedance2.0({}|{}|基本单价:{})",
                                res_key, video_label, rate
                            );
                        }
                    }
                }
            } else if rule.billing_rule == "seedance1.5pro" {
                if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                    let mut rate = None;
                    let mut desc = String::new();
                    if features.has_audio {
                        if let Some(ar) = ext.get("audio_rate").and_then(|v| v.as_f64()) {
                            rate = Some(ar);
                            desc = format!("Seedance1.5Pro(含语音单价:{})", ar);
                        }
                    } else {
                        if let Some(br) = ext.get("base_rate").and_then(|v| v.as_f64()) {
                            rate = Some(br);
                            desc = format!("Seedance1.5Pro(无语音单价:{})", br);
                        }
                    }
                    if let Some(mut r) = rate {
                        if features.service_tier.as_deref() == Some("flex") {
                            let discount = ext
                                .get("offline_discount")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.5);
                            r *= discount;
                            desc.push_str(" [离线推理]");
                        }
                        p_rate = r;
                        c_rate = r;
                        is_overridden = true;
                        detail_desc = desc;
                    }
                }
            } else if rule.billing_rule == "seedance1.0" {
                if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                    if features.service_tier.as_deref() == Some("flex") {
                        if let Some(off_rate) = ext.get("offline_rate").and_then(|v| v.as_f64()) {
                            p_rate = off_rate;
                            c_rate = off_rate;
                            is_overridden = true;
                            detail_desc = format!("Seedance1.0(离线推理单价:{})", off_rate);
                        }
                    } else {
                        if let Some(on_rate) = ext.get("online_rate").and_then(|v| v.as_f64()) {
                            p_rate = on_rate;
                            c_rate = on_rate;
                            is_overridden = true;
                            detail_desc = format!("Seedance1.0(在线推理单价:{})", on_rate);
                        }
                    }
                }
            }

            let mut is_cached_rate_set = false;
            let mut cached_r = rule.cached_rate;
            if cached_r > 0.0 {
                is_cached_rate_set = true;
            }
            // 仅阶梯档位可配置写入费率；未配置时写入量并入未缓存输入（保持旧行为）
            let mut is_cache_write_rate_set = false;
            let mut cache_write_r = 0.0;

            if !is_overridden && rule.billing_rule == "tiered" {
                let mut tiers: Vec<crate::models::PricingTier> =
                    serde_json::from_str(&rule.pricing_tiers).unwrap_or_default();
                tiers.sort_by(|a, b| {
                    a.max_prompt_tokens
                        .partial_cmp(&b.max_prompt_tokens)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                // 前端界限单位为千 (K)
                let p_k = prompt_tokens as f64 / 1000.0;
                let c_k = completion_tokens as f64 / 1000.0;
                let matched = tiers.iter().find(|t| {
                    p_k <= t.max_prompt_tokens
                        && t.max_completion_tokens.map_or(true, |mc| c_k <= mc)
                });
                let (tier, desc) = if let Some(t) = matched {
                    let desc = match t.max_completion_tokens {
                        Some(mc) => format!("阶梯(≤{}K入|≤{}K出)", t.max_prompt_tokens, mc),
                        None => format!("阶梯(≤{}K入)", t.max_prompt_tokens),
                    };
                    (Some(t), desc)
                } else if let Some(last) = tiers.last() {
                    (
                        Some(last),
                        format!("阶梯(超限·最高档{}K入)", last.max_prompt_tokens),
                    )
                } else {
                    (None, detail_desc.clone())
                };
                if let Some(tier) = tier {
                    p_rate = tier.prompt_rate;
                    c_rate = tier.completion_rate;
                    // 阶梯未设置缓存费率时，不继承全局规则
                    is_cached_rate_set = tier.cached_rate > 0.0;
                    if is_cached_rate_set {
                        cached_r = tier.cached_rate;
                    }
                    is_cache_write_rate_set = tier.cache_write_rate > 0.0;
                    if is_cache_write_rate_set {
                        cache_write_r = tier.cache_write_rate;
                    }
                    detail_desc = desc;
                }
            } else if !is_overridden && rule.billing_rule == "doubao_chat" {
                // 豆包聊天阶梯计费：分离音频/非音频独立计价，对齐官方公式
                let mut tiers: Vec<crate::models::PricingTier> =
                    serde_json::from_str(&rule.pricing_tiers).unwrap_or_default();
                tiers.sort_by(|a, b| {
                    a.max_prompt_tokens
                        .partial_cmp(&b.max_prompt_tokens)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                let p_k = prompt_tokens as f64 / 1000.0;
                let c_k = completion_tokens as f64 / 1000.0;

                let matched_tier = tiers
                    .iter()
                    .find(|t| {
                        p_k <= t.max_prompt_tokens
                            && t.max_completion_tokens.map_or(true, |mc| c_k <= mc)
                    })
                    .or(tiers.last());

                if let Some(tier) = matched_tier {
                    let is_fast = features.service_tier.as_deref() == Some("fast");
                    let tier_desc = match tier.max_completion_tokens {
                        Some(mc) => format!("<={}K_P|<={}K_C", tier.max_prompt_tokens, mc),
                        None => format!("<={}K_P", tier.max_prompt_tokens),
                    };

                    // 分离音频/非音频 token（缓存是 prompt 子集，需拆分）
                    // 注：audio_tokens 在豆包语境下指"非缓存的音频输入 token"，audio_cached_tokens 是"已缓存的音频 token"
                    let eff_prompt = (prompt_tokens - cached_tokens).max(0);
                    let non_audio_prompt = (eff_prompt - audio_tokens).max(0);
                    let non_audio_cached = (cached_tokens - audio_cached_tokens).max(0);

                    // 选取费率组：service_tier=fast → 低延迟（未设置降级常规），否则常规
                    let (pr, cr, cache_r, audio_pr, audio_cache_r) = if is_fast {
                        (
                            if tier.fast_prompt_rate > 0.0 {
                                tier.fast_prompt_rate
                            } else {
                                tier.prompt_rate
                            },
                            if tier.fast_completion_rate > 0.0 {
                                tier.fast_completion_rate
                            } else {
                                tier.completion_rate
                            },
                            if tier.fast_cached_rate > 0.0 {
                                tier.fast_cached_rate
                            } else {
                                tier.cached_rate
                            },
                            if tier.fast_audio_prompt_rate > 0.0 {
                                tier.fast_audio_prompt_rate
                            } else {
                                tier.audio_prompt_rate
                            },
                            if tier.fast_audio_cached_rate > 0.0 {
                                tier.fast_audio_cached_rate
                            } else {
                                tier.audio_cached_rate
                            },
                        )
                    } else {
                        (
                            tier.prompt_rate,
                            tier.completion_rate,
                            tier.cached_rate,
                            tier.audio_prompt_rate,
                            tier.audio_cached_rate,
                        )
                    };

                    let cost_raw = (non_audio_prompt as f64 * pr
                        + audio_tokens as f64 * audio_pr
                        + non_audio_cached as f64 * cache_r
                        + audio_cached_tokens as f64 * audio_cache_r
                        + completion_tokens as f64 * cr)
                        / 1_000_000.0;

                    let mode_label = if is_fast { "低延迟" } else { "常规" };
                    let d_raw = format!("豆包阶梯[{}]({}) -> ({}非音P*{} + {}音P*{} + {}非音C*{} + {}音C*{} + {}Out*{})/1M",
                        mode_label, tier_desc,
                        non_audio_prompt, pr, audio_tokens, audio_pr,
                        non_audio_cached, cache_r, audio_cached_tokens, audio_cache_r,
                        completion_tokens, cr);
                    let (cost, d) = apply_web_search(cost_raw, d_raw);
                    return (cost, d);
                }
            } else if !is_overridden && rule.billing_rule == "multimodal" {
                // 多模态 tokens 分类计价：文本输入使用已配置的输入单价，图片输入使用 image_prompt_rate，无需输出单价
                let mut img_prompt_rate = rule.prompt_rate;
                if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                    if let Some(r) = ext.get("image_prompt_rate").and_then(|v| v.as_f64()) {
                        img_prompt_rate = r;
                    }
                }

                // 计算公式：文本输入 + 图片输入
                let cost_raw = (prompt_tokens as f64 * p_rate
                    + usage.image_tokens as f64 * img_prompt_rate)
                    / 1_000_000.0;

                let d_raw = format!(
                    "多模态计费 -> ({}文本P*{} + {}图片P*{})/1M",
                    prompt_tokens, p_rate, usage.image_tokens, img_prompt_rate
                );
                let (cost, d) = apply_web_search(cost_raw, d_raw);
                return (cost, d);
            } else if !is_overridden && rule.billing_rule == "gpt_billing" {
                // GPT 官方计费逻辑：对输入文本、输入图片、输出图片、输入文本缓存、输入图片缓存等分别计费，每一项可开启关闭，开启即使用
                let mut cost_raw = 0.0;
                let mut detail_parts = Vec::new();

                if let Ok(ext) = serde_json::from_str::<serde_json::Value>(&rule.extended_config) {
                    if let Some(gpt_config) = ext.get("gpt_config") {
                        // 推导各项 tokens 消耗
                        let total_input = prompt_tokens;
                        let img_input = usage.image_tokens;
                        let text_input = (total_input - img_input).max(0);
                        let total_cached = cached_tokens;

                        // 根据请求特征是否包含图片直接区分文本缓存和图片缓存
                        let (img_cached, text_cached) = if total_cached > 0 {
                            if features.has_image_ref {
                                (total_cached, 0)
                            } else {
                                (0, total_cached)
                            }
                        } else {
                            (0, 0)
                        };

                        let non_cached_text = (text_input - text_cached).max(0);
                        let non_cached_image = (img_input - img_cached).max(0);
                        let img_output = completion_tokens; // 直接复用 completion

                        // 循环遍历计费项，减少冗余代码
                        let items = [
                            ("input_text", "文本输入", non_cached_text),
                            ("input_image", "图片输入", non_cached_image),
                            ("cached_input_text", "文本缓存", text_cached),
                            ("cached_input_image", "图片缓存", img_cached),
                            ("output_image", "图片输出", img_output),
                        ];

                        for &(key, label, tokens) in &items {
                            let enabled = gpt_config
                                .pointer(&format!("/{}/enabled", key))
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if enabled {
                                let rate = gpt_config
                                    .pointer(&format!("/{}/rate", key))
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(0.0);
                                cost_raw += tokens as f64 * rate;
                                detail_parts.push(format!("{}:{}*{}", label, tokens, rate));
                            }
                        }
                        cost_raw /= 1_000_000.0;
                    }
                }

                let d_raw = if detail_parts.is_empty() {
                    "GPT图片计费(未启用任何计费项)".to_string()
                } else {
                    format!("GPT图片计费 -> ({})/1M", detail_parts.join(" + "))
                };

                let cost = cost_raw * discount;
                let detail = format!("{} * {:.2}倍率", d_raw, discount);
                return (cost, detail);
            }

            // 上游 prompt_tokens 通常已包含图片 token，无条件累加会造成双重计费；
            // 仅当 prompt 小于 image_tokens（极少数非标上游未计入图片）时才用 image_tokens 兜底
            let raw_prompt_tokens = prompt_tokens;
            let prompt_tokens = if usage.image_tokens > prompt_tokens {
                usage.image_tokens
            } else {
                prompt_tokens
            };

            // 补充兜底的 flex 离线折扣检查（当使用 standard / tiered 或者其他策略不匹配导致回退时）
            // 确保不与其他已经自带离线的策略(如已被拦截处理过的描述中包含了"离线")产生重复折扣
            // 注意：火山的 Seedance 2.0/2.0 fast 已不支持 flex 离线降级
            if features.service_tier.as_deref() == Some("flex")
                && !detail_desc.contains("离线")
                && rule.billing_rule != "seedance2.0"
            {
                let off_discount = if let Ok(ext) =
                    serde_json::from_str::<serde_json::Value>(&rule.extended_config)
                {
                    ext.get("offline_discount")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.5)
                } else {
                    0.5
                };
                p_rate *= off_discount;
                c_rate *= off_discount;
                if is_cached_rate_set {
                    cached_r *= off_discount;
                }
                if is_cache_write_rate_set {
                    cache_write_r *= off_discount;
                }
                detail_desc = format!("{} [叠加Flex离线折扣: {}倍]", detail_desc, off_discount);
            }

            // Claude 语义判定：有缓存创建 token，或配置了 Claude 读取费率且有缓存读取 token
            let is_claude = features.cache_creation.filter(|&n| n > 0).is_some()
                || (rule.claude_cache_read_rate > 0.0 && cached_tokens > 0);

            // 上游显式 total_tokens 且 total==prompt+completion → OpenAI 子集语义（回填 total 不算）
            // 同时接受未加图片兜底前的原值匹配，避免守卫抬高 prompt_tokens 后误判成 Anthropic 语义
            let is_openai_format = usage.has_total_tokens
                && (usage.total == prompt_tokens + completion_tokens
                    || usage.total == raw_prompt_tokens + completion_tokens);
            if !is_openai_format && cached_tokens > 0 && is_cached_rate_set {
                tracing::warn!(
                    prompt_tokens,
                    raw_prompt_tokens,
                    cached_tokens,
                    "计费协议判据失配：缓存可能重复计费"
                );
            }
            let cc = features.cache_creation.unwrap_or(0);

            let (cost, detail_str) = if is_claude {
                // Claude：创建/读取独立计价；OpenAI 包装含缓存时拆分，Anthropic 原生不拆
                let effective_prompt = if is_openai_format && prompt_tokens >= cc + cached_tokens {
                    prompt_tokens - cc - cached_tokens
                } else {
                    prompt_tokens
                };
                let cc_rate = rule.claude_cache_creation_rate;
                let cr_rate = if rule.claude_cache_read_rate > 0.0 {
                    rule.claude_cache_read_rate
                } else if is_cached_rate_set {
                    cached_r
                } else {
                    p_rate
                };
                let creation = if cc > 0 && cc_rate > 0.0 {
                    cc as f64 * cc_rate
                } else {
                    0.0
                };
                let read = if cached_tokens > 0 {
                    cached_tokens as f64 * cr_rate
                } else {
                    0.0
                };
                let cost_raw = (effective_prompt as f64 * p_rate
                    + completion_tokens as f64 * c_rate
                    + creation
                    + read)
                    / 1_000_000.0;
                let d = format!(
                    "{} -> ({}P*{} + {}C*{} + {}创建@{} + {}读取@{})/1M",
                    detail_desc,
                    effective_prompt,
                    p_rate,
                    completion_tokens,
                    c_rate,
                    cc,
                    cc_rate,
                    cached_tokens,
                    cr_rate
                );
                (cost_raw, d)
            } else {
                let (effective_prompt, bill_cache, bill_write) = split_prompt_subsets(
                    prompt_tokens,
                    cached_tokens,
                    cache_write_tokens,
                    is_cached_rate_set,
                    is_cache_write_rate_set,
                    is_openai_format,
                );

                let cost_raw = (effective_prompt as f64 * p_rate
                    + completion_tokens as f64 * c_rate
                    + bill_cache as f64 * cached_r
                    + bill_write as f64 * cache_write_r)
                    / 1_000_000.0;

                let mut d = format!(
                    "{} -> ({:.0}入*{} + {:.0}出*{}",
                    detail_desc, effective_prompt, p_rate, completion_tokens, c_rate
                );
                if bill_cache > 0 {
                    d.push_str(&format!(" + {:.0}读*{}", bill_cache, cached_r));
                }
                if bill_write > 0 {
                    d.push_str(&format!(" + {:.0}写*{}", bill_write, cache_write_r));
                }
                d.push_str(")/1M");
                if cached_tokens > 0 && bill_cache == 0 {
                    d.push_str(&format!(" [含{:.0}读·按输入价]", cached_tokens));
                }
                if cache_write_tokens > 0 && bill_write == 0 {
                    d.push_str(&format!(" [含{:.0}写·按输入价]", cache_write_tokens));
                }
                (cost_raw, d)
            };
            let (cost, d) = apply_web_search(cost, detail_str);
            (cost, d)
        }
    }
}

/// OpenAI（显式 total 且 prompt 盖住缓存）：独立费率则从 prompt 拆出，否则已含在内。
/// 否则按 Anthropic：独立费率累加，否则并入输入价。
fn split_prompt_subsets(
    prompt: i32,
    cached: i32,
    cache_write: i32,
    bill_cached: bool,
    bill_write: bool,
    is_openai_format: bool,
) -> (i32, i32, i32) {
    let cached = cached.max(0);
    let cache_write = cache_write.max(0);
    let bill_c = if bill_cached { cached } else { 0 };
    let bill_w = if bill_write { cache_write } else { 0 };
    if is_openai_format && prompt >= cached + cache_write {
        let sub = bill_c + bill_w;
        if sub > 0 {
            (prompt - sub, bill_c, bill_w)
        } else {
            (prompt, 0, 0)
        }
    } else {
        (
            prompt + cached - bill_c + cache_write - bill_w,
            bill_c,
            bill_w,
        )
    }
}

/// price_table 查表辅助：精确匹配优先，fallback 大小写不敏感遍历
/// - 被 price_table_disabled 标记的 key 视为未配置（返回 None），防止 0 值覆盖默认费率
/// - 大小写不敏感 fallback 兼容 features.resolution 统一转小写后与后台 key 不一致的情况
fn lookup_price_table(ext: &serde_json::Value, table_key: &str) -> Option<f64> {
    // 检查 disabled 列表（大小写不敏感）
    if let Some(disabled) = ext.get("price_table_disabled").and_then(|v| v.as_array()) {
        let lower_key = table_key.to_lowercase();
        if disabled
            .iter()
            .any(|k| k.as_str().map_or(false, |s| s.to_lowercase() == lower_key))
        {
            return None;
        }
    }
    let pt = ext.get("price_table")?;
    // 精确匹配
    if let Some(v) = pt.get(table_key).and_then(|v| v.as_f64()) {
        return Some(v);
    }
    // 大小写不敏感 fallback
    let lower_key = table_key.to_lowercase();
    pt.as_object()?
        .iter()
        .find(|(k, _)| k.to_lowercase() == lower_key)
        .and_then(|(_, v)| v.as_f64())
}

/// 将 price_table 中用 | 拼接的多维度 key 翻译成友好中文格式显示
pub fn translate_billing_key(key: &str) -> String {
    let parts: Vec<String> = key
        .split('|')
        .map(|part| {
            let part_trimmed = part.trim().to_lowercase();
            match part_trimmed.as_str() {
                "std" => "标准".to_string(),
                "pro" => "专业".to_string(),
                "fast" => "极速".to_string(),
                "standard" => "标准".to_string(),
                "ai" => "大模型".to_string(),
                "text" => "文生".to_string(),
                "image" => "图生".to_string(),
                "ref" => "视频参考".to_string(),
                "on" => "有声".to_string(),
                "off" => "无声".to_string(),
                "yes" => "含视频".to_string(),
                "no" => "无视频".to_string(),
                "img2img" => "图生图".to_string(),
                "ref_1_3" => "1-3张参考".to_string(),
                "ref_4_7" => "4-7张参考".to_string(),
                other => other.to_string(),
            }
        })
        .collect();
    parts.join("|")
}

/// 阶梯 enabled 缺省视为启用（与前端 Switch / RateDisplay 语义一致）
pub(crate) fn default_tier_enabled() -> bool {
    true
}

/// 从 extended_config 读取输入图免费张数（缺省用 default，且不为负）
fn free_image_count_from_ext(extended_config: &str, default: i32) -> i32 {
    serde_json::from_str::<serde_json::Value>(extended_config)
        .ok()
        .and_then(|ext| {
            ext.get("free_image_count")
                .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        })
        .unwrap_or(default as i64)
        .max(0) as i32
}

/// 将 size 参数统一为像素分辨率格式（小写 x 分隔）
/// 支持：1024x1024、1024*1024、1024×1024、1024X1024、2:3
/// K 等级映射：1k→1024x1024、2k→2048x2048、3k→3072x3072、4k→4096x4096
fn normalize_pixel_size(size: &str) -> String {
    let s = size.trim().to_lowercase();
    // K 等级映射
    if let Some(k) = s.strip_suffix('k') {
        if let Ok(n) = k.parse::<u32>() {
            let px = n * 1024;
            return format!("{}x{}", px, px);
        }
    }
    // 像素格式：支持 x、*、×（Unicode 乘号）、:（比例）分隔
    let normalized = s
        .replace('*', "x")
        .replace('\u{00d7}', "x")
        .replace(':', "x");
    let parts: Vec<&str> = normalized.split('x').collect();
    if parts.len() == 2 {
        if parts[0].trim().parse::<u32>().is_ok() && parts[1].trim().parse::<u32>().is_ok() {
            return format!("{}x{}", parts[0].trim(), parts[1].trim());
        }
    }
    String::new()
}

#[cfg(test)]
mod billing_hotfix_tests {
    use crate::models::BillingRule;
    use crate::relay::usage_extractor::{ExtractedFeatures, UsageTokens};
    use serde_json::json;

    fn rule(prompt_rate: f64, completion_rate: f64, cached_rate: f64) -> BillingRule {
        serde_json::from_value(json!({
            "id": 91, "name": "hotfix_rule", "billing_type": "tokens",
            "prompt_rate": prompt_rate, "completion_rate": completion_rate, "cached_rate": cached_rate,
            "claude_cache_creation_rate": 0.0, "claude_cache_read_rate": 0.0,
            "fixed_rate": 0.0, "duration_rate": 0.0, "billing_rule": "standard",
            "pricing_tiers": "[]", "extended_config": "{}",
            "is_active": 1, "is_system": 0, "pid": "p_hotfix", "pricing_type": "tokens",
            "sort_order": 0, "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
            "applied_multiplier": 1.0,
        }))
        .expect("构造 BillingRule 失败")
    }

    /// 线上真实错计行 id=721603：prompt 已含 18395 图片 token，旧代码会再加一次
    #[test]
    fn image_tokens_are_not_added_again() {
        let u = UsageTokens {
            prompt: 680772, completion: 769, cached: 678912,
            image_tokens: 18395, total: 681541, has_total_tokens: true,
            ..Default::default()
        };
        let (cost, detail) = super::compute_cost_raw(
            None, Some(&rule(12.0, 36.0, 1.5)), &u, 0.66, &ExtractedFeatures::default());
        assert!(detail.contains("1860入*12"), "detail = {}", detail);
        assert!(!detail.contains("699167"), "仍发生图片重复相加：{}", detail);
        let expect = (1860.0 * 12.0 + 769.0 * 36.0 + 678912.0 * 1.5) / 1_000_000.0 * 0.66;
        assert!((cost - expect).abs() < 1e-5, "cost={} expect={}", cost, expect);
    }

    /// Anthropic 语义不得受影响（线上真实行 id=777397）
    #[test]
    fn anthropic_claude_billing_unchanged() {
        let mut r = rule(27.12, 135.6, 0.0);
        r.claude_cache_read_rate = 1.36;
        r.claude_cache_creation_rate = 33.9;
        let u = UsageTokens {
            prompt: 2, completion: 616, cached: 58342, cache_creation: 352,
            ..Default::default()
        };
        let f = ExtractedFeatures { cache_creation: Some(352), ..Default::default() };
        let (cost, detail) = super::compute_cost_raw(None, Some(&r), &u, 1.0, &f);
        assert!(detail.contains("58342读取"), "detail = {}", detail);
        let expect = (2.0 * 27.12 + 616.0 * 135.6 + 352.0 * 33.9 + 58342.0 * 1.36) / 1_000_000.0;
        assert!((cost - expect).abs() < 1e-5, "cost={} expect={}", cost, expect);
    }

    /// 非标上游：prompt 未含图片，守卫补差后仍须判为 OpenAI 子集
    #[test]
    fn nonstandard_upstream_keeps_openai_format() {
        let u = UsageTokens {
            prompt: 100, completion: 10, image_tokens: 5000,
            total: 110, has_total_tokens: true, ..Default::default()
        };
        let (cost, detail) = super::compute_cost_raw(
            None, Some(&rule(12.0, 36.0, 0.0)), &u, 1.0, &ExtractedFeatures::default());
        assert!(detail.contains("5000入*12"), "detail = {}", detail);
        let expect = (5000.0 * 12.0 + 10.0 * 36.0) / 1_000_000.0;
        assert!((cost - expect).abs() < 1e-6, "cost={} expect={}", cost, expect);
    }

    /// 无图无缓存普通行不得受影响
    #[test]
    fn plain_row_unaffected() {
        let u = UsageTokens {
            prompt: 1057, completion: 22, total: 1079, has_total_tokens: true,
            ..Default::default()
        };
        let (_, detail) = super::compute_cost_raw(
            None, Some(&rule(27.12, 135.6, 0.0)), &u, 1.0, &ExtractedFeatures::default());
        assert!(detail.contains("1057入*27.12"), "detail = {}", detail);
    }
}

#[cfg(test)]
mod billing_io_tests {
    use crate::models::BillingRule;
    use crate::relay::usage_extractor::{extract_request_features, ExtractedFeatures, UsageTokens};
    use serde_json::json;

    fn rule(billing_type: &str, billing_rule: &str, tiers: serde_json::Value, ext: serde_json::Value) -> BillingRule {
        serde_json::from_value(json!({
            "id": 92, "name": "io_rule", "billing_type": billing_type,
            "prompt_rate": 0.2, "completion_rate": 0.0, "cached_rate": 0.0,
            "claude_cache_creation_rate": 0.0, "claude_cache_read_rate": 0.0,
            "fixed_rate": 0.0, "duration_rate": 0.5, "billing_rule": billing_rule,
            "pricing_tiers": tiers.to_string(), "extended_config": ext.to_string(),
            "is_active": 1, "is_system": 0, "pid": "p_io", "pricing_type": "custom",
            "sort_order": 0, "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
            "applied_multiplier": 1.0,
        }))
        .expect("构造 BillingRule 失败")
    }

    fn cost(r: &BillingRule, f: &ExtractedFeatures, discount: f64) -> (f64, String) {
        super::compute_cost(None, Some(r), &UsageTokens::default(), discount, f)
    }

    fn close(got: f64, want: f64) {
        assert!((got - want).abs() < 1e-6, "cost={} want={}", got, want);
    }

    #[test]
    fn image_io_charges_input_and_output_separately() {
        let r = rule("requests", "image_resolution_io",
            json!([{ "resolution": "2k", "input_rate": 0.1, "rate": 0.3, "enabled": true }]), json!({}));
        let f = ExtractedFeatures {
            image_count: Some(2), image_ref_count: Some(3), resolution: Some("2k".into()),
            ..Default::default()
        };
        let (c, d) = cost(&r, &f, 0.5);
        close(c, (3.0 * 0.1 + 2.0 * 0.3) * 0.5);
        assert!(d.contains("输入:3张"), "detail = {}", d);
        assert!(d.contains("生成:2张"), "detail = {}", d);
    }

    #[test]
    fn image_io_unmatched_resolution_uses_highest_tier() {
        let r = rule("requests", "image_resolution_io", json!([
            { "resolution": "1k", "input_rate": 0.05, "rate": 0.1, "enabled": true },
            { "resolution": "2k", "input_rate": 0.1, "rate": 0.3, "enabled": true },
        ]), json!({}));
        let f = ExtractedFeatures {
            image_count: Some(1), image_ref_count: Some(2), resolution: Some("8k".into()),
            ..Default::default()
        };
        let (c, d) = cost(&r, &f, 1.0);
        close(c, 2.0 * 0.1 + 1.0 * 0.3);
        assert!(d.contains("兜底最高阶梯(2k)"), "detail = {}", d);
    }

    #[test]
    fn image_io_tier_without_input_rate_defaults_to_zero() {
        let r = rule("requests", "image_resolution_io",
            json!([{ "resolution": "2k", "rate": 0.3, "enabled": true }]), json!({}));
        let f = ExtractedFeatures {
            image_count: Some(2), image_ref_count: Some(3), resolution: Some("2k".into()),
            ..Default::default()
        };
        close(cost(&r, &f, 1.0).0, 2.0 * 0.3);
    }

    #[test]
    fn video_io_splits_input_and_output_seconds_plus_extra_images() {
        let r = rule("duration", "video_seconds_io",
            json!([{ "resolution": "2k", "input_rate": 0.15, "rate": 0.8, "enabled": true }]),
            json!({ "free_image_count": 5 }));
        let f = ExtractedFeatures {
            duration_seconds: Some(10.0), video_ref_seconds: Some(4.0),
            image_ref_count: Some(7), resolution: Some("2k".into()),
            ..Default::default()
        };
        let (c, d) = cost(&r, &f, 1.0);
        close(c, 10.0 * 0.8 + 4.0 * 0.15 + 2.0 * 0.2);
        assert!(d.contains("输入:4.00秒"), "detail = {}", d);
        assert!(d.contains("输入图超额:2张(共7张,免费5张)"), "detail = {}", d);
    }

    /// 第一步不引入网络探测：上游没回输入秒时输入侧按 0 计，不得凭空收费
    #[test]
    fn video_io_without_upstream_input_seconds_charges_zero_input() {
        let r = rule("duration", "video_seconds_io",
            json!([{ "resolution": "2k", "input_rate": 0.15, "rate": 0.8, "enabled": true }]),
            json!({ "free_image_count": 5 }));
        let f = ExtractedFeatures {
            duration_seconds: Some(10.0), resolution: Some("2k".into()),
            ..Default::default()
        };
        close(cost(&r, &f, 1.0).0, 10.0 * 0.8);
    }

    #[test]
    fn video_io_images_within_free_count_are_not_billed() {
        let r = rule("duration", "video_seconds_io",
            json!([{ "resolution": "2k", "input_rate": 0.15, "rate": 0.8, "enabled": true }]),
            json!({ "free_image_count": 5 }));
        let f = ExtractedFeatures {
            duration_seconds: Some(10.0), image_ref_count: Some(3), resolution: Some("2k".into()),
            ..Default::default()
        };
        close(cost(&r, &f, 1.0).0, 10.0 * 0.8);
    }

    #[test]
    fn video_io_skips_disabled_tiers() {
        let r = rule("duration", "video_seconds_io", json!([
            { "resolution": "4k", "input_rate": 0.5, "rate": 2.0, "enabled": false },
            { "resolution": "2k", "input_rate": 0.15, "rate": 0.8, "enabled": true },
        ]), json!({}));
        let f = ExtractedFeatures {
            duration_seconds: Some(10.0), video_ref_seconds: Some(4.0), resolution: Some("2k".into()),
            ..Default::default()
        };
        close(cost(&r, &f, 1.0).0, 10.0 * 0.8 + 4.0 * 0.15);
    }

    // ---- 存量回归：新字段不得改变任何既有模式的金额 ----

    #[test]
    fn legacy_image_resolution_ignores_input_rate() {
        let r = rule("requests", "image_resolution",
            json!([{ "resolution": "2k", "input_rate": 0.1, "rate": 0.3, "enabled": true }]), json!({}));
        let f = ExtractedFeatures {
            image_count: Some(2), image_ref_count: Some(3), resolution: Some("2k".into()),
            ..Default::default()
        };
        close(cost(&r, &f, 0.5).0, 2.0 * 0.3 * 0.5);
    }

    #[test]
    fn legacy_minimax_h3_ignores_input_side() {
        let r = rule("duration", "minimax_h3",
            json!([{ "resolution": "2k", "input_rate": 0.15, "rate": 0.8, "enabled": true }]),
            json!({ "free_image_count": 5 }));
        let f = ExtractedFeatures {
            duration_seconds: Some(10.0), video_ref_seconds: Some(4.0),
            image_ref_count: Some(7), resolution: Some("2k".into()),
            ..Default::default()
        };
        close(cost(&r, &f, 1.0).0, 10.0 * 0.8 + 2.0 * 0.2);
    }

    #[test]
    fn legacy_standard_duration_ignores_video_ref_seconds() {
        let r = rule("duration", "standard", json!([]), json!({}));
        let f = ExtractedFeatures {
            duration_seconds: Some(10.0), video_ref_seconds: Some(4.0),
            ..Default::default()
        };
        close(cost(&r, &f, 1.0).0, 10.0 * 0.5);
    }

    // ---- 取数侧 ----

    #[test]
    fn video_ref_seconds_read_from_all_usage_aliases() {
        for key in ["input_video_duration", "input_seconds", "InputSeconds"] {
            let mut u = serde_json::Map::new();
            u.insert(key.to_string(), json!(4.5));
            let f = extract_request_features(&json!({ "usage": u }));
            assert_eq!(f.video_ref_seconds, Some(4.5), "key={}", key);
        }
        let f = extract_request_features(&json!({ "task": { "usage": { "input_seconds": "3.5" } } }));
        assert_eq!(f.video_ref_seconds, Some(3.5));
    }

    #[test]
    fn video_ref_seconds_rejects_non_positive_and_missing() {
        for v in [json!(0), json!(-1), json!(null), json!("abc")] {
            let f = extract_request_features(&json!({ "usage": { "input_seconds": v } }));
            assert_eq!(f.video_ref_seconds, None, "v={}", v);
        }
        assert_eq!(extract_request_features(&json!({})).video_ref_seconds, None);
    }

    #[test]
    fn settlement_usage_overrides_video_ref_seconds() {
        let mut f = ExtractedFeatures { video_ref_seconds: Some(1.0), ..Default::default() };
        f.merge(extract_request_features(&json!({ "usage": { "input_video_duration": 7.0 } })));
        assert_eq!(f.video_ref_seconds, Some(7.0));
    }
}


#[cfg(test)]
mod billing_prod_replay_tests {
    use super::*;
    use crate::relay::usage_extractor::{ExtractedFeatures, UsageTokens};
    use serde_json::Value;

    const FIXTURES: &str = r#"[{"log_id": 4377831, "model": "deepseek-v4.1-flash-1017", "billing_rule": "standard", "rule": {"id": 47, "name": "DeepSeek-V4.1-Flash计费", "billing_type": "tokens", "prompt_rate": 2, "completion_rate": 8, "cached_rate": 0.04, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":true,\"time_multipliers\":[{\"end\":\"09:00\",\"multiplier\":0.5,\"start\":\"00:00\"},{\"end\":\"14:00\",\"multiplier\":0.5,\"start\":\"12:00\"},{\"end\":\"23:59\",\"multiplier\":0.5,\"start\":\"18:00\"}],\"web_search_rate\":30}", "is_active": 1, "is_system": 0, "pid": "69969", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-15T10:08:16.106598+08:00", "updated_at": "2026-10-24T23:04:27.39509+08:00"}, "usage": {"prompt": 33773, "completion": 69, "cached": 33536, "total": 33842, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 0.5}, "discount": 0.6, "expect_cost": 0.00071}, {"log_id": 4377828, "model": "deepseek-v4.1-flash-1017", "billing_rule": "standard", "rule": {"id": 47, "name": "DeepSeek-V4.1-Flash计费", "billing_type": "tokens", "prompt_rate": 2, "completion_rate": 8, "cached_rate": 0.04, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":true,\"time_multipliers\":[{\"end\":\"09:00\",\"multiplier\":0.5,\"start\":\"00:00\"},{\"end\":\"14:00\",\"multiplier\":0.5,\"start\":\"12:00\"},{\"end\":\"23:59\",\"multiplier\":0.5,\"start\":\"18:00\"}],\"web_search_rate\":30}", "is_active": 1, "is_system": 0, "pid": "69969", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-15T10:08:16.106598+08:00", "updated_at": "2026-10-24T23:04:27.39509+08:00"}, "usage": {"prompt": 37929, "completion": 129, "cached": 36736, "total": 38058, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 0.5}, "discount": 0.6, "expect_cost": 0.001466}, {"log_id": 4377827, "model": "deepseek-v4.1-flash-1017", "billing_rule": "standard", "rule": {"id": 47, "name": "DeepSeek-V4.1-Flash计费", "billing_type": "tokens", "prompt_rate": 2, "completion_rate": 8, "cached_rate": 0.04, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":true,\"time_multipliers\":[{\"end\":\"09:00\",\"multiplier\":0.5,\"start\":\"00:00\"},{\"end\":\"14:00\",\"multiplier\":0.5,\"start\":\"12:00\"},{\"end\":\"23:59\",\"multiplier\":0.5,\"start\":\"18:00\"}],\"web_search_rate\":30}", "is_active": 1, "is_system": 0, "pid": "69969", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-15T10:08:16.106598+08:00", "updated_at": "2026-10-24T23:04:27.39509+08:00"}, "usage": {"prompt": 32628, "completion": 97, "cached": 30720, "total": 32725, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 0.5}, "discount": 0.6, "expect_cost": 0.001746}, {"log_id": 4377638, "model": "qwen3.8-max", "billing_rule": "standard", "rule": {"id": 39, "name": "qwen-3.8-max", "billing_type": "tokens", "prompt_rate": 12, "completion_rate": 36, "cached_rate": 1.5, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "67061", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-07T17:52:04.172005+08:00", "updated_at": "2026-10-13T12:37:55.069927+08:00"}, "usage": {"prompt": 151545, "completion": 626, "cached": 150528, "total": 152171, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.64, "expect_cost": 0.16674}, {"log_id": 4377586, "model": "qwen3.8-max", "billing_rule": "standard", "rule": {"id": 39, "name": "qwen-3.8-max", "billing_type": "tokens", "prompt_rate": 12, "completion_rate": 36, "cached_rate": 1.5, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "67061", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-07T17:52:04.172005+08:00", "updated_at": "2026-10-13T12:37:55.069927+08:00"}, "usage": {"prompt": 52904, "completion": 393, "cached": 51200, "total": 53297, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.64, "expect_cost": 0.071293}, {"log_id": 4377529, "model": "qwen3.8-max", "billing_rule": "standard", "rule": {"id": 39, "name": "qwen-3.8-max", "billing_type": "tokens", "prompt_rate": 12, "completion_rate": 36, "cached_rate": 1.5, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "67061", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-07T17:52:04.172005+08:00", "updated_at": "2026-10-13T12:37:55.069927+08:00"}, "usage": {"prompt": 38064, "completion": 258, "cached": 32768, "total": 38322, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.64, "expect_cost": 0.078075}, {"log_id": 4377254, "model": "glm-5-3-flashx", "billing_rule": "standard", "rule": {"id": 63, "name": "GLM-5.3-FlashX计费", "billing_type": "tokens", "prompt_rate": 2, "completion_rate": 7, "cached_rate": 0.57, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":10}", "is_active": 1, "is_system": 0, "pid": "68481", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-14T10:43:42.364939+08:00", "updated_at": "2026-10-27T15:22:42.518832+08:00"}, "usage": {"prompt": 26352, "completion": 2508, "cached": 192, "total": 28860, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.66, "expect_cost": 0.04619}, {"log_id": 4377200, "model": "glm-5-3-flashx", "billing_rule": "standard", "rule": {"id": 63, "name": "GLM-5.3-FlashX计费", "billing_type": "tokens", "prompt_rate": 2, "completion_rate": 7, "cached_rate": 0.57, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":10}", "is_active": 1, "is_system": 0, "pid": "68481", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-14T10:43:42.364939+08:00", "updated_at": "2026-10-27T15:22:42.518832+08:00"}, "usage": {"prompt": 26036, "completion": 1350, "cached": 192, "total": 27386, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.66, "expect_cost": 0.040423}, {"log_id": 4377176, "model": "glm-5-3-flashx", "billing_rule": "standard", "rule": {"id": 63, "name": "GLM-5.3-FlashX计费", "billing_type": "tokens", "prompt_rate": 2, "completion_rate": 7, "cached_rate": 0.57, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":10}", "is_active": 1, "is_system": 0, "pid": "68481", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-14T10:43:42.364939+08:00", "updated_at": "2026-10-27T15:22:42.518832+08:00"}, "usage": {"prompt": 25239, "completion": 310, "cached": 0, "total": 25549, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.66, "expect_cost": 0.034748}, {"log_id": 4376861, "model": "doubao-seedance-2-5", "billing_rule": "seedance2.0", "rule": {"id": 18, "name": "Seedance2.5官方计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "seedance2.0", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"resolution_rates\":{\"1080p\":{\"with_video\":46,\"without_video\":77},\"480p\":{\"with_video\":42,\"without_video\":70},\"720p\":{\"with_video\":42,\"without_video\":70}},\"time_multipliers\":[]}", "is_active": 1, "is_system": 1, "pid": "73119", "pricing_type": "official", "sort_order": 0, "created_at": "2026-08-10T14:58:21.722285+08:00", "updated_at": "2026-10-20T15:27:25.36732+08:00"}, "usage": {"prompt": 0, "completion": 540900, "cached": 0, "total": 540900, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": true, "has_image_ref": false, "duration_seconds": 25.0, "resolution": "720p", "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": "on", "cache_creation": null, "image_ref_count": 3, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.9, "expect_cost": 34.0767}, {"log_id": 4375848, "model": "gpt-image-2", "billing_rule": "gpt_billing", "rule": {"id": 20, "name": "gpt-image-2计费", "billing_type": "tokens", "prompt_rate": 34, "completion_rate": 204, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "gpt_billing", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"gpt_config\":{\"cached_input_image\":{\"enabled\":true,\"rate\":13.6},\"cached_input_text\":{\"enabled\":true,\"rate\":8.5},\"input_image\":{\"enabled\":true,\"rate\":54.4},\"input_text\":{\"enabled\":true,\"rate\":34},\"output_image\":{\"enabled\":true,\"rate\":204}},\"time_multipliers\":[]}", "is_active": 1, "is_system": 0, "pid": "64085", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-13T18:36:21.805233+08:00", "updated_at": "2026-10-27T15:50:09.323135+08:00"}, "usage": {"prompt": 479, "completion": 7370, "cached": 0, "total": 7849, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": "2k", "image_count": 1, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": "2560x1440", "quality": "high", "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 1.519766}, {"log_id": 4375783, "model": "gpt-image-2", "billing_rule": "gpt_billing", "rule": {"id": 20, "name": "gpt-image-2计费", "billing_type": "tokens", "prompt_rate": 34, "completion_rate": 204, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "gpt_billing", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"gpt_config\":{\"cached_input_image\":{\"enabled\":true,\"rate\":13.6},\"cached_input_text\":{\"enabled\":true,\"rate\":8.5},\"input_image\":{\"enabled\":true,\"rate\":54.4},\"input_text\":{\"enabled\":true,\"rate\":34},\"output_image\":{\"enabled\":true,\"rate\":204}},\"time_multipliers\":[]}", "is_active": 1, "is_system": 0, "pid": "64085", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-13T18:36:21.805233+08:00", "updated_at": "2026-10-27T15:50:09.323135+08:00"}, "usage": {"prompt": 479, "completion": 7370, "cached": 0, "total": 7849, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": "2k", "image_count": 1, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": "2560x1440", "quality": "high", "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 1.519766}, {"log_id": 4375643, "model": "gpt-image-2", "billing_rule": "gpt_billing", "rule": {"id": 20, "name": "gpt-image-2计费", "billing_type": "tokens", "prompt_rate": 34, "completion_rate": 204, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "gpt_billing", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"gpt_config\":{\"cached_input_image\":{\"enabled\":true,\"rate\":13.6},\"cached_input_text\":{\"enabled\":true,\"rate\":8.5},\"input_image\":{\"enabled\":true,\"rate\":54.4},\"input_text\":{\"enabled\":true,\"rate\":34},\"output_image\":{\"enabled\":true,\"rate\":204}},\"time_multipliers\":[]}", "is_active": 1, "is_system": 0, "pid": "64085", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-13T18:36:21.805233+08:00", "updated_at": "2026-10-27T15:50:09.323135+08:00"}, "usage": {"prompt": 481, "completion": 7370, "cached": 0, "total": 7851, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": "2k", "image_count": 1, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": "2560x1440", "quality": "high", "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 1.519834}, {"log_id": 4372149, "model": "doubao-seedance-2-0-fast", "billing_rule": "seedance2.0", "rule": {"id": 1, "name": "Seedance2.0Fast官方计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "seedance2.0", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"resolution_rates\":{\"480p\":{\"with_video\":22,\"without_video\":37},\"720p\":{\"with_video\":22,\"without_video\":37}},\"time_multipliers\":[]}", "is_active": 1, "is_system": 1, "pid": "73230", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-07-29T12:24:59.97958+08:00", "updated_at": "2026-10-12T17:25:02.949568+08:00"}, "usage": {"prompt": 0, "completion": 108900, "cached": 0, "total": 108900, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": 5.0, "resolution": "720p", "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": "off", "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 4.0293}, {"log_id": 4371722, "model": "doubao-seedance-2-0-fast", "billing_rule": "seedance2.0", "rule": {"id": 1, "name": "Seedance2.0Fast官方计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "seedance2.0", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"resolution_rates\":{\"480p\":{\"with_video\":22,\"without_video\":37},\"720p\":{\"with_video\":22,\"without_video\":37}},\"time_multipliers\":[]}", "is_active": 1, "is_system": 1, "pid": "73230", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-07-29T12:24:59.97958+08:00", "updated_at": "2026-10-12T17:25:02.949568+08:00"}, "usage": {"prompt": 0, "completion": 108900, "cached": 0, "total": 108900, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": 5.0, "resolution": "720p", "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": "off", "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 4.0293}, {"log_id": 4371694, "model": "gemini-3.8-flash", "billing_rule": "standard", "rule": {"id": 58, "name": "gemini-3.8-flash计费", "billing_type": "tokens", "prompt_rate": 5.085, "completion_rate": 25.425, "cached_rate": 0.5085, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":94.92}", "is_active": 1, "is_system": 0, "pid": "69505", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-08T15:25:48.976912+08:00", "updated_at": "2026-10-27T15:38:07.025938+08:00"}, "usage": {"prompt": 5, "completion": 70, "cached": 0, "total": 75, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.001805}, {"log_id": 4368987, "model": "gemini-nano-banana-2.1", "billing_rule": "standard", "rule": {"id": 23, "name": "Nano Banana 2计费", "billing_type": "tokens", "prompt_rate": 3.4, "completion_rate": 408, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"web_search_rate\":0,\"enable_time_multipliers\":false,\"time_multipliers\":[]}", "is_active": 1, "is_system": 0, "pid": "68778", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-14T15:31:25.885494+08:00", "updated_at": "2026-08-14T15:31:25.885494+08:00"}, "usage": {"prompt": 7, "completion": 1120, "cached": 0, "total": 1127, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": "1k", "image_count": 1, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.456984}, {"log_id": 4368671, "model": "doubao-seedance-2-0-fast", "billing_rule": "seedance2.0", "rule": {"id": 1, "name": "Seedance2.0Fast官方计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "seedance2.0", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"resolution_rates\":{\"480p\":{\"with_video\":22,\"without_video\":37},\"720p\":{\"with_video\":22,\"without_video\":37}},\"time_multipliers\":[]}", "is_active": 1, "is_system": 1, "pid": "73230", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-07-29T12:24:59.97958+08:00", "updated_at": "2026-10-12T17:25:02.949568+08:00"}, "usage": {"prompt": 0, "completion": 108900, "cached": 0, "total": 108900, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": true, "has_image_ref": false, "duration_seconds": 5.0, "resolution": "720p", "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": "on", "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 4.0293}, {"log_id": 4368618, "model": "gemini-3.8-flash", "billing_rule": "standard", "rule": {"id": 58, "name": "gemini-3.8-flash计费", "billing_type": "tokens", "prompt_rate": 5.085, "completion_rate": 25.425, "cached_rate": 0.5085, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":94.92}", "is_active": 1, "is_system": 0, "pid": "69505", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-08T15:25:48.976912+08:00", "updated_at": "2026-10-27T15:38:07.025938+08:00"}, "usage": {"prompt": 2, "completion": 7, "cached": 0, "total": 9, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.000188}, {"log_id": 4367874, "model": "gpt-6.1-sol", "billing_rule": "tiered", "rule": {"id": 66, "name": "GPT-6.1 Sol计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":16.95,\"cached_rate\":0.68,\"completion_rate\":67.8,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":13.56},{\"cache_write_rate\":33.9,\"cached_rate\":1.36,\"completion_rate\":101.7,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":27.12}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "69199", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-19T15:57:53.479241+08:00", "updated_at": "2026-10-27T15:49:26.109252+08:00"}, "usage": {"prompt": 4232, "completion": 110, "cached": 4096, "total": 4342, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.45, "expect_cost": 0.005439}, {"log_id": 4367783, "model": "gpt-6-sol", "billing_rule": "tiered", "rule": {"id": 66, "name": "GPT-6.1 Sol计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":16.95,\"cached_rate\":0.68,\"completion_rate\":67.8,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":13.56},{\"cache_write_rate\":33.9,\"cached_rate\":1.36,\"completion_rate\":101.7,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":27.12}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "69199", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-19T15:57:53.479241+08:00", "updated_at": "2026-10-27T15:49:26.109252+08:00"}, "usage": {"prompt": 4396, "completion": 93, "cached": 4224, "total": 4489, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.45, "expect_cost": 0.00518}, {"log_id": 4367613, "model": "gpt-6-astra", "billing_rule": "tiered", "rule": {"id": 66, "name": "GPT-6.1 Sol计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":16.95,\"cached_rate\":0.68,\"completion_rate\":67.8,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":13.56},{\"cache_write_rate\":33.9,\"cached_rate\":1.36,\"completion_rate\":101.7,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":27.12}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "69199", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-19T15:57:53.479241+08:00", "updated_at": "2026-10-27T15:49:26.109252+08:00"}, "usage": {"prompt": 4130, "completion": 124, "cached": 3968, "total": 4254, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.45, "expect_cost": 0.005986}, {"log_id": 4367472, "model": "gpt-5.6-terra", "billing_rule": "tiered", "rule": {"id": 32, "name": "gpt-5.6-terra计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0,\"cached_rate\":1.356,\"completion_rate\":81.36,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":13.56},{\"cache_write_rate\":0,\"cached_rate\":2.712,\"completion_rate\":122.04,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":27.12}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "63257", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T21:22:29.894602+08:00", "updated_at": "2026-10-27T15:49:46.870563+08:00"}, "usage": {"prompt": 16, "completion": 104, "cached": 0, "total": 120, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.45, "expect_cost": 0.003905}, {"log_id": 4367299, "model": "gpt-5.6-sol", "billing_rule": "tiered", "rule": {"id": 33, "name": "gpt-5.6-sol计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0,\"cached_rate\":2.712,\"completion_rate\":135.6,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":27.12},{\"cache_write_rate\":0,\"cached_rate\":5.424,\"completion_rate\":203.4,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":54.24}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "63667", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T21:22:51.496406+08:00", "updated_at": "2026-10-27T15:49:35.366166+08:00"}, "usage": {"prompt": 4396, "completion": 119, "cached": 4224, "total": 4515, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.45, "expect_cost": 0.014515}, {"log_id": 4367180, "model": "gpt-5.6-luna", "billing_rule": "tiered", "rule": {"id": 31, "name": "gpt-5.6-luna计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0,\"cached_rate\":0.1356,\"completion_rate\":8.136,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":1.356},{\"cache_write_rate\":0,\"cached_rate\":0.2712,\"completion_rate\":12.204,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":2.712}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "68712", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T21:21:56.377109+08:00", "updated_at": "2026-10-27T15:48:32.12566+08:00"}, "usage": {"prompt": 4396, "completion": 121, "cached": 4224, "total": 4517, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.45, "expect_cost": 0.000806}, {"log_id": 4367106, "model": "claude-sonnet-4-6", "billing_rule": "standard", "rule": {"id": 17, "name": "claude-sonnet-4-6计费", "billing_type": "tokens", "prompt_rate": 20.34, "completion_rate": 101.7, "cached_rate": 0, "claude_cache_creation_rate": 2.034, "claude_cache_read_rate": 25.425, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"web_search_rate\":0,\"enable_time_multipliers\":false,\"time_multipliers\":[]}", "is_active": 1, "is_system": 0, "pid": "67328", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-06T11:52:19.357542+08:00", "updated_at": "2026-08-06T11:52:19.357542+08:00"}, "usage": {"prompt": 487, "completion": 159, "cached": 0, "total": 646, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.026076}, {"log_id": 4367007, "model": "claude-opus-5", "billing_rule": "standard", "rule": {"id": 37, "name": "claude-opus-5计费", "billing_type": "tokens", "prompt_rate": 33.9, "completion_rate": 169.5, "cached_rate": 0, "claude_cache_creation_rate": 42.375, "claude_cache_read_rate": 3.39, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "63949", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-02T10:44:37.296293+08:00", "updated_at": "2026-09-07T14:52:15.216113+08:00"}, "usage": {"prompt": 25, "completion": 242, "cached": 60, "total": 267, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.04207}, {"log_id": 4366941, "model": "claude-opus-4-8", "billing_rule": "standard", "rule": {"id": 27, "name": "claude-opus计费", "billing_type": "tokens", "prompt_rate": 33.9, "completion_rate": 169.5, "cached_rate": 0, "claude_cache_creation_rate": 42.375, "claude_cache_read_rate": 3.39, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "68535", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T15:30:18.359718+08:00", "updated_at": "2026-09-07T14:53:29.01716+08:00"}, "usage": {"prompt": 25, "completion": 201, "cached": 60, "total": 226, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": 3, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.035248}, {"log_id": 4366821, "model": "claude-opus-4-6", "billing_rule": "standard", "rule": {"id": 27, "name": "claude-opus计费", "billing_type": "tokens", "prompt_rate": 33.9, "completion_rate": 169.5, "cached_rate": 0, "claude_cache_creation_rate": 42.375, "claude_cache_read_rate": 3.39, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "68535", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T15:30:18.359718+08:00", "updated_at": "2026-09-07T14:53:29.01716+08:00"}, "usage": {"prompt": 25, "completion": 229, "cached": 60, "total": 254, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": 3, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.039994}, {"log_id": 4366710, "model": "claude-opus-4-5", "billing_rule": "standard", "rule": {"id": 37, "name": "claude-opus-5计费", "billing_type": "tokens", "prompt_rate": 33.9, "completion_rate": 169.5, "cached_rate": 0, "claude_cache_creation_rate": 42.375, "claude_cache_read_rate": 3.39, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "63949", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-02T10:44:37.296293+08:00", "updated_at": "2026-09-07T14:52:15.216113+08:00"}, "usage": {"prompt": 25, "completion": 200, "cached": 60, "total": 225, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": 3, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.035078}, {"log_id": 4366617, "model": "claude-haiku-4-5", "billing_rule": "tiered", "rule": {"id": 70, "name": "Claude Haiku 4.5计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0.125,\"cached_rate\":0.01,\"completion_rate\":3.39,\"enabled\":true,\"max_completion_tokens\":100,\"max_prompt_tokens\":100,\"prompt_rate\":0.678},{\"cache_write_rate\":0.625,\"cached_rate\":0.05,\"completion_rate\":16.95,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":3.39}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "64509", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-23T15:07:47.997183+08:00", "updated_at": "2026-10-27T14:49:18.380703+08:00"}, "usage": {"prompt": 2525, "completion": 142, "cached": 0, "total": 2667, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.002193}, {"log_id": 4366538, "model": "gemini-3.8-flash", "billing_rule": "standard", "rule": {"id": 58, "name": "gemini-3.8-flash计费", "billing_type": "tokens", "prompt_rate": 5.085, "completion_rate": 25.425, "cached_rate": 0.5085, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":94.92}", "is_active": 1, "is_system": 0, "pid": "69505", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-08T15:25:48.976912+08:00", "updated_at": "2026-10-27T15:38:07.025938+08:00"}, "usage": {"prompt": 14, "completion": 1966, "cached": 0, "total": 1980, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.050057}, {"log_id": 4366517, "model": "claude-fable-5-1", "billing_rule": "standard", "rule": {"id": 61, "name": "Claude Fable 5.1计费", "billing_type": "tokens", "prompt_rate": 67.8, "completion_rate": 339, "cached_rate": 0, "claude_cache_creation_rate": 84.75, "claude_cache_read_rate": 1.695, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "69062", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-13T09:49:37.624474+08:00", "updated_at": "2026-10-13T09:49:48.237771+08:00"}, "usage": {"prompt": 4, "completion": 200, "cached": 645, "total": 204, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.65, "expect_cost": 0.044957}, {"log_id": 4366403, "model": "claude-fable-5", "billing_rule": "standard", "rule": {"id": 38, "name": "claude-fable-5计费", "billing_type": "tokens", "prompt_rate": 67.8, "completion_rate": 339, "cached_rate": 0, "claude_cache_creation_rate": 84.75, "claude_cache_read_rate": 6.78, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "64011", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-02T10:45:16.883777+08:00", "updated_at": "2026-09-07T14:51:56.221144+08:00"}, "usage": {"prompt": 2, "completion": 191, "cached": 645, "total": 193, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.069258}, {"log_id": 4365913, "model": "claude-sonnet-5", "billing_rule": "standard", "rule": {"id": 36, "name": "claude-sonnet-5计费", "billing_type": "tokens", "prompt_rate": 13.56, "completion_rate": 67.8, "cached_rate": 0, "claude_cache_creation_rate": 16.95, "claude_cache_read_rate": 1.356, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "64645", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-02T10:43:55.258086+08:00", "updated_at": "2026-09-07T14:52:35.486751+08:00"}, "usage": {"prompt": 25, "completion": 453, "cached": 481, "total": 478, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.031705}, {"log_id": 4365490, "model": "claude-sonnet-5", "billing_rule": "standard", "rule": {"id": 36, "name": "claude-sonnet-5计费", "billing_type": "tokens", "prompt_rate": 13.56, "completion_rate": 67.8, "cached_rate": 0, "claude_cache_creation_rate": 16.95, "claude_cache_read_rate": 1.356, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "64645", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-02T10:43:55.258086+08:00", "updated_at": "2026-09-07T14:52:35.486751+08:00"}, "usage": {"prompt": 25, "completion": 520, "cached": 0, "total": 545, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.035595}, {"log_id": 4365299, "model": "claude-opus-5-5", "billing_rule": "standard", "rule": {"id": 62, "name": "Claude Opus 5.5计费", "billing_type": "tokens", "prompt_rate": 27.12, "completion_rate": 135.6, "cached_rate": 0, "claude_cache_creation_rate": 33.9, "claude_cache_read_rate": 1.36, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "69135", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-13T09:51:08.642277+08:00", "updated_at": "2026-10-13T09:51:08.642277+08:00"}, "usage": {"prompt": 4, "completion": 200, "cached": 662, "total": 204, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.65, "expect_cost": 0.018284}, {"log_id": 4365229, "model": "claude-opus-5-5", "billing_rule": "standard", "rule": {"id": 62, "name": "Claude Opus 5.5计费", "billing_type": "tokens", "prompt_rate": 27.12, "completion_rate": 135.6, "cached_rate": 0, "claude_cache_creation_rate": 33.9, "claude_cache_read_rate": 1.36, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "69135", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-13T09:51:08.642277+08:00", "updated_at": "2026-10-13T09:51:08.642277+08:00"}, "usage": {"prompt": 4, "completion": 200, "cached": 662, "total": 204, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.65, "expect_cost": 0.018284}, {"log_id": 4364849, "model": "claude-opus-5-5", "billing_rule": "standard", "rule": {"id": 62, "name": "Claude Opus 5.5计费", "billing_type": "tokens", "prompt_rate": 27.12, "completion_rate": 135.6, "cached_rate": 0, "claude_cache_creation_rate": 33.9, "claude_cache_read_rate": 1.36, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "69135", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-13T09:51:08.642277+08:00", "updated_at": "2026-10-13T09:51:08.642277+08:00"}, "usage": {"prompt": 4, "completion": 198, "cached": 662, "total": 202, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.65, "expect_cost": 0.018107}, {"log_id": 4364799, "model": "claude-sonnet-5", "billing_rule": "standard", "rule": {"id": 36, "name": "claude-sonnet-5计费", "billing_type": "tokens", "prompt_rate": 13.56, "completion_rate": 67.8, "cached_rate": 0, "claude_cache_creation_rate": 16.95, "claude_cache_read_rate": 1.356, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "64645", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-02T10:43:55.258086+08:00", "updated_at": "2026-09-07T14:52:35.486751+08:00"}, "usage": {"prompt": 37, "completion": 378, "cached": 469, "total": 415, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.026766}, {"log_id": 4364598, "model": "GLM-5.3", "billing_rule": "standard", "rule": {"id": 26, "name": "GLM-5.3计费", "billing_type": "tokens", "prompt_rate": 8, "completion_rate": 28, "cached_rate": 2, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":10}", "is_active": 1, "is_system": 0, "pid": "61816", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T15:19:10.645671+08:00", "updated_at": "2026-10-27T15:20:32.078582+08:00"}, "usage": {"prompt": 330951, "completion": 467, "cached": 330816, "total": 331418, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.44, "expect_cost": 0.297347}, {"log_id": 4364585, "model": "GLM-5.3", "billing_rule": "standard", "rule": {"id": 26, "name": "GLM-5.3计费", "billing_type": "tokens", "prompt_rate": 8, "completion_rate": 28, "cached_rate": 2, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":10}", "is_active": 1, "is_system": 0, "pid": "61816", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T15:19:10.645671+08:00", "updated_at": "2026-10-27T15:20:32.078582+08:00"}, "usage": {"prompt": 330857, "completion": 67, "cached": 330752, "total": 330924, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.44, "expect_cost": 0.292257}, {"log_id": 4364571, "model": "GLM-5.3", "billing_rule": "standard", "rule": {"id": 26, "name": "GLM-5.3计费", "billing_type": "tokens", "prompt_rate": 8, "completion_rate": 28, "cached_rate": 2, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":10}", "is_active": 1, "is_system": 0, "pid": "61816", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T15:19:10.645671+08:00", "updated_at": "2026-10-27T15:20:32.078582+08:00"}, "usage": {"prompt": 330778, "completion": 49, "cached": 330624, "total": 330827, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.44, "expect_cost": 0.292095}, {"log_id": 4363242, "model": "gpt-5.6-terra", "billing_rule": "tiered", "rule": {"id": 32, "name": "gpt-5.6-terra计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0,\"cached_rate\":1.356,\"completion_rate\":81.36,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":13.56},{\"cache_write_rate\":0,\"cached_rate\":2.712,\"completion_rate\":122.04,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":27.12}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "63257", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T21:22:29.894602+08:00", "updated_at": "2026-10-27T15:49:46.870563+08:00"}, "usage": {"prompt": 7, "completion": 13, "cached": 0, "total": 20, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.45, "expect_cost": 0.000519}, {"log_id": 4363239, "model": "gpt-5.6-sol", "billing_rule": "tiered", "rule": {"id": 33, "name": "gpt-5.6-sol计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0,\"cached_rate\":2.712,\"completion_rate\":135.6,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":27.12},{\"cache_write_rate\":0,\"cached_rate\":5.424,\"completion_rate\":203.4,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":54.24}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "63667", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T21:22:51.496406+08:00", "updated_at": "2026-10-27T15:49:35.366166+08:00"}, "usage": {"prompt": 4387, "completion": 12, "cached": 4224, "total": 4399, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.45, "expect_cost": 0.007876}, {"log_id": 4363236, "model": "gpt-5.6-luna", "billing_rule": "tiered", "rule": {"id": 31, "name": "gpt-5.6-luna计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0,\"cached_rate\":0.1356,\"completion_rate\":8.136,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":1.356},{\"cache_write_rate\":0,\"cached_rate\":0.2712,\"completion_rate\":12.204,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":2.712}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "68712", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T21:21:56.377109+08:00", "updated_at": "2026-10-27T15:48:32.12566+08:00"}, "usage": {"prompt": 4387, "completion": 14, "cached": 4224, "total": 4401, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.45, "expect_cost": 0.000408}, {"log_id": 4363228, "model": "claude-sonnet-4-6", "billing_rule": "standard", "rule": {"id": 17, "name": "claude-sonnet-4-6计费", "billing_type": "tokens", "prompt_rate": 20.34, "completion_rate": 101.7, "cached_rate": 0, "claude_cache_creation_rate": 2.034, "claude_cache_read_rate": 25.425, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"web_search_rate\":0,\"enable_time_multipliers\":false,\"time_multipliers\":[]}", "is_active": 1, "is_system": 0, "pid": "67328", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-06T11:52:19.357542+08:00", "updated_at": "2026-08-06T11:52:19.357542+08:00"}, "usage": {"prompt": 12504, "completion": 138, "cached": 15197, "total": 12642, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": 14982, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.685223}, {"log_id": 4363225, "model": "claude-opus-5", "billing_rule": "standard", "rule": {"id": 37, "name": "claude-opus-5计费", "billing_type": "tokens", "prompt_rate": 33.9, "completion_rate": 169.5, "cached_rate": 0, "claude_cache_creation_rate": 42.375, "claude_cache_read_rate": 3.39, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "63949", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-02T10:44:37.296293+08:00", "updated_at": "2026-09-07T14:52:15.216113+08:00"}, "usage": {"prompt": 33, "completion": 1, "cached": 0, "total": 34, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.001288}, {"log_id": 4363222, "model": "claude-opus-4-8", "billing_rule": "standard", "rule": {"id": 27, "name": "claude-opus计费", "billing_type": "tokens", "prompt_rate": 33.9, "completion_rate": 169.5, "cached_rate": 0, "claude_cache_creation_rate": 42.375, "claude_cache_read_rate": 3.39, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "68535", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T15:30:18.359718+08:00", "updated_at": "2026-09-07T14:53:29.01716+08:00"}, "usage": {"prompt": 1351, "completion": 106, "cached": 9854, "total": 1457, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.097171}, {"log_id": 4363216, "model": "claude-haiku-4-5", "billing_rule": "tiered", "rule": {"id": 70, "name": "Claude Haiku 4.5计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0.125,\"cached_rate\":0.01,\"completion_rate\":3.39,\"enabled\":true,\"max_completion_tokens\":100,\"max_prompt_tokens\":100,\"prompt_rate\":0.678},{\"cache_write_rate\":0.625,\"cached_rate\":0.05,\"completion_rate\":16.95,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":3.39}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "64509", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-23T15:07:47.997183+08:00", "updated_at": "2026-10-27T14:49:18.380703+08:00"}, "usage": {"prompt": 22, "completion": 1, "cached": 0, "total": 23, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 1.8e-05}, {"log_id": 4363215, "model": "claude-fable-5", "billing_rule": "standard", "rule": {"id": 38, "name": "claude-fable-5计费", "billing_type": "tokens", "prompt_rate": 67.8, "completion_rate": 339, "cached_rate": 0, "claude_cache_creation_rate": 84.75, "claude_cache_read_rate": 6.78, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":0}", "is_active": 1, "is_system": 0, "pid": "64011", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-02T10:45:16.883777+08:00", "updated_at": "2026-09-07T14:51:56.221144+08:00"}, "usage": {"prompt": 33, "completion": 1, "cached": 0, "total": 34, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.002576}, {"log_id": 4363213, "model": "claude-fable-5-1", "billing_rule": "standard", "rule": {"id": 61, "name": "Claude Fable 5.1计费", "billing_type": "tokens", "prompt_rate": 67.8, "completion_rate": 339, "cached_rate": 0, "claude_cache_creation_rate": 84.75, "claude_cache_read_rate": 1.695, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "69062", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-13T09:49:37.624474+08:00", "updated_at": "2026-10-13T09:49:48.237771+08:00"}, "usage": {"prompt": 35, "completion": 1, "cached": 0, "total": 36, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.65, "expect_cost": 0.001763}, {"log_id": 4359590, "model": "glm-5-3-flash", "billing_rule": "standard", "rule": {"id": 52, "name": "GLM-5.3-Flash计费", "billing_type": "tokens", "prompt_rate": 0.8, "completion_rate": 2.8, "cached_rate": 0.23, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":10}", "is_active": 1, "is_system": 0, "pid": "64115", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-09-22T14:40:51.983891+08:00", "updated_at": "2026-10-27T15:22:33.417224+08:00"}, "usage": {"prompt": 156092, "completion": 277, "cached": 0, "total": 156369, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 0.66, "expect_cost": 0.082928}, {"log_id": 4359552, "model": "gpt-5.6-sol", "billing_rule": "tiered", "rule": {"id": 33, "name": "gpt-5.6-sol计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0,\"cached_rate\":2.712,\"completion_rate\":135.6,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":27.12},{\"cache_write_rate\":0,\"cached_rate\":5.424,\"completion_rate\":203.4,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":54.24}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "63667", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T21:22:51.496406+08:00", "updated_at": "2026-10-27T15:49:35.366166+08:00"}, "usage": {"prompt": 4389, "completion": 5, "cached": 4224, "total": 4394, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.016608}, {"log_id": 4359550, "model": "gpt-5.6-terra", "billing_rule": "tiered", "rule": {"id": 32, "name": "gpt-5.6-terra计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0,\"cached_rate\":1.356,\"completion_rate\":81.36,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":13.56},{\"cache_write_rate\":0,\"cached_rate\":2.712,\"completion_rate\":122.04,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":27.12}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "63257", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T21:22:29.894602+08:00", "updated_at": "2026-10-27T15:49:46.870563+08:00"}, "usage": {"prompt": 9, "completion": 5, "cached": 0, "total": 14, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.000529}, {"log_id": 4359546, "model": "gpt-5.6-luna", "billing_rule": "tiered", "rule": {"id": 31, "name": "gpt-5.6-luna计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0,\"cached_rate\":0.1356,\"completion_rate\":8.136,\"enabled\":true,\"max_completion_tokens\":272,\"max_prompt_tokens\":272,\"prompt_rate\":1.356},{\"cache_write_rate\":0,\"cached_rate\":0.2712,\"completion_rate\":12.204,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":2.712}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "68712", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-17T21:21:56.377109+08:00", "updated_at": "2026-10-27T15:48:32.12566+08:00"}, "usage": {"prompt": 4389, "completion": 5, "cached": 4224, "total": 4394, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.000837}, {"log_id": 4359542, "model": "claude-sonnet-4-6", "billing_rule": "standard", "rule": {"id": 17, "name": "claude-sonnet-4-6计费", "billing_type": "tokens", "prompt_rate": 20.34, "completion_rate": 101.7, "cached_rate": 0, "claude_cache_creation_rate": 2.034, "claude_cache_read_rate": 25.425, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "standard", "pricing_tiers": "[]", "extended_config": "{\"web_search_rate\":0,\"enable_time_multipliers\":false,\"time_multipliers\":[]}", "is_active": 1, "is_system": 0, "pid": "67328", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-08-06T11:52:19.357542+08:00", "updated_at": "2026-08-06T11:52:19.357542+08:00"}, "usage": {"prompt": 11, "completion": 2, "cached": 60, "total": 13, "has_total_tokens": true}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": 5, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 0.001963}, {"log_id": 4359532, "model": "claude-haiku-4-5", "billing_rule": "tiered", "rule": {"id": 70, "name": "Claude Haiku 4.5计费", "billing_type": "tokens", "prompt_rate": 0, "completion_rate": 0, "cached_rate": 0, "claude_cache_creation_rate": 0, "claude_cache_read_rate": 0, "fixed_rate": 0, "duration_rate": 0, "billing_rule": "tiered", "pricing_tiers": "[{\"cache_write_rate\":0.125,\"cached_rate\":0.01,\"completion_rate\":3.39,\"enabled\":true,\"max_completion_tokens\":100,\"max_prompt_tokens\":100,\"prompt_rate\":0.678},{\"cache_write_rate\":0.625,\"cached_rate\":0.05,\"completion_rate\":16.95,\"enabled\":true,\"max_completion_tokens\":1024,\"max_prompt_tokens\":1024,\"prompt_rate\":3.39}]", "extended_config": "{\"enable_time_multipliers\":false,\"time_multipliers\":[],\"web_search_rate\":67.8}", "is_active": 1, "is_system": 0, "pid": "64509", "pricing_type": "custom", "sort_order": 0, "created_at": "2026-10-23T15:07:47.997183+08:00", "updated_at": "2026-10-27T14:49:18.380703+08:00"}, "usage": {"prompt": 22, "completion": 1, "cached": 0, "total": 23, "has_total_tokens": true, "image_tokens": 0}, "features": {"has_video": false, "has_audio": false, "has_image_ref": false, "duration_seconds": null, "resolution": null, "image_count": null, "service_tier": null, "prompt_extend": false, "mode": null, "sound": null, "cache_creation": null, "image_ref_count": null, "size": null, "quality": null, "text_characters": null, "fps": null, "version": null, "web_search": null, "time_multiplier": 1.0}, "discount": 1.0, "expect_cost": 1.8e-05}]"#;

    /// 生产真实账单回放：把线上 billing_rules + 日志用量/特征/折扣喂给新 compute_cost，
    /// 断言金额与线上已收 cost 逐分一致（验证存量计费不被本次迁移改变）。
    #[test]
    fn production_bills_replay_identical() {
        let fixtures: Vec<Value> = serde_json::from_str(FIXTURES).expect("fixture 解析失败");
        let mut ok = 0usize;
        let mut fails: Vec<String> = Vec::new();
        for f in &fixtures {
            let rule: crate::models::BillingRule =
                serde_json::from_value(f["rule"].clone()).expect("rule 反序列化失败");
            let u = &f["usage"];
            let usage = UsageTokens {
                prompt: u["prompt"].as_i64().unwrap_or(0) as i32,
                completion: u["completion"].as_i64().unwrap_or(0) as i32,
                cached: u["cached"].as_i64().unwrap_or(0) as i32,
                total: u["total"].as_i64().unwrap_or(0) as i32,
                has_total_tokens: u["has_total_tokens"].as_bool().unwrap_or(false),
                image_tokens: u["image_tokens"].as_i64().unwrap_or(0) as i32,
                ..Default::default()
            };
            let features: ExtractedFeatures =
                serde_json::from_value(f["features"].clone()).unwrap_or_default();
            let discount = f["discount"].as_f64().unwrap_or(1.0);
            let expect = f["expect_cost"].as_f64().unwrap_or(0.0);
            let (cost, detail) = compute_cost(None, Some(&rule), &usage, discount, &features);
            if (cost - expect).abs() < 1e-6 {
                ok += 1;
            } else {
                fails.push(format!(
                    "log_id={} model={} rule={} expect={} got={} detail={}",
                    f["log_id"], f["model"], f["billing_rule"], expect, cost, detail
                ));
            }
        }
        eprintln!("生产账单回放: {} 条一致, {} 条不一致", ok, fails.len());
        assert!(fails.is_empty(), "存量金额发生变化:\n{}", fails.join("\n"));
    }
}
