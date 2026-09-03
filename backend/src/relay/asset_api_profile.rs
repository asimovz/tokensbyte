/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 上游素材 API 描述符（asset_api_profile）：声明式协议适配引擎。
//!
//! 背景：素材透传主链路（ark_asset_upstream_passthrough）硬编码了火山协议
//! （?Action= POST、`/Result/Id`、`Result.{Items|Assets|AssetGroups}` 信封）。
//! 对于非火山上游（如平行幻帧 fantaframe / cmcc），通过在绑定表
//! `upstream_asset_bindings.asset_api_profile` 存放一段 JSON 描述符，
//! 在"入口把火山请求转成上游格式、出口把上游响应包回火山信封"，
//! 使归属校验 / List 过滤 / 后置记录逻辑**零改动**复用。
//!
//! 设计要点（两头转换、中间零改动）：
//! - 请求侧：`inject` 注入固定值（provider 等）、`defaults` 补缺省、`rename`
//!   字段改名、`path_params` 把请求体字段填进 URL 路径占位符；
//! - 响应侧：按 `response` 规格把上游响应包回火山信封——
//!   Create 写 `Result.Id`（供后置记录 `/Result/Id` 取值）、
//!   List 写 `Result.{target_key}[].Id` + `Result.TotalCount`
//!   （精确对齐 `filter_ark_list_result` 的过滤约定）、
//!   其余动作把载荷原样放进 `Result`。

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// 一个绑定的完整协议描述符。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AssetApiProfile {
    /// 动作名（火山 Action，如 CreateAssetGroup）→ 该动作的适配规格。
    #[serde(default)]
    pub actions: HashMap<String, ActionSpec>,
    /// 明确不支持的动作：命中即返回"暂未接入"，不再透传。
    #[serde(default)]
    pub unsupported: Vec<String>,
}

/// 单个动作的请求/响应适配规格。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ActionSpec {
    /// HTTP 方法：GET / POST / PUT / DELETE（缺省 POST）。
    #[serde(default = "default_method")]
    pub method: String,
    /// 上游路径模板，可含 `{占位符}`，由 `path_params` 从请求体填充。
    /// 例：`/v1/video/assets/groups/{groupID}`。
    #[serde(default)]
    pub path: String,
    /// 固定注入到出参体的键值（总是覆盖），如 `provider:"cmcc"`。
    #[serde(default)]
    pub inject: HashMap<String, Value>,
    /// 仅当出参体缺失时补的默认值，如 `statuses:["ACTIVE"]`。
    #[serde(default)]
    pub defaults: HashMap<String, Value>,
    /// 入参字段 → 出参字段改名，如 `Name → groupName`。
    #[serde(default)]
    pub rename: HashMap<String, String>,
    /// 出参体仅保留这些字段（空 = 不裁剪）。改名后的字段名。
    #[serde(default)]
    pub keep: Vec<String>,
    /// 需从出参体删除的字段（改名后的字段名）。
    #[serde(default)]
    pub drop: Vec<String>,
    /// 是否发送 JSON 请求体（GET/DELETE 通常 false）。缺省按方法推断。
    #[serde(default)]
    pub send_body: Option<bool>,
    /// URL 路径占位符 → 入参体字段名，如 `{ "groupID": "Id" }`；
    /// 多候选用 `|` 分隔（如 `"SessionId|BytedToken"`），按序取第一个非空字符串。
    #[serde(default)]
    pub path_params: HashMap<String, String>,
    /// 响应回转规格。
    #[serde(default)]
    pub response: ResponseSpec,
}

/// 上游响应 → 火山信封的回转规格。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResponseSpec {
    /// 上游载荷是否包裹在 `body` 字段内（fantaframe 素材类接口为 true）。
    #[serde(default)]
    pub unwrap_body: bool,
    /// 成功判定字段路径（相对顶层），如 `state`。
    #[serde(default)]
    pub ok_path: String,
    /// 成功判定值，如 `OK`。`ok_path` 非空而实际值不符时视为上游业务错误。
    #[serde(default)]
    pub ok_value: String,
    /// Create 类动作：新建资源 ID 的取值路径（相对解包后载荷），如 `groupId`；
    /// 特殊值 `$` 表示解包后载荷本身即为 ID 字符串（fantaframe CreateAsset）。
    #[serde(default)]
    pub id_path: String,
    /// List 类动作规格；非空时按列表回转。
    #[serde(default)]
    pub list: Option<ListSpec>,
    /// 其余动作：把解包后载荷整体放进 `Result`（Get/Delete/认证查询等）。
    #[serde(default)]
    pub raw_result: bool,
}

/// List 动作的回转规格（对齐 `filter_ark_list_result`）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ListSpec {
    /// 列表项数组路径（相对解包后载荷），如 `data`。
    #[serde(default)]
    pub items_path: String,
    /// 每项中的原始 ID 字段名，如 `groupId` / `assetId`（将改写为 `Id`）。
    #[serde(default)]
    pub item_id_field: String,
    /// 总数字段路径（相对解包后载荷），如 `total`。
    #[serde(default)]
    pub total_path: String,
    /// 火山信封中的目标键：`Assets` / `AssetGroups` / `Items`。
    #[serde(default)]
    pub target_key: String,
}

fn default_method() -> String {
    "POST".to_string()
}

impl AssetApiProfile {
    /// 从绑定表存储的 JSON 文本解析描述符；空/非法返回 None。
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        serde_json::from_str::<AssetApiProfile>(trimmed).ok()
    }

    /// 该动作是否被描述符显式标记为不支持。
    pub fn is_unsupported(&self, action: &str) -> bool {
        self.unsupported.iter().any(|a| a == action)
    }
}

/// 按点路径读取值（与转换引擎一致）。
fn path_get<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(v);
    }
    let mut cur = v;
    for seg in path.split('.').filter(|s| !s.is_empty()) {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

/// 按点路径写入值，中间层级自动创建。
fn path_set(root: &mut Value, path: &str, value: Value) {
    let segs: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return;
    }
    let mut cur = root;
    for seg in &segs[..segs.len() - 1] {
        if !cur.is_object() {
            *cur = json!({});
        }
        let entry = cur
            .as_object_mut()
            .unwrap()
            .entry((*seg).to_string())
            .or_insert_with(|| json!({}));
        if !entry.is_object() {
            *entry = json!({});
        }
        cur = entry;
    }
    if let Some(obj) = cur.as_object_mut() {
        obj.insert(segs[segs.len() - 1].to_string(), value);
    }
}

/// 构造发往上游的请求：返回 (方法, 相对路径, 可选请求体)。
/// `incoming` 为火山格式的原始请求体。
pub fn build_upstream_request(
    spec: &ActionSpec,
    incoming: &Value,
) -> (reqwest::Method, String, Option<Value>) {
    let method = match spec.method.to_ascii_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "PATCH" => reqwest::Method::PATCH,
        _ => reqwest::Method::POST,
    };

    // 路径占位符填充：{groupID} ← incoming[Id]；源字段支持 | 多候选（按序取第一个非空）
    let mut path = spec.path.clone();
    for (ph, src) in &spec.path_params {
        let mut val = String::new();
        for cand in src.split('|') {
            if let Some(s) = incoming
                .get(cand.trim())
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                val = s.to_string();
                break;
            }
        }
        path = path.replace(&format!("{{{}}}", ph), &val);
    }

    // 是否发送请求体：显式 send_body 优先，否则按方法推断（GET/DELETE 不发）
    let send_body = spec.send_body.unwrap_or(!matches!(
        method,
        reqwest::Method::GET | reqwest::Method::DELETE
    ));

    let body = if send_body {
        let mut out = json!({});
        if let Some(obj) = incoming.as_object() {
            for (k, v) in obj {
                let new_key = spec.rename.get(k).cloned().unwrap_or_else(|| k.clone());
                out.as_object_mut().unwrap().insert(new_key, v.clone());
            }
        }
        // 注入固定值（覆盖）
        for (k, v) in &spec.inject {
            out.as_object_mut().unwrap().insert(k.clone(), v.clone());
        }
        // 补缺省（仅缺失时）
        for (k, v) in &spec.defaults {
            if out.get(k).map(|x| x.is_null()).unwrap_or(true) {
                out.as_object_mut().unwrap().insert(k.clone(), v.clone());
            }
        }
        // 删除字段
        for k in &spec.drop {
            out.as_object_mut().unwrap().remove(k);
        }
        // 仅保留
        if !spec.keep.is_empty() {
            let mut kept = json!({});
            for k in &spec.keep {
                if let Some(v) = out.get(k) {
                    kept.as_object_mut().unwrap().insert(k.clone(), v.clone());
                }
            }
            out = kept;
        }
        Some(out)
    } else {
        None
    };

    (method, path, body)
}

/// 把上游响应回转成火山信封 `{ResponseMetadata, Result}`。
/// `action`/`version` 用于回填 ResponseMetadata。失败时返回 Err（业务错误信息）。
pub fn transform_response(
    spec: &ActionSpec,
    action: &str,
    version: &str,
    upstream: &Value,
) -> Result<Value, String> {
    let rs = &spec.response;

    // 成功判定（仅当配置了 ok_path）
    if !rs.ok_path.is_empty() && !rs.ok_value.is_empty() {
        let actual = path_get(upstream, &rs.ok_path)
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if actual != rs.ok_value {
            let msg = upstream
                .get("message")
                .or_else(|| upstream.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return Err(format!(
                "上游业务错误({}={}): {}",
                rs.ok_path,
                actual,
                msg
            ));
        }
    }

    // 解包载荷
    let payload: Value = if rs.unwrap_body {
        upstream.get("body").cloned().unwrap_or(Value::Null)
    } else {
        upstream.clone()
    };

    let result: Value = if let Some(list) = &rs.list {
        build_list_result(list, &payload)
    } else if !rs.id_path.is_empty() {
        // Create：取新建资源 ID
        let id = if rs.id_path == "$" {
            payload
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_default()
        } else {
            path_get(&payload, &rs.id_path)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };
        json!({ "Id": id })
    } else if rs.raw_result {
        payload.clone()
    } else {
        payload.clone()
    };

    let request_id = upstream
        .get("requestId")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    Ok(json!({
        "ResponseMetadata": {
            "Action": action,
            "Version": version,
            "Service": "ark",
            "Region": "cn-north-1",
            "RequestId": request_id,
        },
        "Result": result,
    }))
}

/// List 回转：`Result.{target_key}[].Id` + `Result.TotalCount`，
/// 精确对齐 `filter_ark_list_result` 的过滤约定。
fn build_list_result(list: &ListSpec, payload: &Value) -> Value {
    let items_src = path_get(payload, &list.items_path)
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut items: Vec<Value> = Vec::with_capacity(items_src.len());
    for mut item in items_src {
        // 原 ID 字段 → Id（保留原字段，另写 Id 供过滤）
        if !list.item_id_field.is_empty() {
            if let Some(id) = item.get(&list.item_id_field).cloned() {
                if let Some(obj) = item.as_object_mut() {
                    obj.insert("Id".to_string(), id);
                }
            }
        }
        items.push(item);
    }
    let total = path_get(payload, &list.total_path)
        .and_then(|v| v.as_i64())
        .unwrap_or(items.len() as i64);
    let target = if list.target_key.is_empty() {
        "Items".to_string()
    } else {
        list.target_key.clone()
    };
    json!({
        (target): items,
        "TotalCount": total,
    })
}
