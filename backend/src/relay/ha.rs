/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! 高可用（HA）failover 策略：端点只碰 [`HaAttempt`] + 少量选渠/落库辅助。
//! 语义：仅当「插件启用 AND 令牌 high_availability≠0」时可选 HA 组并做子渠切换。
//!
//! 终态对齐规则：
//! - **全部上游失败**：客户端错误 + 日志的 status/error/`channel_config_id` 保留「第一次」失败（子渠 1）
//! - **某次成功**：日志 channel_id / `channel_config_id` / 上游 URL 使用**成功那一次**的子渠
//! - HA 中间失败不 UPDATE `logs`；环结束一次记账；插件表 `ha_usage_logs` 记全量子渠过程
//! - 展示用 YID：读路径 JOIN `channel_configs`，日志表不落 `yid` 列
//! - 展示用 HA：写路径快照 `logs.is_ha`（见 [`channel_is_ha_flag`]），禁止 JOIN 当前 `channels.provider_type`
//! - **墙钟预算**（仅 failover 开启）：备渠切换受 `ha_total_timeout_secs` 约束，避免嵌套转发被入口 Nginx 切成 504 HTML

use crate::error::AppError;
use crate::models::{ApiToken, Channel};
use crate::AppState;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FAILED_CHANNELS_SOFT_CAP: usize = 4096;
/// 备渠切换最少剩余预算；过短则收口首败
const MIN_RETRY_BUDGET: Duration = Duration::from_secs(5);
/// send 时剩余预算不足仍给极短超时，便于快速失败
const MIN_REQ_TIMEOUT: Duration = Duration::from_secs(1);

/// 一次解析：`(failover_on, max_attempts)`，供选渠开环前调用。
pub async fn policy(state: &AppState, token_ha: i32) -> (bool, usize) {
    let plugin_on = super::relay_settings::get_cached_ha_enabled(&state.db).await;
    let on = plugin_on && token_ha != 0;
    let attempts = if on {
        state
            .ha_max_retries
            .load(std::sync::atomic::Ordering::Relaxed)
            .max(1) as usize
    } else {
        1
    };
    (on, attempts)
}

/// HA 整次墙钟预算秒数。`0`=自动 `min(540, 上游超时-60)`（为常见 Nginx 600s 留回写裕量）。
fn resolve_budget_secs(state: &AppState) -> u64 {
    let configured = state
        .ha_total_timeout_secs
        .load(std::sync::atomic::Ordering::Relaxed);
    if configured > 0 {
        configured as u64
    } else {
        let upstream = crate::services::http_client::upstream_timeout_duration().as_secs();
        upstream.saturating_sub(60).min(540).max(60)
    }
}

/// spawn 内在真正 send 前重算超时（避免 transform 空耗导致超时过宽）
#[derive(Clone, Copy)]
pub struct HaTimeoutCtx {
    attempt: usize,
    started_at: Instant,
    budget: Option<Duration>,
}

impl HaTimeoutCtx {
    #[inline]
    fn remaining(self) -> Option<Duration> {
        self.budget
            .map(|b| b.saturating_sub(self.started_at.elapsed()))
    }

    /// 按当前墙钟解析本轮上游超时
    #[inline]
    pub fn resolve(self) -> Duration {
        let base = crate::services::http_client::upstream_timeout_duration();
        if self.attempt == 0 {
            return base;
        }
        self.remaining()
            .filter(|&rem| rem < base)
            .map(|rem| rem.max(MIN_REQ_TIMEOUT))
            .unwrap_or(base)
    }
}

/// 日志展示用 yid（空 → `-`，与后台「上游 YID」对齐）
#[inline]
pub fn yid_label(yid: Option<&str>) -> &str {
    yid.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("-")
}

/// 落库用 0/1（列类型 INTEGER / INT4）
#[inline]
pub fn channel_is_ha_flag(channel: &Channel) -> i32 {
    let is_ha = channel.provider_type == "high_availability_group"
        || channel.group_aid.as_deref().is_some_and(is_ha_aid);
    i32::from(is_ha)
}

/// 落库用子配 id：运行时 aid/preset 优先，否则用内存 `Channel.yid` 反查
pub async fn resolve_log_config_id(state: &AppState, channel: &Channel) -> Option<i32> {
    if let Some(id) = resolve_config_id(channel) {
        return Some(id);
    }
    let y = channel
        .yid
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    sqlx::query_scalar::<_, i32>(
        &state
            .db
            .format_query("SELECT id FROM channel_configs WHERE yid = ? LIMIT 1"),
    )
    .bind(y)
    .fetch_optional(&state.db.pool)
    .await
    .ok()
    .flatten()
}

/// 是否仍在熔断窗口内；已过期则立即移除该键
#[inline]
pub fn is_melted_down(state: &AppState, aid: &str) -> bool {
    let Some(entry) = state.failed_channels.get(aid) else {
        return false;
    };
    if *entry.value() > Instant::now() {
        true
    } else {
        drop(entry);
        state.failed_channels.remove(aid);
        false
    }
}

/// 批量清除过期熔断；超软上限则整清
pub fn scrub_failed_channels(state: &AppState) {
    let now = Instant::now();
    state.failed_channels.retain(|_, until| *until > now);
    let n = state.failed_channels.len();
    if n > FAILED_CHANNELS_SOFT_CAP {
        tracing::warn!(
            "[HA] 熔断表数量={} 超过软上限={}，整表清空",
            n,
            FAILED_CHANNELS_SOFT_CAP
        );
        state.failed_channels.clear();
    }
}

/// spawn 内上游失败暂存（不写 logs）；外环 [`HaAttempt::fail`] 终态再记账
#[derive(Debug, Default)]
pub struct FailBill {
    pub latency_ms: u32,
    pub response_body: String,
    pub response_content: Option<String>,
    pub upstream_req: Option<String>,
    pub billing_detail: Option<String>,
    pub prefer_status: Option<u16>,
    pub client_msg: Option<String>,
    pub pre_deducted: f64,
    pub pre_deduct_gift: f64,
    pub is_stream: i32,
    pub request_content: String,
}

impl FailBill {
    /// 传输/连接失败（默认 502；`response_content` 默认同 body）
    #[inline]
    pub fn transport(
        latency_ms: u32,
        msg: impl Into<String>,
        request: impl Into<String>,
        upstream: impl Into<String>,
    ) -> Self {
        let msg = msg.into();
        Self {
            latency_ms,
            response_body: msg.clone(),
            response_content: Some(msg),
            upstream_req: Some(upstream.into()),
            prefer_status: Some(502),
            request_content: request.into(),
            ..Default::default()
        }
    }

    /// HTTP 非 2xx（body 同时写入 response_body / response_content）
    #[inline]
    pub fn http(
        latency_ms: u32,
        status: u16,
        body: impl Into<String>,
        request: impl Into<String>,
        upstream: impl Into<String>,
    ) -> Self {
        let body = body.into();
        Self {
            latency_ms,
            response_body: body.clone(),
            response_content: Some(body),
            upstream_req: Some(upstream.into()),
            prefer_status: Some(status),
            request_content: request.into(),
            ..Default::default()
        }
    }

    /// HTTP 200 业务失败（prefer_status=None）
    #[inline]
    pub fn biz(
        latency_ms: u32,
        body: impl Into<String>,
        client_msg: impl Into<String>,
        request: impl Into<String>,
        upstream: impl Into<String>,
    ) -> Self {
        let body = body.into();
        Self {
            latency_ms,
            response_body: body.clone(),
            response_content: Some(body),
            upstream_req: Some(upstream.into()),
            prefer_status: None,
            client_msg: Some(client_msg.into()),
            request_content: request.into(),
            ..Default::default()
        }
    }

    #[inline]
    pub fn stream(mut self, s: i32) -> Self {
        self.is_stream = s;
        self
    }

    #[inline]
    pub fn detail(mut self, d: impl Into<String>) -> Self {
        self.billing_detail = Some(d.into());
        self
    }

    #[inline]
    pub fn detail_opt(mut self, d: Option<String>) -> Self {
        self.billing_detail = d;
        self
    }

    #[inline]
    pub fn pre(mut self, pre: f64, gift: f64) -> Self {
        self.pre_deducted = pre;
        self.pre_deduct_gift = gift;
        self
    }

    #[inline]
    pub fn content(mut self, c: Option<String>) -> Self {
        self.response_content = c;
        self
    }

    #[inline]
    pub fn body(mut self, b: impl Into<String>) -> Self {
        self.response_body = b.into();
        self
    }

    #[inline]
    pub fn client(mut self, m: impl Into<String>) -> Self {
        self.client_msg = Some(m.into());
        self
    }
}

/// [`HaAttempt::fail`] / [`HaAttempt::finish`] 共用记账上下文
pub struct HaBillCtx<'a> {
    pub state: &'a Arc<AppState>,
    pub token: &'a ApiToken,
    pub model: &'a str,
    pub ep: &'a str,
    pub hint_category: Option<&'a str>,
    pub billing_model_hint: Option<&'a str>,
    pub db_model: Option<&'a crate::models::Model>,
}

impl<'a> HaBillCtx<'a> {
    #[inline]
    pub fn new(state: &'a Arc<AppState>, token: &'a ApiToken, model: &'a str, ep: &'a str) -> Self {
        Self {
            state,
            token,
            model,
            ep,
            hint_category: None,
            billing_model_hint: None,
            db_model: None,
        }
    }

    #[inline]
    pub fn category(mut self, c: &'a str) -> Self {
        self.hint_category = Some(c);
        self
    }

    #[inline]
    pub fn billing_model(mut self, m: &'a str) -> Self {
        self.billing_model_hint = Some(m);
        self
    }

    #[inline]
    pub fn db(mut self, m: Option<&'a crate::models::Model>) -> Self {
        self.db_model = m;
        self
    }
}

/// 插件 attempts 精简字段
#[derive(Debug, Clone, Serialize)]
struct HaSnap {
    n: u16,
    #[serde(skip_serializing_if = "String::is_empty")]
    yid: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    url: String,
    ms: u32,
    ok: u8,
}

/// 首败快照：渠 + 对外文案 + 落库账单 + 首败 endpoint（中间续试不覆盖）
struct FirstFail {
    channel: Channel,
    status: u16,
    message: String,
    bill: Option<FailBill>,
    endpoint: String,
}

/// HA 外环状态：各端点共用。上游失败走 [`HaAttempt::fail`]；业务侧 [`HaAttempt::on_access_err`]。
pub struct HaAttempt {
    pub exclude_aids: Vec<String>,
    pub attempt: usize,
    pub max_attempts: usize,
    pub failover_on: bool,
    pub pending_log_id: Option<i64>,
    pub had_upstream: bool,
    pub last_err: AppError,
    started_at: Instant,
    budget: Option<Duration>,
    fail_buf: Arc<Mutex<Option<FailBill>>>,
    snaps: Vec<HaSnap>,
    first: Option<FirstFail>,
    group_aid: Option<String>,
    billed: bool,
    saved: bool,
}

impl HaAttempt {
    /// 开环：解析 HA 策略，初始化排除列表与终态错误占位
    pub async fn begin(state: &AppState, token_ha: i32) -> Self {
        let (failover_on, max_attempts) = policy(state, token_ha).await;
        let budget_secs = resolve_budget_secs(state);
        let budget = failover_on.then(|| Duration::from_secs(budget_secs));
        if failover_on {
            tracing::info!(
                "[HA] 开始 最大尝试={} 令牌HA={} 墙钟预算={}s",
                max_attempts,
                token_ha,
                budget_secs
            );
        }
        Self {
            exclude_aids: vec![],
            attempt: 0,
            max_attempts,
            failover_on,
            pending_log_id: None,
            had_upstream: false,
            last_err: AppError::UpstreamError("No available models".into()),
            started_at: Instant::now(),
            budget,
            fail_buf: Arc::new(Mutex::new(None)),
            snaps: Vec::new(),
            first: None,
            group_aid: None,
            billed: false,
            saved: false,
        }
    }

    /// 供 spawn 克隆，写入 [`FailBill`]
    #[inline]
    pub fn buf(&self) -> Arc<Mutex<Option<FailBill>>> {
        Arc::clone(&self.fail_buf)
    }

    /// spawn 内：暂存账单并构造对外错误（不写 logs）
    #[inline]
    pub fn park(
        buf: &Arc<Mutex<Option<FailBill>>>,
        bill: FailBill,
        headers: Option<axum::http::HeaderMap>,
    ) -> AppError {
        let err = err_of(&bill, headers);
        if let Ok(mut g) = buf.lock() {
            *g = Some(bill);
        }
        err
    }

    #[inline]
    fn take_bill(&self) -> Option<FailBill> {
        self.fail_buf.lock().ok().and_then(|mut g| g.take())
    }

    #[inline]
    fn remaining_budget(&self) -> Option<Duration> {
        self.timeout_ctx().remaining()
    }

    #[inline]
    fn budget_secs(&self) -> u64 {
        self.budget.map(|d| d.as_secs()).unwrap_or(0)
    }

    #[inline]
    fn has_budget_for_retry(&self) -> bool {
        self.remaining_budget()
            .is_none_or(|rem| rem >= MIN_RETRY_BUDGET)
    }

    /// 快照供 spawn 在 send 前 [`HaTimeoutCtx::resolve`]
    #[inline]
    pub fn timeout_ctx(&self) -> HaTimeoutCtx {
        HaTimeoutCtx {
            attempt: self.attempt,
            started_at: self.started_at,
            budget: self.budget,
        }
    }

    #[inline]
    pub fn cont(&self) -> bool {
        if self.attempt >= self.max_attempts {
            return false;
        }
        // 首次尝试不受墙钟预算限制；备渠需剩余预算
        if self.attempt > 0 && !self.has_budget_for_retry() {
            tracing::info!(
                "[HA] 墙钟预算耗尽 已耗时={}s 预算={}s 尝试={}/{}，收口首败",
                self.started_at.elapsed().as_secs(),
                self.budget_secs(),
                self.attempt,
                self.max_attempts
            );
            return false;
        }
        true
    }

    /// 本轮上游超时（立即计算；spawn 内用 [`timeout_ctx`] 在 send 前再 resolve）
    #[inline]
    pub fn attempt_timeout(&self) -> Duration {
        self.timeout_ctx().resolve()
    }

    /// 选渠失败：尚无上游交互时用选渠错误作为对外文案
    pub fn on_select_err(&mut self, e: AppError) {
        if !self.had_upstream {
            self.last_err = e;
        }
    }

    /// 权限/余额等不可 failover 错误
    pub fn on_access_err(&mut self, e: AppError) {
        self.last_err = e;
    }

    /// 上游失败：push snap；HA 续试则不写 logs；停环则按首败一次记账并 [`save`]。
    /// 返回 true → `bump(); continue`。
    pub async fn fail(
        &mut self,
        ctx: &HaBillCtx<'_>,
        channel: &Channel,
        err: AppError,
        url: Option<&str>,
    ) -> bool {
        if is_access_side_err(&err) {
            let _ = self.take_bill();
            tracing::info!(
                "[HA] 业务侧停止 状态码={} 上游YID={} 尝试={}/{}",
                err.http_status(),
                yid_label(channel.yid.as_deref()),
                self.attempt + 1,
                self.max_attempts
            );
            self.on_access_err(err);
            return false;
        }

        let bill = self.take_bill();
        let (fail_status, msg) = match &bill {
            Some(b) => fail_status_msg(b),
            None => status_msg(&err),
        };
        let latency_ms = bill.as_ref().map(|b| b.latency_ms).unwrap_or(0);
        let masked = url.map(|u| super::forward::mask_key_in_string(u, &channel.api_key));

        self.had_upstream = true;
        // 首败账单 move 进快照；后续失败账单留局部（退预扣 / 并入预扣额）
        let mut bill = bill;
        if self.first.is_none() {
            self.first = Some(FirstFail {
                channel: channel.clone(),
                status: fail_status,
                message: msg.clone(),
                bill: bill.take(),
                endpoint: ctx.ep.to_string(),
            });
            self.last_err = err;
        } else {
            let _ = err;
        }

        self.push_snap(
            channel,
            fail_status,
            Some(super::proxy::extract_error_message(&msg)),
            masked.as_deref().unwrap_or(""),
            latency_ms,
            false,
        );

        if self.try_switch(ctx.state, channel, fail_status, &msg) {
            self.refund_continue(ctx, bill.as_ref()).await;
            return true;
        }

        // 末次：未退预扣并入首败后一次落库
        if let Some(last) = bill {
            if let Some(f) = self.first.as_mut() {
                if let Some(b) = f.bill.as_mut() {
                    b.pre_deducted = last.pre_deducted;
                    b.pre_deduct_gift = last.pre_deduct_gift;
                } else if last.pre_deducted > 0.0 || last.pre_deduct_gift > 0.0 {
                    f.bill = Some(FailBill {
                        pre_deducted: last.pre_deducted,
                        pre_deduct_gift: last.pre_deduct_gift,
                        ..Default::default()
                    });
                }
            }
        }
        self.settle_first(ctx).await;
        self.save(ctx.state).await;
        false
    }

    /// 续试：有预扣时按日志行退款，并清零首败预扣字段防 finish 双退
    async fn refund_continue(&mut self, ctx: &HaBillCtx<'_>, last: Option<&FailBill>) {
        let has_pre = last
            .map(|b| b.pre_deducted > 0.0 || b.pre_deduct_gift > 0.0)
            .or_else(|| {
                self.first
                    .as_ref()
                    .and_then(|f| f.bill.as_ref())
                    .map(|b| b.pre_deducted > 0.0 || b.pre_deduct_gift > 0.0)
            })
            .unwrap_or(false);
        if has_pre {
            if let Some(log_id) = self.pending_log_id {
                super::proxy::refund_pending(ctx.state, log_id).await;
            }
        }
        if let Some(fb) = self.first.as_mut().and_then(|f| f.bill.as_mut()) {
            fb.pre_deducted = 0.0;
            fb.pre_deduct_gift = 0.0;
        }
    }

    /// 成功子渠：记 snap + 写插件表（主站 logs 已由外部 record_and_bill）
    pub async fn ok(&mut self, state: &AppState, channel: &Channel, url: &str, ms: u32) {
        self.billed = true;
        let masked = super::forward::mask_key_in_string(url, &channel.api_key);
        self.push_snap(channel, 200, None, &masked, ms, true);
        self.save(state).await;
    }

    /// 写入 `ha_usage_logs`（仅有 HA 子渠 snap 时）
    pub async fn save(&mut self, state: &AppState) {
        if self.saved || self.snaps.is_empty() {
            return;
        }
        // 精简优化：只记录包含错误/失败子渠尝试的日志。
        // 如果全部尝试都成功（通常意味着一次调用即成功，无任何错误），则不重复记录
        if self.snaps.iter().all(|s| s.ok == 1) {
            return;
        }
        let Some(log_id) = self.pending_log_id else {
            return;
        };
        let attempts = match serde_json::to_value(&self.snaps) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("[HA] attempts 序列化失败: {}", e);
                return;
            }
        };
        let (final_ok, final_status) = self
            .snaps
            .iter()
            .rev()
            .find(|s| s.ok == 1)
            .map(|s| (true, s.status as i32))
            .unwrap_or_else(|| {
                (
                    false,
                    self.first.as_ref().map(|f| f.status as i32).unwrap_or(0),
                )
            });
        let group_aid = self.group_aid.clone().unwrap_or_default();
        let n = self.snaps.len() as i16;
        if let Err(e) = sqlx::query(
            &state.db.format_query(
                "INSERT INTO ha_usage_logs (log_id, group_aid, attempt_count, final_ok, final_status_code, attempts) \
                 VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (log_id) DO NOTHING",
            ),
        )
        .bind(log_id)
        .bind(&group_aid)
        .bind(n)
        .bind(i16::from(final_ok))
        .bind(final_status)
        .bind(sqlx::types::Json(attempts))
        .execute(&state.db.pool)
        .await
        {
            tracing::warn!("[HA] 写 ha_usage_logs 失败 日志id={}: {:?}", log_id, e);
            return;
        }
        self.saved = true;
    }

    #[inline]
    pub fn bump(&mut self) {
        self.attempt += 1;
    }

    /// 写入 pending id；禁止用 None 覆盖已有 id（防 HA 重试丢 id → 双日志）
    #[inline]
    pub fn set_pending(&mut self, id: Option<i64>) {
        if id.is_some() {
            self.pending_log_id = id;
        }
    }

    /// 环结束：补记仍为处理中的首败 → 写插件表 → 返回对外错误
    pub async fn finish(mut self, ctx: &HaBillCtx<'_>) -> AppError {
        tracing::info!(
            "[HA] 结束 已尝试={} 最大={} 已耗时={}s 预算={}s 已排除={:?}",
            self.attempt,
            self.max_attempts,
            self.started_at.elapsed().as_secs(),
            self.budget_secs(),
            self.exclude_aids
        );
        if !self.billed && self.first.is_some() {
            if let Some(id) = self.pending_log_id {
                if pending_open(ctx.state, id).await {
                    self.settle_first(ctx).await;
                }
            }
        }
        self.save(ctx.state).await;

        if is_access_side_err(&self.last_err) {
            return self.last_err;
        }
        match self.first {
            Some(f) => match self.last_err {
                AppError::UpstreamHttpError(s, m, h) if s == f.status => {
                    AppError::UpstreamHttpError(s, m, h)
                }
                _ => super::proxy::upstream_fail(f.status, &f.message, None),
            },
            None => self.last_err,
        }
    }

    /// 按首败一次记账（仅 pending 复用；无 pending 则放弃以防双日志）
    async fn settle_first(&mut self, ctx: &HaBillCtx<'_>) {
        if self.billed {
            return;
        }
        if self.pending_log_id.is_none() {
            tracing::error!(
                "[HA] 终态记账缺少 pending_log_id，放弃落库以防双日志 上游YID={}",
                self.first
                    .as_ref()
                    .map(|f| yid_label(f.channel.yid.as_deref()))
                    .unwrap_or("-")
            );
            return;
        }
        let Some(first) = self.first.as_mut() else {
            return;
        };
        let status = first.status;
        let fallback = first.message.clone();
        let channel = first.channel.clone();
        let endpoint = if first.endpoint.is_empty() {
            ctx.ep
        } else {
            first.endpoint.as_str()
        };

        let bill = first.bill.take();
        let prefer_http_status = match &bill {
            Some(b) => b.prefer_status,
            None => Some(status),
        };
        let (pre, gift, is_stream, req, resp_c, up_req, detail, lat, response_body, client) =
            match bill {
                Some(b) => {
                    let body = if b.response_body.is_empty() {
                        fallback.clone()
                    } else {
                        b.response_body
                    };
                    let client = b
                        .client_msg
                        .unwrap_or_else(|| super::proxy::extract_error_message(&body));
                    (
                        b.pre_deducted,
                        b.pre_deduct_gift,
                        b.is_stream,
                        b.request_content,
                        b.response_content,
                        b.upstream_req,
                        b.billing_detail,
                        b.latency_ms,
                        body,
                        client,
                    )
                }
                None => (
                    0.0,
                    0.0,
                    0,
                    String::new(),
                    None,
                    None,
                    None,
                    0,
                    fallback.clone(),
                    super::proxy::extract_error_message(&fallback),
                ),
            };
        let response_content =
            resp_c.or_else(|| (!response_body.is_empty()).then(|| response_body.clone()));

        let _ = super::proxy::record_zero_cost_fail(super::proxy::ZeroCostUpstreamFail {
            state: ctx.state,
            token: ctx.token,
            channel: &channel,
            model: ctx.model,
            prefer_http_status,
            endpoint,
            latency_ms: lat,
            is_stream,
            request_content: req,
            response_body,
            response_content,
            upstream_req_content: up_req,
            billing_detail: detail,
            hint_category: ctx.hint_category,
            pending_log_id: self.pending_log_id,
            billing_model_hint: ctx.billing_model_hint,
            db_model: ctx.db_model,
            client_msg: Some(&client),
            pre_deducted: pre,
            pre_deduct_gift: gift,
        })
        .await;
        self.billed = true;
    }

    fn push_snap(
        &mut self,
        channel: &Channel,
        status: u16,
        error: Option<String>,
        url: &str,
        ms: u32,
        ok: bool,
    ) {
        if channel_is_ha_flag(channel) == 0 {
            return;
        }
        if self.group_aid.is_none() {
            self.group_aid = Some(group_key(channel));
        }
        let yid = channel
            .yid
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_string();
        let n = (self.snaps.len() as u16).saturating_add(1);
        self.snaps.push(HaSnap {
            n,
            yid,
            name: channel.name.clone(),
            status,
            error,
            url: url.to_string(),
            ms,
            ok: u8::from(ok),
        });
    }

    /// 熔断 / exclude；返回是否续试
    fn try_switch(
        &mut self,
        state: &Arc<AppState>,
        channel: &Channel,
        fail_status: u16,
        msg: &str,
    ) -> bool {
        let aid = channel.group_aid.as_deref().unwrap_or("-");
        let yid = yid_label(channel.yid.as_deref());
        let n = self.attempt + 1;

        let mut cont = try_failover(
            state,
            self.failover_on,
            channel.group_aid.as_deref(),
            channel.yid.as_deref(),
            fail_status,
            msg,
            &mut self.exclude_aids,
        );
        if cont && !self.has_budget_for_retry() {
            tracing::info!(
                "[HA] 切换取消(预算不足) 状态码={} 上游YID={} 子渠标识={} 已耗时={}s 预算={}s",
                fail_status,
                yid,
                aid,
                self.started_at.elapsed().as_secs(),
                self.budget_secs()
            );
            cont = false;
        }
        // 本轮已是最后一次尝试：必须在 fail 内 settle，避免落到弱 finish 上下文
        if cont && n >= self.max_attempts {
            tracing::info!(
                "[HA] 切换取消(已达最大尝试) 状态码={} 上游YID={} 子渠标识={} 尝试={}/{}",
                fail_status,
                yid,
                aid,
                n,
                self.max_attempts
            );
            cont = false;
        }

        if cont {
            tracing::info!(
                "[HA] 切换 状态码={} 上游YID={} 子渠标识={} 尝试={}/{} 已排除={} 剩余预算={}s",
                fail_status,
                yid,
                aid,
                n,
                self.max_attempts,
                self.exclude_aids.len(),
                self.remaining_budget().map(|d| d.as_secs()).unwrap_or(0)
            );
        } else {
            tracing::info!(
                "[HA] 停止无切换 状态码={} 上游YID={} 子渠标识={} 尝试={}/{}",
                fail_status,
                yid,
                aid,
                n,
                self.max_attempts
            );
        }
        cont
    }
}

// ── 私有实现 ──────────────────────────────────────────────────

#[inline]
fn is_ha_aid(aid: &str) -> bool {
    aid.starts_with("ha_group_")
}

/// pending 行仍为处理中（status_code=0）才允许补记终态，避免覆盖已落库行
async fn pending_open(state: &AppState, log_id: i64) -> bool {
    let code: Option<i32> = sqlx::query_scalar(
        &state
            .db
            .format_query("SELECT status_code FROM logs WHERE id = ?"),
    )
    .bind(log_id)
    .fetch_optional(&state.db.pool)
    .await
    .ok()
    .flatten();
    matches!(code, Some(0))
}

fn group_key(channel: &Channel) -> String {
    if let Some(aid) = channel.group_aid.as_deref() {
        if let Some(pos) = aid.find("_config_") {
            return aid[..pos].to_string();
        }
        if is_ha_aid(aid) {
            return aid.to_string();
        }
    }
    format!("ha_group_{}", channel.id)
}

#[inline]
pub(crate) fn ha_config_id_from_aid(channel: &Channel) -> Option<i32> {
    let aid = channel.group_aid.as_deref()?;
    if !is_ha_aid(aid) {
        return None;
    }
    let pos = aid.rfind("_config_")?;
    aid[pos + "_config_".len()..].parse().ok()
}

#[inline]
fn resolve_config_id(channel: &Channel) -> Option<i32> {
    ha_config_id_from_aid(channel).or_else(|| channel.preset_id.map(|p| p as i32))
}

#[inline]
fn status_msg(err: &AppError) -> (u16, String) {
    match err {
        AppError::UpstreamHttpError(s, m, _) => (*s, m.clone()),
        AppError::UpstreamError(m) => (502, m.clone()),
        other => (other.http_status(), other.to_string()),
    }
}

fn fail_status_msg(b: &FailBill) -> (u16, String) {
    let raw_status = b
        .prefer_status
        .unwrap_or_else(|| super::proxy::infer_error_status_code_from_str(&b.response_body));
    let status = super::proxy::normalize_error_http_status(raw_status);
    let msg = match b.prefer_status {
        Some(_) => super::proxy::upstream_error_text(status, &b.response_body),
        None => super::proxy::extract_error_message(&b.response_body),
    };
    (status, msg)
}

/// 由暂存账单构造上游错误（不写库）
fn err_of(bill: &FailBill, headers: Option<axum::http::HeaderMap>) -> AppError {
    let (status, msg) = fail_status_msg(bill);
    let raw = bill.client_msg.as_deref().unwrap_or(&msg);
    super::proxy::upstream_fail(status, raw, headers)
}

/// 业务侧错误：禁止 HA failover
#[inline]
fn is_access_side_err(err: &AppError) -> bool {
    match err {
        AppError::PaymentRequired(_)
        | AppError::Forbidden(_)
        | AppError::BadRequest(_)
        | AppError::Unauthorized
        | AppError::AuthFailed(_) => true,
        AppError::Internal(m) if m.contains("预扣费") => true,
        _ => false,
    }
}

/// 上游客户端侧 HTTP：仍切换，但不熔断
#[inline]
fn is_client_side_http_status(status: u16) -> bool {
    matches!(status, 400 | 402 | 403 | 422)
}

/// 上游把请求形态本身拒了（体积超限 / 媒体类型不支持）：换一家同样失败
#[inline]
fn is_passthrough_http_status(status: u16) -> bool {
    matches!(status, 413 | 415)
}

/// 仅 HA 子渠且策略开启时 exclude（并可能熔断）；黑名单命中则停止切换；400/402/403/422 只切换不熔断；413/415 直返客户
fn try_failover(
    state: &Arc<AppState>,
    failover_on: bool,
    group_aid: Option<&str>,
    yid: Option<&str>,
    status: u16,
    err_msg: &str,
    exclude_aids: &mut Vec<String>,
) -> bool {
    if !failover_on {
        return false;
    }
    let Some(aid) = group_aid.filter(|a| is_ha_aid(a)) else {
        return false;
    };
    let yid_disp = yid_label(yid);

    if let Some(pattern) = match_err_keywords(err_msg, &state.ha_meltdown_blacklist) {
        tracing::info!(
            "[HA] 黑名单停止切换 状态码={} 上游YID={} 子渠标识={} 关键词={}",
            status,
            yid_disp,
            aid,
            pattern
        );
        return false;
    }

    if is_passthrough_http_status(status) {
        tracing::info!(
            "[HA] 直返客户错误 状态码={} 上游YID={} 子渠标识={}（不切换、不熔断）",
            status,
            yid_disp,
            aid
        );
        return false;
    }

    if is_client_side_http_status(status) {
        tracing::info!(
            "[HA] 切换跳过熔断 状态码={} 上游YID={} 子渠标识={}（上游客户端侧错误）",
            status,
            yid_disp,
            aid
        );
    } else {
        trigger_ha_meltdown(state, aid, status, err_msg, yid);
    }
    if !exclude_aids.iter().any(|a| a == aid) {
        exclude_aids.push(aid.to_string());
    }
    true
}

fn match_err_keywords(
    error_message: &str,
    list: &std::sync::RwLock<Vec<String>>,
) -> Option<String> {
    if error_message.is_empty() {
        return None;
    }
    let Ok(patterns) = list.read() else {
        return None;
    };
    if patterns.is_empty() {
        return None;
    }
    let err_lower = error_message.to_lowercase();
    patterns
        .iter()
        .find(|p| !p.is_empty() && err_lower.contains(p.as_str()))
        .cloned()
}

fn trigger_ha_meltdown(
    state: &AppState,
    group_aid: &str,
    status_code: u16,
    error_message: &str,
    yid: Option<&str>,
) {
    if !is_ha_aid(group_aid) {
        return;
    }

    use std::sync::atomic::Ordering;

    let yid = yid_label(yid);

    if let Some(pattern) = match_err_keywords(error_message, &state.ha_meltdown_whitelist) {
        tracing::info!(
            "[HA] 白名单跳过熔断 上游YID={} 子渠标识={} 关键词={}",
            yid,
            group_aid,
            pattern
        );
        return;
    }

    let cooldown = match status_code {
        429 => state.ha_cooldown_429.load(Ordering::Relaxed),
        401 | 402 => state.ha_cooldown_auth.load(Ordering::Relaxed),
        404 => state.ha_cooldown_404.load(Ordering::Relaxed),
        _ => state.ha_cooldown_network.load(Ordering::Relaxed),
    };

    if cooldown > 0 {
        let block_until = Instant::now() + Duration::from_secs(cooldown.max(0) as u64);
        state
            .failed_channels
            .insert(group_aid.to_string(), block_until);
        tracing::info!(
            "[HA] 熔断 上游YID={} 子渠标识={} 状态码={} 冷却={}秒",
            yid,
            group_aid,
            status_code,
            cooldown
        );
    }

    static CLEANUP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = CLEANUP_COUNTER.fetch_add(1, Ordering::Relaxed);
    if n % 32 == 0 || state.failed_channels.len() > 2048 {
        scrub_failed_channels(state);
    }
}

/// 组内绑定级策略的一条：任一字段可缺省，缺省字段各自独立回落上游全局值。
#[derive(Debug, Clone, Copy, Default)]
pub struct SubBindingPolicy {
    pub priority: Option<i32>,
    pub weight: Option<i32>,
}

/// `channels.config.sub_channel_policy` 的解析结果；key 为 `channel_configs.id` 的十进制字符串。
/// 无键 / 畸形一律退化为空表，调用方按上游全局值回落，请求绝不因此失败。
#[derive(Debug, Clone, Default)]
pub struct SubChannelPolicy {
    bindings: HashMap<String, SubBindingPolicy>,
}

impl SubChannelPolicy {
    /// 组内优先级；policy 缺键或缺字段或类型畸形时回落 `fallback`（上游全局 priority）。
    pub fn priority_of(&self, config_id: i64, fallback: i32) -> i32 {
        self.binding(config_id)
            .and_then(|b| b.priority)
            .unwrap_or(fallback)
    }

    /// 组内权重；回落规则同 [`SubChannelPolicy::priority_of`]。
    pub fn weight_of(&self, config_id: i64, fallback: i32) -> i32 {
        self.binding(config_id).and_then(|b| b.weight).unwrap_or(fallback)
    }

    pub(crate) fn binding(&self, config_id: i64) -> Option<SubBindingPolicy> {
        self.bindings.get(&config_id.to_string()).copied()
    }
}

/// 解析 HA 组 `config` 里的 `sub_channel_policy`。逐条目容错：单条畸形只丢该条，不影响同组其它绑定。
pub fn parse_sub_channel_policy(channel_config: &str) -> SubChannelPolicy {
    let parsed = match serde_json::from_str::<serde_json::Value>(channel_config) {
        Ok(v) => v,
        Err(_) => return SubChannelPolicy::default(),
    };
    let policy = match parsed.get("sub_channel_policy").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return SubChannelPolicy::default(),
    };
    let mut bindings = HashMap::with_capacity(policy.len());
    for (key, entry) in policy {
        let item = match entry.as_object() {
            Some(o) => o,
            None => continue,
        };
        bindings.insert(
            key.clone(),
            SubBindingPolicy {
                priority: read_policy_i32(item, "priority"),
                weight: read_policy_i32(item, "weight"),
            },
        );
    }
    SubChannelPolicy { bindings }
}

/// 整数容错读取：非整数或超出 i32 范围 -> None，该字段独立回落上游全局值。
fn read_policy_i32(
    item: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<i32> {
    let n = match item.get(field) {
        Some(v) => v,
        None => return None,
    };
    match n.as_i64() {
        Some(value) => i32::try_from(value).ok(),
        None => None,
    }
}
