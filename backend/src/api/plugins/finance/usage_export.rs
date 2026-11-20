/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

use crate::error::AppError;
use crate::time_system::core::{now_utc, try_iana_timezone_name};
use crate::time_system::period::local_calendar_day_bounds;
use crate::AppState;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::Duration;
use sqlx::Row;
use std::sync::Arc;

/// 列顺序与 quota-report 报表一致；「对应成本」留空，等上游账单回填。
const CSV_HEADER: &str = "\u{FEFF}日期,用户UID,用户名称,公司名称,模型名称,折扣,收入,对应成本,token使用量,对应供应商\n";

fn csv_cell(raw: &str) -> String {
    if raw.contains(',') || raw.contains('"') || raw.contains('\n') || raw.contains('\r') {
        format!("\"{}\"", raw.replace('"', "\"\""))
    } else {
        raw.to_string()
    }
}

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
       COALESCE(NULLIF(a.supplier,''), '无渠道记录') AS supplier,
       (SUM(a.revenue) OVER ())::text AS total_revenue,
       (SUM(a.tokens) OVER ())::text AS total_tokens
FROM agg a
WHERE NOT (a.raw_cost = 0 AND a.tokens = 0 AND COALESCE(a.supplier,'') = '')
ORDER BY a.day,
         SUM(a.raw_cost) OVER (PARTITION BY a.day, a.uid, a.model) DESC,
         a.username, a.model, a.raw_cost DESC"#
    )
}

/// 固定窗口：站点时区「昨天 00:00」→ 本次点击时刻，与页面上的日期选择无关。
pub async fn export_usage_detail(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    let tz_cached = crate::relay::relay_settings::get_cached_site_timezone(&state.db).await;
    let tz = try_iana_timezone_name(&tz_cached).unwrap_or_else(|| "Asia/Shanghai".to_string());

    let now = now_utc();
    let yesterday = local_calendar_day_bounds(now.date_naive() - Duration::days(1), &tz);

    let rows = sqlx::query(&detail_sql(&tz, &yesterday.start_rfc3339, &now.to_rfc3339()))
        .fetch_all(&state.db.pool)
        .await?;

    let mut csv = String::from(CSV_HEADER);
    let mut total_revenue = String::new();
    let mut total_tokens = String::new();
    for (idx, row) in rows.iter().enumerate() {
        let day: Option<String> = row.get(0);
        let uid: Option<String> = row.get(1);
        let username: Option<String> = row.get(2);
        let company: Option<String> = row.get(3);
        let model: Option<String> = row.get(4);
        let discount: Option<String> = row.get(5);
        let revenue: Option<String> = row.get(6);
        let tokens: Option<String> = row.get(7);
        let supplier: Option<String> = row.get(8);
        if idx == 0 {
            total_revenue = row.get::<Option<String>, usize>(9).unwrap_or_default();
            total_tokens = row.get::<Option<String>, usize>(10).unwrap_or_default();
        }
        let cells: Vec<String> = vec![
            day.as_deref().unwrap_or(""),
            uid.as_deref().unwrap_or(""),
            username.as_deref().unwrap_or(""),
            company.as_deref().unwrap_or(""),
            model.as_deref().unwrap_or(""),
            discount.as_deref().unwrap_or(""),
            revenue.as_deref().unwrap_or(""),
            "",
            tokens.as_deref().unwrap_or(""),
            supplier.as_deref().unwrap_or("无渠道记录"),
        ]
        .into_iter()
        .map(csv_cell)
        .collect();
        csv.push_str(&cells.join(","));
        csv.push('\n');
    }
    if !rows.is_empty() {
        csv.push_str(&format!(
            "合计,,,,,{revenue},,{tokens},\n",
            revenue = total_revenue,
            tokens = total_tokens
        ));
    }

    let filename = format!("usage_detail_{}.csv", now.format("%Y%m%d_%H%M%S"));
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(axum::body::Body::from(csv))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}
