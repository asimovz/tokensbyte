/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

//! Relay 热路径配置缓存：分槽 TTL；写穿 `put`；miss 回填不盖未过期值。
//! 业务判定（档位/限额）在 [`RelaySettings`]，本模块只做读写缓存。

use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::models::RelaySettings;
use crate::time_system::DEFAULT_TIMEDISPLAY;
use std::future::Future;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(60);
pub const HA_PLUGIN_NAME: &str = "high_availability_channel";
const VIDEO_INFLIGHT_MSG: &str = "当前余额较低，任务过多，请充值";

struct TtlCell<T> {
    value: T,
    at: Instant,
}

struct CacheSlot<T>(OnceLock<RwLock<Option<TtlCell<T>>>>);

impl<T> CacheSlot<T> {
    const fn new() -> Self {
        Self(OnceLock::new())
    }

    fn lock(&self) -> &RwLock<Option<TtlCell<T>>> {
        self.0.get_or_init(|| RwLock::new(None))
    }

    fn put(&self, value: T) {
        if let Ok(mut guard) = self.lock().write() {
            *guard = Some(TtlCell {
                value,
                at: Instant::now(),
            });
        }
    }

    fn clear(&self) {
        if let Ok(mut guard) = self.lock().write() {
            *guard = None;
        }
    }
}

impl<T: Clone> CacheSlot<T> {
    fn get(&self) -> Option<T> {
        let Ok(guard) = self.lock().read() else {
            return None;
        };
        let cell = guard.as_ref()?;
        (Instant::now().duration_since(cell.at) < CACHE_TTL).then(|| cell.value.clone())
    }

    /// 不覆盖未过期条目（写穿 / 并发先填优先）
    fn fill_miss(&self, value: T) -> T {
        let Ok(mut guard) = self.lock().write() else {
            return value;
        };
        if let Some(cell) = guard.as_ref() {
            if Instant::now().duration_since(cell.at) < CACHE_TTL {
                return cell.value.clone();
            }
        }
        let out = value.clone();
        *guard = Some(TtlCell {
            value,
            at: Instant::now(),
        });
        out
    }

    async fn get_or_load<F>(&self, load: F) -> T
    where
        F: Future<Output = T>,
    {
        if let Some(v) = self.get() {
            return v;
        }
        self.fill_miss(load.await)
    }
}

async fn setting_value(db: &Database, key: &str) -> Option<String> {
    sqlx::query_scalar(&db.format_query("SELECT value FROM settings WHERE key = ?"))
        .bind(key)
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten()
}

static RELAY_SETTINGS: CacheSlot<Arc<RelaySettings>> = CacheSlot::new();
static SITE_TZ: CacheSlot<Arc<str>> = CacheSlot::new();
static HA_ENABLED: CacheSlot<bool> = CacheSlot::new();

pub fn put_cached_relay_settings(settings: RelaySettings) {
    RELAY_SETTINGS.put(Arc::new(settings.prepared()));
}

pub async fn get_cached_relay_settings(db: &Database) -> Arc<RelaySettings> {
    RELAY_SETTINGS
        .get_or_load(async {
            let s = setting_value(db, "relay_settings")
                .await
                .and_then(|v| serde_json::from_str::<RelaySettings>(&v).ok())
                .unwrap_or_default();
            Arc::new(s.prepared())
        })
        .await
}

/// 素材上游「按渠道分组推导」灰度开关。复用 `RELAY_SETTINGS` 槽（TTL 60s），
/// 管理端保存 relay_settings 时已写穿缓存，改完即生效不需重启。
pub async fn get_cached_asset_route_by_group(db: &Database) -> bool {
    get_cached_relay_settings(db).await.asset_route_by_group
}

pub fn put_cached_site_timezone(tz: impl AsRef<str>) {
    SITE_TZ.put(Arc::<str>::from(tz.as_ref()));
}

pub async fn get_cached_site_timezone(db: &Database) -> Arc<str> {
    SITE_TZ
        .get_or_load(async {
            let tz = setting_value(db, "site_settings")
                .await
                .and_then(|v| serde_json::from_str::<crate::models::SiteSettings>(&v).ok())
                .map(|s| s.default_timezone)
                .unwrap_or_else(|| DEFAULT_TIMEDISPLAY.to_string());
            Arc::<str>::from(tz)
        })
        .await
}

pub fn put_cached_ha_enabled(enabled: bool) {
    HA_ENABLED.put(enabled);
}

pub fn invalidate_all() {
    RELAY_SETTINGS.clear();
    SITE_TZ.clear();
    HA_ENABLED.clear();
}

pub async fn get_cached_ha_enabled(db: &Database) -> bool {
    HA_ENABLED
        .get_or_load(async {
            let enabled: Option<i64> = sqlx::query_scalar(
                &db.format_query("SELECT is_enabled FROM plugins WHERE name = ?"),
            )
            .bind(HA_PLUGIN_NAME)
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
            enabled == Some(1)
        })
        .await
}

pub async fn enforce_video_inflight_gate(
    db: &Database,
    user_id: &str,
    available: f64,
) -> AppResult<()> {
    let Some(max) = get_cached_relay_settings(db)
        .await
        .max_video_inflight(available)
    else {
        return Ok(());
    };
    let n: i64 = sqlx::query_scalar(
        &db.format_query(
            "SELECT COUNT(*) FROM logs \
             WHERE user_id = ? AND is_completed = 0 \
             AND billing_detail LIKE '%冻结%' \
             AND action_type LIKE '%视频%'",
        ),
    )
    .bind(user_id)
    .fetch_one(&db.pool)
    .await?;
    if n >= i64::from(max) {
        return Err(AppError::TooManyRequests(VIDEO_INFLIGHT_MSG.to_string()));
    }
    Ok(())
}
