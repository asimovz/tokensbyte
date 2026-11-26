/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

use crate::error::AppError;
use crate::time_system::core::{now_utc, parse_timedisplay, try_iana_timezone_name};
use crate::time_system::period::local_calendar_day_bounds;
use crate::AppState;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{Datelike, Duration, Timelike};
use sqlx::Row;
use std::sync::Arc;

use super::usage_xlsx::{build_workbook, dos_stamp, pct_encode, XlsxRow};

/// 口径必须与 skill `quota-report/scripts/dump_detail.sh` 逐字一致，两份数不能漂。
/// 三处插值全是服务端自产：时区名已由 `try_iana_timezone_name` 校验，起止时刻由 chrono 生成。
fn detail_sql(tz: &str, start: &str, end: &str) -> String {
    format!(
        r#"WITH base AS (
  SELECT to_char((l.created_at AT TIME ZONE '{tz}'),'YYYY-MM-DD') AS day,
         u.uid,
         u.username,
         COALESCE(REPLACE(REPLACE(u.admin_remark, E'\n', ' '), E'\r', ' '),'') AS company,
         COALESCE(NULLIF(l.model,''),'(未知模型)') AS model,
         l.cost::numeric AS cost,
         (COALESCE(l.prompt_tokens,0) + COALESCE(l.completion_tokens,0))::bigint AS tok,
         COALESCE((regexp_match(l.billing_detail,'([0-9.]+)倍率\([^)]*折扣\)'))[1]::numeric, 1) AS f,
         COALESCE(cc.name, c.name, '') AS supplier
  FROM logs l
  JOIN users u ON u.id = l.user_id
  LEFT JOIN channels c ON c.id = l.channel_id
  LEFT JOIN channel_configs cc ON cc.id = l.channel_config_id
  WHERE l.created_at >= '{start}'::timestamptz
    AND l.created_at <  '{end}'::timestamptz
),
agg AS (
  SELECT day, uid, username, company, model, supplier,
         ROUND(SUM(cost), 6) AS revenue,
         SUM(tok) AS tokens,
         ROUND(COALESCE(SUM(f*cost) / NULLIF(SUM(cost),0),
                        SUM(f*tok)  / NULLIF(SUM(tok),0),
                        1), 2) AS discount,
         SUM(cost) AS raw_cost
  FROM base
  GROUP BY 1,2,3,4,5,6
)
SELECT a.day,
       a.uid,
       a.username,
       a.company,
       a.model,
       a.discount::text AS discount,
       a.revenue::text AS revenue,
       a.tokens::text AS tokens,
       COALESCE(NULLIF(a.supplier,''), '无渠道记录') AS supplier
FROM agg a
WHERE NOT (a.raw_cost = 0 AND a.tokens = 0 AND COALESCE(a.supplier,'') = '')
ORDER BY a.day,
         SUM(a.raw_cost) OVER (PARTITION BY a.day, a.uid, a.model) DESC,
         a.username, a.model, a.raw_cost DESC"#
    )
}

/// 固定窗口：站点时区昨天 00:00 → 本次点击时刻，与页面上的日期选择器无关。
/// 产物改成 skill「用户 × 模型 × 供应商」明细格式的 xlsx：每个自然日一张 sheet，
/// sheet 名 MM-DD，文件名 {首日}-{末日} 各用户模型用量明细.xlsx。
pub async fn export_usage_detail(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    let tz_cached = crate::relay::relay_settings::get_cached_site_timezone(&state.db).await;
    let tz = try_iana_timezone_name(&tz_cached).unwrap_or_else(|| "Asia/Shanghai".to_string());

    let now = now_utc();
    // 「昨天」必须按站点时区取当天再减一天：now.date_naive() 是 UTC 日历，
    // 在站点 00:00–07:59 那段会退到前天，窗口多出整整一天。
    let local_today = now.with_timezone(&parse_timedisplay(&tz)).date_naive();
    let yesterday = local_calendar_day_bounds(local_today - Duration::days(1), &tz);

    let rows = sqlx::query(&detail_sql(&tz, &yesterday.start_rfc3339, &now.to_rfc3339()))
        .fetch_all(&state.db.pool)
        .await?;

    // SQL 已按 day 排好，同一天连续出现，所以只在换日时另起一张 sheet
    let mut groups: Vec<(String, Vec<XlsxRow>)> = Vec::new();
    for row in rows.iter() {
        let day: Option<String> = row.get(0);
        let uid: Option<String> = row.get(1);
        let username: Option<String> = row.get(2);
        let company: Option<String> = row.get(3);
        let model: Option<String> = row.get(4);
        let discount: Option<String> = row.get(5);
        let revenue: Option<String> = row.get(6);
        let tokens: Option<String> = row.get(7);
        let supplier: Option<String> = row.get(8);

        let day = day.unwrap_or_default();
        let rec = XlsxRow {
            uid: uid.unwrap_or_default(),
            username: username.unwrap_or_default(),
            company: company.unwrap_or_default(),
            model: model.unwrap_or_default(),
            discount: discount.unwrap_or_default(),
            revenue: revenue.unwrap_or_default().parse().unwrap_or(0.0),
            tokens: tokens.unwrap_or_default().parse().unwrap_or(0),
            supplier: supplier.unwrap_or_else(|| "无渠道记录".to_string()),
        };
        match groups.last_mut() {
            Some((cur, list)) if *cur == day => list.push(rec),
            _ => groups.push((day, vec![rec])),
        }
    }

    // 一行都没有时补一张空 sheet，否则 0 sheet 的包不是合法 xlsx
    if groups.is_empty() {
        let empty_day = (local_today - Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        groups.push((empty_day, Vec::new()));
    }

    let (dos_time, dos_date) = dos_stamp(
        now.year(),
        now.month(),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    );
    let out = build_workbook(&groups, dos_time, dos_date);

    let start_day = groups.first().map(|g| g.0.clone()).unwrap_or_default();
    let end_day = groups.last().map(|g| g.0.clone()).unwrap_or_default();
    let filename = format!("{start_day}-{end_day} 各用户模型用量明细.xlsx");
    let ascii_name = format!("usage_detail_{}.xlsx", now.format("%Y%m%d_%H%M%S"));

    tracing::info!(
        sheets = out.sheets,
        rows = out.rows,
        bytes = out.bytes.len(),
        file = %filename,
        "usage detail xlsx built"
    );

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        )
        .header(
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{}",
                pct_encode(&filename)
            ),
        )
        .body(axum::body::Body::from(out.bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}
