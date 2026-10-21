/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

use dashmap::DashMap;
use governor::{state::InMemoryState, state::NotKeyed, Quota, RateLimiter};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct GlobalRateLimiter {
    // Key: TokenID, Value: Limiter (RPS)
    token_rps_limits:
        DashMap<i64, Arc<RateLimiter<NotKeyed, InMemoryState, governor::clock::DefaultClock>>>,
    // Key: TokenID, Value: Limiter (RPM)
    token_rpm_limits:
        DashMap<i64, Arc<RateLimiter<NotKeyed, InMemoryState, governor::clock::DefaultClock>>>,
    /// 登录/管理登录按 IP 限流（每分钟）
    login_ip_limits:
        DashMap<String, Arc<RateLimiter<NotKeyed, InMemoryState, governor::clock::DefaultClock>>>,
    /// 兑换码：IP 短窗计数（次数, 窗口起点）
    redeem_ip_windows: DashMap<String, (u32, Instant)>,
    /// 兑换码：IP 封禁至（Instant）
    redeem_ip_bans: DashMap<String, Instant>,
}

impl GlobalRateLimiter {
    pub fn new() -> Self {
        Self {
            token_rps_limits: DashMap::new(),
            token_rpm_limits: DashMap::new(),
            login_ip_limits: DashMap::new(),
            redeem_ip_windows: DashMap::new(),
            redeem_ip_bans: DashMap::new(),
        }
    }

    /// 登录尝试限流：同一 IP 每分钟最多 `per_minute` 次（默认 10）。
    pub fn check_login_ip(&self, ip: &str, per_minute: u32) -> bool {
        let limit = per_minute.max(1);
        let limiter = self
            .login_ip_limits
            .entry(ip.to_string())
            .or_insert_with(|| {
                let quota = Quota::per_minute(NonZeroU32::new(limit).unwrap());
                Arc::new(RateLimiter::direct(quota))
            });
        limiter.check().is_ok()
    }

    /// 兑换码 IP 防刷：
    /// - 已封禁：直接拒绝（24 小时）
    /// - 1 分钟内超过 [`REDEEM_IP_MAX_PER_MINUTE`] 次：立即封禁 24 小时
    /// - 每次调用计 1 次（含无效码尝试）
    pub fn check_redeem_ip(&self, ip: &str) -> Result<(), String> {
        const WINDOW_SECS: u64 = 60;
        const REDEEM_IP_MAX_PER_MINUTE: u32 = 20;
        const BAN_SECS: u64 = 24 * 60 * 60;

        let ip = ip.trim();
        if ip.is_empty() || ip == "unknown" {
            return Ok(());
        }

        let now = Instant::now();

        // 清理过期封禁 / 过期窗口，避免内存无限涨
        if self.redeem_ip_bans.len() > 4_096 {
            self.redeem_ip_bans.retain(|_, until| *until > now);
        }
        if self.redeem_ip_windows.len() > 8_192 {
            self.redeem_ip_windows.retain(|_, (_, start)| {
                now.duration_since(*start) <= Duration::from_secs(WINDOW_SECS * 2)
            });
        }

        if let Some(until) = self.redeem_ip_bans.get(ip) {
            if *until > now {
                return Err("当前 IP 因异常兑换请求已被封禁 24 小时，请稍后再试".to_string());
            }
            drop(until);
            self.redeem_ip_bans.remove(ip);
        }

        let mut banned = false;
        {
            let mut entry = self
                .redeem_ip_windows
                .entry(ip.to_string())
                .or_insert((0, now));
            let (count, start) = *entry;
            if now.duration_since(start) > Duration::from_secs(WINDOW_SECS) {
                *entry = (1, now);
            } else {
                let new_count = count.saturating_add(1);
                *entry = (new_count, start);
                if new_count > REDEEM_IP_MAX_PER_MINUTE {
                    banned = true;
                }
            }
        }

        if banned {
            self.redeem_ip_bans
                .insert(ip.to_string(), now + Duration::from_secs(BAN_SECS));
            self.redeem_ip_windows.remove(ip);
            tracing::warn!(
                ip = %ip,
                "Redemption IP banned for 24h due to excessive attempts"
            );
            return Err("当前 IP 因异常兑换请求已被封禁 24 小时，请稍后再试".to_string());
        }

        Ok(())
    }

    pub fn check_rps(&self, token_id: i64, rps: i32) -> bool {
        if rps <= 0 {
            return true;
        }

        let limiter = self.token_rps_limits.entry(token_id).or_insert_with(|| {
            let quota = Quota::per_second(NonZeroU32::new(rps as u32).unwrap());
            Arc::new(RateLimiter::direct(quota))
        });

        limiter.check().is_ok()
    }

    pub fn check_rpm(&self, token_id: i64, rpm: i32) -> bool {
        if rpm <= 0 {
            return true;
        }

        let limiter = self.token_rpm_limits.entry(token_id).or_insert_with(|| {
            let quota = Quota::per_minute(NonZeroU32::new(rpm as u32).unwrap());
            Arc::new(RateLimiter::direct(quota))
        });

        limiter.check().is_ok()
    }
}
