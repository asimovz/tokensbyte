/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtractedFeatures {
    pub has_video: bool,
    pub has_audio: bool,
    /// 请求是否包含参考图（用于区分文生图/图生图计费，如可灵）
    pub has_image_ref: bool,
    pub duration_seconds: Option<f64>,
    pub resolution: Option<String>,
    /// 图片数量（用于按张计费）：请求阶段取 n，响应阶段取实际返回数量
    pub image_count: Option<i32>,
    /// 服务等级（用于离线推理等特定计费，如 flex）
    pub service_tier: Option<String>,
    /// 提示词扩写（DashScope 等图片模型，可能影响计费）
    pub prompt_extend: bool,
    /// 可灵视频生成模式（std/pro/4k），影响计费倍率，默认 std
    pub mode: Option<String>,
    /// 可灵视频有声/无声（on/off），影响计费倍率，默认 off
    pub sound: Option<String>,
    /// Claude 缓存创建 Token 数（来自 usage 提取，合并 5m+1h）
    pub cache_creation: Option<i32>,
    /// 参考图数量（用于腾讯云 Vidu 图片计费区分 ref_1_3 / ref_4_7）
    pub image_ref_count: Option<i32>,
    /// 原始 size 参数（如 "1024x1024"），用于按分辨率像素计费
    pub size: Option<String>,
    /// 画质等级（如 "low"、"medium"、"high"），用于画质倍率计费
    pub quality: Option<String>,
    /// 文本字符数（语音合成按万字符计费）
    pub text_characters: Option<i32>,
    /// 视频帧率（用于画质计费等按分辨率+帧率计费场景）
    pub fps: Option<f64>,
    /// 级联画质增强计费档位（fast|standard，由级联模型 Id 推导），与可灵 mode 解耦
    pub version: Option<String>,
    /// 联网搜索次数（Responses API 联网搜索计费）
    pub web_search: Option<i32>,
    /// 峰谷时段倍率快照（请求开始/冻结时锁定；异步结算优先使用，避免跨时段变价）
    pub time_multiplier: Option<f64>,
    /// 输入/参考视频总时长（秒），取自上游 usage，仅 video_seconds_io 消费
    pub video_ref_seconds: Option<f64>,
}

/// content[].type 是否包含指定关键字
#[inline]
fn type_contains(item: &Value, needle: &str) -> bool {
    item.get("type")
        .and_then(|v| v.as_str())
        .is_some_and(|t| t.contains(needle))
}

/// 腾讯云 FileInfos Usage：首帧/末帧/参考图
#[inline]
fn is_tencent_ref_usage(usage: &str) -> bool {
    usage.eq_ignore_ascii_case("FirstFrame")
        || usage.eq_ignore_ascii_case("LastFrame")
        || usage.eq_ignore_ascii_case("Reference")
}

#[inline]
fn nonempty_str_field(obj: &Value, key: &str) -> bool {
    obj.get(key)
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty())
}

#[inline]
fn nonempty_array_field(obj: &Value, key: &str) -> bool {
    obj.get(key)
        .and_then(|v| v.as_array())
        .is_some_and(|a| !a.is_empty())
}

/// 媒体字段是否有有效载荷（非空字符串 / 对象 / 非空数组）
#[inline]
fn media_payload_present(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty())
        || v.is_object()
        || v.as_array().is_some_and(|a| !a.is_empty())
}

#[inline]
fn field_media_present(body: &Value, key: &str) -> bool {
    body.get(key).is_some_and(media_payload_present)
}

fn set_str_if_none(dst: &mut Option<String>, val: Option<&str>) {
    if dst.is_none() {
        if let Some(s) = val.filter(|s| !s.is_empty()) {
            *dst = Some(s.to_string());
        }
    }
}

#[inline]
fn set_res_field(dst: &mut Option<String>, node: Option<&Value>, key: &str) {
    set_str_if_none(dst, node.and_then(|n| n.get(key)).and_then(|r| r.as_str()));
}

fn set_f64_if_none(dst: &mut Option<f64>, val: Option<f64>) {
    if dst.is_none() {
        if let Some(v) = val {
            *dst = Some(v);
        }
    }
}

fn set_opt_if_none<T>(dst: &mut Option<T>, src: Option<T>) {
    if dst.is_none() {
        *dst = src;
    }
}

fn overwrite_if_some<T>(dst: &mut Option<T>, src: Option<T>) {
    if src.is_some() {
        *dst = src;
    }
}

/// SSE 行 → JSON 载荷；空行 / [DONE] 返回 None
#[inline]
fn sse_json_payload(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.is_empty() || line.ends_with("[DONE]") {
        return None;
    }
    Some(if let Some(rest) = line.strip_prefix("data: ") {
        rest
    } else if let Some(rest) = line.strip_prefix("data:") {
        rest
    } else {
        line
    })
}

#[inline]
fn json_i32(v: &Value) -> i32 {
    v.as_i64().unwrap_or(0) as i32
}

#[inline]
fn json_field_i32(obj: &Value, key: &str) -> i32 {
    obj.get(key).map(json_i32).unwrap_or(0)
}

/// on/off：true→on，false→off
#[inline]
fn on_off(enabled: bool) -> String {
    if enabled { "on" } else { "off" }.to_string()
}

/// 扫描 content 数组：可分别开关 video/audio（Responses input 仅检 audio）
fn scan_typed_content(
    arr: Option<&Vec<Value>>,
    video: bool,
    audio: bool,
    has_video: &mut bool,
    has_audio: &mut bool,
) {
    if (!video || *has_video) && (!audio || *has_audio) {
        return;
    }
    let Some(arr) = arr else { return };
    for item in arr {
        if video && !*has_video && type_contains(item, "video") {
            *has_video = true;
        }
        if audio && !*has_audio && type_contains(item, "audio") {
            *has_audio = true;
        }
        if (!video || *has_video) && (!audio || *has_audio) {
            break;
        }
    }
}

/// 可灵 3.0 contents：一次扫描视频参考 + 输入图条目
fn scan_kling_contents(arr: &[Value]) -> (bool, i32) {
    let mut has_video = false;
    let mut image_n = 0i32;
    for c in arr {
        match c.get("type").and_then(|t| t.as_str()) {
            Some("base_video" | "feature_video") => has_video = true,
            Some("first_frame" | "last_frame" | "refer_image") => image_n += 1,
            _ => {}
        }
    }
    (has_video, image_n)
}

/// 腾讯云 FileInfos：参考图数量 + 是否含视频参考；LastFrameUrl 计入参考图
fn scan_tencent_file_infos(body: &Value) -> (i32, bool) {
    let mut refs = 0i32;
    let mut has_video = false;
    if let Some(arr) = body.get("FileInfos").and_then(|v| v.as_array()) {
        for item in arr {
            let usage = item.get("Usage").and_then(|u| u.as_str()).unwrap_or("");
            if is_tencent_ref_usage(usage) {
                refs += 1;
            }
            if item.get("Category").and_then(|c| c.as_str()) == Some("Video")
                && usage.eq_ignore_ascii_case("Reference")
            {
                has_video = true;
            }
        }
    }
    if nonempty_str_field(body, "LastFrameUrl") {
        refs += 1;
    }
    (refs, has_video)
}

fn count_image_field(body: &Value) -> i32 {
    body.get("image")
        .map(|v| {
            if v.as_str().filter(|s| !s.is_empty()).is_some() {
                1
            } else {
                v.as_array().map(|a| a.len() as i32).unwrap_or(0)
            }
        })
        .unwrap_or(0)
}

fn count_image_list_fields(body: &Value) -> i32 {
    body.get("images")
        .or(body.get("image_urls"))
        .or(body.get("image_list"))
        .and_then(|v| v.as_array())
        .map(|a| a.len() as i32)
        .unwrap_or(0)
}

pub fn extract_request_features(body: &Value) -> ExtractedFeatures {
    let mut has_video = false;
    let mut has_audio = false;
    let mut duration_seconds = None;
    let mut video_ref_seconds = None;

    // service_tier：根 / parameters；腾讯云 OffPeak=Enabled → flex
    let service_tier = body
        .get("service_tier")
        .or_else(|| body.get("parameters").and_then(|p| p.get("service_tier")))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .or_else(|| {
            body.get("OutputConfig")
                .and_then(|oc| oc.get("OffPeak"))
                .and_then(|v| v.as_str())
                .filter(|s| s.eq_ignore_ascii_case("Enabled"))
                .map(|_| "flex".to_string())
        });

    has_audio |= body
        .get("modalities")
        .and_then(|m| m.as_array())
        .is_some_and(|mods| mods.iter().any(|m| m.as_str() == Some("audio")));
    has_audio |= body.get("generate_audio").and_then(|v| v.as_bool()) == Some(true);

    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            scan_typed_content(
                msg.get("content").and_then(|c| c.as_array()),
                true,
                true,
                &mut has_video,
                &mut has_audio,
            );
            if has_video && has_audio {
                break;
            }
        }
    }
    has_video |= nonempty_array_field(body, "videos");

    let (kling_video, contents_images) = body
        .get("contents")
        .and_then(|v| v.as_array())
        .map(|a| scan_kling_contents(a))
        .unwrap_or((false, 0));
    has_video |= kling_video;

    scan_typed_content(
        body.get("content").and_then(|c| c.as_array()),
        true,
        true,
        &mut has_video,
        &mut has_audio,
    );
    // Responses / 方舟 response：input[].content 仅检 audio（保持原语义）
    if let Some(input_arr) = body.get("input").and_then(|i| i.as_array()) {
        for item in input_arr {
            scan_typed_content(
                item.get("content").and_then(|c| c.as_array()),
                false,
                true,
                &mut has_video,
                &mut has_audio,
            );
            if has_audio {
                break;
            }
        }
    }
    // DashScope input.media：仅检 video
    scan_typed_content(
        body.get("input")
            .and_then(|i| i.get("media"))
            .and_then(|m| m.as_array()),
        true,
        false,
        &mut has_video,
        &mut has_audio,
    );

    // duration：根、task、parameters、OutputConfig、settings（分辨率见 extract_resolution）
    set_f64_if_none(
        &mut duration_seconds,
        body.get("duration").and_then(parse_json_f64),
    );
    if let Some(task) = body.get("task") {
        set_f64_if_none(
            &mut duration_seconds,
            task.get("duration").and_then(parse_json_f64),
        );
    }
    if let Some(params) = body.get("parameters") {
        set_f64_if_none(
            &mut duration_seconds,
            params.get("duration").and_then(|d| d.as_f64()),
        );
    }
    let output_config = body.get("OutputConfig");
    set_f64_if_none(
        &mut duration_seconds,
        output_config
            .and_then(|oc| oc.get("Duration"))
            .and_then(|d| d.as_f64()),
    );
    set_f64_if_none(
        &mut duration_seconds,
        body.pointer("/settings/duration").and_then(parse_json_f64),
    );

    let prompt_extend = body
        .get("prompt_extend")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        || body
            .get("parameters")
            .and_then(|p| p.get("prompt_extend"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

    let mode = body
        .get("mode")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());
    // generate_audio 优先于 sound；新协议 settings.audio（native→on）
    let sound = if let Some(ga) = body.get("generate_audio") {
        let enabled = ga.as_bool().unwrap_or(false) || ga.as_str() == Some("true");
        Some(on_off(enabled))
    } else if let Some(a) = body.pointer("/settings/audio").and_then(|v| v.as_str()) {
        let a = a.to_ascii_lowercase();
        Some(on_off(!(a == "off" || a == "false")))
    } else {
        body.get("sound")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase())
    };

    let (tencent_refs, tencent_video) = scan_tencent_file_infos(body);
    has_video |= tencent_video;

    let mut has_image_ref = field_media_present(body, "image")
        || field_media_present(body, "image_tail")
        || nonempty_array_field(body, "image_urls")
        || nonempty_array_field(body, "image_list")
        || nonempty_array_field(body, "subject_image_list")
        || body.get("image_reference").is_some_and(|v| !v.is_null())
        || tencent_refs > 0
        || contents_images > 0;

    let mut image_ref_count = {
        let flat = count_image_field(body)
            + if field_media_present(body, "image_tail") {
                1
            } else {
                0
            };
        let count = if flat > 0 {
            flat
        } else {
            let from_lists = count_image_list_fields(body);
            if from_lists > 0 {
                from_lists
            } else if contents_images > 0 {
                contents_images
            } else {
                tencent_refs
            }
        };
        (count > 0).then_some(count)
    };

    // usage / task.usage 覆盖时长、输入图数（分辨率统一走 extract_resolution）
    let mut input_images = None;
    if let Some(usage) = body
        .get("usage")
        .or_else(|| body.get("task").and_then(|t| t.get("usage")))
    {
        if let Some(dur) = usage
            .get("total_seconds")
            .and_then(parse_json_f64)
            .or_else(|| usage.get("duration").and_then(parse_json_f64))
        {
            duration_seconds = Some(dur);
        }
        if let Some(ii) = usage
            .get("input_images")
            .or_else(|| usage.get("image_count"))
            .and_then(|v| v.as_i64())
        {
            input_images = Some(ii as i32);
        }
        if let Some(in_sec) = usage
            .get("input_video_duration")
            .or_else(|| usage.get("input_seconds"))
            .or_else(|| usage.get("InputSeconds"))
            .and_then(parse_json_f64)
            .filter(|&d| d > 0.0)
        {
            video_ref_seconds = Some(in_sec);
        }
    }
    if input_images.is_none() {
        input_images = body
            .get("input_images")
            .or_else(|| body.get("parameters").and_then(|p| p.get("input_images")))
            .and_then(|v| v.as_i64())
            .map(|ii| ii as i32);
    }

    // 即梦等：frames → 时长
    if duration_seconds.is_none() {
        if let Some(frames) = body
            .get("frames")
            .and_then(|f| f.as_f64().or_else(|| f.as_i64().map(|i| i as f64)))
        {
            if frames > 0.0 {
                duration_seconds = Some(if (frames - 121.0).abs() < 1e-3 {
                    5.0
                } else if (frames - 241.0).abs() < 1e-3 {
                    10.0
                } else {
                    (frames - 1.0) / 24.0
                });
            }
        }
    }

    let size = body
        .get("size")
        .and_then(|v| v.as_str())
        .or_else(|| {
            body.get("data")
                .and_then(|d| d.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| item.get("size"))
                .and_then(|s| s.as_str())
        })
        .map(|s| s.to_string());
    let quality = body
        .get("quality")
        .and_then(|v| v.as_str())
        .or_else(|| {
            body.get("parameters")
                .and_then(|p| p.get("quality"))
                .and_then(|v| v.as_str())
        })
        .map(|s| s.to_lowercase());
    let text_characters = body
        .get("input")
        .and_then(|v| v.as_str())
        .or_else(|| {
            body.get("req_params")
                .and_then(|r| r.get("text"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            body.get("request")
                .and_then(|r| r.get("text"))
                .and_then(|v| v.as_str())
        })
        .map(|s| s.chars().count() as i32);
    let image_count = body
        .get("n")
        .and_then(|v| v.as_i64())
        .or_else(|| {
            body.get("OutputConfig")
                .and_then(|oc| oc.get("OutputImageCount"))
                .and_then(|v| v.as_i64())
        })
        .map(|v| v.max(1) as i32);
    let fps = body.get("fps").and_then(|v| v.as_f64());
    let version = body
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());

    let resolution = extract_resolution(body);
    if let Some(ii) = input_images {
        image_ref_count = Some(ii);
        if ii > 0 {
            has_image_ref = true;
        }
    }

    ExtractedFeatures {
        has_video,
        has_audio,
        has_image_ref,
        duration_seconds,
        resolution,
        image_count,
        service_tier,
        prompt_extend,
        mode,
        sound,
        cache_creation: None,
        image_ref_count,
        size,
        quality,
        text_characters,
        fps,
        version,
        web_search: None,
        time_multiplier: None,
        video_ref_seconds,
    }
}

impl ExtractedFeatures {
    /// 严谨合并另外一个特征结构体中的字段（用于出参特征与入参特征融合，保障计费一致性）
    pub fn merge(&mut self, other: ExtractedFeatures) {
        overwrite_if_some(&mut self.duration_seconds, other.duration_seconds);
        set_opt_if_none(&mut self.resolution, other.resolution);
        set_opt_if_none(&mut self.mode, other.mode);
        set_opt_if_none(&mut self.sound, other.sound);
        set_opt_if_none(&mut self.version, other.version);
        self.has_video |= other.has_video;
        self.has_audio |= other.has_audio;
        self.has_image_ref |= other.has_image_ref;
        overwrite_if_some(&mut self.web_search, other.web_search);
        overwrite_if_some(&mut self.image_ref_count, other.image_ref_count);
        overwrite_if_some(&mut self.size, other.size);
        overwrite_if_some(&mut self.image_count, other.image_count);
        // 峰谷快照：已锁定的值不被后续 merge 冲掉
        set_opt_if_none(&mut self.time_multiplier, other.time_multiplier);
        overwrite_if_some(&mut self.video_ref_seconds, other.video_ref_seconds);
    }

    /// 轮询结算：叠加终态响应特征，并应用厂商覆盖（顺序与历史一致，不可打乱）
    pub fn merge_settlement_response(
        &mut self,
        resp_json: &Value,
        store_body: &str,
        category: &str,
    ) {
        // 火山 MediaKit 终态 result（时长/分辨率/帧率）；fps 不经 merge，需先写入
        if let Some(result) = resp_json.get("result") {
            if let Some(duration) = result.get("duration").and_then(|v| v.as_f64()) {
                self.duration_seconds = Some(duration);
            }
            if let Some(res) = result.get("resolution").and_then(|v| v.as_str()) {
                self.resolution = Some(res.to_string());
            }
            if let Some(fps) = result.get("fps").and_then(|v| v.as_f64()) {
                self.fps = Some(fps);
            } else if let Some(fps) = result.get("fps").and_then(|v| v.as_i64()) {
                self.fps = Some(fps as f64);
            }
        }
        // 合并终态通用特征（如 input_images / size）；已有 resolution 不被覆盖
        self.merge(extract_request_features(resp_json));
        // 厂商终态覆盖放在 merge 之后，确保不被冲掉
        if let Some(d) = extract_kling_video_duration(resp_json) {
            self.duration_seconds = Some(d);
        }
        let (tc_dur, tc_res) = extract_tencent_vod_video_settlement(resp_json);
        if let Some(d) = tc_dur {
            self.duration_seconds = Some(d);
        }
        if let Some(r) = tc_res {
            self.resolution = Some(r);
        }
        if category.contains("视频") && self.duration_seconds.is_none() {
            self.duration_seconds = Some(5.0);
        }
        if let Some(n) = count_response_images(store_body) {
            self.image_count = Some(n);
        }
    }
}

/// 从接口响应中提取实际返回的图片数量。
/// 支持 OpenAI/火山方舟 `data` 数组、Google Gemini `candidates.content.parts` 中的图片，
/// 以及 SSE 流式缓冲后的文本（逐行解析 `data: {...}` 提取图片数组）。
/// 返回 None 表示响应中无法识别图片数组（非图片类接口）。
pub fn count_response_images(response: &str) -> Option<i32> {
    // 尝试整体 JSON 解析（非流式响应）
    if let Ok(v) = serde_json::from_str::<Value>(response) {
        if let Some(count) = count_images_from_value(&v) {
            return Some(count);
        }
    }

    // SSE 流式缓冲回落：逐行解析 data: {...} 中的图片数量
    let mut accumulated_from_arrays = 0i32;
    let mut usage_total: Option<i32> = None;

    for line in response.lines() {
        let Some(json_str) = sse_json_payload(line) else {
            continue;
        };

        if let Ok(v) = serde_json::from_str::<Value>(json_str) {
            // 优先官方总计（MiniMax metadata.success_count / 火山 usage.*）
            if let Some(c) = official_image_count(&v) {
                usage_total = Some(c);
            }

            // 累加数组中的实体数
            if let Some(count) = count_images_from_arrays(&v) {
                accumulated_from_arrays += count;
            }
        }
    }

    // 如果流式数据中包含 usage 统计总数，则优先使用该总数（通常流的最后一条包含准确总计）
    if usage_total.is_some() {
        return usage_total;
    }

    if accumulated_from_arrays > 0 {
        Some(accumulated_from_arrays)
    } else {
        None
    }
}

/// 从单个 JSON Value 中提取图片数量
fn count_images_from_value(v: &Value) -> Option<i32> {
    // 官方计数字段优先（MiniMax metadata.success_count / 火山 usage.*）
    if let Some(c) = official_image_count(v) {
        return Some(c);
    }
    count_images_from_arrays(v)
}

/// 解析 JSON 中的非负计数值（兼容整数与数字字符串，如 MiniMax 示例 `"3"`）
#[inline]
fn json_nonneg_count(v: &Value) -> Option<i32> {
    v.as_i64()
        .or_else(|| v.as_u64().map(|n| n as i64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
        .filter(|&n| n >= 0)
        .map(|n| n as i32)
}

/// 上游官方成功张数：MiniMax `metadata.success_count` → usage.generated_images / output_image_count / image_count
fn official_image_count(v: &Value) -> Option<i32> {
    v.pointer("/metadata/success_count")
        .and_then(json_nonneg_count)
        .or_else(|| {
            let usage = v.get("usage")?;
            usage
                .get("generated_images")
                .or_else(|| usage.get("output_image_count"))
                .or_else(|| usage.get("image_count"))
                .and_then(json_nonneg_count)
        })
}

/// 内部辅助函数：深度遍历各种嵌套的 data/results 数组结构提取数量
fn count_images_from_arrays(v: &Value) -> Option<i32> {
    let mut total_count = 0i32;

    #[inline]
    fn count_url_val(url: &Value) -> i32 {
        if let Some(arr) = url.as_array() {
            arr.iter()
                .filter(|u| u.as_str().is_some_and(|s| !s.is_empty()))
                .count() as i32
        } else if url.as_str().is_some_and(|s| !s.is_empty()) {
            1
        } else {
            0
        }
    }
    #[inline]
    fn has_media(item: &Value) -> bool {
        nonempty_str_field(item, "b64_json")
            || nonempty_str_field(item, "image")
            || nonempty_str_field(item, "url")
    }

    // 1. OpenAI / 火山方舟: { "data": [{"url"|"b64_json"|"image": "..."}, ...] }
    if let Some(data) = v.get("data").and_then(|d| d.as_array()) {
        for item in data {
            let from_url = item.get("url").map(count_url_val).unwrap_or(0);
            if from_url > 0 {
                total_count += from_url;
            } else if nonempty_str_field(item, "b64_json") || nonempty_str_field(item, "image") {
                total_count += 1;
            }
        }
    }

    // 2. 异步终态: data.result.images / data.task_result.images / result.images / images
    if total_count == 0 {
        let images_node = v
            .pointer("/data/result/images")
            .or_else(|| v.pointer("/data/task_result/images"))
            .or_else(|| v.pointer("/result/images"))
            .or_else(|| v.pointer("/task_result/images"))
            .or_else(|| v.get("images"));
        if let Some(images) = images_node.and_then(|i| i.as_array()) {
            for img in images {
                if let Some(url) = img.get("url") {
                    total_count += count_url_val(url);
                } else if nonempty_str_field(img, "b64_json") || nonempty_str_field(img, "image") {
                    total_count += 1;
                }
            }
        }
    }

    // 3. Google Gemini: candidates[].content.parts[]
    if total_count == 0 {
        if let Some(candidates) = v.get("candidates").and_then(|c| c.as_array()) {
            for candidate in candidates {
                if let Some(parts) = candidate
                    .get("content")
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.as_array())
                {
                    for part in parts {
                        if part.get("inline_data").is_some() || part.get("inlineData").is_some() {
                            total_count += 1;
                        } else if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            let md = text.matches("](data:").count() as i32;
                            if md > 0 {
                                total_count += md;
                            } else {
                                for word in text.split_whitespace() {
                                    if word.starts_with("http://") || word.starts_with("https://") {
                                        total_count += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. DashScope: output.results[]（须含有效媒体字段）
    if total_count == 0 {
        if let Some(results) = v.pointer("/output/results").and_then(|r| r.as_array()) {
            total_count = results.iter().filter(|item| has_media(item)).count() as i32;
        }
    }

    // 4b. DashScope multimodal: output.choices[].message.content[].image
    if total_count == 0 {
        if let Some(choices) = v.pointer("/output/choices").and_then(|c| c.as_array()) {
            for choice in choices {
                if let Some(parts) = choice
                    .pointer("/message/content")
                    .and_then(|c| c.as_array())
                {
                    total_count += parts
                        .iter()
                        .filter(|p| nonempty_str_field(p, "image"))
                        .count() as i32;
                }
            }
        }
    }

    // 5. 即梦 / MiniMax: data.image_urls[] / data.binary_data_base64[] / data.image_base64[]
    if total_count == 0 {
        for path in [
            "/data/image_urls",
            "/data/binary_data_base64",
            "/data/image_base64",
        ] {
            if let Some(arr) = v.pointer(path).and_then(|a| a.as_array()) {
                let n = arr
                    .iter()
                    .filter(|item| item.as_str().is_some_and(|s| !s.is_empty()))
                    .count() as i32;
                if n > 0 {
                    total_count = n;
                    break;
                }
            }
        }
    }

    (total_count > 0).then_some(total_count)
}

#[derive(Debug, Clone, Default)]
pub struct UsageTokens {
    pub prompt: i32,
    pub completion: i32,
    pub total: i32,
    /// 上游是否显式返回了 total_tokens（parse_usage 回填的不算；供 is_openai_format 判定）
    pub has_total_tokens: bool,
    /// 缓存命中 Token（OpenAI：prompt 子集；Anthropic：独立于 input_tokens）
    pub cached: i32,
    /// 缓存写入 Token（OpenAI GPT-5.6+；Chat: prompt_tokens_details / Responses: input_tokens_details）
    pub cache_write: i32,
    /// Claude 缓存创建 Token 数量（5m+1h 合并，不属于 prompt 子集）
    pub cache_creation: i32,
    /// 音频输入 Token 数量（属于 prompt 的子集，用于豆包聊天分离计价）
    pub audio_tokens: i32,
    /// 音频缓存命中 Token 数量（属于 cached 的子集）
    pub audio_cached_tokens: i32,
    /// 图片输入 Token 数量（用于多模态 tokens 分类计价）
    pub image_tokens: i32,
    /// 联网搜索次数
    pub web_search: i32,
}

/// Chat Completions 与 Responses 的 input/prompt details 双路径取较大值
fn max_usage_detail_i32(usage: &Value, key: &str) -> i32 {
    let from = |obj: &str| {
        usage
            .get(obj)
            .and_then(|d| d.get(key))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32
    };
    from("prompt_tokens_details").max(from("input_tokens_details"))
}

#[inline]
fn assign_max(dst: &mut i32, v: i32) {
    if v > *dst {
        *dst = v;
    }
}

/// 记录上游显式 total（回填不算）；供计费 is_openai_format 判定
#[inline]
fn assign_upstream_total(u: &mut UsageTokens, total: i32) {
    if total > 0 {
        assign_max(&mut u.total, total);
        u.has_total_tokens = true;
    }
}

/// 将 usage 中的计费特征写回 ExtractedFeatures（流式/非流式共用）
pub fn enrich_features_from_usage(features: &mut ExtractedFeatures, usage: &UsageTokens) {
    if usage.cache_creation > 0 {
        features.cache_creation = Some(usage.cache_creation);
    }
    if usage.web_search > 0 {
        features.web_search = Some(usage.web_search);
    }
}

/// 从 usage JSON 对象提取 token 字段，取大值写入 UsageTokens（初始 0 时等同赋值，
/// SSE 多事件场景防止后续缺失字段覆盖已提取值，如 Anthropic message_start 提供
/// input_tokens、message_delta 提供 output_tokens）
fn apply_usage_max(u: &mut UsageTokens, usage: &Value) {
    // 独立提取两组字段名后取较大值，避免上游同时返回 prompt_tokens=0 和 input_tokens>0
    // 时 or_else 回退链不触发导致漏取（如 gpt-image-2 上游返回 prompt_tokens:0 + input_tokens:1136）
    let p = json_field_i32(usage, "prompt_tokens").max(json_field_i32(usage, "input_tokens"));
    let img_tokens =
        json_field_i32(usage, "image_tokens").max(max_usage_detail_i32(usage, "image_tokens"));
    let c = json_field_i32(usage, "completion_tokens").max(json_field_i32(usage, "output_tokens"));
    assign_max(&mut u.prompt, p);
    assign_max(&mut u.completion, c);
    assign_max(&mut u.image_tokens, img_tokens);
    assign_upstream_total(u, json_field_i32(usage, "total_tokens"));
    // Chat Completions: prompt_tokens_details；Responses: input_tokens_details
    assign_max(&mut u.cached, max_usage_detail_i32(usage, "cached_tokens"));
    assign_max(
        &mut u.cache_write,
        max_usage_detail_i32(usage, "cache_write_tokens"),
    );
    assign_max(
        &mut u.audio_tokens,
        max_usage_detail_i32(usage, "audio_tokens"),
    );
    assign_max(
        &mut u.audio_cached_tokens,
        max_usage_detail_i32(usage, "audio_cached_tokens"),
    );
    // Claude 缓存创建（APImart: 5m+1h 合并，兜底: Claude 原生 cache_creation_input_tokens）
    let cc_5m = json_field_i32(usage, "claude_cache_creation_5_m_tokens");
    let cc_1h = json_field_i32(usage, "claude_cache_creation_1_h_tokens");
    let cc = if cc_5m + cc_1h > 0 {
        cc_5m + cc_1h
    } else {
        json_field_i32(usage, "cache_creation_input_tokens")
    };
    assign_max(&mut u.cache_creation, cc);
    // 根级缓存命中兜底：Claude cache_read_input_tokens / DeepSeek prompt_cache_hit_tokens
    if u.cached == 0 {
        u.cached = json_field_i32(usage, "cache_read_input_tokens")
            .max(json_field_i32(usage, "prompt_cache_hit_tokens"));
    }
    // 联网搜索次数提取（Responses API 等）
    assign_max(
        &mut u.web_search,
        usage
            .get("tool_usage")
            .and_then(|t| t.get("web_search"))
            .map(json_i32)
            .unwrap_or(0),
    );
}

pub fn parse_usage(response: &str) -> UsageTokens {
    let mut u = UsageTokens::default();

    let mut extract_from_value = |v: &Value| -> bool {
        let mut found = false;
        // 1. OpenAI / Volcengine / Anthropic（根级 usage）
        if let Some(usage) = v.get("usage") {
            apply_usage_max(&mut u, usage);
            found = true;
        }
        // 1b. Anthropic message_start：usage 嵌套在 message 对象内
        if let Some(usage) = v.get("message").and_then(|m| m.get("usage")) {
            apply_usage_max(&mut u, usage);
            found = true;
        }
        // 1c. Responses API (response.completed)：usage 嵌套在 response 对象内
        if let Some(usage) = v.get("response").and_then(|r| r.get("usage")) {
            apply_usage_max(&mut u, usage);
            found = true;
        }
        // Google Gemini
        if let Some(usage) = v.get("usageMetadata") {
            u.prompt = json_field_i32(usage, "promptTokenCount");
            let total = json_field_i32(usage, "totalTokenCount");
            assign_upstream_total(&mut u, total);
            u.completion = if total >= u.prompt {
                total - u.prompt
            } else {
                0
            };
            u.cached = json_field_i32(usage, "cachedContentTokenCount");
            found = true;
        }
        // 3. 包裹格式: { code, data: { usage: {...} } }
        if !found {
            if let Some(usage) = v.get("data").and_then(|d| d.get("usage")) {
                apply_usage_max(&mut u, usage);
                found = true;
            }
        }
        found
    };

    if let Ok(v) = serde_json::from_str::<Value>(response) {
        extract_from_value(&v);
    } else {
        // SSE流的情况下按行解析（兼容有无 data: 前缀的情况）
        for line in response.lines() {
            let Some(json_str) = sse_json_payload(line) else {
                continue;
            };
            if let Ok(v) = serde_json::from_str::<Value>(json_str) {
                extract_from_value(&v);
            }
        }
    }

    // 下游仍依赖 total；缺省时回填。has_total_tokens 保持 false，避免误判 OpenAI 子集语义
    if u.total == 0 {
        u.total = u.prompt + u.completion;
    }

    u
}

/// 响应体里是否存在实际输出内容（零输出不计费守卫专用，需求二 2026-11-03，v2 修正）。
/// 返回 false = 能确定本次响应没有任何输出（空 delta 流 / 流内 error 事件 / 输出容器为空）。
/// 返回 true  = 确有输出，或响应体判定不了（无响应体、非已知结构、一行 JSON 都解析不出）；
/// 判定不了走 true 是保守计费分支，避免把图片/音频/自定义上游的未知字段误判成零输出。
pub fn resp_has_output_content(response: Option<&str>) -> bool {
    let Some(text) = response else { return true };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return rhoc_decide(&v);
    }
    // SSE 文本：逐行取 JSON 事件，任一事件带输出即算有输出
    let mut saw_event = false;
    for line in trimmed.lines() {
        let Some(payload) = sse_json_payload(line) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(payload) else { continue };
        saw_event = true;
        if rhoc_decide(&v) {
            return true;
        }
    }
    !saw_event
}

/// Some(true)=确有输出，Some(false)=确定无输出，None=判定不了（由调用方走保守计费分支）
fn rhoc_decide(v: &Value) -> bool {
    rhoc_obj_has_output(v).unwrap_or(true)
}

fn rhoc_str_nonempty(v: &Value, key: &str) -> bool {
    v.get(key)
        .and_then(|s| s.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

fn rhoc_arr_nonempty(v: &Value, key: &str) -> bool {
    v.get(key)
        .and_then(|a| a.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false)
}

/// 上游在整包或流内事件里明确报错：200 状态码下的 stream_error、upstream_bad_request 等
fn rhoc_is_error(v: &Value) -> bool {
    v.get("error").map(|e| !e.is_null()).unwrap_or(false)
}

fn rhoc_output_keys(v: &Value) -> bool {
    ["choices", "output", "content", "candidates"]
        .iter()
        .any(|k| v.get(*k).is_some())
}

fn rhoc_obj_has_output(v: &Value) -> Option<bool> {
    if !v.is_object() {
        return v.as_str().map(|s| !s.trim().is_empty());
    }
    // OpenAI Chat Completions（非流式 message / 流式 delta 同形）
    if let Some(choices) = v.get("choices").and_then(|c| c.as_array()) {
        for c in choices {
            for holder in ["message", "delta"] {
                if let Some(m) = c.get(holder) {
                    if rhoc_str_nonempty(m, "content")
                        || rhoc_str_nonempty(m, "reasoning_content")
                        || rhoc_arr_nonempty(m, "tool_calls")
                    {
                        return Some(true);
                    }
                }
            }
            if rhoc_str_nonempty(c, "text") {
                return Some(true);
            }
        }
        return Some(false);
    }
    // Responses API
    if let Some(out) = v.get("output") {
        if rhoc_str_nonempty(v, "output_text") {
            return Some(true);
        }
        if let Some(arr) = out.as_array() {
            for item in arr {
                if rhoc_str_nonempty(item, "text")
                    || rhoc_arr_nonempty(item, "tool_calls")
                    || item.get("function_call").is_some()
                    || item.get("functionCall").is_some()
                {
                    return Some(true);
                }
                if let Some(contents) = item.get("content").and_then(|c| c.as_array()) {
                    if contents.iter().any(|c| rhoc_str_nonempty(c, "text")) {
                        return Some(true);
                    }
                }
            }
            return Some(false);
        }
    }
    // Anthropic Messages：content 可为字符串或块数组
    if let Some(content) = v.get("content") {
        if let Some(s) = content.as_str() {
            return Some(!s.trim().is_empty());
        }
        if let Some(blocks) = content.as_array() {
            for b in blocks {
                if rhoc_str_nonempty(b, "text")
                    || rhoc_str_nonempty(b, "thinking")
                    || rhoc_str_nonempty(b, "data")
                    || b.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                {
                    return Some(true);
                }
            }
            return Some(false);
        }
        return Some(false);
    }
    // Gemini / Vertex：candidates[].content.parts[]
    if let Some(cands) = v.get("candidates").and_then(|c| c.as_array()) {
        for cand in cands {
            let parts = cand
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array());
            if let Some(parts) = parts {
                for part in parts {
                    if rhoc_str_nonempty(part, "text")
                        || part.get("inlineData").is_some()
                        || part.get("inline_data").is_some()
                        || part.get("functionCall").is_some()
                        || part.get("function_call").is_some()
                    {
                        return Some(true);
                    }
                }
            }
        }
        return Some(false);
    }
    // 图片输出：data[] / images[] / results[] / artifacts[]
    for key in ["data", "images", "results", "artifacts"] {
        if let Some(arr) = v.get(key).and_then(|a| a.as_array()) {
            for d in arr {
                if d.is_object() {
                    if ["url", "b64_json", "image_url", "image"]
                        .iter()
                        .any(|f| rhoc_str_nonempty(d, f))
                    {
                        return Some(true);
                    }
                } else if d.as_str().map(|s| !s.trim().is_empty()).unwrap_or(false) {
                    return Some(true);
                }
            }
            return Some(false);
        }
    }
    // 顶层 text / output_text
    for key in ["text", "output_text"] {
        if let Some(s) = v.get(key).and_then(|s| s.as_str()) {
            return Some(!s.trim().is_empty());
        }
    }
    // 包裹形态 { code, data: { choices|output|content|candidates } }
    if let Some(d) = v.get("data") {
        if d.is_object() && rhoc_output_keys(d) {
            return rhoc_obj_has_output(d);
        }
    }
    // 流内/整包 error 事件：确定无输出
    if rhoc_is_error(v) {
        return Some(false);
    }
    // 判定不了
    None
}

pub fn extract_usage_json_string(response: &str) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<Value>(response) {
        // 仅提取 usage 节点，不返回完整响应体（避免存入 choices 等大量聊天内容）
        if let Some(usage) = v.get("usage") {
            return Some(serde_json::json!({ "usage": usage }).to_string());
        }
        if let Some(usage) = v.get("usageMetadata") {
            return Some(serde_json::json!({ "usageMetadata": usage }).to_string());
        }
        // Responses API: response.usage
        if let Some(usage) = v.get("response").and_then(|r| r.get("usage")) {
            return Some(serde_json::json!({ "usage": usage }).to_string());
        }
        if let Some(usage) = v.get("data").and_then(|d| d.get("usage")) {
            return Some(serde_json::json!({ "usage": usage }).to_string());
        }
    } else {
        // SSE 模式下，寻找最后一条包含 usage 字段的 chunk，仅提取 usage 部分
        let mut last_usage_json = None;
        for line in response.lines() {
            let Some(json_str) = sse_json_payload(line) else {
                continue;
            };

            if let Ok(v) = serde_json::from_str::<Value>(json_str) {
                if let Some(usage) = v.get("usage") {
                    last_usage_json = Some(serde_json::json!({ "usage": usage }).to_string());
                } else if let Some(usage) = v.get("response").and_then(|r| r.get("usage")) {
                    last_usage_json = Some(serde_json::json!({ "usage": usage }).to_string());
                } else if let Some(usage) = v.get("usageMetadata") {
                    last_usage_json =
                        Some(serde_json::json!({ "usageMetadata": usage }).to_string());
                }
            }
        }
        if last_usage_json.is_some() {
            return last_usage_json;
        }
    }
    None
}

/// 从可灵视频终态响应中提取实际生成时长（秒）。
/// 旧: data.task_result.videos[0].duration；新 3.0: data[0].outputs[0].duration
fn extract_kling_video_duration(resp: &Value) -> Option<f64> {
    resp.pointer("/data/0/outputs/0/duration")
        .or_else(|| resp.pointer("/data/task_result/videos/0/duration"))
        .and_then(parse_json_f64)
        .filter(|&d| d > 0.0)
}

/// 腾讯云 VOD 生视频任务节点（含场景化 / TaskType 动态字段）。
fn tencent_vod_aigc_video_task(resp: &Value) -> Option<&Value> {
    let response = resp.get("Response")?;
    if let Some(t) = response
        .get("AigcVideoTask")
        .or_else(|| response.get("SceneAigcVideoTask"))
    {
        return Some(t);
    }
    let tt = response.get("TaskType").and_then(|v| v.as_str())?;
    tt.contains("AigcVideo").then(|| response.get(tt)).flatten()
}

/// 腾讯 VOD 生视频终态结算：仅用产出 `MetaData.Duration/Width/Height`（短边定档）。
/// 不读 `Resolution`（终态常为空，且请求侧可能非法）。
fn extract_tencent_vod_video_settlement(resp: &Value) -> (Option<f64>, Option<String>) {
    let Some(meta) =
        tencent_vod_aigc_video_task(resp).and_then(|t| t.pointer("/Output/FileInfos/0/MetaData"))
    else {
        return (None, None);
    };
    let duration = meta
        .get("Duration")
        .and_then(parse_json_f64)
        .filter(|&d| d > 0.0);
    let resolution = match (
        meta.get("Width").and_then(parse_json_u64),
        meta.get("Height").and_then(parse_json_u64),
    ) {
        (Some(w), Some(h)) if w > 0 && h > 0 => {
            let short = w.min(h);
            Some(
                if short <= 480 {
                    "480p"
                } else if short <= 720 {
                    "720p"
                } else if short <= 1080 {
                    "1080p"
                } else if short <= 1440 {
                    "2k"
                } else {
                    "4k"
                }
                .to_string(),
            )
        }
        _ => None,
    };
    (duration, resolution)
}

fn parse_json_u64(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_i64().filter(|&i| i >= 0).map(|i| i as u64))
        .or_else(|| {
            v.as_f64()
                .filter(|&f| f.is_finite() && f >= 0.0)
                .map(|f| f as u64)
        })
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn parse_json_f64(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|i| i as f64))
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .filter(|f| f.is_finite())
}

/// 分辨率标签规范化：`720P`→`720p`，`1K`→`1k`，纯数字 `720`→`720p`。
pub fn normalize_resolution_label(raw: &str) -> String {
    let mut res = raw.trim().to_lowercase().replace('*', "x");
    if !res.is_empty() && res.chars().all(|c| c.is_ascii_digit()) {
        res.push('p');
    }
    res
}

/// 从请求/响应体提取规范化分辨率（计费特征与模型别名映射共用唯一入口）。
/// 来源：resolution / parameters / settings / OutputConfig / usage.SR，缺省时由 size→1k/2k/4k。
pub fn extract_resolution(body: &Value) -> Option<String> {
    let mut resolution = None;
    set_res_field(&mut resolution, Some(body), "resolution");
    set_res_field(&mut resolution, body.get("task"), "resolution");
    set_res_field(&mut resolution, body.get("parameters"), "resolution");
    set_res_field(&mut resolution, body.get("OutputConfig"), "Resolution");
    set_str_if_none(
        &mut resolution,
        body.pointer("/settings/resolution").and_then(|r| r.as_str()),
    );
    if let Some(usage) = body
        .get("usage")
        .or_else(|| body.get("task").and_then(|t| t.get("usage")))
    {
        if let Some(sr) = usage.get("SR") {
            if let Some(n) = sr.as_i64() {
                resolution = Some(format!("{}p", n));
            } else if let Some(s) = sr.as_str().filter(|s| !s.is_empty()) {
                resolution = Some(s.to_string());
            }
        }
    }
    if let Some(res) = resolution {
        let res = normalize_resolution_label(&res);
        if !res.is_empty() {
            return Some(res);
        }
    }
    body.get("size")
        .and_then(|v| v.as_str())
        .or_else(|| {
            body.get("data")
                .and_then(|d| d.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| item.get("size"))
                .and_then(|s| s.as_str())
        })
        .and_then(parse_pixel_resolution)
}

/// 从像素尺寸字符串识别分辨率等级（从 forward.rs 移入本特征提取模块）。
/// 支持像素格式（如 "1024x1024"、"2048×1536"）和已有的等级格式（如 "1k"、"2k"、"4k"）。
/// 按最小边长严格判定区间（图片质量由短边决定）：
///   - ≤1024  → "1k"（标准，如 512x512, 1024x1024, 2048x1024）
///   - ≤2048  → "2k"（高清，如 2048x1536, 2048x2048）
///   - >2048  → "4k"（超高清，如 3840x2160, 4096x2304）
/// 比例格式（如 "1:1"）和无法解析的输入返回 None。
pub fn parse_pixel_resolution(size: &str) -> Option<String> {
    let s = size.trim().to_lowercase();
    // 已经是分辨率等级格式，直接返回其小写
    if s.ends_with('k') {
        return Some(s);
    }
    // 比例格式（含 ':'）不属于像素分辨率，返回 None
    if s.contains(':') {
        return None;
    }
    // 解析像素尺寸：支持 x、*、× (Unicode 乘号) 分隔符
    let (w_str, h_str) = s
        .split_once('x')
        .or_else(|| s.split_once('*'))
        .or_else(|| s.split_once('×'))?;
    let w = w_str.trim().parse::<u32>().ok()?;
    let h = h_str.trim().parse::<u32>().ok()?;
    let min_edge = w.min(h);
    // 按最小边长判定分辨率等级区间
    Some(if min_edge <= 1024 {
        "1k".to_string()
    } else if min_edge <= 2048 {
        "2k".to_string()
    } else {
        "4k".to_string() // >2048 统一归为 4k
    })
}
