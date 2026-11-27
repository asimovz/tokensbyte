/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

use crate::time_system::DbTs;
use serde::{Deserialize, Serialize};

fn default_rate() -> f64 {
    1.0
}

fn default_weight() -> i32 {
    1
}

fn default_status() -> i32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ChannelConfig {
    pub id: i64,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub remark: Option<String>,
    pub created_at: DbTs,
    pub updated_at: DbTs,
    #[sqlx(default)]
    pub yid: String,
    #[sqlx(default)]
    pub sort_order: i32,
    pub rate: f64,
    #[sqlx(default)]
    pub priority: i32,
    #[sqlx(default)]
    pub weight: i32,
    #[sqlx(default)]
    pub quota_limit: f64,
    #[sqlx(default)]
    pub quota_used: f64,
    #[sqlx(default)]
    pub daily_quota_limit: f64,
    #[sqlx(default)]
    pub daily_quota_used: f64,
    #[sqlx(default)]
    pub weekly_quota_limit: f64,
    #[sqlx(default)]
    pub weekly_quota_used: f64,
    #[sqlx(default)]
    pub monthly_quota_limit: f64,
    #[sqlx(default)]
    pub monthly_quota_used: f64,
    #[sqlx(default)]
    pub last_reset_day: String,
    #[sqlx(default)]
    pub last_reset_week: String,
    #[sqlx(default)]
    pub last_reset_month: String,
    /// 日额度刷新时（0-23），站点时区；默认 0 即自然日 00:00
    #[sqlx(default)]
    pub daily_reset_hour: i32,
    /// 日额度刷新分（0-59）
    #[sqlx(default)]
    pub daily_reset_minute: i32,
    /// 到达刷新时刻后再冷却多少分钟才真正刷新日已用（0=立即）
    #[sqlx(default)]
    pub daily_reset_cooldown_minutes: i32,
    /// 1=启用, 0=禁用（迁移后列恒有值；缺列时 sqlx 回退为 0）
    #[sqlx(default)]
    pub status: i32,
    /// 上游分类（复用 channel_categories）
    #[sqlx(default)]
    pub category_id: Option<i64>,
    /// 上游系统：兼容 / 官方 / newapi / akeapi / 火山引擎 / 阿里云，空=未选
    #[sqlx(default)]
    pub upstream_system: String,
    /// 已选同步分组名
    #[sqlx(default)]
    pub upstream_group: String,
    /// 自动同步间隔分钟，0=关闭
    #[sqlx(default)]
    pub upstream_sync_interval_minutes: i32,
    /// 同步时叠加到分组倍率的增量，0=不叠加
    #[sqlx(default)]
    pub upstream_sync_rate_add: f64,
    /// 上次成功同步时间
    #[sqlx(default)]
    pub upstream_synced_at: Option<DbTs>,
}

impl ChannelConfig {
    /// 按本配置的刷新时刻 + 冷却，计算当前额度日键
    pub fn quota_day_key(&self, tz_name: &str) -> String {
        crate::time_system::quota_day_key_with_cutover(
            tz_name,
            self.daily_reset_hour,
            self.daily_reset_minute,
            self.daily_reset_cooldown_minutes,
        )
    }

    pub fn has_available_quota(&self, tz_name: &str, now_week: &str, now_month: &str) -> bool {
        let now_day = self.quota_day_key(tz_name);
        crate::models::channel_quota::has_available_quota(
            self.quota_limit,
            self.quota_used,
            self.daily_quota_limit,
            self.daily_quota_used,
            &self.last_reset_day,
            &now_day,
            self.weekly_quota_limit,
            self.weekly_quota_used,
            &self.last_reset_week,
            now_week,
            self.monthly_quota_limit,
            self.monthly_quota_used,
            &self.last_reset_month,
            now_month,
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateChannelConfigRequest {
    pub name: String,
    #[serde(default)]
    pub provider_type: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    pub remark: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default = "default_rate")]
    pub rate: f64,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_weight")]
    pub weight: i32,
    pub quota_limit: Option<f64>,
    pub daily_quota_limit: Option<f64>,
    pub weekly_quota_limit: Option<f64>,
    pub monthly_quota_limit: Option<f64>,
    /// 日额度刷新时（0-23）
    pub daily_reset_hour: Option<i32>,
    /// 日额度刷新分（0-59）
    pub daily_reset_minute: Option<i32>,
    /// 刷新冷却分钟（0=立即）
    pub daily_reset_cooldown_minutes: Option<i32>,
    /// 1=启用, 0=禁用；缺省为启用
    #[serde(default = "default_status")]
    pub status: i32,
    pub category_id: Option<i64>,
    #[serde(default)]
    pub upstream_system: String,
    #[serde(default)]
    pub upstream_group: String,
    #[serde(default)]
    pub upstream_sync_interval_minutes: i32,
    #[serde(default)]
    pub upstream_sync_rate_add: f64,
}

#[derive(Debug, Deserialize)]
pub struct UpdateChannelConfigRequest {
    pub name: Option<String>,
    pub provider_type: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub remark: Option<String>,
    pub sort_order: Option<i32>,
    pub rate: Option<f64>,
    pub priority: Option<i32>,
    pub weight: Option<i32>,
    pub quota_limit: Option<f64>,
    pub daily_quota_limit: Option<f64>,
    pub weekly_quota_limit: Option<f64>,
    pub monthly_quota_limit: Option<f64>,
    /// 日额度刷新时（0-23）
    pub daily_reset_hour: Option<i32>,
    /// 日额度刷新分（0-59）
    pub daily_reset_minute: Option<i32>,
    /// 刷新冷却分钟（0=立即）
    pub daily_reset_cooldown_minutes: Option<i32>,
    /// 1=启用, 0=禁用
    pub status: Option<i32>,
    /// None = 未传不改；Some(None) = 清空；Some(Some(id)) = 设置
    #[serde(
        default,
        deserialize_with = "crate::models::user::deserialize_some_option"
    )]
    pub category_id: Option<Option<i64>>,
    pub upstream_system: Option<String>,
    pub upstream_group: Option<String>,
    pub upstream_sync_interval_minutes: Option<i32>,
    pub upstream_sync_rate_add: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ChannelConfigListResponse {
    pub data: Vec<ChannelConfigSafe>,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct BoundChannelSummary {
    pub id: i64,
    pub name: String,
    pub group_aid: Option<String>,
    pub is_ha: bool,
    pub status: i32,
    pub effective_priority: i32,
    pub effective_weight: i32,
    pub custom_priority: Option<i32>,
    pub custom_weight: Option<i32>,
    pub has_custom_schedule: bool,
}

#[derive(Debug, Serialize)]
pub struct ChannelConfigSafe {
    pub id: i64,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub api_key: String,
    pub has_api_key: bool,
    pub remark: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub yid: String,
    pub sort_order: i32,
    pub rate: f64,
    pub priority: i32,
    pub weight: i32,
    pub quota_limit: f64,
    pub quota_used: f64,
    pub daily_quota_limit: f64,
    pub daily_quota_used: f64,
    pub weekly_quota_limit: f64,
    pub weekly_quota_used: f64,
    pub monthly_quota_limit: f64,
    pub monthly_quota_used: f64,
    pub last_reset_day: String,
    pub last_reset_week: String,
    pub last_reset_month: String,
    pub daily_reset_hour: i32,
    pub daily_reset_minute: i32,
    pub daily_reset_cooldown_minutes: i32,
    pub status: i32,
    pub category_id: Option<i64>,
    pub upstream_system: String,
    pub upstream_group: String,
    pub upstream_sync_interval_minutes: i32,
    pub upstream_sync_rate_add: f64,
    pub upstream_synced_at: Option<String>,
    pub bound_channel_count: i64,
    pub bound_channels: Vec<BoundChannelSummary>,
}

impl ChannelConfigSafe {
    /// 根据用户角色构建安全的响应数据
    /// - 管理员：返回原文密钥，前端通过 Input.Password 眼睛图标控制显隐
    /// - 非管理员：返回脱敏密钥，保证数据安全
    pub fn from_with_role(c: ChannelConfig, is_admin: bool) -> Self {
        let key = if is_admin {
            c.api_key.clone()
        } else {
            crate::models::channel::mask_secret(&c.api_key)
        };
        ChannelConfigSafe {
            id: c.id,
            name: c.name,
            provider_type: c.provider_type,
            base_url: c.base_url,
            has_api_key: !c.api_key.is_empty(),
            api_key: key,
            remark: c.remark,
            created_at: c.created_at.into_string(),
            updated_at: c.updated_at.into_string(),
            yid: c.yid,
            sort_order: c.sort_order,
            rate: c.rate,
            priority: c.priority,
            weight: c.weight,
            quota_limit: c.quota_limit,
            quota_used: c.quota_used,
            daily_quota_limit: c.daily_quota_limit,
            daily_quota_used: c.daily_quota_used,
            weekly_quota_limit: c.weekly_quota_limit,
            weekly_quota_used: c.weekly_quota_used,
            monthly_quota_limit: c.monthly_quota_limit,
            monthly_quota_used: c.monthly_quota_used,
            last_reset_day: c.last_reset_day,
            last_reset_week: c.last_reset_week,
            last_reset_month: c.last_reset_month,
            daily_reset_hour: c.daily_reset_hour,
            daily_reset_minute: c.daily_reset_minute,
            daily_reset_cooldown_minutes: c.daily_reset_cooldown_minutes,
            status: c.status,
            category_id: c.category_id,
            upstream_system: c.upstream_system,
            upstream_group: c.upstream_group,
            upstream_sync_interval_minutes: c.upstream_sync_interval_minutes,
            upstream_sync_rate_add: c.upstream_sync_rate_add,
            upstream_synced_at: c.upstream_synced_at.map(|t| t.into_string()),
            bound_channel_count: 0,
            bound_channels: Vec::new(),
        }
    }
}
