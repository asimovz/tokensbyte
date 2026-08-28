/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

use std::sync::Arc;

mod api;
mod auth;
mod config;
mod db;
mod error;
mod middleware;
mod models;
mod money;
mod providers;
mod relay;
mod services;
mod time_system;
//start patch @bobcat
mod patch;
//end patch

use config::AppConfig;
use db::Database;

pub struct DashboardCacheEntry {
    pub stats: models::DashboardStats,
    pub timestamp: std::time::Instant,
}

pub struct AppState {
    pub db: Database,
    pub config: AppConfig,
    pub http_client: reqwest::Client,
    pub rate_limiter: middleware::rate_limit::GlobalRateLimiter,
    /// OAuth / 代入登录一次性兑换码：code → (jwt, expires_at)
    pub login_codes: dashmap::DashMap<String, (String, std::time::Instant)>,
    pub icon_sync_progress: api::plugins::site_icons::SyncProgress,
    pub dashboard_cache: dashmap::DashMap<String, DashboardCacheEntry>,
    // 高可用熔断与配置缓存
    pub failed_channels: dashmap::DashMap<String, std::time::Instant>,
    pub ha_max_retries: std::sync::atomic::AtomicI64,
    pub ha_cooldown_429: std::sync::atomic::AtomicI64,
    pub ha_cooldown_network: std::sync::atomic::AtomicI64,
    pub ha_cooldown_auth: std::sync::atomic::AtomicI64,
    pub ha_cooldown_404: std::sync::atomic::AtomicI64,
    /// HA 整次请求墙钟预算（秒）；0=自动 min(540, 上游超时-60)
    pub ha_total_timeout_secs: std::sync::atomic::AtomicI64,
    pub ha_meltdown_whitelist: std::sync::RwLock<Vec<String>>,
    pub ha_meltdown_blacklist: std::sync::RwLock<Vec<String>>,
    /// 级联阶段二进行中互斥（log_id → ()），防并发轮询重复裁剪/超分
    pub cascade_s2_inflight: dashmap::DashMap<i64, ()>,
    /// 日限额内存拦截器（DashMap + DB hydration）
    pub quota_memory: relay::quota_memory::MemoryQuotaGuard,
    /// 异步计费事件投递（Worker 批量刷库；停机时 drain）
    pub billing_ingress: relay::billing_pipeline::BillingIngress,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    // timesystem：进程级锁定 UTC+0（须早于任何 Local/日志时区依赖）
    time_system::enforce_process_utc();
    // 关于页运行时「启动时间」锚点
    services::runtime_info::mark_process_start();

    // 1. 初始化控制台标准输出写入器
    let (non_blocking_stdout, _stdout_guard) = tracing_appender::non_blocking(std::io::stdout());
    let stdout_layer = tracing_subscriber::fmt::layer().with_writer(non_blocking_stdout);

    // 2. 根据是否存在 LOG_DIR 环境变量，动态判断是否开启按天滚动文件写入
    let mut file_layer = None;
    let mut _file_guard = None;

    if let Ok(log_dir) = std::env::var("LOG_DIR") {
        let file_appender = tracing_appender::rolling::daily(&log_dir, "app.log");
        let (non_blocking_file, guard) = tracing_appender::non_blocking(file_appender);
        _file_guard = Some(guard);

        file_layer = Some(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking_file)
                .with_ansi(false),
        );
    }

    // 3. 配置过滤器与组合 Layer
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::EnvFilter;

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();

    let config_data = AppConfig::from_env();
    let database = Database::new(&config_data.database_url).await?;
    database.run_migrations().await?;
    //start patch @bobcat
    crate::patch::run_migrations(&database.pool).await?;
    //end patch

    // 同步 REGISTER_ENABLED 环境变量到数据库设置
    sync_registration_settings(&database, config_data.register_enabled).await?;

    // 优雅关闭信号广播通道：所有后台任务监听此信号，收到后完成当前工作并退出
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // 计费 MPSC Worker：与 shutdown 联动，退出前排空管道再刷库
    let billing_pipeline = relay::billing_pipeline::BillingPipelineHandle::start(
        database.clone(),
        shutdown_rx.clone(),
    );
    let billing_ingress = billing_pipeline.ingress();

    let state = Arc::new(AppState {
        db: database,
        config: config_data.clone(),
        http_client: services::http_client::build_outbound_client(),
        rate_limiter: middleware::rate_limit::GlobalRateLimiter::new(),
        login_codes: dashmap::DashMap::new(),
        icon_sync_progress: api::plugins::site_icons::SyncProgress::new(),
        dashboard_cache: dashmap::DashMap::new(),
        failed_channels: dashmap::DashMap::new(),
        ha_max_retries: std::sync::atomic::AtomicI64::new(3),
        ha_cooldown_429: std::sync::atomic::AtomicI64::new(60),
        ha_cooldown_network: std::sync::atomic::AtomicI64::new(300),
        ha_cooldown_auth: std::sync::atomic::AtomicI64::new(1800),
        ha_cooldown_404: std::sync::atomic::AtomicI64::new(3),
        ha_total_timeout_secs: std::sync::atomic::AtomicI64::new(0),
        ha_meltdown_whitelist: std::sync::RwLock::new(Vec::new()),
        ha_meltdown_blacklist: std::sync::RwLock::new(Vec::new()),
        cascade_s2_inflight: dashmap::DashMap::new(),
        quota_memory: relay::quota_memory::MemoryQuotaGuard::new(),
        billing_ingress,
    });

    let mut bg_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    // 1. 启动时恢复：处理上次中断遗留的"处理中"日志，退还预扣费（直接同步执行）
    relay::proxy::recover_interrupted_logs(&state).await;

    // 启动时预热：从数据库加载火山方舟视频监控的调试日志开关状态
    #[cfg(feature = "commercial_plugins")]
    crate::services::volc_ark_monitor::load_debug_log_config(&state).await;

    // 2. 异步加载高可用配置
    bg_handles.push({
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(e) = state_clone.load_ha_configs().await {
                tracing::error!("加载高可用插件配置失败: {}", e);
            }
        })
    });

    // 3. 启动后台异步任务轮询器（周期见 RelaySettings.poll_tick_secs，缓存；检查未结算视频/图片任务）
    bg_handles.push(relay::task::start(state.clone(), shutdown_rx.clone()));

    // 4. 实时指标冷用户清理（每 5 分钟，空闲 >1h 剔除）
    bg_handles.push(tokio::spawn(middleware::live_metrics::run_cleanup_loop(
        shutdown_rx.clone(),
    )));

    // 4b. 看板缓存 TTL 清理（每 5 分钟，条目 >30min 剔除）
    bg_handles.push(tokio::spawn(
        api::dashboard::run_dashboard_cache_cleanup_loop(state.clone(), shutdown_rx.clone()),
    ));

    bg_handles.push(spawn_cron_task(
        state.clone(),
        shutdown_rx.clone(),
        60,
        "UpstreamRateSync",
        |s| async move {
            api::channel_configs::run_upstream_rate_sync_tick(s).await;
        },
    ));

    // 5. 启动孤儿日志清理定时任务（每 5 分钟检查 status_code=0 超过 30 分钟的日志）
    bg_handles.push(spawn_cron_task(
        state.clone(),
        shutdown_rx.clone(),
        300,
        "OrphanLogsCleanup",
        |s| async move {
            relay::proxy::cleanup_orphan_pending_logs(&s).await;
        },
    ));

    // 6. 每日维护（下一次 UTC 03:00）：日志大字段清理/归档 + TOS 过期 + 火山素材保留清理
    bg_handles.push({
        let state_clone = state.clone();
        let mut rx = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                let wait = duration_until_next_utc_hms(3, 0, 0);
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {
                        cleanup_log_content(&state_clone).await;
                        archive_old_logs(&state_clone).await;
                        relay::tos_persist::cleanup_expired_files(&state_clone).await;
                        #[cfg(feature = "commercial_plugins")]
                        api::plugins::assets::cleanup_expired_volc_assets(&state_clone).await;
                    }
                    _ = rx.changed() => return,
                }
            }
        })
    });

    // 7. 创作中心画布中断节点自动恢复（每 5 分钟；两版画布同 tick）
    #[cfg(feature = "commercial_plugins")]
    bg_handles.push(spawn_cron_task(
        state.clone(),
        shutdown_rx.clone(),
        300,
        "PlaygroundNodesCleanup",
        |s| async move {
            api::plugins::playground::cleanup_stale_playground_nodes(&s).await;
            api::plugins::playground_2026::cleanup_stale_playground_2026_nodes(&s).await;
        },
    ));

    // 8. 启动时检查站点图标文件完整性，缺失则自动恢复（一次性任务）
    bg_handles.push({
        let state_clone = state.clone();
        tokio::spawn(async move {
            api::plugins::site_icons::auto_recover_on_startup(state_clone).await;
        })
    });

    // 9. 启动时在后台静默执行历史数据回填落地（一次性任务）
    bg_handles.push({
        let state_clone = state.clone();
        tokio::spawn(async move {
            // 稍稍休眠几秒，给应用服务器充分启动的时间，再开始历史回填
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            if let Err(e) =
                relay::usage_stats::backfill_usage_daily_stats_on_startup(&state_clone).await
            {
                tracing::error!(
                    "❌ [StartupBackfill] 启动初始化使用量每日统计表失败: {:?}",
                    e
                );
            }
        })
    });

    // 10. 每日日志增量统计（睡到站点时区次日 00:00，避免每 5 分钟空转）
    bg_handles.push(tokio::spawn(relay::usage_stats::run_daily_stats_loop(
        state.clone(),
        shutdown_rx.clone(),
    )));

    // 11. 火山方舟视频监控：同步视频列表 + 分账账单 + 超额熔断（每 1 分钟）
    #[cfg(feature = "commercial_plugins")]
    bg_handles.push(spawn_cron_task(
        state.clone(),
        shutdown_rx.clone(),
        60,
        "VolcArkMonitorSync",
        |s| async move {
            if let Err(e) = services::volc_ark_monitor::run_sync(s).await {
                tracing::error!("❌ [CronArkMonitor] 火山方舟视频监控同步失败: {:?}", e);
            }
        },
    ));

    let assets_dir = config_data.assets_dir.clone();
    let portal_dir = config_data.portal_dir.clone();
    let portal_pro_dir = config_data.portal_pro_dir.clone();
    // 确保 portal 目录存在
    std::fs::create_dir_all(&portal_dir).ok();
    std::fs::create_dir_all(&portal_pro_dir).ok();
    // 确保 assets 目录存在
    std::fs::create_dir_all(&assets_dir).ok();
    let app = api::build_router(state.clone())
        .nest_service("/portal", tower_http::services::ServeDir::new(&portal_dir))
        .nest_service(
            "/portal-pro",
            tower_http::services::ServeDir::new(&portal_pro_dir),
        )
        .nest_service("/assets", tower_http::services::ServeDir::new(&assets_dir));

    let addr = format!("{}:{}", config_data.host, config_data.port);
    eprintln!("⚙️ Binding server to {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("🚀 TokensByte server running at http://{}", addr);
    eprintln!(
        "✅ TokensByte server is now completely online at http://{} !",
        addr
    );

    // 优雅关闭：仅显式 APP_ENV=dev|development 时秒退；默认按生产优雅 drain
    // 禁止用 CARGO_MANIFEST_DIR 推断（cargo run 注入会误伤生产）
    let is_dev = matches!(
        std::env::var("APP_ENV")
            .or_else(|_| std::env::var("RUN_MODE"))
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "dev" | "development"
    );
    if is_dev {
        tracing::warn!("⚡ APP_ENV=dev：关闭信号将秒退，跳过计费管道 drain（勿用于生产）");
        drop(billing_pipeline);
        let shutdown_signal_dev = async move {
            wait_for_shutdown_signal().await;
            tracing::info!("⚡ 开发环境收到关闭信号，强制秒退释放端口...");
            std::process::exit(0);
        };

        tokio::select! {
            res = axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()) => {
                res?;
            }
            _ = shutdown_signal_dev => {}
        }
    } else {
        let shutdown_signal = async move {
            wait_for_shutdown_signal().await;
            tracing::info!("⏳ 收到关闭信号，停止接受新请求，等待进行中的任务完成...");
            let _ = shutdown_tx.send(true);
        };

        let serve_future = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal);

        let mut serve_shutdown_rx = shutdown_rx.clone();
        tokio::select! {
            res = serve_future => {
                res?;
            }
            _ = async {
                while !*serve_shutdown_rx.borrow() {
                    if serve_shutdown_rx.changed().await.is_err() {
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                tracing::warn!("⚠️ HTTP 服务未在 30 秒内优雅关闭，强制结束连接");
            } => {}
        }

        // HTTP 服务已关闭，等待后台任务完成（包括正在轮询中的异步任务）
        tracing::info!("⏳ HTTP 连接已关闭，等待后台任务完成...");
        let wait_bg = async {
            for h in bg_handles {
                let _ = h.await;
            }
            // 计费管道：shutdown 已发出，Worker 排空 MPSC 剩余事件并刷库
            billing_pipeline.join().await;
        };
        match tokio::time::timeout(std::time::Duration::from_secs(30), wait_bg).await {
            Ok(_) => tracing::info!("✅ 所有后台任务已完成，服务安全退出"),
            Err(_) => tracing::warn!("⚠️ 部分后台任务未在 30 秒内完成，强制退出"),
        }
    }

    Ok(())
}

async fn fetch_storage_settings(state: &AppState) -> models::StorageSettings {
    match sqlx::query_scalar::<_, String>(
        &state
            .db
            .format_query("SELECT value FROM settings WHERE key = ?"),
    )
    .bind("storage_settings")
    .fetch_optional(&state.db.pool)
    .await
    {
        Ok(Some(val)) => serde_json::from_str::<models::StorageSettings>(&val).unwrap_or_default(),
        _ => models::StorageSettings::default(),
    }
}

/// 清理超期日志的大字段内容（request_content / response_content / upstream_req_content）
/// 仅置 NULL，不删除日志记录，统计数据不受影响
async fn cleanup_log_content(state: &AppState) {
    let retention_days = fetch_storage_settings(state).await.log_retention_days;
    if retention_days <= 0 {
        return;
    }

    // 分批清理，每批 10000 条，避免长事务锁表
    let cleanup_sql = state.db.format_query(
        "UPDATE logs SET request_content = NULL, response_content = NULL, upstream_req_content = NULL, post_response = NULL \
         WHERE id IN (\
            SELECT id FROM logs \
            WHERE created_at < CURRENT_TIMESTAMP - (? * INTERVAL '1 day') \
              AND (request_content IS NOT NULL OR response_content IS NOT NULL OR upstream_req_content IS NOT NULL OR post_response IS NOT NULL) \
            LIMIT 10000\
         )"
    );

    loop {
        let result = sqlx::query(&cleanup_sql)
            .bind(retention_days as f64)
            .execute(&state.db.pool)
            .await;

        match result {
            Ok(r) => {
                let affected = r.rows_affected();
                if affected > 0 {
                    tracing::info!(
                        "🧹 日志清理: 已清理 {} 条超过 {} 天的日志详情",
                        affected,
                        retention_days
                    );
                }
                if affected < 10000 {
                    break;
                }
            }
            Err(e) => {
                tracing::error!("日志清理失败: {}", e);
                break;
            }
        }
    }
}

/// 将超期日志行迁入 logs_archive 并从热表删除（分批；`log_row_retention_days<=0` 时跳过）。
/// jsonb 按列名填充，热表加列后无需再给 archive 做列序体操。
async fn archive_old_logs(state: &AppState) {
    let row_days = fetch_storage_settings(state).await.log_row_retention_days;
    if row_days <= 0 {
        return;
    }

    // 配置天数 +2 缓冲，降低「今日统计未落档就被迁走」的风险
    let effective_days = (row_days as i64).saturating_add(2);
    let archive_sql = state.db.format_query(
        "WITH candidates AS (\
            SELECT id FROM logs \
            WHERE created_at < CURRENT_TIMESTAMP - (? * INTERVAL '1 day') \
            ORDER BY created_at ASC LIMIT 5000\
         ), inserted AS (\
            INSERT INTO logs_archive \
            SELECT p.* FROM logs l \
            JOIN candidates c ON c.id = l.id \
            CROSS JOIN LATERAL jsonb_populate_record(\
                NULL::logs_archive,\
                to_jsonb(l) || jsonb_build_object('archived_at', NOW())\
            ) AS p \
            ON CONFLICT (id) DO NOTHING \
            RETURNING id\
         ) \
         DELETE FROM logs WHERE id IN (\
            SELECT id FROM inserted \
            UNION \
            SELECT c.id FROM candidates c \
            WHERE EXISTS (SELECT 1 FROM logs_archive a WHERE a.id = c.id)\
         )",
    );

    let mut total: u64 = 0;
    loop {
        let result = sqlx::query(&archive_sql)
            .bind(effective_days as f64)
            .execute(&state.db.pool)
            .await;

        match result {
            Ok(r) => {
                let affected = r.rows_affected();
                total += affected;
                if affected < 5000 {
                    break;
                }
            }
            Err(e) => {
                tracing::error!("日志归档失败: {}", e);
                break;
            }
        }
    }

    if total > 0 {
        tracing::info!(
            "📦 日志归档: 已迁入 logs_archive {} 条（保留热表 {}+2 天）",
            total,
            row_days
        );
    }
}

/// 将 REGISTER_ENABLED 环境变量同步到数据库的 registration_settings
pub(crate) async fn sync_registration_settings(db: &Database, register_enabled: bool) -> anyhow::Result<()> {
    use crate::api::settings::default_registration_settings;

    let existing: Option<String> = sqlx::query_scalar(
        &db.format_query("SELECT value FROM settings WHERE key = 'registration_settings'"),
    )
    .fetch_optional(&db.pool)
    .await?;

    if existing.is_some() {
        return Ok(());
    }

    // 不存在时用默认值
    let mut s = default_registration_settings();
    s.enable_username_registration = register_enabled;
    s.enable_email_registration = false;
    s.enable_password_recovery = true;

    let val = serde_json::to_string(&s).unwrap_or_default();
    sqlx::query(&db.format_query("INSERT INTO settings (key, value) VALUES ('registration_settings', ?) ON CONFLICT(key) DO NOTHING"))
        .bind(val)
        .execute(&db.pool)
        .await?;

    tracing::info!(
        "📝 Registration settings initialized: username={}, email={}, mobile={}, password_recovery={}",
        s.enable_username_registration,
        s.enable_email_registration,
        s.enable_mobile_registration,
        s.enable_password_recovery,
    );

    Ok(())
}

impl AppState {
    pub async fn load_ha_configs(&self) -> anyhow::Result<()> {
        let configs: Vec<(String, String)> = match sqlx::query_as(
            "SELECT config_key, config_value FROM plugin_configs WHERE plugin_name = 'high_availability_channel'"
        )
        .fetch_all(&self.db.pool)
        .await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("获取高可用配置失败，使用默认配置: {}", e);
                return Ok(());
            }
        };

        for (key, val) in configs {
            if key == "ha_meltdown_whitelist" || key == "ha_meltdown_blacklist" {
                // 名单：JSON 数组 → trim + 小写；解析失败视为空（避免脏数据残留）
                let lowered = serde_json::from_str::<Vec<String>>(&val)
                    .map(|list| {
                        list.into_iter()
                            .map(|s| s.trim().to_lowercase())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let slot = if key == "ha_meltdown_whitelist" {
                    &self.ha_meltdown_whitelist
                } else {
                    &self.ha_meltdown_blacklist
                };
                if let Ok(mut guard) = slot.write() {
                    *guard = lowered;
                }
                continue;
            }
            if let Ok(parsed_val) = val.parse::<i64>() {
                match key.as_str() {
                    "ha_max_retries" => self
                        .ha_max_retries
                        .store(parsed_val, std::sync::atomic::Ordering::Relaxed),
                    "ha_cooldown_429" => self
                        .ha_cooldown_429
                        .store(parsed_val, std::sync::atomic::Ordering::Relaxed),
                    "ha_cooldown_network" => self
                        .ha_cooldown_network
                        .store(parsed_val, std::sync::atomic::Ordering::Relaxed),
                    "ha_cooldown_auth" => self
                        .ha_cooldown_auth
                        .store(parsed_val, std::sync::atomic::Ordering::Relaxed),
                    "ha_cooldown_404" => self
                        .ha_cooldown_404
                        .store(parsed_val, std::sync::atomic::Ordering::Relaxed),
                    "ha_total_timeout_secs" => self
                        .ha_total_timeout_secs
                        .store(parsed_val.max(0), std::sync::atomic::Ordering::Relaxed),
                    _ => {}
                }
            }
        }
        tracing::info!("高可用插件配置加载成功");
        Ok(())
    }

    /// 清空数据库后：丢掉旧业务内存态，不杀进程，前端可立刻进入全新安装
    pub async fn reset_runtime_after_db_wipe(&self) {
        self.login_codes.clear();
        self.dashboard_cache.clear();
        self.failed_channels.clear();
        self.cascade_s2_inflight.clear();
        self.quota_memory.clear_all();
        crate::relay::relay_settings::invalidate_all();
        self.ha_max_retries
            .store(3, std::sync::atomic::Ordering::Relaxed);
        self.ha_cooldown_429
            .store(60, std::sync::atomic::Ordering::Relaxed);
        self.ha_cooldown_network
            .store(300, std::sync::atomic::Ordering::Relaxed);
        self.ha_cooldown_auth
            .store(1800, std::sync::atomic::Ordering::Relaxed);
        self.ha_cooldown_404
            .store(3, std::sync::atomic::Ordering::Relaxed);
        self.ha_total_timeout_secs
            .store(0, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut g) = self.ha_meltdown_whitelist.write() {
            g.clear();
        }
        if let Ok(mut g) = self.ha_meltdown_blacklist.write() {
            g.clear();
        }
        if let Err(e) = self.load_ha_configs().await {
            tracing::warn!("清空数据库后重新加载高可用配置失败: {e}");
        }
    }
}

/// 距下一次 UTC 整点时刻的等待时长（至少 1 秒）。
fn duration_until_next_utc_hms(hour: u32, min: u32, sec: u32) -> std::time::Duration {
    let now = chrono::Utc::now();
    let today = now
        .date_naive()
        .and_hms_opt(hour, min, sec)
        .unwrap()
        .and_utc();
    let next = if now < today {
        today
    } else {
        (now.date_naive() + chrono::Duration::days(1))
            .and_hms_opt(hour, min, sec)
            .unwrap()
            .and_utc()
    };
    (next - now)
        .to_std()
        .unwrap_or(std::time::Duration::from_secs(60))
        .max(std::time::Duration::from_secs(1))
}

/// 抽象的高可用定时任务派发器，统一接管定时休眠、异常捕捉以及优雅退出监听
fn spawn_cron_task<F, Fut>(
    state: Arc<AppState>,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
    interval_secs: u64,
    task_name: &'static str,
    mut job: F,
) -> tokio::task::JoinHandle<()>
where
    F: FnMut(Arc<AppState>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let mut rx = shutdown_rx;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(interval_secs)) => {
                    job(state.clone()).await;
                }
                _ = rx.changed() => {
                    tracing::info!("[CronTask] {} 定时任务已优雅关闭退出", task_name);
                    return;
                }
            }
        }
    })
}

/// 跨平台等待 SIGINT (Ctrl+C) / SIGTERM 关闭信号的异步辅助函数
async fn wait_for_shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("无法注册 SIGTERM 信号处理器");
        tokio::select! {
            _ = ctrl_c => {},
            _ = sigterm.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}
